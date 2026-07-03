# Daemon memory reduction — embedding model lifecycle (design)

**Date:** 2026-07-03 · **Branch:** `perf/daemon-memory` (off `foundation/runtime-loop-closure`) · **Author:** Claude (coordinator)

## Thesis and provenance

During dogfooding, the installed `memoryd` daemon held **2GB+ RSS at baseline** — unacceptable for an always-on background process. Recon (2026-07-03, read-only, this branch) established the cause precisely: the Qwen3-Embedding-0.6B model is loaded in-process via fastembed/candle **~1s after daemon startup, unconditionally, and held for the daemon's entire lifetime** by two `Arc<dyn EmbeddingProvider>` owners (`server.rs:191` load, `server.rs:235-236` publication into the request-path `EmbeddingProviderSlot` and the drain worker). There is **no unload, idle, or eviction path of any kind** — the only knob is `MEMORUM_DISABLE_EMBEDDING_WORKER`, a hard opt-out before load.

Measured basis (branch binary, release, fresh temp store):

| State | RSS |
| --- | --- |
| Daemon, model not loaded (load failed, retrying) | **188–194 MB** steady over 2 min, writes+search included |
| Model resident, in-process (real-model smoke test, Metal fp16) | **1,139 MB peak** (load: 7.75s cold cache, 0.4s warm; embed 28–38ms) |

The non-model daemon is already at ~194 MB. The entire fix is **model lifecycle**, not micro-optimization.

## Loss function (hard goal)

- **Target:** idle baseline RSS **< 200 MB** (user floor: < 256 MB). Transient spikes during active embedding allowed; RSS must return below target within one idle-unload window after embedding work stops.
- **Scorer:** `ps -o rss= -p <memoryd pid>` sampled at: daemon start, post-startup idle (2 min), during a real write burst (model loaded, embeddings drained), and after the idle window elapses.
- **Anti-gaming constraints:** embedding identity triple `(fastembed-candle, Qwen/Qwen3-Embedding-0.6B, 1024)` unchanged (critical invariant 3); recall hybrid behavior unchanged when the model is resident; all pinning tests in §Tests stay green; full `scripts/check.sh` green at branch end. Deleting or permanently disabling embedding does not count.

## Design

### Primary: managed lifecycle — load on demand, unload on idle (in-process)

Replace "load once, hold forever" with a small state machine owning the provider:

```
Dormant ──(demand)──> Loading ──ok──> Active ──(idle ≥ window)──> Dormant
                         │
                         └──fail──> Failed (existing 300s retry backoff, but only while demand persists)
```

