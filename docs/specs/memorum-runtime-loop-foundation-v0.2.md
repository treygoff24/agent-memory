# Memorum — Runtime-Loop Foundation & Local-First Closure (v0.2)

**Status:** Draft for review. Not yet an accepted implementation contract. This document specifies the **foundation layer** that the Stream E ambient-recall redesign (`stream-e-ambient-recall-v3.0.md`) assumes but that the live system does not provide. It amends the runtime behavior of Stream A (substrate git), Stream F (dreaming lease), and Stream G (observability/doctor), and defines the local-first architectural principle for v1. On acceptance it becomes **Phase 0** of the v3.0 build — the prerequisite beneath v3.0 Phase 2 (continuity engine + C0 capture).

**Date:** 2026-06-25.

**Authors:** Claude, from a co-founder design session with Trey, grounded in a live-system dogfood diagnosis (2026-06-25).

**Sources:** the live `~/memorum` dogfood install (6 days, single-device, no git remote); a four-lane root-cause diagnosis (two native Explore passes over the `memoryd` source + a Cursor decorrelated read + direct spec/code reading); the shipped Stream A–I contracts; `stream-e-ambient-recall-v3.0.md`; `system-v0.2.md` §1.1 and §20.

**Review pass (2026-06-25):** revised after three decorrelated adversarial reviews (native `plan-reviewer`/opus, Cursor, GLM via delegate; Codex was usage-capped). All three verified the code anchors against source and converged on the same structural gaps. This revision folds in: F1 needs an identity-setting commit helper (`auto_commit` sets no git author and fails on an unconfigured-git host) and the latent `sources/` staging bug; F3 dedup must move to the dispatcher on the structured event (the queue stores only a flattened string) and needs an explicit re-reconcile trigger; F4 doctor must take `HandlerState` to see recall counters, D3 must measure *stale* untracked, and a capture-freshness check (D5) is added so the drought cannot silently recur after P2; F5 must name the *promotion* carve-out distinctly from B7's *governance-check* carve-out; the cross-process pre-dream flush + git-lock serialization; corrected anchors and split P0/full-loop gates. A "Review revision history" entry is at the foot of the document.

---

## Revision goal (v0.2)

**Quarantine-resolution CLI surface reduced to `--edited` only (F3).** The v0.1 contract specified `memoryd quarantine resolve <id> [--accept-ours | --accept-theirs | --edited]` (§F3 "Required" item 3). During implementation those side-selection modes proved un-honorable at P0: by the time a memory is quarantined, the three-way merge driver has already collapsed the conflict into a single canonical body, so no preserved "ours"/"theirs" side survives for the CLI to select between. All three modes therefore resolved to the identical action — promote the current on-disk body — differing only in the recorded audit reason, which advertises a capability the daemon cannot perform. v0.2 reduces the contract surface to `memoryd quarantine resolve <id> --edited`: the operator resolves the conflicted file by hand, then certifies it. True side-selection is deferred until a substrate side-swap API preserves both conflict sides through quarantine; when that lands, `--accept-ours`/`--accept-theirs` return in a later revision. This is the only contract change from v0.1; every other behavior, invariant, and acceptance test is carried forward unchanged.

## Revision goal (v0.1)

The streams shipped and each passed its own gate. The **runtime loop that connects them** — `write → commit → sync → dream → capture → recall → observe` — was never closed end-to-end on a real single-device install. A 6-day dogfood on one laptop with no git remote surfaced that the deployed system recalls a frozen import, captures nothing from daily use, and reports itself healthy throughout. This spec closes the loop and makes the most common install (one machine, no remote) the first-class case. It is deliberately a **foundation** contract: the smallest set of changes that make v3.0's continuity engine physically able to run, plus the observability to know it is running.

---

## 1. Problem statement — the five cut seams

Each seam is anchored to source so the fix is buildable.

### 1.1 Seam 1 — Durability: writes never commit to git

`Substrate::write_memory` (`crates/memory-substrate/src/api/write.rs:9-91`) persists to three places: the `.md` file (`:33`), the SQLite index (`:55`), and the per-device event log (`:90`, `commit_lifecycle_event` — an **event-log** commit, not git). There is no `git add`/`git commit` in the write ladder.

