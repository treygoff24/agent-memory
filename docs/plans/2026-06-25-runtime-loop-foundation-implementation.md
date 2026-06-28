# Runtime-Loop Foundation — Implementation Plan (P0)

**Status:** Reviewed (plan-reviewer + delegate Codex) and patched. **Executed** on branch `foundation/runtime-loop-closure` (Waves A–D + desloppify), then audited and a fix wave applied.

> **Plan revision history**
>
> - **2026-06-28 (post-execution amendment):** All waves shipped. A five-lane audit + a `delegate codex safe` review of the fix wave drove a round of corrections. Contract change: the F3 quarantine-resolution CLI surface was reduced from v0.1's `[--accept-ours|--accept-theirs|--edited]` to `--edited` only (side-selection is un-honorable until a substrate side-swap API preserves both conflict sides) — captured in **spec v0.2** (`docs/specs/memorum-runtime-loop-foundation-v0.2.md`), now the live foundation spec. Also: doctor's D2 unified onto the shared `status OR trust_level` quarantine predicate; the commit worker got a dedicated shutdown channel so the shutdown join cannot hang on an early socket error; `quinn-proto`/`memmap2` bumped for RustSec advisories; a latent `memoryd-tui` break from the desloppify pass (DTO `status` String→`ReviewStatus`) was fixed.

> ## ▶ Session-resume kickoff (read first)
>
> This plan was authored, reviewed twice, and patched in a prior session; the design is locked. To start:
> 1. **Load the `rust-engineer` skill** (you + every native review sub-agent). Read this plan top-to-bottom, then the spec `docs/specs/memorum-runtime-loop-foundation-v0.1.md`, then the memory note `runtime-loop-foundation-diagnosis-and-plan`.
> 2. **Ground-truth live state** (don't trust this doc's line numbers blindly — re-Read before each wave): `git -C /Users/treygoff/Code/agent-memory status` (the spec/plan/explainer are **uncommitted on `main`** — confirm with Trey, then commit them onto the feature branch as the first commit), `git branch`, and the crate layout under `crates/`.
> 3. **Cut the branch:** `foundation/runtime-loop-closure` from `main`. All work lands there.
> 4. **Codex quota:** Codex was capped 2026-06-25 (resets ~06-26 10:38). If a Codex lane errors with a usage cap, substitute per the execution-model note and record it. Probe before assuming.
> 5. **The live daemon is a stale Jun-19 binary** — nothing on `~/memorum` changes until Wave E rebuilds + relaunchd. Back up `~/memorum` before Wave E.
> 6. Start **Wave A (F2)** via the wave protocol below. Honor the checkpoint discipline (execution-log on a >30-min same-root-cause block).

**Goal:** Implement the Phase-0 foundation specified in `docs/specs/memorum-runtime-loop-foundation-v0.1.md` so that, on a single-device no-remote install, the runtime loop *commits writes → dreams locally → recovers from wedges → observes its own health*. This closes Seams 1, 2, and 5 and the foundation half of the loop. **Capture (Seam 3 / F5 / C0) is out of scope** — it ships with the v3.0 continuity engine in a follow-on P2 plan; this plan only lands F5's prerequisites (F1, F2) and reserves its config/finding surfaces.

**Source of truth:** the foundation spec (above). Where this plan and the spec disagree on a contract detail, the spec wins and the drift is a bug to surface. The recall redesign `docs/specs/stream-e-ambient-recall-v3.0.md` is the downstream consumer, not a dependency of this plan.

**Tech stack:** Rust 2021, Tokio, `git2`/shell-git, the `memory-substrate` / `memoryd` crates. No new crate, no new daemon.

---

## Execution model (orchestrator + delegate lanes)

I (Claude) am the **orchestrator and code reviewer**, never the primary implementer. All implementation is delegated:

