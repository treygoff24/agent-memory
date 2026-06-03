# Handoff — 2026-06-03 18:44 EDT — index-first refactor + codebase-excellence campaign

## TL;DR (read this first)

We are mid-campaign to make this "the best codebase we've ever made" (Trey's words —
excellence, pre-existing debt included, time/tokens not a constraint). The campaign has
6 phases tracked as tasks #1–6 (see the in-session task list / "Campaign plan" below).

**Right now we are finishing Phase 1: landing the in-flight index-first read refactor.**
The refactor was already written (pre-session, uncommitted) and turned out to have two
real correctness bugs that an Opus + Codex review pair caught. All fixes + 3 regression
tests are written. As of this handoff a **targeted validation** (boundary lint + substrate
test suite) was running in the background (task `bmsor1bu8`); the previous full gate
(gate-5) was green on everything EXCEPT a project-specific lint that the fixes now address.

**NOT yet committed.** Resume by confirming validation green, then committing (2 commits,
messages drafted below), then move to Phase 2 (audit fan-out).

Branch: `onboarding/agent-driven-onboarding`  •  HEAD: `dc02f07` (desloppify style commit).
`main` is fast-forward-only; do not commit to main.

---

## What this session did

1. **Oriented:** the "deslop workflow" the session opened on was already done & committed
   (commits `8bd86a0`..`dc02f07`: desloppify rounds 1–4 + oxfmt/rustfmt). The large
   uncommitted working tree is a *different* thing: an **index-first read-path refactor**
   (B-API-7 + Stream G/I caller rewiring + new `events_read` mirror module + 2 new tests).

2. **Gated it** — found & fixed pre-existing/new gate failures in order:
   - `daemon.rs` `&PathBuf`→`&Path` (clippy::ptr_arg, pre-existing debt).
   - rustdoc private-intra-doc-link in `count_recall_index` doc.

3. **Dual review (Opus subagent + `delegate codex safe`)** of the refactor. Both confirmed
   the two load-bearing invariants hold (no encrypted content via plaintext read path; SQL
   fully parameterized). Codex went harder and caught two real blockers Opus under-weighted.
   I verified both against the actual code before acting.

4. **Fixed everything (11 edits across 4 files) + wrote 3 regression tests.** Details below.

5. **Validation iterations:** gates 1→5. gate-5 ran the full suite; all 5 new tests passed;
   only failure was the `rust_boundary_check` lint (raw `.expect()` in my new `src/` test
   modules). Fixed by converting those to a `must()` helper. Targeted re-validation running.

---

## The fixes (all applied, in working tree, uncommitted)

| # | What | File | Status |
|---|------|------|--------|
| Blocker 1 | `read_memory` now verifies `frontmatter.id` and falls through to the disk-walk on a stale index (file missing OR id mismatch). Pre-fix it could return the WRONG memory or a spurious `Io` error where the old disk-walk returned `NotFound` (a trust-artifact caller at `handlers/mod.rs:974` turns that into a bogus *retryable* error). | `crates/memory-substrate/src/api.rs` (`read_memory_with_hash`) | done |
| Blocker 2 | Event-seq reuse, scoped to `DurabilityTier::BestEffort`: best-effort recording paths (`record_event_best_effort`, `record_recall_hits`) now call a new `guard_event_sequence_state(&device)` that does the full `sync_event_sequence_state` reconcile in the BestEffort tier (where the in-memory `best_effort_event_seq` counter and `reserve_event_sequence` can diverge and reuse a seq) and the cheap `ensure_event_sequence_state` in durable tiers (perf hot path preserved). | `api.rs` | done |
| R3 | `resolve_memory_id_to_path_opt`: poisoned lock / failed lookup now `tracing::warn!` instead of silently degrading every read to an O(n) disk-walk. Normal "not indexed yet" stays quiet. | `api.rs` | done |
| R4 | `events_read` mirror rows: `mirror_event_from_row` is now STRICT (negative seq / unparseable ts = corruption → error, not silent 0/epoch); new `collect_mirror_events` skips-and-warns per bad row so one forward-skew row can't abort a whole dashboard page. Real SQLite cursor errors still propagate. | `crates/memory-substrate/src/index/events_read.rs` | done |
| R5 | Peer claim-lock conflict detection: index failure now `tracing::warn!` instead of silently returning "no conflicts" (fail-open behavior kept — failing a heartbeat on a transient index hiccup is worse — but now visible). | `crates/memoryd/src/handlers/peer.rs` (`conflicting_claim_locks_for_heartbeat`) | done |
| Codex risk 3 | Added `idx_events_log_kind_ts ON events_log(kind, ts)` — the `(kind, memory_id, ts)` index can't serve `WHERE kind=? AND ts>? ORDER BY ts DESC` (recent_recall_hits) or `MAX(ts) WHERE kind=?` (latest_ts_for_kind). Corrected the 3 over-claiming index comments. | `schema.rs`, `query.rs`, `events_read.rs` | done |
| Nit | Deterministic `event_id` ordering tiebreaker on mirror page/window queries (mirror can hold multiple devices with identical ts/seq). | `events_read.rs` | done |
| Gate | rustdoc private-intra-doc-link fix; daemon.rs `&Path` clippy fix. | `query.rs`, `daemon.rs` | done |