`Substrate::auto_commit` exists (`crates/memory-substrate/src/api/mod.rs:112-114`) with **zero callers in `memoryd`**. The only git commit at runtime is the one-time bootstrap (`crates/memory-substrate/src/git/init.rs:21`). The serve loop (`crates/memoryd/src/server.rs:61-76`) spawns the notification dispatcher, coordination cleanup, reality-check scheduler, and embedding worker — no commit task, no sync task.

**Consequence:** on the live install, 552 memory files are untracked, and the Stream A git-backing (versioning, merge driver, two-clone convergence) is inert at runtime. v3.0 §4.2's "the merge driver still guarantees convergence" holds only once writes commit.

### 1.2 Seam 2 — Cognition: dreaming cannot run (two walls)

**Wall 1 (no remote).** Lease acquisition's first step is `git fetch origin` (`crates/memoryd/src/dream/git.rs:75-77`, `NativeLeaseGit::fetch_origin`), propagated via `?` at `crates/memoryd/src/dream/lease.rs:123`. On a no-remote repo this fails → `LeaseError::Unavailable` → the scheduled loop (`lease.rs:225-227`) records `lease_unavailable`, burns all retry windows, writes `outcome: "missed"`. The live install shows `consecutive_missed_runs: 6`, `dream_runs_invoked_total: 0`. The behavior is tested-in: `crates/memoryd/tests/dream_lease_election.rs:98` (`unavailable_fetch_without_origin_returns_lease_unavailable_and_cli_exits_5`).

**Wall 2 (dirty tree from Seam 1).** Even past Wall 1, the lease's clean-tree guard `dirty_user_work_paths` (`dream/git.rs:168-177`) treats any path not in the allowlist `is_substrate_managed_path` (`dream/git.rs:187-195`, which tolerates only `.memorum/`, `.memoryd/`, `events/`, `leases/journal.lease`) as "dirty user work," returning `LeaseError::DirtyTree` (`lease.rs:132-135`). The 552 untracked `projects/*.md` from Seam 1 trip it. The guard, meant to protect the **user's** manual edits, misfires on the **daemon's own uncommitted output**.

**Consequence:** dreaming — the only automatic synthesis/capture engine — is dead two ways. v3.0 §5.2's "reuses the Stream F nightly pipeline" assumes a pipeline that does not run.

### 1.3 Seam 3 — Capture: no ambient capture exists

Setup wires exactly three lifecycle hooks, all read-only recall: `SessionStart`, `UserPromptSubmit`, `SubagentStart` (`crates/memoryd/src/setup/hooks_wire.rs:275-285`; handler maps only these, `passive: true`, `crates/memoryd/src/cli/recall_hook.rs:114-144`; the `_ => return None` arm at `:144` drops `Stop`/`SessionEnd`/`PostToolUse`). Memories arrive only via (a) explicit MCP/CLI writes agents do not make organically, (b) dreaming → **candidates needing manual review** (Stream F never auto-promotes, `stream-f-dreaming-v0.3.md:187`), or (c) one-time import.

**Consequence:** six days of heavy use produced zero new memories — expected, independent of Seam 2. v3.0's `C0` closeout hook (§5.1) is the missing capture primitive and is spec-only.

### 1.4 Seam 4 — Recall: the read path is degraded

Recency, not relevance; slug summaries (`crates/memoryd/src/handlers/governance/meta.rs:612-619` falls back to title when import sends no summary); `snippet: None` hardcoded for recent-memory (`crates/memoryd/src/recall/startup.rs:150`); empty entity-recall because every imported memory has `entities: []` (enrichment scoped out by design, `docs/plans/2026-05-27-memorum-importer-and-predogfood-ux.md:29,153`); and a passive-hook section budget of ~1,772 tokens (`HOOK_STARTUP_BUDGET_TOKENS = 1900` in `crates/memoryd/src/recall/types.rs`, minus the 128-token reserve at `startup.rs:102`; the non-hook default is 3,600) crammed by 552 short-summary memories → 3,465 cumulative `BudgetExhausted` events. **This seam is owned by `stream-e-ambient-recall-v3.0.md` (Phases 1/3) and is out of scope here** — but v3.0 P1/P3 cannot improve a store that never grows, so this foundation is its precondition.