- **Implementation:** `delegate codex work` is the default lane (design discipline, best failure reporting; Trey's Pro plan makes it ~free). `delegate cursor work` (Composer) for fast/mechanical sweeps and for **fixing review findings**.
- **Review (every wave):** three decorrelated reads in parallel — (a) `delegate codex safe`, (b) a **native sub-agent with the `rust-engineer` skill loaded**, and (c) my own read with `rust-engineer` loaded. I ground-truth and adjudicate findings.
- **Fixing review findings:** `delegate cursor work` in file-disjoint lanes (orchestrator partitions per the `orchestrator` skill's file-ownership rules).
- **rust-engineer is mandatory:** every native sub-agent's prompt loads `rust-engineer`; I load it for my own reviews. Delegate Codex/Cursor lanes are instructed to follow idiomatic-Rust discipline (ownership/lifetimes/async-tokio correctness, `Result`/error-typing, no `unwrap` on fallible paths, no defensive slop).
- **Codex availability note:** Codex was usage-capped on 2026-06-25 (resets 06-26 ~10:38). At execution time it is expected available. If any Codex lane is unavailable, substitute Cursor (implementation/fix) or a `delegate droid glm`/native Opus sub-agent (review), and record the substitution. **Codex is load-bearing in four roles — default implementer, one review lane, both desloppify reviewers, the desloppify fixer — so a cap cascades.** To degrade gracefully, mechanical waves (notably Wave D's TypeScript/web finding-rendering) default to **Cursor** for implementation; Codex is reserved for the design-heavy waves (B, C) and the review lanes.

I own: scope, final integration writes, `Cargo.lock` merges, all gates, review adjudication, the live-store redeploy, and the closeout sequence. Workers touch `Cargo.toml` only; never `Cargo.lock`.

---

## Repository & branch strategy

- **One feature branch:** `foundation/runtime-loop-closure`, cut from `main` at start. All work lands here. `main` stays untouched until Trey approves a merge (not in this plan's scope — this plan ends at "ready to merge").
- **Per-wave isolation:** each wave's implementation runs in a `delegate … work --isolation worktree --forbid-commit` run off the current feature-branch tip, returning a branch + dirty diff for review. After review + fixes + the wave gate, I integrate (fast-forward/merge) the wave onto the feature branch myself. Sequential waves → no cross-wave file collisions except shared config (orchestrator-owned, §below).
- **Gate discipline (per `CLAUDE.md`):** workers/waves run only the **narrow** gate (`cargo test -p <crate> --tests`, `cargo clippy -p <crate> --all-targets -- -D warnings`, `cargo fmt -p <crate> -- --check`). The full `bash scripts/check.sh` runs **at the orchestrator, on the integrated feature branch**, after each wave merges — never inside a worktree (stub modules would fail for the wrong reason). `scripts/check.sh` already bakes the macOS `CARGO_TARGET_DIR=$(mktemp -d)` syspolicyd workaround.
- **Cargo.lock:** orchestrator-merged with `cargo build --workspace --locked` + targeted `cargo update -p <crate>`; never `cargo generate-lockfile`.
- **Shared-file ownership:** the synced config types at `crates/memory-substrate/src/config/mod.rs` (`SyncedConfig`, `DreamsConfig`) are touched by Wave B (adds a `substrate:` section with `commit_debounce_ms`) and Wave D (adds `commit_stale_grace_ms`, plus `DreamsConfig.doctor_missed_threshold` and reserved `capture_drought_days`). Because waves are **sequential**, this is not a concurrency hazard, but the orchestrator owns the final config-file integration. **Explicit cross-wave dependency:** Wave D's D3 stale threshold is `commit_debounce_ms + commit_stale_grace_ms`, so D3 reads Wave B's key — the B-before-D order satisfies this, but D3 must be authored against a key that already exists. The synchronous `origin_remote_configured()` helper is created in Wave A; within this plan only F2 calls it (F1 never pushes), so it has no second P0 caller — broader reuse is future remote-sync work.
- **Do NOT:** run `cargo test --workspace` inside a worktree; `git pull` `agentlinters`; overwrite `bench/baseline.*.json` programmatically; bump any spec/plan version without Trey's ask.

---

## Non-negotiable invariants (will fail review if violated)

From the spec's contracts plus the repo's standing invariants (`CLAUDE.md`):

1. **F1 must not regress two-clone canonical-content convergence** (spec §13.6.1) or the merge driver. Committing daemon writes changes *when* git sees content, not *what* converges.
2. **`MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION` stays the single source of truth** for the merge driver's schema gate (F3 touches quarantine/merge).
3. **`secret` is never persisted; every write carries a `ClassificationOutcome`.** F1's commit path commits already-written files; it introduces no new write that bypasses classification.
4. **Device IDs stay in local runtime state**, never in synced `config.yaml` (F1/F2 git work must not stage device identity).
5. **Performance baselines** (`bench/baseline.*.json`) are human-commit-only; no wave or the desloppify pass overwrites them.
6. **Local-first (spec §2):** every network git call **on a runtime path** is guarded by `origin_remote_configured()`; absent a remote, all cognition completes with zero network. (Scope note: the `memory-substrate::git::sync` APIs — `fetch_and_merge`/`push`/`fetch_origin`/`fetch_inspect`, `git/sync.rs:50-142` — have **no daemon caller** today, so they are not on a runtime path; they receive the same guard when remote-sync ships, out of P0 scope.) A *configured* remote that fails still errors (I-F2.4) — never silently swallowed.
7. **Doctor honesty (I-F4.1):** a cut seam is never silently green.

---

## The wave protocol (applied to every wave A–D)

Each wave runs this loop. It is the spine; per-wave sections below only specify the deltas (owned files, contract, gate, lane choice).

1. **Brief.** I write a bounded delegate prompt: the spec contract (the `F-X` required behavior + `I-Fx.y` invariants + named acceptance tests), the **exact owned files**, **forbidden files**, the narrow gate the lane may self-run, idiomatic-Rust discipline, and the report format. Launch `delegate codex work --isolation worktree --forbid-commit` (or Cursor for mechanical waves).
2. **Verify on disk.** Never trust the lane's "done." I Read the changed files and confirm the edits landed, then finish any trailing call-site updates / test additions the lane cut off (a known Codex/opus failure mode per `CLAUDE.md`).
3. **Review — three decorrelated reads in parallel:**
   - `delegate codex safe` over the wave diff (correctness, failure modes, missed call sites);
   - a **native sub-agent (`rust-engineer` loaded)** over the wave diff (ownership/lifetimes/async/error-typing/clean-code);
   - my own `rust-engineer` read.
   Each gets the diff + the spec contract + a checklist (does it satisfy every `I-Fx.y`? every named acceptance test present and real? any invariant from the list above touched?).
4. **Adjudicate + fix.** Ground-truth each finding against source (finders report diff-offset lines — confirm real location). Partition into **file-disjoint** Cursor `work` fix-lanes; serialize any that share a file into sub-waves. Re-review the one or two highest-stakes fixes with a second engine.
5. **Narrow gate.** `cargo test -p <crate> --tests --no-fail-fast` + `clippy -D warnings` + `fmt --check` + `cargo doc` (rustdoc) for the touched crates. (Per the `refactor-wave-gate-gap` lesson: include fmt + rustdoc per wave so they don't all surface at the final `check.sh`.)
6. **Integrate + full gate.** Merge the wave onto `foundation/runtime-loop-closure`; orchestrator-merge `Cargo.lock`; run `bash scripts/check.sh` on the integrated branch. Green = wave done.
7. **Checkpoint discipline.** If a wave is blocked on the same root cause >30 min, write `docs/plans/2026-06-25-runtime-loop-foundation-implementation-execution-log.md` (blocker, what was tried, what would unblock) before any further retry. Different root cause each cycle = progress; same one = stop and surface. After 15–20 min spinning on structurally-similar hypotheses, escalate to a fresh `delegate codex` rescue with different inductive bias.

---

## Waves

Order follows spec §5: F2 → F1 → F3 → F4. F2 first (smallest independent unblock); F1 immediately after (the keystone the rest need).

### Wave A — F2: single-device lease & local-first git

- **Lane:** `delegate codex work` (state-machine/git logic — Codex strength). Small, ~1 file + tests.
- **Owned files:** `crates/memoryd/src/dream/git.rs` (the `fetch_origin`/`push` guards), a **new synchronous** `origin_remote_configured(repo: &Path) -> Result<bool, LeaseError>` built on `run_git`/`std::process::Command` (modeled on the async probe `git_origin_remote` at `recall/project.rs:145-153` but **not extracted from it** — `fetch_origin`/`push` are sync trait methods, so an async helper would hit the async-in-sync wall or block a runtime thread). It returns `Ok(false)` **only** for the specific "no origin remote" case; a probe/config failure is `Err` (don't collapse corrupt git config into "no remote" — that would silently swallow a real failure, violating I-F2.4). `crates/memoryd/tests/dream_lease_election.rs`.
- **Contract:** I-F2.1–I-F2.4. `fetch_origin`/`push` no-op-and-succeed when no origin; byte-identical with a remote; held-semantics and `--force` unchanged; a configured-but-failing remote still `lease_unavailable`.
- **Stale-lease decision (spec §8.2):** with no remote there is no `fetch_origin` to refresh a stale `leases/journal.lease`, so a crashed dream can block the next run for up to `lease_window_seconds` (3600s) — and this bites Wave E if a `launchctl` restart interrupts an in-flight dream. Wave A implements a **no-remote stale-lease eviction** (evict a self-owned expired/abandoned record before acquiring) so the dogfood gate can't wedge; it's cheap and removes the §8.2 risk.
- **Acceptance tests:** rewrite `dream_lease_election.rs:98` (`unavailable_fetch_without_origin_…`) to expect **success**; add `configured_origin_with_fetch_failure_still_unavailable`; add `foreign_active_lease_blocks_with_no_remote`; add/confirm a **with-remote** case asserts byte-identical fetch/push behavior (I-F2.1 — the rewrite touches only the no-origin case). The end-to-end `local_lease_grants_and_dreams_with_no_remote` is **deferred to Wave B** (needs a committed write to dream over) and lands in this Wave-A-owned test file — a sequential cross-wave touch, noted so Wave B's worker is briefed to add it.
- **Narrow gate:** `cargo test -p memoryd --test dream_lease_election --no-fail-fast` + clippy + fmt.
- **Guardrail:** does not relax held-bypass; only network steps no-op.

### Wave B — F1: commit-on-write (the keystone)

- **Lane:** `delegate codex work` — the highest-complexity wave (cross-process, concurrency, git). Pair the review with a **native Opus** sub-agent (not sonnet) given the stakes.
- **Owned files:** `crates/memory-substrate/src/git/commit.rs` (new `commit_substrate_writes(repo, n)` with write-bot identity via `GIT_AUTHOR_*`/`GIT_COMMITTER_*` env, modeled on `run_lease_commit`; **add `sources/` to `STAGED_NAMESPACES`** — the latent-bug fix), `crates/memoryd/src/server.rs` (the debounced commit worker in `serve_substrate_with`), `crates/memoryd/src/cli/dream.rs` (pre-dream flush — see below), **`crates/memoryd/src/dream/git.rs`** (wrap the lease-commit call site at `:84` in the git lock — this file is Wave A's, but Wave B must touch this one site to satisfy I-F1.5; sequential, no concurrency hazard, accounting made explicit here), a repo-level `flock` lock module **in the `memoryd` crate** (e.g. `crates/memoryd/src/substrate_git_lock.rs` on `.memoryd/substrate-git.lock`) reachable by `server.rs`, `cli/dream.rs`, and `dream/git.rs` — `commit_lease_file` itself stays in `memory-substrate` and is wrapped at its memoryd call site. Config key `substrate.commit_debounce_ms` (default 2000, range [0,30000]) in `crates/memory-substrate/src/config/mod.rs`.
- **Dream flush — pre AND post, both entrypoints (review findings):** wire flushes into **both** `run_scheduled_dream` (`cli/dream.rs:130`) and `run_manual_dream` (`cli/dream.rs:68`, the `dream now` path Wave E uses). (a) **Pre-dream flush** before `acquire_manual_lease` (clears Wall 2 — a flush only in the scheduled path lets `dream now` trip the guard). (b) **Post-dream flush** after the dream run, in a finally-style block **before lease release** (on success *and* error), because the dream's own pass-2 candidate writes (`dream/orchestration.rs:248-263`) otherwise return uncommitted from a standalone `dream now`. Both call the `git::commit_substrate_writes` free function (**not** `Substrate::open`) under the git lock.
- **Concurrency & commit scope (review findings):**
  - **Lease-file race:** `commit_substrate_writes` must **exclude `leases/journal.lease`** from its staging (it is in `STAGED_NAMESPACES`, but the lease journal is appended *before* `commit_lease_file` at `lease.rs:137-143`, so a broad flush could commit a half-written lease record mid-transaction). The lease file is committed **only** by the dedicated `commit_lease_file` path; lease append→commit→release is one transaction under the git lock.
  - **Dirty-signal source:** do **not** use `Substrate::watch()` for the write trigger — it self-suppresses the daemon's own writes (`watcher/subscription.rs:100-103`), i.e. exactly the writes F1 must commit. Use a debounce-tick `git status --porcelain` poll (suppression-immune) or the event-log subscription; name the choice.
  - **No Tokio blocking:** `flock` + git subprocesses + the (blocking `std::sync::mpsc`) watcher recv run on a dedicated `std::thread` or via `spawn_blocking` — never a blocking loop inside `tokio::spawn` on the async `serve_substrate_with` path.
  - **Config `PartialEq` trap:** `DreamsConfig` has a manual `PartialEq` (`config/mod.rs:158-177`); every new config field must be added to `PartialEq`, `Default`, validation, and the `config_loading` default/range tests, or equality silently lies.
- **Contract:** I-F1.1–I-F1.5. Steady-state clean tree over the corrected `STAGED_NAMESPACES` (incl. `sources/`, minus `leases/journal.lease`); write-bot identity works on unconfigured git; commit failure never loses a write and surfaces to doctor; no push; **all committers — worker, both dream flushes (pre+post), and the lease commit — hold the git lock** (the review checklist asserts every call site acquires it).
- **Acceptance tests:** `commit_worker_coalesces_burst_into_one_commit`; `commit_succeeds_on_unconfigured_git_identity`; `sources_web_write_is_tracked_after_commit`; `pre_dream_flush_in_scheduled_dream_leaves_clean_tree` **and** `pre_dream_flush_in_manual_dream_leaves_clean_tree`; `post_dream_flush_commits_candidate_writes_before_return`; `partial_dream_writes_committed_before_release_on_error`; `broad_flush_between_lease_append_and_commit_does_not_corrupt_lease` (the Blocker-1 race); `concurrent_worker_and_dream_flush_do_not_corrupt_index` (I-F1.5); `commit_worker_never_pushes` (I-F1.4); `commit_failure_does_not_lose_write_and_surfaces_to_doctor`. Plus the now-unblocked `local_lease_grants_and_dreams_with_no_remote` (F2 e2e).
- **Narrow gate:** `cargo test -p memory-substrate -p memoryd --tests --no-fail-fast` + clippy + fmt + rustdoc.
- **Guardrails:** **invariant 1** (two-clone convergence) and **invariant 4** (device IDs never staged) — the review explicitly checks the commit stages only `STAGED_NAMESPACES` and never device-identity files. Dirty-signal source named (`Substrate::watch()` or debounce-tick poll) — no invented channel.

### Wave C — F3: merge-conflict recovery & notification lifecycle

- **Pre-work (orchestrator, read-only):** **trace the live wedge origin** (spec §8.1) before any code — a native `Explore` sub-agent reads the import/reconcile path to determine whether the 06-23 quarantine came from the import flow or a crashed reconcile, **and reports how many distinct paths are quarantined** (this sizes the dedup design below). If the origin is an import/reconcile *correctness* bug, Wave C grows to fix it — note this is a quarantine/merge-correctness fix, **distinct from** the declined import *enrichment* (spec §3.2); they are different concerns and the wedge-origin fix is in scope.
- **Lane:** `delegate codex work` (notification/reconcile state-machine).
- **Owned files:** `crates/memoryd/src/notifications/passive.rs` (add `dedup_key: Option<String>`; skip-on-duplicate append; a clear-by-key path), `crates/memoryd/src/notifications/dispatcher.rs` (compute the content-only dedup key from the structured `NotificationEvent` *before* `passive_message` flattening), the quarantine-resolution CLI (`memoryd quarantine list|resolve <id> --edited`; the v0.1 `--accept-ours|--accept-theirs` side-selection modes were reduced out during execution — see spec v0.2) and its handler, the `recovery_required` consumer (operator-finding default; auto-resolve opt-in), and the lightweight rescan that recomputes `blocking_conflicts` and prunes the queue.
- **Dedup must collapse the user-visible symptom (review finding):** `passive_message` (`dispatcher.rs:53`) flattens every `BlockingMergeConflict{path}` to the identical string. A per-path dedup key keeps N entries that all render as the same line, so N distinct quarantined paths still show N identical messages — passing `restart_does_not_duplicate_*` on a technicality while the symptom persists. Per the pre-work path count: **one path re-emitted** → per-path dedup is correct; **N distinct paths** → aggregate to a single "Sync is blocked by N merge conflict(s)" notification **or** include the path in `passive_message` so distinct paths render distinctly. The test asserts the **user-visible entry count**, not per-key non-duplication.
- **Contract:** I-F3.1–I-F3.3. Restart does not multiply user-visible notifications; `recovery_required` has a non-no-op consumer; resolving the last quarantine clears the notice **within the daemon lifetime** (the CLI is the re-reconcile trigger).
- **Acceptance tests:** `restart_does_not_duplicate_blocking_conflict_notification` (asserts user-visible entry count); `recovery_required_emits_operator_finding`; `quarantine_resolve_clears_sync_blocked_without_restart`.
- **Narrow gate:** `cargo test -p memoryd --tests --no-fail-fast` + clippy + fmt + rustdoc.
- **Guardrail:** **invariant 2** (`MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION`) — quarantine resolution must not bypass the schema gate.

### Wave D — F4: doctor sees the loop (the integration check)

- **Lane:** `delegate codex work` for the doctor logic + DTO (Rust-only — see scope note).
- **Owned files:** `crates/memoryd/src/handlers/doctor.rs` (signature → `&HandlerState` so it can read `recall` counters; D1–D4 checks + severity tags), `crates/memoryd/src/handlers/mod.rs` (the **single** caller at `:267`, inside `dispatch` which already has `state: &HandlerState` in scope — a one-line, single-caller change), the `DoctorFinding` DTO (`crates/memoryd/src/protocol.rs:895-900` — add an **additive** `severity` field), config keys in `crates/memory-substrate/src/config/mod.rs` (`DreamsConfig.doctor_missed_threshold`=2, `DreamsConfig.doctor_budget_exhausted_threshold`=500, `substrate.commit_stale_grace_ms`=5000, and reserve `DreamsConfig.capture_drought_days`=3 for D5 — update the manual `DreamsConfig` `PartialEq`/`Default`/validation per the Wave B config-trap note).
- **Scope note — CLI/protocol only (review finding):** there is **no** web/TUI doctor surface today (no `/api/doctor` route, no `DoctorResponse` TS type — Codex confirmed). The foundation gate (Wave E) observes via the `memoryd doctor` CLI, whose JSON already serializes findings. So Wave D adds the D1–D4 logic + the additive `severity` field; it builds **no** new web/TUI view (future work consumes `severity` when it ships). Rust-only, no frontend slice — this avoids building a surface the foundation doesn't need.
- **D4 spec (review finding):** `SharedRecallCounters` stores only cumulative totals (`recall/counters.rs`), so P0 D4 is **cumulative-since-daemon-start**: an advisory finding if `budget_exhausted_total` for any section exceeds `doctor_budget_exhausted_threshold` (default 500). No ring-buffer/rate-window machinery in P0 (deferred if dogfood shows it too coarse).
- **Scope note on D5:** D1–D4 are implemented fully. **D5 (capture-freshness) is deferred to the v3.0-P2 plan** because it requires C0 capture to be meaningful — this wave only **reserves** the `capture_drought_days` config key and the finding type, so P2 wires the trigger without a schema change.
- **Contract:** I-F4.1 (D2/D3 fatal, D1/D4 advisory; a cut seam is never silently green), I-F4.2 (severity-tagged findings).
- **Acceptance tests:** `doctor_sees_dead_dream` (advisory), `doctor_sees_blocking_conflict` (fatal), `doctor_sees_stale_uncommitted` (fatal), `doctor_sees_budget_pressure` (advisory, cumulative threshold) — each asserting the finding **and its severity**; `doctor_foundation_loop_green` (write→commit→dream→observe with no seam yields `healthy:true`, capture/D5 excluded).
- **Narrow gate:** `cargo test -p memoryd -p memory-substrate --tests` + clippy + fmt + rustdoc. (No frontend gate — no web doctor surface is built; see scope note.)
- **Guardrail:** D3 measures **stale** untracked (older than `commit_debounce_ms + commit_stale_grace_ms`), not raw untracked — it must not flap during the normal debounce window.

---

## Wave E — Redeploy & foundation dogfood gate (orchestrator-owned, on the live install)

Operational, not delegated; mutates the live `~/memorum` store and the running daemon — handle with care.

1. **Back up first:** snapshot `~/memorum` (tar) and capture `git -C ~/memorum status`, `launchctl list | grep memorum`, and `memoryd status` before touching anything.
2. **Rebuild + redeploy:** build the feature branch's `memoryd` to the install path; `launchctl` stop/start the daemon (manage via `launchctl`, not the pid file). Confirm the new binary mtime > the old Jun-19 one.
3. **Run the foundation dogfood gate (spec §6 steps 1, 2, 4, 5 — no capture):**
   - **Write** a test memory → within debounce, `git -C ~/memorum status` clean and the write-bot commit in the log.
   - **Dream:** `memoryd dream now` (with `--force`, or after clearing any stale `leases/journal.lease` per Wave A's eviction, so a `launchctl`-interrupted prior run can't block it) acquires the lease locally and runs; `dream_runs_invoked_total` increments, `consecutive_missed_runs == 0`.
   - **Observe:** `memoryd doctor` green for the foundation loop, and the F4 test matrix proves it would have caught each failure.
   - **No-remote throughout:** all of it with zero network.
4. **If the gate fails on the live store**, that is the real acceptance bar (§20 recursive condition) — fix forward; green unit gates do not substitute.

---

## Closeout sequence (after all waves + Wave E pass)

Per Trey's directive. All on `foundation/runtime-loop-closure`.

1. **Build-complete commit.** With every wave integrated, all reviews resolved, `scripts/check.sh` green, and the foundation dogfood gate passed, ensure the branch is in a clean committed state (the per-wave integration commits already exist; this confirms the final pre-desloppify state). Commit message ends with the `Claude-Session:` trailer per `CLAUDE.md`.
2. **Desloppify pass.** Run the `desloppify-loop` workflow on the feature branch (8 axes sequentially, loop until a round produces no git changes). Honor `.oxfmtignore` (hand-authored prose/tooling dirs need explicit entries — `oxfmt` does not read `.gitignore`).
3. **Commit the desloppify changes** onto the same branch (separate commit, so the cleanup diff is reviewable in isolation).
4. **Regression gate.** Run the **full** `bash scripts/check.sh` post-desloppify (not the narrow per-wave gate) — the `refactor-wave-gate-gap` lesson: a desloppify pass can disturb fmt/oxfmt/rustdoc/two-clone/durability/bench that per-axis checks miss. Also re-run the foundation dogfood gate (Wave E step 3) to confirm no behavior regression. **No regressions is a hard requirement.**
5. **Desloppify review — Codex.** Launch **two** `delegate codex safe` sub-agents over the desloppify diff specifically: one for behavior-preservation (did any cleanup change semantics — error paths swallowed, a guard removed, a default changed?), one for correctness/clean-code quality of the result. Ground-truth their findings.
6. **Desloppify fixes — Codex.** Any real findings are fixed by `delegate codex work` sub-agents (file-disjoint lanes), re-reviewed, and re-gated (`scripts/check.sh` + dogfood gate).
7. **Final commit.** When built, deslopped, regression-free, and review-clean, the final commit. If step 6 found nothing to fix (step 3 already committed the desloppify diff), this commit may be unnecessary — do **not** manufacture a hollow commit; the branch is simply already final. Branch is then **ready to merge** (the merge itself awaits Trey's go).

---

## Out of scope

- **F5 / C0 capture / the continuity engine** — v3.0-P2 follow-on plan. This plan lands F5's prerequisites (F1, F2) and reserves D5's config/finding surface.
- **Recall rendering/relevance** (v3.0 P1/P3/P4).
- **Import enrichment** (decided: let the import decay).
- **The `main` merge** — this plan ends at "ready to merge."

---

## Risks & open items

1. **Wedge-origin (Wave C pre-work)** may reveal the quarantine came from the import flow — if so, Wave C grows to fix the import-side *correctness* cause (distinct from the declined import *enrichment*, §3.2), not just the recovery. Sized after the trace, which also reports the distinct-path count that decides the dedup shape.
2. **Two-process git contention (F1)** is the subtlest correctness risk; the `flock` + the `concurrent_worker_and_dream_flush_do_not_corrupt_index` test are the guard, and Wave B gets the heaviest (Opus + Codex) review.
3. **Live redeploy (Wave E)** mutates Trey's real dogfood store — backup first; the daemon is managed via `launchctl`, and `claude` dream-auth has a known profile gotcha (use the dogfooding-live-setup memory note).
4. **Codex availability** — if still capped at a review/fix step, substitute per the execution-model note and record it.
5. **Bench non-determinism** — `stream_a_bench` ignores its `--seed`; a cold-reindex "regression" in the post-desloppify gate is usually environmental, not code (per the `bench-corpus-nondeterministic` note). Don't chase a flaky bench delta as a real regression; re-run interleaved.
6. **`dreams/cleanup/` retention (spec §8.3)** — once F2 unblocks dreaming, one JSON per device per day accumulates and `previous_missed_runs` does an unbounded `read_dir`. Low-stakes at dogfood scale; track as a follow-up (a retention/compaction policy in F2's cleanup-write or F4's D1 read path) rather than blocking P0.
