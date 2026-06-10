# 2026-06-09 — Memory Dynamics, Eval Flywheel, and Hardening Plan

**Status:** Draft v0.1, pending plan-reviewer pass and Trey approval.
**Author:** Claude (Fable), from the 2026-06-09 full-repo architectural review (5-agent exploration + system-spec read).
**Executor:** Claude Code sessions (with subagent fan-out where tasks are independent). Not a Codex plan — no Codex idioms.
**Branch strategy:** all work branches off `main` after the in-flight onboarding branch merges. Each phase lands as one or more reviewable commits; `scripts/check.sh` runs on the integrated trunk per repo convention, with per-crate gates listed per task.

## Plan revision history

- v0.2 (2026-06-09): incorporated plan-reviewer findings. Task 1.2 rewritten (full reindex does three things phase 6 doesn't: orphan-row cleanup, encrypted-tier indexing, embedding-job reconciliation — all must be preserved). A3 column-drop strategy changed to schema-bump + index rebuild (reversible) instead of DROP COLUMN. Task 1.5 YAML mechanism corrected (serializer is hand-rolled; parser swap is the real risk). Task 2.1 divergence documented (two pending-op types, two failure-kind mappings). Task 5.3 now strictly sequenced after 4.2 (shared `recall/rank.rs` ownership). Task 6.1 gains an alias-equivalence gate test. Tasks 1.1, 1.4, 2.6, 4.1, 7.3 scope corrections.
- v0.1 (2026-06-09): initial draft.

---

## 1. Context and goals

The 2026-06-09 review found the engineering substrate excellent (durability cascade, fail-closed governance, masked synthesis) but identified one vision-level gap and several capacity/health issues:

1. **No memory dynamics.** Nothing strengthens with use, decays with disuse, or feeds review outcomes back into confidence calibration. The data (events_log RecallHits, supersession projections, review decisions) is already collected; the dynamics are a scoring/lifecycle layer on top.
2. **Eval harness measures plumbing, not quality.** No golden dataset, no precision/recall@K, no quality baseline gating. Without this instrument, the dynamics layer (and the relevance-gate / drift weights) can't be tuned empirically.
3. **Contradiction detection runs at a fraction of design capacity.** `SimilaritySearch::top_k` has no production embedding-backed implementation; in practice only exact-hash duplicates are caught.
4. **Hot-path latency debt**, **dead infrastructure**, **harness-identity sprawl**, and a **cut list** of vestigial surface.

**Explicit non-goal:** the TUI is NOT scoped down. Trey wants it. Instead this plan invests in it (Task 7.3, TUI Reality Check parity).

**Already in flight (Phase 0, background agents, 2026-06-09):** Pass 2 retry corrective preamble; PII downgrade tracing::warn; `path_fraction` prefix matching + claim-lock message wording; duplicate memory-id regex; `drop_embedding_model_report` TOCTOU; `merge_swap_convergence` fuzz target in CI; `json_escape` consolidation. Not re-planned here. Verify they landed before starting Phase 1.

---

## 2. Approvals Trey must grant before the affected tasks start

Per repo convention, spec version bumps require explicit direction. This plan needs:

| # | Approval | Needed by |
|---|---|---|
| A1 | Stream E spec bump (v0.5 → v0.6): recall ranking incorporates a strength term (behavior change to block contents) | Task 5.3 |
| A2 | Stream F spec amendment or bump: substrate fragment archival deferral for frequently-cited fragments; calibration log as a new on-disk surface | Tasks 5.4, 5.2 |
| A3 | Stream A index schema v4 → v5: drop dead columns (`valid_from`, `valid_until`, `ttl`, `file_mtime_ns`). **Strategy (plan-reviewer mandated): bump schema version and omit the columns from `CREATE TABLE`, forcing a derived-index rebuild — fully reversible — NOT `ALTER TABLE DROP COLUMN`** (the migration framework is add-only by design, migrations.rs:51-98). The columns are live in the upsert INSERT/ON CONFLICT SQL (query.rs:866-902) and `file_mtime_ns` is NOT NULL — the write path must change in lockstep. Whether dynamics needs a stored `last_recalled_at` projection is decided in Task 5.1; the default proposal computes strength at query time from `events_log`, needing **no** new column (and no migration-time backfill problem). | Tasks 6.2, 5.3 |
| A4 | System spec §15.3 amendment: `path_overlap` prefix semantics (Phase 0 already implements the spec's plain-language intent; the amendment just records it) | Documentation only |
| A5 | Stream C amendment: similarity threshold/top-k move from engine constants into policy YAML (additive fields, defaulted) | Task 4.2 |
| A6 | Decision: delete `workers.rs` supervisor vs. wire it up. This plan assumes **delete** (Task 6.1) — the real background work (watcher, embedding, sync) is supervised elsewhere or synchronous by design | Task 6.1 |

---

## 3. Phase topology

```
Phase 0  one-liners (in flight, background agents)
Phase 1  cuts + hot-path performance          [parallel-safe internally]
Phase 2  structural refactors                  [after Phase 1 lands]
Phase 3  contradiction similarity wiring       [independent; parallel with 1–2]
Phase 4  eval quality flywheel                 [independent; parallel with 1–3]
Phase 5  memory dynamics                       [design after 4 starts; tuning depends on 4]
Phase 6  harness capability descriptor         [after Phase 2 (touches handlers/setup)]
Phase 7  TUI parity + docs reconciliation      [anytime; 7.3 best after Phase 2]
```

Phases 1, 3, 4 can run concurrently in separate sessions/worktrees — their file ownership is disjoint (see per-task file lists). Phase 5 implementation may start once 5.1's design is approved, but **tuning (5.5) blocks on Phase 4 metrics existing**.

---

## Phase 1 — Cuts and hot-path performance

### Task 1.1 — Delete dead worker supervisor

- **Files:** `crates/memoryd/src/workers.rs` (delete), `crates/memoryd/tests/worker_lifecycle.rs` (delete), `crates/memoryd/src/lib.rs` (module decl).
- **Why:** 4 named workers are all 60-second sleep loops (`run_worker` ignores `_name`); `WorkerSupervisor::start()` is never called in production.
- **Scope correction (plan-reviewer, verified repo-wide):** `WorkersHealth`/`WorkerSupervisor`/`WorkerName` are referenced **only** in `workers.rs` and `worker_lifecycle.rs`. There are no TUI/web/protocol/status consumers. The deletion is a clean two-file removal plus the module decl.
- **Gate:** `cargo test -p memoryd --tests`.

### Task 1.2 — Make `Substrate::open` reindexing incremental (NOT a simple call removal)

- **Files:** `crates/memory-substrate/src/api.rs` (`open_with_options` ~1618-1670; `full_reindex_from_repo` 2295-2307; `collect_reindex_paths` 2315-2357), `crates/memory-substrate/src/runtime/reconcile.rs` (`reindex_stale_memories` 427-461).
- **Why:** `full_reindex_from_repo` walks and rereads every memory on every open — O(n_total) startup — and runs immediately before phase-6 hash-based stale detection, which it makes pointless for plaintext files.
- **Plan-reviewer blocker (verified): the full reindex does three things phase 6 does NOT, and each must be preserved:**
  1. **Orphan-row cleanup** — `index.clear_plaintext_memory_index()` (api.rs:2297) removes index rows for memories deleted/moved on disk. Phase 6 only walks files that exist. Replacement: an explicit orphan sweep (index rows whose `repo_path` no longer stats → delete), which is O(n_index_rows) of stat calls, not O(n) file reads — still the perf win.
  2. **Encrypted-tier indexing** — `collect_reindex_paths` handles `encrypted/` with `metadata_only` + `safe_body` projection; phase 6 explicitly skips `encrypted/` (reconcile.rs:445). Replacement: extend the stale-detection walk to the encrypted tier with the same hash-based comparison, or keep a targeted encrypted-only reindex at open until that lands.
  3. **Embedding-job reconciliation** — `index.reconcile_active_embedding_jobs()` (api.rs:2305) has only one other caller (public `reindex()`, api.rs:1272). Must keep running at open; it directly intersects Phase 3's embedding path.
- **Approach:** replace the unconditional full reindex with: orphan sweep + encrypted-tier stale detection + `reconcile_active_embedding_jobs()`, leaving plaintext freshness to phase 6. Keep `full_reindex_from_repo` itself — it backs `memoryd reindex`.
- **Test matrix (expanded per review):** (a) fresh/empty index → all files incl. encrypted indexed; (b) warm index, one externally-modified plaintext file → only it reindexed; (c) **memory deleted on disk → its index row removed**; (d) **encrypted memory present → indexed with metadata-only projection**; (e) pending embedding jobs survive open. Assert against the reconcile report's actual phase naming (note: reconcile.rs phase numbering is internally confusing — report comment says "phase 7" at line 98 for `phase_6_index_consistency` output; verify names before asserting).
- **Gate:** `cargo test -p memory-substrate --tests` + two-clone convergence test. Solo commit, never batched (§8.1).

### Task 1.3 — Async-ify blocking IO on the recall hot path

- **Files:** `crates/memoryd/src/recall/binding.rs:49` (`fs::canonicalize` → `tokio::fs::canonicalize`), `crates/memoryd/src/recall/dream_questions.rs:91,149` (`fs::read_to_string`, `fs::read_dir` → tokio equivalents), `crates/memoryd/src/recall/startup.rs:510,523` (reality-check marker read/write → tokio).
- **Why:** these run on tokio workers inside the per-prompt recall path; sync syscalls block worker threads and hurt p95 exactly when concurrency is highest.
- **Approach:** mechanical conversion; the functions are already `async`. Where a helper is sync-only, either make it async or wrap in `spawn_blocking` (prefer async fs for single-file ops, `spawn_blocking` for the read_dir + multi-read loop in dream_questions).
- **Gate:** `cargo test -p memoryd --tests`.

### Task 1.4 — Async stdin/stdout in MCP stdio bridge

- **Files:** `crates/memoryd/src/mcp_stdio.rs:81` (stdin loop) **and the synchronous `io::stdout().lock()` write side on the same path** (plan-reviewer addition).
- **Approach:** replace `stdin.lock().lines()` with `tokio::io::BufReader::new(tokio::io::stdin())` + `read_line` loop, and convert the response writes to `tokio::io::stdout()`. Verify shutdown/cancellation behavior: the current blocking loop holds a thread until EOF; the async version must preserve the existing exit semantics (EOF → clean exit). The MCP bridge integration tests in memoryd cover the protocol loop — run them.
- **Gate:** `cargo test -p memoryd --tests mcp`then whole crate.

### Task 1.5 — Dependency trims

- **Files:** `crates/memory-substrate/Cargo.toml` (drop one of `yaml_serde`/`serde_yaml` — finish the "Phase 4 swap" promised at `error.rs:387`), `once_cell` → `std::sync::{OnceLock, LazyLock}` in memory-privacy and memory-substrate (MSRV 1.82 supports both), workspace `Cargo.toml` cleanup.
- **Mechanism correction (plan-reviewer, verified):** frontmatter **serialization is hand-rolled** (`scalar_to_yaml`, `plain_yaml_string` in frontmatter/serialize.rs:81-140) — no YAML library emits canonical bytes, so the serializer does not change in this swap. Both YAML crates are used for **deserialization only** (config/mod.rs:227,249, config/privacy.rs:37, frontmatter/parse.rs:34). The real risk is **parser divergence**: swapping `yaml_serde 0.10.4` → `serde_yaml 0.9` under frontmatter parse could deserialize edge-case scalars differently (quoting, type coercion, multiline), which then re-emit differently through the hand-rolled serializer.
- **Risk control:** the stability fixture test targets the **parse side**: a corpus of edge-case frontmatter files (quoted/unquoted scalars, numbers-as-strings, multiline, unicode) parsed by both libraries with asserted-equal results, BEFORE the swap. Round-trip (parse → hand-rolled serialize) byte-stability over the same corpus as the second layer.
- **Gate:** `cargo test -p memory-substrate --tests` + `cargo test -p memory-privacy --tests` + `cargo build --workspace --locked` (after targeted `cargo update -p` for removed deps; lockfile updates are fine on trunk, just no `cargo generate-lockfile`).

### Task 1.6 — Small cuts

- `crates/memory-substrate/src/runtime/faults.rs` — delete (never wired).
- Legacy `drop_embedding_model` (usize variant) at `api.rs:1427-1432` / `query.rs:159-162` — migrate remaining callers to the report variant, delete.
- `docs/runbooks/init-wizard.md` — delete or rewrite as a pointer to `docs/agent-onboarding.md` (it describes superseded `--non-interactive` behavior). **Ask Trey before deleting docs** — default here is rewrite-as-pointer.
- `thoughts/shared/handoffs/` empty 4/25–5/01 stubs — propose deletion to Trey, don't delete unilaterally.
- **Gate:** `cargo test -p memory-substrate --tests`; docs changes need no gate beyond `scripts/check-doc-cli-surface.sh`.

---

## Phase 2 — Structural refactors

All behavior-preserving. Candidate for `refactor-pilot` agent execution with per-step gates.

### Task 2.1 — Extract the repair-cascade in `api.rs`

- **Files:** `crates/memory-substrate/src/api.rs` (~396-426, ~679-708, ~858-892, ~1159-1180).
- **Approach:** the `index fail → enqueue_pending_index → startup_marker → OperatorRequired` cascade recurs across plaintext write, encrypted write, encrypted-metadata update, and tombstone — but **the four sites are NOT identical** (plan-reviewer, verified): the plaintext site (api.rs:409-415) uses `PendingIndexOp` + a three-arm if/else with an OperatorRequired fallback and `IndexAfterCommitFailed`; the encrypted sites (688, 867) use `PendingEncryptedIndexOp`, a `(repair_kind, kind)` tuple, and `RepairQueueFailed`. A single function signature cannot cover both without either a generic over the pending-op type or a small enum wrapping the two op types + a failure-kind mapping passed per site. **Do not flatten the divergence** — preserving the per-site failure-kind mapping is part of correctness, not noise.
- **Caution:** this cascade is spec-mandated ordering (Stream A §8.7) and the elegance audit deferred the adjacent M2 item for exactly that reason — preserve the ordering exactly, diff event emissions before/after with a targeted test that fault-injects an index failure per write path (all four paths, asserting each path's distinct failure kinds survive).
- **Gate:** `cargo test -p memory-substrate --tests`.

### Task 2.2 — Split `handlers/governance.rs` (2,004 lines)

- **Files:** `crates/memoryd/src/handlers/governance.rs` → `handlers/governance/{pipeline,policy,meta,privacy}.rs` + `mod.rs`.
- **Approach:** pipeline (write/supersede/forget + `execute_write_decision`), policy (`load_policy_set`, tombstone index, active-memory fan-out + semaphore), meta (`GovernanceMeta`, `GovernanceWriteInput`, `MetaSource`), privacy (classification glue). Move the ~700 lines of `#[cfg(test)]` fixtures into the relevant submodule or a `governance/test_fixtures.rs`. Kill the `use super::*` wildcard — each submodule imports explicitly so dependency surface becomes visible.
- **Gate:** `cargo test -p memoryd --tests`.

### Task 2.3 — Move rendering out of `protocol.rs`; HandlerState hygiene

- **Files:** `crates/memoryd/src/protocol.rs:766-884` (`render_peer_status_human`, `render_peer_activity_human` → `cli/` rendering module); `crates/memoryd/src/recall/dream_questions.rs:27` + `crates/memoryd/src/recall/startup.rs:39` (global `static Mutex` state → fields on `HandlerState`, e.g. a `RecallDedupState` struct; threads through `build_startup_response_with_coordination_config`).
- **Why the globals matter:** std Mutexes held in async code + cross-test state bleed; tests can't run two daemons in-process.
- **Gate:** `cargo test -p memoryd --tests`.

### Task 2.4 — Gate web fixture fallbacks out of production

- **Files:** `crates/memoryd-web/src/routes/roi.rs`, `sync_dashboard.rs`, and any other route with a `WebState::fixture()` branch.
- **Approach:** put fixture mode behind a cargo feature (`dev-fixtures`) or `#[cfg(debug_assertions)]`; production builds must fail loudly (5xx with a typed error) rather than serve plausible fake numbers. Frontend tests that rely on fixtures keep working via the feature in dev profile / MSW mocks.
- **Gate:** `cargo test -p memoryd-web --tests` + frontend `pnpm test` in `crates/memoryd-web/frontend`.

### Task 2.5 — Rehydration at review-approval time

- **Files:** `crates/memoryd/src/dream/rehydration.rs`, the review-approve handler (grep `ReviewApprove` / review handlers in `crates/memoryd/src/handlers/`).
- **Approach:** approval of any memory with `grounding_rehydration_required: true` re-runs `verify_dream_candidate` before promotion; failure surfaces as a typed refusal in the review UI rather than a silent promote of drifted evidence. While here: fix the per-citation full `substrate/` directory scan (`rehydration.rs:196-228`) by building a `BTreeMap<fragment_id, path>` once per verification run.
- **Gate:** `cargo test -p memoryd --tests dream` then whole crate.

### Task 2.6 — Eval-crate hygiene

- **Files:** `crates/memorum-eval/src/lib.rs:135` (busy-spin `block_on` → tokio current-thread variant already used in `orchestrator.rs:883`; line ref updated post-Phase-0); `crates/memorum-eval/tests/eval/regression/t19_peer_update_framing.rs:336` (private `assert_framing` reimplementation → use `memorum_coordination::framing_tests::assert_framing` under the `stream-i-deps` feature — **not drop-in**: the shared fn takes `FramingAssertionInput`, t19's takes `FramingAssertion`; reconcile signatures, keep the shared one canonical); `AssertionSpec` single-variant enum (`simulator.rs:46`) — collapse or extend, don't leave a one-variant enum.
- **Gate:** `cargo test -p memorum-eval --lib` + `cargo build -p memorum-eval --tests`.

---

## Phase 3 — Contradiction similarity wiring

### Task 3.1 — Ground-truth the live embedding path — ✅ DONE 2026-06-09

**Findings (verified, file:line evidence in task record):** production embedding inference does not exist. Chunks + `pending_embedding_jobs` are correctly produced on every write (query.rs:944-977) but have **no consumer** — the only vector-write API (`update_embedding`, api.rs:1416) is called solely by the bench binary and tests. The bootstrapped default config ships the **synthetic test triple** (`synthetic/stream-a-test/32`, tree/layout.rs:114-117). Both production read paths (`memory_search`, delta recall) pass `triple: None` and silently degrade to FTS-only bm25; doctor has no finding for job backlog or empty vec tables. The governance `top_k` stub (governance.rs:1001) ignores its candidate and active-memory summaries hardcode `similarity = 1.0` (governance.rs:944) — a landmine if `allow_top_k` (hardcoded `false` on the write path, governance.rs:68) were ever flipped without a real backend. Gates stay green because tests insert vectors manually.

**Consequences:** Task 3.0 added below (blocks 3.2). A6's "real background work is supervised elsewhere or synchronous" was half-wrong for embeddings — the work is simply unbuilt; deleting the stub remains correct, the real worker is 3.0's. The system spec's "hybrid keyword + vector" search description is aspirational in production today — worth a spec honesty note at next amendment.

### Task 3.0 — Ship embedding inference (NEW, prerequisite for 3.2)

**Design v2, locked 2026-06-09 after empirical bench (supersedes the EmbeddingGemma/ONNX v1 design below; evidence: `docs/reference/2026-06-09-embedding-model-research.md` + `docs/reference/2026-06-09-embedding-bench-golden-corpus.md`; Trey directive: research best local model, full autonomy granted):**

- **Model: `Qwen/Qwen3-Embedding-0.6B`** (Apache 2.0, ungated, 1024-dim w/ MRL, 32K ctx). Chosen by golden-corpus bench over EmbeddingGemma-300m, Qwen3-4B, and bge-m3: it wins the two metrics encoding Memorum's real failure modes — **trap-rate@5 0.261** (12/46 vs 16–17/46 for all others; ~30% fewer superseded/wrong-project lookalikes surfaced) and **abstention gap 0.332** (vs EmbeddingGemma's 0.137 — Gemma ranks better on nDCG but is *confidently wrong* on no-answer queries, the worst property for unprompted context injection). Bench harness preserved at `fixtures/golden/_embed_bench/`.
- **Lane: fastembed candle path** — `fastembed = "=5.16.0"` with `features = ["qwen3", "metal"]`, `Qwen3TextEmbedding` struct (not the ONNX `TextEmbedding` API). ⚠️ Metal device (`Device::new_metal(0)`) is wired but undocumented in fastembed examples — smoke-test first; `accelerate` (Apple BLAS, CPU) is the fallback feature if Metal misbehaves. Sync API → drain loop under `spawn_blocking`. Bench latency on this M4 Max: ~103ms/query fp16-MPS via Python (production candle lane expected similar or better; informational only).
- **Asymmetric prompts are part of the contract:** Qwen3 query embeddings take the model-card instruction prompt; document/chunk embeddings are plain. The provider trait must expose `embed_query` vs `embed_document` — collapsing them measurably degrades retrieval.
- **Default triple: `("fastembed-candle", "Qwen/Qwen3-Embedding-0.6B", 1024)`** — provider, model_ref, AND dimension all change vs the spec's literal `embeddinggemma` string (768→1024). Approved by Trey 2026-06-09 ("adapt ur spec and plan accordingly"). Contract-touching per invariant 3 → ship the dated stream-a spec amendment together with Task 3.0's code, not before.
- **Acquisition:** download-on-first-use via hf-hub cache → `<runtime_root>/models`, progress shown during `memoryd init`. **Never bundle weights.** Apache 2.0 noted in init output + runbook (simpler story than the prior Gemma license boundary).
- _(v1 design for the record: fastembed ort/ONNX + ungated `onnx-community/embeddinggemma-300m-ONNX`, 768-dim. Rejected on bench evidence: weakest abstention calibration of all four candidates despite best raw nDCG; also a restricted license and 2K ctx.)_
- **Scope (unchanged):** (1) provider module wrapping fastembed behind a small trait (fixture provider implements same trait via `memory-test-support::perf::synthetic_vectors`); (2) daemon background task draining `pending_embedding_jobs` → `update_embedding` per chunk (stale-job content-hash gate already enforced by `reconcile_active_embedding_jobs`); (3) bootstrap/init writes the real triple (tree/layout.rs:114-117) — typed mismatch errors per invariant 3; (4) doctor finding for pending-job backlog / empty active-triple vec table; (5) e2e on the fixture provider.
- **Gate:** substrate + memoryd whole-crate tests + new e2e: write → drain → vector present → KNN orders correctly, on the fixture provider. Real-model download path is a manual/dogfood verification (network + ~300MB), not CI.

### Task 3.2 — Production `SimilaritySearch` implementation

- **Files:** new module in `crates/memoryd/src/handlers/` or `crates/memory-substrate` (decide by where the vec query naturally lives — substrate owns the vec tables, so likely a substrate query method + a thin adapter in memoryd implementing the governance trait), `crates/memory-governance/src/engine.rs` builder wiring in `handlers/governance.rs`.
- **Approach:** implement `SimilaritySearch::top_k` against the active embedding triple's vec table, restricted to in-scope namespaces and active (non-tombstoned, non-superseded) memories. Respect invariant 3: embedding triple mismatch is a typed error, never silent fallback. Fall back to "no similarity candidates" (current behavior) when no triple is configured — degradation must be visible in the governance decision trace, not silent.
- **Gate:** `cargo test -p memory-governance --tests` + `cargo test -p memoryd --tests governance` + a new integration test: write memory A, write semantically-similar contradicting memory B, assert contradiction detection fires (requires deterministic embedding fixture — use the test-support embedding provider or a fixture triple).

### Task 3.3 — Policy-tunable thresholds (needs A5)

- **Files:** `crates/memory-governance/src/policy.rs` (add optional `contradiction_similarity_threshold`, `contradiction_top_k`, defaulted to current 0.82 / 5), `engine.rs` plumb-through, `policies/*.yaml` docs.
- **Gate:** `cargo test -p memory-governance --tests`.

---

## Phase 4 — Eval quality flywheel

### Task 4.1 — Golden corpus

- **Files:** `crates/memorum-eval/fixtures/golden/` (net-new directory — does not exist yet; the `.gitkeep` placeholders are `fixtures/policies/` and `fixtures/trees/`): ~100–150 hand-authored memories across `me`/`project`/`agent` namespaces with realistic frontmatter (entities, tags, confidence, supersession chains, a few tombstones), plus `queries.yaml`: 40–60 labeled cases `{query или session-context, expected_memory_ids (relevance-graded: essential/useful/irrelevant), namespace scope}`.
- **Approach:** author by hand (LLM-draft + human-curate is fine; curation is the value). Include known-hard cases: near-duplicate facts, superseded chains where only the head should surface, cross-project entity collisions, stale-vs-fresh competing memories. These mirror the failure modes T01–T12 test structurally, but with graded relevance instead of binary sentinel-presence.
- **Gate:** fixture lint (schema-validate every fixture via the frontmatter validator in a test).

### Task 4.2 — Quality metrics runner + baseline gate

- **Files:** new `crates/memorum-eval/src/quality.rs` + test target; baseline at `bench/quality-baseline.json` (same human-commit-only convention as perf baselines — extend the "don't overwrite programmatically" rule in CLAUDE.md to cover it).
- **Approach:** load golden corpus into a scratch substrate, run each labeled query through the real recall candidate selection + ranking (`memory_search` and startup-block assembly paths both), compute precision@K, recall@K, MRR, nDCG (graded labels make nDCG the headline metric). Runner emits JSON; a gate test compares against the committed baseline with a tolerance band and fails on regression beyond it. Wire into the same CI workflow as the existing eval suite (mock-harness mode — quality metrics need no LLM).
- **Gate:** the runner's own tests + one full quality run producing the initial baseline (Trey commits it).

### Task 4.3 — LLM-as-judge for the real-harness e2e tests

- **Files:** `crates/memorum-eval/tests/eval/domain/t13_*.rs`, `t15_*.rs`, shared judge helper in `src/`.
- **Approach:** after the existing structural assertions pass, add a judge step: a second harness-CLI invocation scoring "did the agent's recall/usage of the memory actually serve the task?" on a 3-point rubric, parsed as JSON with the same one-retry + corrective-preamble pattern as dream Pass 2. Judge score is **recorded, not gating** initially (collect distribution during dogfood before making it a gate). Also: log T13's silent parse-retry as a warning in eval output (review finding).
- **Gate:** mock-mode compile + the tests remain green in mock mode (`partial: true` reporting unchanged).

### Task 4.4 — Review-decision calibration log (shared with Phase 5)

- See Task 5.2 — single implementation, listed in both phases because eval consumes it (calibration report) and dynamics produces it.

---

## Phase 5 — Memory dynamics layer

The vision task. Sequenced: design → calibration log → strength-in-ranking → archival deferral → tuning.

### Task 5.1 — Design doc (short spec draft for Trey; needs A1–A3 resolution)

- **Output:** `docs/specs/memory-dynamics-v0.1.md` (new feature spec, same pattern as `feature-memoryd-export-v0.1.md`).
- **Must lock:**
  - **Strength function.** Proposed starting point: `strength(m) = w_f · log1p(recall_count_30d)/log1p(max_30d) + w_r · exp(-days_since_last_recall/τ) + w_c · corroboration(m)` with `τ ≈ 14d`, weights summing to 1, all derived from existing `events_log` + `memory_supersession` projections. Sub-ms per-memory via the existing covering index; computed at candidate-selection time, not stored (no new write path, and no A3 stored-projection column). **Note (plan-reviewer):** the `recall_count_30d / max_30d` input is the same quantity `recall_frequency_norm` already computes for the RC drift score (`reality_check/scoring.rs:76`) — extract/share that computation rather than reimplementing it; the design doc must state the relationship between drift's *inverse*-frequency use and strength's *direct*-frequency use so the two stay coherent.
  - **Integration shape.** `final_rank = relevance_score · (1 + α·strength)` with small α (0.15–0.3) so relevance dominates and strength tiebreaks — memory that's useful keeps surfacing; memory never recalled fades in *ranking competition*, not in existence. No hard deletion ever — tombstones remain the only delete path (governance principle 8 untouched).
  - **Reinforcement semantics.** `RecallHit` already fires per included memory per block. Decide whether inclusion-in-block counts as "use" (cheap, already emitted, but inflates with block size) vs. requiring a stronger signal (memory_get / reveal / RC confirm). Proposal: inclusion counts at low weight; RC `confirm` and explicit `memory_get` count at high weight (needs a per-event weight map, possibly a new `EventKind` weight table — no new event variants required).
  - **Cache stability** (principle 9): strength changes ranking between sessions, not within one — startup block is per-session stable; delta blocks already vary. No cache-thrash risk, but state it.
  - **Fragment archival deferral** (A2): cleanup layer defers archival of substrate fragments cited ≥ N times in dream journals/evidence within the lifetime window; hard cap (e.g. 2× lifetime) so nothing is immortal by citation alone.
  - **What we deliberately do NOT build:** spaced-repetition resurfacing prompts, automatic confidence mutation from recall frequency (confidence stays a provenance-grounded value; strength is a separate axis). Listed as anti-features of this spec.
- **Gate:** plan-reviewer pass on the spec + Trey sign-off.

### Task 5.2 — Calibration log

- **Files:** review handlers in `crates/memoryd/src/handlers/` (accept/reject/edit paths), new append-only JSONL surface `dreams/calibration/<device_id>.jsonl` (synced — per-device files merge by concatenation like the event log; needs A2).
- **Record:** `{candidate_id, scope, author_kind, self_reported_confidence, decision (accept|reject|edit), edited_distance (if edit), decided_at, session_id}`. Written on every review decision for dream-sourced and quarantined candidates.
- **Consumer:** `memoryd dream calibration` CLI report (bucket confidence into deciles, show accept-rate per bucket) — this is the data that justifies (or kills) the v1.1 auto-promotion path the system spec §12 promises.
- **Gate:** `cargo test -p memoryd --tests` + a round-trip test (decide → log line → report).

### Task 5.3 — Strength term in recall ranking (needs A1, 5.1 approved)

- **Files:** `crates/memoryd/src/recall/rank.rs` (+ candidates.rs if the strength inputs join into the candidate query), config surface in `config.yaml` (`dynamics.alpha`, `dynamics.tau_days`, weights — all defaulted, all dogfood-tunable).
- **Sequencing (hard, plan-reviewer):** Task 4.2's quality runner exercises `rank_recall_candidates`/`select_ranked_candidates` (rank.rs:27,40) — the same functions this task edits. **5.3 starts only after 4.2 has landed**; do not run Phase 4 and 5.3 in concurrent sessions.
- **Approach:** per 5.1. Batch the strength inputs in one SQL query over candidate ids (no per-candidate round-trips). Strength values surface in the recall block's explanation metadata and in trust artifacts (Stream G) so the operator can see *why* something ranked.
- **Gate:** `cargo test -p memoryd --tests recall` + whole crate + **Phase 4 quality runner must not regress** (this is the first consumer of the new instrument: run quality metrics with dynamics off vs. on; on-mode becomes the new baseline only if Trey accepts the diff).

### Task 5.4 — Fragment archival deferral (needs A2, 5.1 approved)

- **Files:** Stream F cleanup layer (grep `cleanup` under `crates/memoryd/src/dream/`), citation counting against dream journal evidence refs.
- **Gate:** `cargo test -p memoryd --tests dream`.

### Task 5.5 — Dogfood tuning loop

- **Process task:** during the dogfood window, weekly: quality-runner trend + calibration report + Trey's subjective "did recall feel smarter or noisier"; adjust `dynamics.*` config (no code change needed) and record decisions in the spec's revision log. Final weights land before 1.0.0, same as the drift-score weights.

---

## Phase 6 — Harness capability descriptor

### Task 6.0 — Design note (half-page, in-plan amendment)

Harness identity currently lives in four places: `FULL_COORDINATION_HARNESSES` const (`crates/memorum-coordination/src/session.rs:64`), dream adapter registry (`crates/memoryd/src/dream/registry.rs:13-16`), `HarnessTarget`/wiring (`crates/memoryd/src/setup/mcp_wire.rs`), import `Harness` enum (`crates/memoryd/src/import/sources/`). Define one `HarnessDescriptor { id, aliases, tier, coordination: Full|ObserveOnly, cli: Option<CliSpec>, mcp_config: JsonAtPath|TomlAtPath|None, importer: Option<ImporterId> }` registry: built-ins for claude-code/codex compiled in, additional descriptors loadable from `config.yaml` (coordination capability + CLI spec are safe to make data; importers stay code).

### Task 6.1 — Implement registry; migrate the four sites

- **Files:** new `crates/memorum-coordination/src/harness.rs` or a small new shared crate if memoryd↔coordination layering demands it (decide in 6.0); the four sites above; `crates/memoryd/src/coordination_config.rs` for the runtime-config load (`full_coordination_harnesses` override ships even if the full descriptor is staged).
- **Sequencing:** minimum viable first — move `FULL_COORDINATION_HARNESSES` to runtime config (the review's Refactor 3, small and self-contained), then unify the rest behind the descriptor.
- **Alias reconciliation (plan-reviewer):** the four sites spell the same harness differently — `"claude-code"` (session.rs:64), `"claude"` (dream registry.rs:14), `HarnessTarget::Claude` (mcp_wire.rs:19). The descriptor's `aliases` must prove cross-site equivalence; the gate includes a test asserting `"claude"` and `"claude-code"` resolve to one descriptor (otherwise coordination capability silently changes for one spelling).
- **Gate:** `cargo test -p memorum-coordination --tests` + `cargo test -p memoryd --tests` + the setup e2e (`setup_end_to_end.rs`) + the alias-equivalence test.

---

## Phase 7 — TUI investment + docs reconciliation

### Task 7.1 — Onboarding docs coherence (do before the onboarding branch merges)

- `docs/getting-started.md` says `memoryd init` is "release-target; not current alpha bootstrap"; `docs/agent-onboarding.md` (same branch) makes `memoryd init --non-interactive --json` the primary path. Reconcile to one story (the setup engine shipped — getting-started should promote `init`). Also: execute `docs/runbooks/agent-onboarding-smoke.md` once for real (it self-reports never having been run) — that's the branch's own stated done-condition.
- **Gate:** `scripts/check-doc-cli-surface.sh`.

### Task 7.2 — Reviews index

- `docs/reviews/` has 163 files. Add `docs/reviews/INDEX.md` (one line per review: date, stream, verdict, superseded-by). Generate mechanically, hand-tune. Optional but cheap; navigation pain is real.

### Task 7.3 — TUI Reality Check parity (the keep-the-TUI investment)

- **Files:** `crates/memoryd-tui/src/focus/reality_check.rs` (45 lines today), `app.rs` glue, possibly a shared score-breakdown formatter.
- **Approach:** bring the TUI RC view to parity with web: per-memory drift-score breakdown (all 5 components, reusing the scoring structs from `crates/memoryd/src/reality_check/scoring.rs`), the `not_relevant` response **rendering + keybinding** (scope correction: the action is already plumbed — `app.rs:57` enum + `client.rs:368` protocol mapping exist; only the view/keybind in `focus/reality_check.rs` is missing), and trust-artifact severity cues in `widgets/trust_artifact.rs` (color-code high-drift / quarantined lines via the theme crate instead of flat text).
- **Gate:** `cargo test -p memoryd-tui --tests` (44 test files — extend the RC ones).

---

## 8. Risks and watch-items

1. **Task 1.2 (reindex removal)** is the highest-blast-radius change in the plan — it touches startup correctness for every consumer. It has a clean rollback (re-add one call) and strong existing test coverage, but do it solo in its own commit, never batched.
2. **Task 1.5 YAML consolidation** can silently change canonical serialization bytes → merge-driver and convergence behavior. The stability fixture test goes in *first*.
3. **Phase 5 ordering discipline:** do not tune strength weights by feel before Phase 4's quality runner exists. The whole point of sequencing eval first is to avoid vibes-driven memory dynamics.
4. **Spec drift:** Tasks 5.3/5.4 change Stream E/F behavior. The approvals table (§2) is the contract — no implementation before the corresponding A-item is granted.
5. **Embedding-path unknown (Task 3.1):** if production embedding inference turns out not to run at all, Phase 3 grows a prerequisite (wire the inference worker) and the Phase 1 deletion of `workers.rs` should leave room for a *real* embedding worker design rather than resurrecting the stub.
6. **Parallel-session file collisions:** Phases 1/3/4 are disjoint by crate except `crates/memoryd` (Tasks 1.1/1.3/1.4 vs 3.2 adapter). If running concurrently, keep 1.x and 3.2 in separate sessions only after 1.1 lands (both touch handlers/state surface). **Correction to §3's "disjoint" claim:** Phase 4 (Task 4.2 quality runner) and Task 5.3 both own `recall/rank.rs` — 5.3 is strictly sequenced after 4.2, never concurrent (recorded in Task 5.3).

---

## 9. Done criteria

- All Phase 0–2 cuts/fixes merged; `scripts/check.sh` green on trunk.
- Contradiction detection demonstrably fires on a semantically-similar non-identical pair (Task 3.2 integration test).
- Quality baseline committed; CI gates on it.
- Dynamics shipped behind config, quality-runner-verified, with calibration report producing real deciles from dogfood review decisions.
- Harness coordination capability is runtime-configurable; adding a Tier-3 harness with full coordination requires zero code.
- TUI RC shows score breakdowns and accepts `not_relevant`.
- `getting-started.md` and `agent-onboarding.md` tell the same story.