### 1.5 Seam 5 — Observability: the system cannot see its own failure

`doctor` (`crates/memoryd/src/handlers/doctor.rs:81-87`) computes `healthy = !has_substrate_findings && (enabled_harness_count == 0 || authenticated_harness_count > 0)`. It checks only: substrate tree validation (`PartialSync`, which downgrades missing-ref to warnings), event-mirror lag, embedding backlog (snapshotted *before* it can flip health, `doctor.rs:30-35`), and harness-CLI auth. It never reads dream-run freshness, sync/quarantine state, uncommitted-substrate state, or recall budget — so it returns `{"healthy": true, "findings": []}` while four seams are cut.

Compounding: the "Sync is blocked by a merge conflict" notification is re-emitted at every startup from quarantined files (`crates/memoryd/src/server.rs:91-95`) into an **append-only, no-dedup, no-clear** passive queue (`crates/memoryd/src/notifications/passive.rs`, in-memory, capacity 100), so identical notifications accumulate within a run and re-accumulate on each restart (96 observed). The one flag that detects a mid-merge tree, `recovery_required` (`crates/memory-substrate/src/runtime/reconcile.rs:171-176`), has **zero consumers** — detected, never acted on.

---

## 2. The governing principle — local-first by default

**Decision (Trey, 2026-06-25):** single-device / no-remote is the first-class default architecture, not the degraded case.

> **The local daemon is the coordination point. The git remote is only the cross-*device* sync transport, and it lights up only when configured.**

Implications:

1. **Same-machine multi-session coordination needs no remote.** Stream I claim-locks, presence, and peer-updates route through the one local daemon — they already do. One laptop running parallel Claude + Codex sessions is fully coordinated with zero network.
2. **The dream lease's job ("only one device dreams a scope per day") is a cross-*device* concern**, trivially satisfied locally with one device.
3. **Every network git call (`fetch_origin`, `push`, sync) is guarded by `origin_remote_configured()`.** Absent a remote: commit locally, coordinate through the daemon, no-op the network step as success. Present a remote: behavior is byte-identical to today's multi-device path.
4. **No required network dependency on any cognition path.** Nightly dreaming, capture, and recall complete fully offline.

This makes `system-v0.2.md` §1.1.6 ("local-first… offline-capable… no cloud backend required") and §1.3.5 ("bring your own remote") genuinely *optional* rather than a hidden precondition for cognition.

---

## 3. Locked product decisions

### 3.1 Capture friction at n=1 — trusted-by-default, gated by substance (not by review queue)

**Decision:** continuity-state writes from `C0` closeout and dream-time maintenance land **active/pinned (trusted)**, not as review-queue candidates. The safety mechanism is the v3.0 **B2 substance/acceptance gate** (`stream-e-ambient-recall-v3.0.md` §4.0), **not** the governance review queue.

Rationale: at one user betting real work, routing every session's continuity snapshot through a hand-approved queue is friction that defeats "remembered, not retrieved." The real risk is *confident-wrongness*, which B2 addresses (a hollow/degraded object never renders as authoritative) without a human in the loop.

**Two distinct "carve-out" axes — do not conflate (review finding).** v3.0 §5.2 says the continuity write is "governed exactly like a pass-2 candidate (no carve-out, B7)." But shipped pass-2 writes land `MemoryStatus::Candidate` in the review queue (`crates/memoryd/src/dream/orchestration.rs:283-285`); Stream F never auto-promotes (`stream-f-dreaming-v0.3.md:187`). So "governed like a pass-2 candidate" read literally **contradicts** §3.1 and v3.0 §4.2's "pinned status." The resolution names two different things both called "carve-out":

- **B7 removed the *governance-check* carve-out** — classification, contradiction-detection, and secret-refusal still apply to the continuity write. Kept.
- **§3.1 *adds* a *promotion* carve-out** — this one write auto-promotes to pinned-active where every other pass-2 write goes to the queue. This is new and must be specified explicitly (F5.2), or a Phase-2 implementer following v3.0 §5.2 will silently dump continuity into the review queue.

The review queue is reserved for low-confidence or contradictory **atomic** memories, not the continuity object. `degraded: true` is the held-back state for a hollow/partial continuity write.