- **The seam already exists.** `EmbeddingProviderSlot` (`embedding/mod.rs:57`) is empty-tolerant: recall degrades to FTS-only with `no_embedding_provider`, governance degrades to `no_embedding_provider` finding. An empty slot is already a legal, handled state everywhere. We add: empty may mean *dormant* (healthy, will load on demand), not just *failed*.
- **Load triggers (demand):**
  - Drain worker wakes (5s idle poll, `worker.rs:41`) and finds pending_embedding_jobs → ensure-loaded, then drain.
  - Dream run (sets `index_embeddings:true`) → same path, via the queue.
  - Recall query arrives while dormant → **degrade to FTS for that request** (exactly today's absent-provider behavior, preserving the hook latency budget and fail-open semantics) **and fire a background ensure-load** so subsequent queries get vectors.
- **Unload trigger:** no embedding activity (no drain batches, no query embeds) for `idle window` → drop both Arc owners, slot back to empty/dormant. Never unload mid-batch; the drain worker owns the unload check between batches so a burst can't be interrupted.
- **Knob:** env `MEMORUM_EMBED_IDLE_UNLOAD_SECS`, default **900** (raised from 300 per amendment F2); `0` = never unload (exact legacy behavior, the escape hatch). Per-device tuning — deliberately **not** in synced `config.yaml` (that file carries identity, not tuning; invariant 4 discipline).
- **Observability:** worker state gauge (`dormant | loading | active | failed`) + load/unload counters, surfaced through `memoryd status`/`doctor` — dormant must read as *healthy* (doctor stays green), failed keeps today's degraded finding.
- **Triple discipline across reloads:** the existing triple-mismatch disable (`worker.rs:76-90`) must re-check on every reload, not just first load — a config change while dormant must not resurrect a mismatched provider.

### Fallback: out-of-process embed worker (only if the spike fails)

**Known risk, spiked first (Wave 1, step 0):** dropping `FastembedProvider` may not return RSS to the OS — allocator retention or candle Metal heap caching could hold pages. The spike is a tiny `examples/` binary: sample RSS → load provider → embed a batch → drop provider → `sleep` → sample RSS. **Pass:** post-drop RSS within ~250 MB of pre-load. **Fail →** pivot to a short-lived subprocess (`memoryd embed-worker` hidden subcommand speaking length-prefixed bincode over stdin/stdout; parent spawns on demand, kills on idle; OS reclaims everything by construction). The fallback is a bigger diff (IPC, process lifecycle) and is **not built speculatively** — it exists in this doc so the pivot decision is pre-made, not re-litigated. ONNX-runtime note: fastembed links ort unconditionally even on the candle path (`Cargo.toml:18-23`); if ort's baseline residency alone breaks the 200 MB target post-drop, that also forces the fallback.

### Explicitly rejected

- **Quantized/smaller model:** changes the identity triple → re-embedding the store, recall-quality risk. Out of scope.
- **Configurable device/dtype:** orthogonal tuning; doesn't fix residency-forever.
- **In-RAM index changes:** recon confirmed vector+FTS storage is disk-backed SQLite (sqlite-vec `vec0`, `index/embedding.rs:194`); nothing to fix there.

## Storage and state

No on-disk format changes. No `config.yaml` changes. State machine is purely in-memory daemon state; runtime metrics extend existing counters.

## Dependencies

**None added.** The primary design is a pure refactor of `crates/memoryd` (`server.rs`, `embedding/mod.rs`, `worker.rs`, status/doctor surfaces). The fallback would also add no external deps (std process + existing serde/bincode family) — but is not being built unless the spike fails.

## Testing strategy

- **Keep green (pinning, from recon):** `tests/embedding_real_model_smoke.rs` (#[ignore], real model, dim=1024, query≠doc); `tests/governance_contradiction_similarity_e2e.rs`; `worker.rs` unit tests (drain, triple-mismatch disable, retry budget); `fastembed_provider.rs` triple-gating tests; dimension invariants (`embedding/mod.rs:188`, substrate `index/embedding.rs`, `index/vector.rs`).
- **New unit tests (FixtureProvider, no real model):** dormant→active on queued jobs; active→dormant after idle window; unload never fires mid-batch; query-while-dormant returns FTS-degraded AND triggers load; `0` disables unload; triple-mismatch disable persists across reload; failed-load retains existing backoff.
- **Spike + acceptance (real model, live):** the drop-reclaims-RSS spike; then live deploy on `~/memorum` (backup first) with the loss-function scorer — start / idle / write-burst / post-idle-window RSS, plus `doctor` green and recall hits present with model resident.

## Non-goals

- Recall relevance/rendering (v3.0 P1/P3/P4), capture engine, dreaming behavior.
- Reducing the ~194 MB no-model baseline (already under target; revisit only if acceptance shows it creeping).
- Multi-model support, remote embedding APIs, model download UX.

## Wave plan

- **Wave 1 (Codex, `work` lane):** spike binary + measurement first (pivot gate); then the lifecycle state machine, load triggers, idle unload, env knob, metrics/status surface, unit tests. Owned files: `crates/memoryd/src/{server.rs, embedding/*, worker paths, handlers/status-doctor surface}`, `crates/memoryd/examples/embed_rss_spike.rs`, `crates/memoryd/tests/*`. Gate: `cargo clippy -p memoryd --all-targets -- -D warnings` + `cargo test -p memoryd -- --test-threads=2` (5x sweep by coordinator).
- **Wave 2 (coordinator-owned):** live acceptance per loss function — backup `~/memorum`, deploy branch binary via launchd, measure, fix forward. Doubles as the runtime-loop foundation Wave E redeploy groundwork.
- Review lanes: Cursor `safe` diff review (Wave 1), GLM fix lane for accepted findings. Full `scripts/check.sh` once at branch end.

## Amendments

### 2026-07-03 — Codex design review (codex-54), all findings dispositioned

**F1 (blocker) — spike pass criterion. ACCEPT.** The "within ~250MB of pre-load" delta gate could pass a design that fails the product target, and macOS RSS can hide Metal/IOSurface residency. New criterion: spike passes only if post-drop **absolute** memory is **< 200 MB on BOTH `ps` RSS and `phys_footprint`** (via `/usr/bin/footprint <pid>` or `vmmap -summary`). Marginal results (200–256 MB) escalate to the coordinator, never self-judged a pass. Acceptance uses the same dual metric.

**F2 (major) — FTS-only common path + reload storm. ACCEPT-REDUCED.** Accepted mechanisms: (a) **single coalesced in-flight load** — concurrent demand joins the existing load, never stacks; (b) **activity-extends-window** — any embedding demand (including an FTS-degraded recall that fired the background load) resets the idle timer, so an active session keeps the model warm; (c) default idle window raised **300s → 900s** to span human pauses; (d) load-failure backoff (existing 300s) prevents demand-driven retry storms. Rejected: warm-on-SessionStart plumbing (new signal path for marginal gain — 900s + activity-extension covers the session shape). **Accepted tradeoff, explicit:** the first recall after >15 min of true idle is lexical-only while the model loads in the background. Fresh-runtime download risk is unchanged from today (startup load already does this). If dogfood shows the FTS-only turn is still too common, the knob tunes it without code change.

**F3 (major) — in-flight Arc accounting. ACCEPT.** Clearing the slot is not unloading: detached `spawn_blocking` embeds (query timeout drops the JoinHandle, task runs on) and the drain worker's permanent Arc clone would keep the model resident invisibly. Design now requires a **lifecycle manager as sole provider owner**: consumers acquire short-lived clones through it with an in-flight counter (guard object); the drain worker acquires per batch, never holds across idle; unload proceeds only when idle-window elapsed AND in-flight == 0, and re-checks after the last guard drops. A wedged detached embed means the unload defers — state gauge exposes `unloading-pending-inflight` so it's observable, not silent.

**F4 (major) — dormant vs failed semantics. ACCEPT.** New explicit lifecycle state (`dormant | loading | active | failed{last_error}`) exposed via status/doctor, decoupled from `Option<Arc<_>>` emptiness. Degradation markers split: **`embedding_dormant`** (healthy, load in flight or pending demand) vs **`no_embedding_provider`** (disabled/failed — today's meaning preserved). Doctor: dormant/loading are green; failed keeps today's finding. Consumers/tests asserting on `no_embedding_provider` are audited in Wave 1 (owned-files list includes recall + governance markers and their tests).

**F5 (major) — config edit while dormant. ACCEPT-REDUCED.** No live config-reload path is built. The contract is stated explicitly: **`active_embedding` changes require a daemon restart** — which is the de-facto behavior today (triple frozen in `Index` at open). Every reload re-probes dimension and re-checks the triple against the frozen in-memory value (same guarantee as today's single load, now enforced per reload). The invented hazard is closed by contract, not by new machinery.

**F6 (minor) — env knob vs launchd. ACCEPT-REDUCED.** Keep `MEMORUM_EMBED_IDLE_UNLOAD_SECS`; wire it into the launchd plist template (installer support) and surface the **effective** idle window + its source in `memoryd status`. No config-file surface added.

**F7 (minor) — 5x sweep churn. ACCEPT-REDUCED.** Per-repo CPU discipline (syspolicyd), the gate is: **1×** `cargo clippy -p memoryd --all-targets -- -D warnings` + **1×** `cargo test -p memoryd -- --test-threads=2`, plus **5× repeat of the new lifecycle test module only** (the concurrency-flake-prone surface, cheap to repeat). Full `scripts/check.sh` once at branch end, unchanged.