**3 regression tests written (all passed in gate-5):**
- `crates/memory-substrate/tests/read_memory_stale_index.rs` (NEW): blocker-1, both halves
  (moved file → disk-walk fallback finds it; index hit holding a different id → `NotFound`).
- `crates/memory-substrate/src/events/sequence.rs` `#[cfg(test)] mod tests`: blocker-2
  (`sync` reconciles a stale state file vs log high-water; `ensure` trusts it; reserve→11 no reuse).
- `crates/memory-substrate/src/index/events_read.rs` `#[cfg(test)] mod tests`: R4
  (skip-and-warn on unknown-kind/bad-ts/negative-seq rows; all-valid returns everything).

---

## RESUME HERE — exact next steps

1. **Check validation** (background task `bmsor1bu8`, read its output file): expect
   `BOUNDARY_OK`, substrate tests all pass, `VALIDATION_DONE`. If anything red, fix it.
   (gate-5 was green except the boundary lint, which the `must()` conversion fixes; the
   `must()` changes are test-only and can't affect production behavior.)

2. **Optional belt-and-suspenders:** rerun the full gate to be 100% before commit:
   ```bash
   cd /Users/treygoff/Code/agent-memory
   export CARGO_TARGET_DIR=/tmp/memorum-gate-target MEMORUM_CHECK_KEEP_TARGET=1
   bash scripts/check.sh > /tmp/memorum-gate-6.log 2>&1; echo "GATE_EXIT=$?" >> /tmp/memorum-gate-6.log
   ```
   The persistent warm target dir `/tmp/memorum-gate-target` makes this incremental/fast.
   Always read the `GATE_EXIT=` line in the log (the background notifier reports the trailing
   echo's exit, not the gate's).

3. **Clean up the transient review dir** (untracked, used to feed Codex the frozen diff):
   ```bash
   rm -rf /Users/treygoff/Code/agent-memory/.review
   ```

4. **Commit — two commits, by explicit path (NEVER `git add -A`):**

   Commit A (pre-existing debt, standalone):
   ```
   fix(memoryd): take &Path in auto_start_daemon (clippy::ptr_arg)

   The cold full-workspace clippy gate (-D warnings) flagged auto_start_daemon's three
   &PathBuf parameters; the helpers it forwards to (spawn_serve_child, await_socket_ready)
   already take &Path. Pre-existing debt, unrelated to the index-first work.
   ```
   Stage: `git add crates/memoryd/src/cli/daemon.rs`

   Commit B (the refactor + review hardening + tests). Subject ≤72 chars:
   ```
   perf(substrate): index-first reads + Opus/Codex review hardening

   Resolve reads from the SQLite index / events_log mirror instead of disk-walks and
   per-call Connection::open: read_memory does a PK lookup on memories.id and reads one
   file (B-API-7); new index-served count/projection APIs serve Stream G/I callers; a new
   events_read module pages the events_log mirror without a full JSONL parse.

   Hardened per a dual Opus + Codex review before landing:

   - read_memory verifies frontmatter.id and falls through to the disk-walk on a stale
   index (missing file or id mismatch), preserving legacy NotFound/plaintext-only semantics
   instead of returning the wrong memory or a spurious Io error.

   - best-effort event recording reconciles the persisted high-water (sync) in the
   BestEffort durability tier, where the in-memory counter and reserve() can otherwise
   reuse a seq; durable tiers keep the cheap guard.

   - events_log mirror rows parse strictly and skip-and-warn per row so one forward-skew
   row can't break a dashboard page; poisoned-lock and claim-lock index failures now warn
   instead of degrading silently; added idx_events_log_kind_ts for the recall-hit/latest-ts
   queries; deterministic event_id ordering tiebreaker.

   Regression tests: stale-index read parity, seq reconcile-vs-trust, mirror skip-and-warn.
   ```
   Stage the rest by name (34 modified + 2 new). List them with
   `git diff HEAD --name-only`, then `git add <each>`, plus the two new files
   `crates/memory-substrate/src/index/events_read.rs` and
   `crates/memory-substrate/tests/read_memory_stale_index.rs`. Use a HEREDOC for the body;
   do NOT hard-wrap body paragraphs (soft-wrap per Trey's commit convention).

5. **Mark task #1 complete**, then start **Phase 2 (task #2): the audit fan-out.**

---

## Campaign plan (tasks #1–6, dependency-ordered)

Routing principle (Trey's directive): **GLM (z.ai) via `delegate droid glm work` and
`delegate codex work` carry the mechanical volume; native Opus subagents are reserved for
anything that can silently break a spec invariant.** Audits use the free local `desloppify`
scanner + `delegate * safe` (read-only, isolated copy) for breadth; one Opus subagent for
the 7-invariant-critical audit. Coordinator gates once after each fan-out (never trust a
delegate's "done"; verify on disk).

- **#1 Land in-flight index-first refactor** — IN PROGRESS, ~1 commit away (see Resume).
- **#2 Audit fan-out** (blocked by #1): desloppify rust+ts; `delegate codex/glm/cursor safe`
  for perf + security breadth; 1 Opus subagent on the 7 invariants. Read-only, no tree mutation.
- **#3 Triage** → one risk-tagged backlog; scrutinize desloppify's 2165 "future-proofing"
  count before chasing it (likely #[non_exhaustive]/#[must_use] noise — don't churn for score).
- **#4 Deslop + pre-existing-debt fixes** → delegate glm/codex work, isolated worktrees.
- **#5 Perf + security hardening** → mechanical to delegate, invariant-touching to Opus/me.
- **#6 Elegance refactors** → bounded to refactor-pilot/delegate, cross-stream to Opus/me.

### Deferred items parked for #5/#6 (NOT dropped):
- **Codex risk 4:** `events_log_page` now reads the mirror (rebuild reads ALL device logs)
  vs the old `Substrate::events()` which read only THIS device's log → dashboard event pages
  may now include peer-device events. Needs a product/parity decision: document the intended
  scope change OR add a device filter to the mirror queries. (`handlers/mod.rs:~397`)
- **Dual seq-allocator cleanup:** `record_event` uses an in-memory atomic counter in the
  BestEffort tier while everything else uses `reserve_event_sequence`. Blocker-2's fix closes
  the reuse window surgically; unifying the two allocators is an elegance-phase item.

---

## Codebase facts & gotchas (save yourself the rediscovery)

- **Scale:** ~112k Rust LOC / 12 crates (memoryd 56k, memory-substrate 25k are the giants),
  ~8.7k TS in `crates/memoryd-web/frontend`.
- **The gate:** `bash scripts/check.sh` is the canonical workspace gate. It self-isolates
  `CARGO_TARGET_DIR` to a fresh `mktemp` by default (avoids the macOS syspolicyd hang). For
  fast iteration set `CARGO_TARGET_DIR=/tmp/memorum-gate-target MEMORUM_CHECK_KEEP_TARGET=1`
  (persistent warm dir — already populated this session). Phase order: parallel cheap checks
  (fmt/oxfmt/oxlint/docs/installer/baseline/specgate) → clippy → tests → doctests →
  `RUSTDOCFLAGS=-D warnings cargo doc`. Exits at the first failing phase.
- **`GATE_EXIT` trap:** a compound `bash check.sh; echo X` reports the *echo's* exit to the
  background notifier (always 0). ALWAYS read the `GATE_EXIT=` line written into the log.
- **`git diff` is raw again:** earlier this session `git diff`/`grep` output was being
  compacted by `rtk` (a CLI-proxy output summarizer) wired in as a `PreToolUse` Bash hook in
  `~/.claude-shared/settings.json` (which `~/.claude` and `~/.claude-personal` symlink to).
  Trey asked for it gone — `rtk` is now `brew uninstall`ed and the hook block removed, so
  `git diff` returns normal unified diff with no workaround. (Settings backups:
  `~/.claude*/settings.json.bak-rtk-20260603-185610`.) The separate Code Briefcase `Read`
  hook (the "[Code Briefcase orientation]" blocks) was intentionally left in place.
- **`rust_boundary_check`** (`crates/memory-test-support/src/bin/rust_boundary_check.rs`, run
  by `scripts/two-clone-convergence.sh`): forbids raw `.unwrap()`/`.expect(` in
  `crates/memory-substrate/src/**` (incl. `#[cfg(test)]` modules) unless the same or next line
  contains `unwrap-justified:` / `expect-justified:`. Convention: a local `must(result, ctx)`
  helper (used in api.rs `event_seq_tests` and now in the two new test modules). Also forbids
  absolute path literals (`/Users/`, `/tmp/`, …) in `crates/memory-substrate/tests/**` unless
  the line mentions `Roots` — use `tempfile::tempdir()`.
- **rustfmt:** project uses a wide `max_width`; run `cargo fmt --all` before any gate.
- **clippy.toml:** `too-many-arguments-threshold = 4` (bit me — a 5-arg test helper; fixed by
  passing a tuple). Whole-repo `-D warnings`.
- **Durability tier is probe-determined by the filesystem** (`InitOptions.force_unsafe_durability`
  only forces past `Refused`; you CANNOT force `BestEffort` in a test). That's why blocker-2's
  test exercises the `sync` vs `ensure` mechanism directly rather than spinning up a BestEffort
  substrate.
- **desloppify scan (Phase 2 input):** the rust scan got partial results before I killed it
  (it wedged at step 8/13 "Test coverage" — coverage instrumentation on 112k LOC is brutally
  slow and was contending with the gates). Partial signal: 460 structural, 202 rust_error_boundary,
  **2165 rust_future_proofing (treat as suspect — likely noise)**, 5 rust_async_locking. Rerun
  for Phase 2 with coverage disabled (check `desloppify --lang rust scan --help`) or use steps
  1–7. The TS frontend scan was never run.
- **Don't** run `cargo test --workspace` inside Codex's task worktrees; **don't** touch
  `bench/baseline.*.json`; **don't** bump spec/plan versions without Trey's ask. (See CLAUDE.md.)

## Background tasks left running at handoff
- `bmsor1bu8` — fmt + boundary lint + `cargo test -p memory-substrate --tests`. Check its
  output; this is the gating validation for the commit.
- (killed) `bhjgixl31` — the wedged desloppify rust scan. Already stopped.

## Reviews (full text in this session's transcript)
- Opus review: no hard blockers; R1/R2 (stale-index error/findability), R3 (silent lock
  degradation), R4 (mirror brittleness); confirmed parity + invariants.
- Codex review (`delegate run-output codex-19 --stdout --raw`): 2 blockers (id-verification,
  seq-reuse), 5 risks (incl. risk 3 index plan, risk 4 multi-device scope, risk 5 peer
  swallow); confirmed no SQL injection. Both blockers verified true against the code.