### 3.2 The 552 frozen import memories — let them decay, do not batch-enrich

**Decision:** no one-time enrichment pass over the imported corpus. Let import memories decay under normal salience/recency dynamics while the continuity engine and live writes rebuild salience from use. Entity/summary extraction is worth building for **live** writes; retrofitting it onto the import is low-value.

---

## 4. The fixes (buildable contracts)

Each fix: current behavior (anchored), required behavior, contract/invariants, acceptance tests. Sequenced in §5.

### F1 — Commit-on-write (Stream A substrate)

**Current:** writes never git-commit (§1.1); `auto_commit` (`commit.rs:66-82`) stages `STAGED_NAMESPACES` (`commit.rs:12-23`) and runs `git commit -m <message>` with **no `--author` and no `GIT_AUTHOR_*` env** — so on a host with unset `user.name`/`user.email` the first commit fails ("Please tell me who you are"). Only `commit_lease_file`→`run_lease_commit` (`commit.rs:133-155`) sets a bot identity, hard-scoped to `leases/journal.lease`.

**Required:** the daemon commits substrate writes to the local git repo so the working tree reflects committed state.

**Design — debounced commit worker + identity-setting helper.**

1. **New `commit_substrate_writes(repo, n)` helper** in `git/commit.rs`, modeled on `run_lease_commit`: stages `STAGED_NAMESPACES`, sets `GIT_AUTHOR_NAME`/`GIT_AUTHOR_EMAIL`/`GIT_COMMITTER_*` env to a fixed write-bot identity (`memoryd write-bot <noreply@memoryd.local>`) so it works on an unconfigured-git host, and commits with message `substrate: commit <n> write(s)`. *(This is new machinery — the prior draft's "no new commit machinery / reuse `auto_commit`" claim was wrong; reuse fails on unconfigured git.)*
2. **Fix the latent `sources/` staging bug (review finding):** `sources/` is a real canonical namespace (`crates/memory-substrate/src/tree/layout.rs:32-37,91-92`, `sources/web/**`) but is absent from `STAGED_NAMESPACES`, so web-source writes stay untracked forever. Add `sources/` to `STAGED_NAMESPACES` as part of P0.2.
3. **Commit worker** in `serve_substrate_with`: wakes on a dirty signal sourced from `Substrate::watch()` (`api/mod.rs:132`) or a debounce-tick `git status` poll (implementer names the channel), commits pending changes on a short debounce (config `substrate.commit_debounce_ms`, default **2000**, range `[0, 30000]`), coalescing bursts into one commit.
4. **Two forced flushes:** a **shutdown flush** (clean stop commits pending writes), and a **pre-dream flush** — see the cross-process note below.

**Cross-process pre-dream flush (review finding).** Scheduled/manual dreaming is a **separate `memoryd dream` process** (`crates/memoryd/src/cli/dream.rs:130`, `run_scheduled_dream`), which builds its own `NativeLeaseGit` and never opens a `Substrate`. So the in-daemon commit worker cannot flush for it. The dream process must call the `git::commit_substrate_writes` **free function directly** (not `Substrate::open`, which would re-run reconcile) **before** `acquire_manual_lease_with_git`.

**Cross-process git-lock serialization (review finding).** The daemon commit worker and the dream process's flush + `commit_lease_file` + (when remote) `push` can run concurrently and collide on `.git/index.lock`; a lost race leaves the tree dirty at the Wall-2 guard and misses the run. All substrate git commits (worker, dream flush, lease) acquire a **repo-level advisory file lock** (`flock` on `.memoryd/substrate-git.lock`) before touching the index.

**Contract / invariants:**

- **I-F1.1** Steady-state committed substrate: within `commit_debounce_ms + one commit` of the last write, `git status --porcelain` reports no untracked/modified files under the daemon-managed namespaces — defined as `STAGED_NAMESPACES` (corrected): `me/ projects/ agent/ dreams/ encrypted/ substrate/ events/ tombstones/ policies/ leases/ sources/`. The gitignored `.memoryd/`/`.memorum/` are excluded by design.
- **I-F1.2** Commits use the fixed write-bot identity (works on unconfigured git) and message `substrate: commit <n> write(s)`.
- **I-F1.3** Commit failure never loses a write: `.md` + index + event log remain the durability spine; a failed commit retries on the next tick and surfaces to doctor (F4/D3).
- **I-F1.4** F1 never pushes; push is remote-sync's concern and a no-op with no remote (§2).
- **I-F1.5** All substrate git commits hold the repo-level commit lock; no two committers touch `.git/index` concurrently.

**Acceptance:** `commit_worker_coalesces_burst_into_one_commit`; `commit_succeeds_on_unconfigured_git_identity` (I-F1.2); `sources_web_write_is_tracked_after_commit` (the latent-bug fix); `pre_dream_flush_in_dream_process_leaves_clean_tree` (run against the **scheduled CLI path**, not in-process); `concurrent_worker_and_dream_flush_do_not_corrupt_index` (I-F1.5); `commit_failure_does_not_lose_write_and_surfaces_to_doctor`.

### F2 — Single-device lease & local-first git (Stream F + substrate git)

**Current:** `fetch_origin`/`push` hard-require a remote (§1.2 Wall 1).

**Required:** a shared `origin_remote_configured(repo) -> bool` helper (a `git remote get-url origin` probe, modeled on the private `git_origin_remote` at `crates/memoryd/src/recall/project.rs:145-153`). In `NativeLeaseGit::fetch_origin` and `NativeLeaseGit::push` (`dream/git.rs:75,91`): no origin → `Ok(())`. All local lease logic unchanged — read `leases/journal.lease`, held-check, dirty-tree guard, append, local commit.

**Contract / invariants:**

- **I-F2.1** With a remote, `fetch_origin`/`push` are byte-identical to today (multi-device election preserved).
- **I-F2.2** With no remote, the lease is granted locally; held-check, dirty-tree guard, and local commit still run.
- **I-F2.3** Local held-semantics unchanged: a foreign active lease record still blocks (`lease.rs:125-129`); `--force`/held-bypass are **not** relaxed by no-remote mode — only the network steps no-op.
- **I-F2.4** "No remote by design," not "broken remote": a *configured* origin whose fetch/push fails is still `lease_unavailable` (a real network failure is never silently swallowed).

**Acceptance:** rewrite `dream_lease_election.rs:98` to expect **success** on a local-only repo; add `configured_origin_with_fetch_failure_still_unavailable` (I-F2.4); add `foreign_active_lease_blocks_with_no_remote` (I-F2.3). The end-to-end `local_lease_grants_and_dreams_with_no_remote` requires F1 (a write to dream over must be committed) and is therefore a **P0.2/P0.5** test, not a P0.1 gate (see §5).

### F3 — Merge-conflict recovery & notification lifecycle (Stream A + notifications)

**Current:** `recovery_required` detected, never consumed (§1.5); blocking-conflict notification re-emitted every startup; passive queue append-only, stores only `{ message: String, created_at }` (`passive.rs:13-16`) with the path already flattened to a constant string by `passive_message` (`dispatcher.rs:53`) before append; `passive_notification_id` (`passive.rs:69`) salts its hash with nanosecond `created_at`, so it cannot serve as a content dedup key.

**Required:**

1. **Dedup at the dispatcher on the structured event (review finding).** `NotificationDispatcher::dispatch_event` (`dispatcher.rs:28`) computes a **content-only dedup key** from the structured `NotificationEvent` (e.g. `BlockingMergeConflict{path}` → `"blocking_merge_conflict:<path>"`) *before* flattening to a message. `PassiveNotification` gains an optional `dedup_key: Option<String>`; `append` skips when an entry with the same key is already in the ring. This both stops the 96-duplicate accumulation and preserves the path for clearing.
2. **An explicit re-reconcile trigger for the clear (review finding).** `startup_reconcile_report()` is a one-time `Arc<ReconcileReport>` snapshot at `Substrate::open` (`api/mod.rs:56,66`); nothing recomputes `blocking_conflicts` in the running daemon. The quarantine-resolution CLI (below) is the trigger: resolving a quarantine re-runs a lightweight rescan, recomputes `blocking_conflicts`, and prunes passive entries whose `dedup_key` is no longer present. (Daemon restart also clears, via a fresh reconcile that no longer finds the quarantine.)
3. **Quarantine-resolution CLI.** `memoryd quarantine list` and `memoryd quarantine resolve <id> --edited` (v0.2: side-selection modes deferred — see Revision goal v0.2) lift `status: quarantined` (the `blocking_conflicts` source, `reconcile.rs:497-522`), reindex, re-run the rescan in (2), and clear the notification.
4. **Consume `recovery_required`.** A startup routine reads `recovery_required`/`blocking_conflicts`. On a stranded mid-merge tree (`.git/MERGE_HEAD`) the **default is an explicit operator-action finding, not auto-resolve** (review finding): on a no-remote single-device install the daemon never runs `git merge` (F2 no-ops `fetch_and_merge`), so a real `MERGE_HEAD` can only come from the user's own manual merge and must not be auto-aborted. Auto-resolve is opt-in.

**Contract / invariants:**

- **I-F3.1** A daemon restart does not multiply identical notifications (96 → bounded by dedup key).
- **I-F3.2** `recovery_required == true` has a consumer that emits an operator-action finding (and may auto-resolve only when explicitly opted in); it is never a no-op.
- **I-F3.3** Resolving the last quarantine via the CLI re-runs the rescan and clears the blocking-conflict notification **within the daemon lifetime** (not only on restart).

**Acceptance:** `restart_does_not_duplicate_blocking_conflict_notification`; `recovery_required_emits_operator_finding`; `quarantine_resolve_clears_sync_blocked_without_restart` (I-F3.3).

### F4 — Doctor & observability sees the whole loop (Stream G)

**Current:** doctor's health model covers tree + embedding + auth only (§1.5), and `doctor_response(&Substrate)` (`doctor.rs:5`) cannot reach `HandlerState.recall` where `budget_exhausted_total` lives.

**Required:** change the doctor entry signature to take `&HandlerState` (or thread the recall snapshot in), and add five checks. Each emits a structured, **severity-tagged** finding:

- **D1 dream freshness (advisory)** — read `dream::status::collect_last_runs` (`crates/memoryd/src/dream/status.rs`); finding if `consecutive_missed_runs >= dreams.doctor_missed_threshold` (default **2**) or no successful run in `> 48h`.
- **D2 sync/quarantine (fatal)** — finding if `blocking_conflicts` non-empty or `recovery_required`.
- **D3 uncommitted substrate (fatal)** — finding if daemon-managed files are **untracked/modified for longer than `commit_debounce_ms + substrate.commit_stale_grace_ms`** (default grace **5000**). Measuring *stale* untracked (via pending-commit-queue depth or file mtime), not raw untracked, so D3 does not flap during the normal debounce window. This is F1's self-check.
- **D4 recall budget pressure (advisory)** — finding if `budget_exhausted_total` for any section exceeds a rate threshold over a recent window (the v3.0 P3 gating signal).
- **D5 capture freshness (advisory; owned by v3.0 P2, referenced here)** — once C0 capture is wired, a finding if no `C0` closeout in `dreams.capture_drought_days` (default **3**). **This is the check that prevents the headline drought from silently recurring** after P2.

**Contract / invariants:**

- **I-F4.1** Doctor is never *silently green* when a seam is active: D2 or D3 flips `healthy` to `false` (loop broken); D1/D4/D5 keep `healthy` true but populate a non-empty, rendered `findings` array. Stated as a test matrix: for each seam, construct the broken state and assert doctor is not silently green.
- **I-F4.2** Findings carry a `severity` tag (`fatal` | `advisory`) so D2/D3 and D1/D4/D5 are distinguishable by the TUI/web/CLI.

**Acceptance:** a `doctor_sees_<seam>` test per check (dead-dream advisory, blocking-conflict fatal, stale-uncommitted fatal, budget-pressure advisory, capture-drought advisory) each asserting the finding **and its severity** (I-F4.2); `doctor_foundation_loop_green` — write→commit→dream→observe with no active seam yields `healthy: true` (capture/D5 excluded until P2, see §5).

### F5 — C0 capture wiring & its foundation dependencies (Stream E bridge to v3.0 P2)

**Current:** no closeout hook (§1.3). The hook is specced in `stream-e-ambient-recall-v3.0.md` §5.1; `HOOK_EVENTS` lives in `crates/memoryd/src/setup/unwire.rs:30` and the wire matchers in `hooks_wire.rs:275-285` (both fixed-size `[…; 3]`, so adding SessionEnd changes the arity to `; 4` across three sites — correcting v3.0 §5.1's single-location framing).

**Required (foundation-owned parts only):**

- **F5.1** The `C0` SessionEnd hook depends on **F1** (the continuity write must commit to survive the night) and **F2** (dream-time refine must run). This spec makes those dependencies ordering-binding (§5).
- **F5.2** Per §3.1, the closeout/dream continuity write lands **trusted-active (pinned)** via a **distinct governed supersede path** — *not* `SubstrateCandidateWriter` (which writes `Candidate`/review-queue). Implementers must add a governed supersede that runs the classification/contradiction/secret checks (B7 governance-check carve-out stays removed) and lands pinned (the §3.1 *promotion* carve-out). v3.0 §5.2 and §11 (Stream C) must be amended to name this path on acceptance; until then, building Phase 2 from v3.0 §5.2's "pass-2 candidate" wording would violate §3.1.
- **F5.3** The B2 two-state acceptance test (rich vs. hollow closeout, v3.0 §4.0) **gates** F5: until it passes, `C0` is not wired and T0 reads skeleton + desk only. v3.0 §4.0's "substantive" threshold (still marked open in v3.0 §14.6) must be pinned before C0 ships.

Recollection rendering, ranking, and relevance remain owned by v3.0 (Phases 1/3/4).

---

## 5. Build sequence

P0 is this foundation; P1–P4 are v3.0's existing phases, re-anchored on a foundation that commits, dreams, and observes.

| Phase | Fix / scope | Unblocks | P0 gate |
| --- | --- | --- | --- |
| **P0.1** | **F2** single-device lease | dreaming Wall 1 | `dream_lease_election` no-remote test green (clean-tree lease acquire only) |
| **P0.2** | **F1** commit-on-write (+ `sources/` staging fix) | git-backing real; dreaming Wall 2; F5 durability | `commit_worker_*`, `commit_succeeds_on_unconfigured_git_identity`, `pre_dream_flush_in_dream_process_*` green |
| **P0.3** | **F3** recovery + notification dedup/clear | un-wedge; honest sync state | `quarantine_resolve_clears_sync_blocked_without_restart` green |
| **P0.4** | **F4** doctor visibility | can *observe* the foundation loop is closed | per-seam `doctor_sees_*` + `doctor_foundation_loop_green` |
| **P0.5** | **redeploy ritual** | live daemon stops being stale | **foundation** dogfood gate (§6 steps 1,2,4,5) on `~/memorum` |
| **P1** | v3.0 render/safety/budget | Seam 4 (noise) | v3.0 §15 |
| **P2** | v3.0 continuity engine + **F5** C0 capture + **D5** | Seam 3 (the headline) | v3.0 §4.0 B2 test + F5 + **full-loop** dogfood gate (§6 all 5) |
| **P3** | v3.0 relevance/gating | Seam 4 (recency) | v3.0 §15 |
| **P4** | v3.0 desk + re-entry | orientation anchor | v3.0 §15 |

**Ordering rationale:** F2 before F1 because it is the smaller, independently-testable unblock (clean-tree lease acquisition, no writes needed). F1 immediately after, because dreaming Wall 2 and F5 durability both need committed writes — so the *end-to-end* write→dream test is a P0.2 test, not a P0.1 gate. F4 is last in P0 because its `doctor_foundation_loop_green` asserts the other fixes landed. The full-loop green (including capture) is a **P2** gate, not P0.4 — capture (Seam 3) does not close until F5/v3.0-P2. P0.5 is non-optional: the live daemon binary must be rebuilt and relaunched (launchd), or every fix is invisible.

---

## 6. Dogfood gates

Acceptance is the **recursive §20 condition**: the loop demonstrably runs on the live `~/memorum` single-device no-remote install. Split so the foundation has self-contained acceptance.

**Foundation gate (P0.5 — steps 1, 2, 4, 5; no capture):**

1. **Write** — a session writes a memory; within the debounce, `git -C ~/memorum status` is clean and `git log` shows the write-bot commit. *(F1)*
2. **Dream** — `dream now` (or nightly) acquires the lease locally and runs; `dream_runs_invoked_total` increments, `consecutive_missed_runs == 0`. *(F2 + F1 pre-dream flush)*
4. **Observe** — `memoryd doctor` is green for the foundation loop **and** would have caught each failure (F4 test matrix, not trust). *(F4)*
5. **No-remote throughout** — steps complete with zero network. *(§2)*

**Full-loop gate (P2 — adds step 3):**

3. **Capture** — a second session's `C0` closeout updates the continuity object; the next session's T0 reads it back (remembered, not recent), and D5 would flag a drought. *(F5 / v3.0 P2)*

Until the foundation gate passes on the real install, the loop is not closed regardless of green unit gates — the §20 lesson this spec encodes.

---

## 7. Non-goals

- **Recall rendering/relevance** (v3.0 Phases 1/3/4) — referenced, not re-specified.
- **Import enrichment** — declined (§3.2).
- **Remote sync hardening / multi-device merge** — local-first is the default; remote behavior is preserved when a remote *is* configured, and the no-remote path is newly defined by F2 (B3 stays re-priced per v3.0 §16).
- **A new stream or daemon** — every fix lands in existing crates (`memory-substrate`, `memoryd/dream`, `memoryd/notifications`, `memoryd/handlers/doctor`, `memoryd/cli`).

---

## 8. Open questions / risks

1. **Wedge origin must be identified before F3 ships (review finding).** On a no-remote single-device install no `git merge` should run, so the live quarantine + stranded `MERGE_HEAD` (06-23) most likely came from the **import flow or a crashed reconcile**, not normal operation. F3 un-wedges the current state, but if the origin is not found and fixed the wedge recurs. **Action:** trace the live quarantine's provenance (import reconcile vs. crash) as the first task of P0.3.
2. **Stale local lease after a crashed dream (review finding).** With no remote there is no `fetch_origin` to refresh a stale `leases/journal.lease`; a crashed dream can leave an active record whose `expires_at > now`, blocking the next night's run locally until the lease window expires. Consider a no-remote stale-lease eviction in F2, or accept the ≤`lease_window_seconds` delay.
3. **`dreams/cleanup/<device>/<date>.json` retention.** Once F2 unblocks dreaming these accumulate daily and `previous_missed_runs` does an unbounded `read_dir`. Add a retention/compaction policy in F2 or F4/D1.
4. **Commit debounce vs. crash window.** Between a write and its debounced commit, durability rests on disk + event log (committed); git history lags ≤`commit_debounce_ms`. Acceptable (git is the sync/history layer, not the durability spine); document in the runbook. A crash-then-no-further-write leaves the tree dirty until the next write or the pre-dream flush — D3 (stale-untracked) surfaces it.

**Resolved by this revision (formerly open):** the doctor verdict policy is decided (D2/D3 fatal, D1/D4/D5 advisory — F4/I-F4.1); the `recovery_required` auto-resolve question is decided (operator-finding default, auto-resolve opt-in — F3); the v3.0 §4.2-vs-§5.2 ambiguity is resolved via the promotion-vs-governance carve-out distinction (§3.1, F5.2).

---

## Review revision history

- **2026-06-25 (v0.1, review pass):** Three decorrelated adversarial reviews (plan-reviewer/opus, Cursor, GLM). Changes: F1 introduces an identity-setting `commit_substrate_writes` helper (reuse of `auto_commit` fails on unconfigured git) and fixes the latent `sources/` `STAGED_NAMESPACES` omission; F1 pre-dream flush moved into the dream CLI process with a repo-level git lock (cross-process); F3 dedup moved to the dispatcher on the structured event with an explicit re-reconcile (CLI) trigger and an operator-finding default for `recovery_required`; F4 doctor takes `HandlerState`, D3 measures stale-untracked, D5 capture-freshness added; F5.2 names the promotion carve-out distinctly from B7; P0.1/P0.4 gates and the §6 dogfood gate split from the full loop; anchors corrected (`dirty_user_work_paths` vs `is_substrate_managed_path`, the hook budget constant, `HOOK_EVENTS` in `unwire.rs`). Codex review lane was usage-capped and did not run.
