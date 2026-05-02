# Plan: 2026-05-02 — Fix Codex 12-hour autonomous run review findings

> **Goal:** close the punch list from the Stream G/H/I fresh-eyes reviews so the streams can honestly carry "shipped" status. Snapshot baseline: `6095cf6 wip: snapshot Codex 12-hour autonomous Stream G/H/I run`.

## Roles

- **Coordinator:** Claude (this session). Owns the DAG, orchestrates subagents, runs the project gate, commits.
- **Code reviewer:** `opus-reviewer` — persistent opus subagent, `/clean-code` + `/rust-engineer` skills loaded. Reviews each implementation agent's diff for behavior preservation, idiom, edge cases, test coverage, and scope discipline.
- **Implementation agents:** 3 sonnet subagents, one per stream. `/rust-engineer` skill loaded. Each is briefed with the precise punch-list items for its stream. Each **skips `scripts/check.sh`** (coordinator runs it). Each runs only targeted `cargo check -p <pkg>` and `cargo test -p <pkg> -- --test-threads=2` for packages it modified.

## Status legend

- 🔵 pending
- 🟡 in_progress (implementation)
- 🟠 in_review (opus)
- 🔁 revising (after review)
- 🟢 done
- 🔴 blocked

## DAG

```
                                        ┌─────────────────────┐
                                        │  opus-reviewer      │
                                        │  (persistent)       │
                                        │  /clean-code        │
                                        │  /rust-engineer     │
                                        └──────────┬──────────┘
                                                   │ reviews
            ┌──────────────────────┬───────────────┴───────────────┬──────────────────────┐
            ▼                      ▼                               ▼                      ▼
     ┌──────────────┐       ┌──────────────┐                ┌──────────────┐       ┌──────────────┐
     │ sonnet-impl-h │       │ sonnet-impl-g │                │ sonnet-impl-i │       │ Coordinator  │
     │              │       │              │                │              │       │ runs full    │
     │ Stream H     │       │ Stream G     │                │ Stream I     │       │ scripts/     │
     │ blockers +   │       │ small fixes  │                │ small fixes  │       │ check.sh     │
     │ risks        │       │              │                │              │       │              │
     └──────────────┘       └──────────────┘                └──────────────┘       └──────────────┘
            │ diff                  │ diff                          │ diff                  ▲
            ▼                       ▼                               ▼                      │
     ──────────────── all reviewed + fixed ───────────────────────────────────────────────┘
```

## Wave 1 — Implementation (parallel, one sonnet agent per stream)

| Stream | Agent name | Items | Status |
|---|---|---|---|
| H | `sonnet-impl-h` | H-B1, H-B2, H-B3, H-R1, H-R2, H-R4, H-nit (7) | 🔁 revising (round 2: 2 must-fix from opus) |
| G | `sonnet-impl-g` | G-R1, G-R2, G-R3, G-R4 (4) | 🟢 done (impl + opus approved, no must-fix) |
| I | `sonnet-impl-i` | I-R1, I-R2, I-R3, I-R4, I-R5 (5) | 🟢 impl done — sent to opus |

## Wave 2 — Code review (per stream, opus-reviewer)

| Stream | Reviewer | Trigger | Status |
|---|---|---|---|
| H | `opus-reviewer` | sonnet-impl-h done | 🔁 round 1: revisions required (H-B3 adoption gap, H-R4/B1 stderr-vs-stdout) |
| G | `opus-reviewer` | sonnet-impl-g done | 🟢 approved (round 1, all 4 items, no must-fix) |
| I | `opus-reviewer` | sonnet-impl-i done | 🟠 in_review (queued behind H) |

## Wave 3 — Revision rounds (only if review surfaces issues)

| Stream | Status |
|---|---|
| H | 🟡 round 2 in progress (sonnet-impl-h) |
| G | 🟢 not needed (opus approved round 1) |
| I | 🔵 pending opus review |

## Wave 4 — Coordinator gate

Single `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh` end-to-end. Status: 🔵

## Wave 5 — Commit

One commit per stream, clear messages. Status: 🔵

## Punch list with file:line

### Stream H — 3 blockers, 3 risks, 1 nit (sonnet-impl-h)

| ID | Severity | File:line | What |
|---|---|---|---|
| H-B1 | Blocker | `crates/memorum-eval/src/orchestrator.rs:518-521` | Remove unconditional orchestrator-level skips for T17/T18. Test files have honest internal skip guards — let those drive behavior. |
| H-B2 | Blocker | `.github/workflows/stream-h-eval.yml` + `crates/memorum-eval/Cargo.toml` | Either enable `stream-i-deps` feature by default or remove the gate entirely. T19 must run in CI now that Stream I is shipped. |
| H-B3 | Blocker | `crates/memorum-eval/src/orchestrator.rs:641-654` | Replace `assertions: 1, assertions_passed: 1` hardcode with real counter. Spec §6.2 implies per-test granularity. |
| H-R1 | Risk | `crates/memorum-eval/tests/eval/domain/t16_drift_scoring_sanity.rs:161-268` | Replace direct `sqlite3` CLI injection with spec's `EventLogInjector` via `RequestPayload::TestInjectEvent`. |
| H-R2 | Risk | `crates/memorum-eval/src/orchestrator.rs:711-722` | Replace home-rolled `block_on` busy-spin with `pollster::block_on` or `tokio::runtime::Handle::block_on`. |
| H-R4 | Risk | `t16_drift_scoring_sanity.rs:19-25` | T16 should report `skipped` (not silent-pass) when Stream G `RealityCheck(List)` handler is absent. |
| H-nit | Nit | `orchestrator.rs:836-844` | Emit ISO 8601 timestamps, not `unix-ms:<millis>`. Spec §6.2. |

### Stream G — 4 risks (sonnet-impl-g)

| ID | Severity | File:line | What |
|---|---|---|---|
| G-R1 | Risk (genuine bug) | `crates/memoryd/src/reality_check/scheduling.rs:63-67` | `is_overdue` returns false when `last_completed_at` is None. Use `is_none_or` to mirror `is_due`. Add test for None case. |
| G-R3 | Risk (spec violation) | `crates/memoryd-web/src/config.rs:34-41` | Accept `::1` (IPv6 loopback). Spec §4.4. |
| G-R4 | Risk | `crates/memoryd-web/src/routes/audit.rs:148` | `audit_walk` returns 501 unconditionally. Either implement or move to spec §11 deferred list and document. |
| G-R2 | Risk (bench honesty) | `bench/stream-g-observability-results.darwin-arm64.json` | TUI bench measures synthetic 144-byte frames. Either replace with real ratatui integration bench or split into "smoke" + "real-load" tiers and document the synthetic shortcut. |

### Stream I — 5 risks (sonnet-impl-i)

| ID | Severity | File:line | What |
|---|---|---|---|
| I-R2 | Risk (bench honesty) | `bench/stream-i-cross-session-results.darwin-arm64.json:18` | `precomputed_embedding_dimension: 16`. Production is 1,024-3,072. Re-run at ≥1,536 dim. |
| I-R3 | Risk | `crates/memoryd/src/recall/render.rs:269` | `<entity-recall entities="">` always empty. Populate from `SessionContext.salient_entities` per spec §4.3. |
| I-R1 | Risk | `crates/memorum-coordination/src/presence.rs:276` + `crates/memoryd/src/handlers.rs:590-591` | `conflicting_claim_locks` populated only at L3. Either remove L3 guard or document scope explicitly. |
| I-R4 | Risk | `crates/memorum-coordination/src/claim_lock.rs:174-178` | `acquire_at` on occupied entry replaces existing holder before returning `Contended`. Either preserve holder priority or document eviction semantics with a `// SAFETY/INVARIANT:` comment. |
| I-R5 | Risk | `crates/memoryd/src/recall/startup.rs:283,309` | Cool-down registry not shared between same-device and cross-device passes (separate clones). Edge case but diverges from §4.2 single-session cool-down semantics. |

## Constraints for impl agents

1. **Do not run `scripts/check.sh`.** Coordinator runs it once at the end.
2. **Run only targeted package gates:** `cargo check -p <pkg>` and `cargo test -p <pkg> -- --test-threads=2`.
3. **Do not modify Stream A modules** (`crates/memory-substrate/`) unless an item explicitly requires it. None of the items above do.
4. **Behavior-preserving** on bench fixtures — re-running benches is fine; arbitrarily revising what they measure is a design change requiring confirmation.
5. **Each agent reports a structured diff summary** at completion (file list, brief description per change, rationale, test additions).
6. **No new dependencies without justification.** `pollster` is fine for H-R2 if `tokio::runtime::Handle::block_on` doesn't fit context (the orchestrator may not have a runtime handle available).

## Constraints for opus reviewer

1. Look for: behavior preservation, scope discipline (no out-of-punch-list edits), idiom (`/rust-engineer` patterns), edge cases the impl agent missed, test coverage of the fix.
2. Distinguish must-fix from nice-to-have. Impl agents do another revision pass only on must-fix.
3. Cap review at ~800 words per stream. Be specific.

## ⏸ PAUSED — HANDOFF FOR RESUMPTION (2026-05-02)

**Trey ran out of subscription usage mid-round-2. All subagents killed. Resume from here.**

### Snapshot commit
`6095cf6 wip: snapshot Codex 12-hour autonomous Stream G/H/I run` — all 322 files of Codex's 12-hour autonomous run captured. If anything goes wrong, `git reset --hard 6095cf6` recovers to that point.

### What's already done
- ✅ Snapshot commit landed.
- ✅ Three fresh-eyes review docs written (untracked at `docs/reviews/stream-{g,h,i}-claude-fresh-eyes-review.md`).
- ✅ Coordinator gate (`BENCH_PROFILE=darwin-arm64 bash scripts/check.sh`) ran clean against the snapshot.
- ✅ Stream G impl (sonnet) **+ opus review** complete, no must-fix. Uncommitted on disk.
- ✅ Stream H impl round 1 (sonnet) complete. Uncommitted on disk.
- ✅ Stream I impl (sonnet) complete. Uncommitted on disk.

### What's in-flight (killed at this point)
- 🔁 **Stream H round 2** — sonnet-impl-h was actively working on 2 must-fix items from opus's round-1 review when killed. Disk state may reflect partial round-2 edits. **Must inspect git diff before continuing.**
- 🟠 **Stream I review** — opus-reviewer was queued to review the Stream I diff but hadn't reported yet when killed.

### Uncommitted work on disk by stream

**Stream G (review-clean, ready to commit):**
- `crates/memoryd/src/reality_check/scheduling.rs` — `is_some_and` → `is_none_or`
- `crates/memoryd/tests/scheduling.rs` — `test_overdue_when_never_completed`
- `crates/memoryd-web/src/config.rs` — `is_localhost()` accepts `::1` + 4 tests
- `bench/stream-g-observability-results.darwin-arm64.json` — `synthetic_caveat` field
- `docs/specs/stream-g-observability-v0.1.md` — §11.8 (audit_walk deferred), §12.1 (synthetic bench caveat), revision-history entry

**Stream H (round-1 done; round-2 may be partial):**
- `crates/memorum-eval/Cargo.toml` — `default = ["stream-i-deps"]`, `stream-i-deps` feature with `dep:memorum-coordination`
- `crates/memorum-eval/src/lib.rs` — `eval_assert!`/`eval_assert_eq!` macros + thread-local counter
- `crates/memorum-eval/src/orchestrator.rs` — T17/T18 unblocked, `extract_assertion_count`, `extract_skip_marker`, ISO 8601 timestamps, real `block_on`
- `crates/memorum-eval/src/daemon_scaffold.rs` — builds memoryd with `--features test-utils`
- `crates/memorum-eval/tests/eval/domain/t16_drift_scoring_sanity.rs` — `rusqlite::Connection` + `MEMORUM_EVAL_SKIP:` markers
- `crates/memorum-eval/tests/eval/handbook/t01_*.rs` — migrated to `eval_assert!` (only T01 in round 1)
- `crates/memorum-eval/tests/orchestrator_integration.rs` — 4 regression tests
- `crates/memoryd/Cargo.toml` — `[features] test-utils = []`
- `crates/memoryd/src/protocol.rs` — `RequestPayload::TestInjectEvent`, `InjectableEventKind`
- `crates/memoryd/src/handlers.rs` — `test_inject_event_response` gated `#[cfg(feature = "test-utils")]`

**Stream I (impl done, never reviewed):**
- `crates/memorum-coordination/src/bin/peer_relevance_bench.rs` — `--embedding-dimension` flag, default 1536
- `crates/memorum-coordination/src/claim_lock.rs` — removed `entry.insert()` from `Entry::Occupied` arm
- `crates/memorum-coordination/tests/claim_lock_unit.rs` — test renames/rewrites
- `crates/memorum-coordination/tests/gate_unit.rs` — 2 new I-R5 tests
- `crates/memoryd/src/handlers.rs` — `INVARIANT` comment for L3-only `conflicting_claim_locks`
- `crates/memoryd/src/recall/render.rs` — `salient_entities` field on `StartupCoordinationRender`, `entity_recall_opening_tag` helper
- `crates/memoryd/src/recall/startup.rs` — `salient_entities` threading + cool-down sharing via clone-pre-seeding
- `crates/memoryd/tests/coordination_recall_render.rs` — 4 new I-R3 tests
- `crates/memoryd/tests/dream_recall_integration.rs` — baseline updated `entities=""` → `entities="dream-project,proj_dream"`
- `docs/api/stream-i-cross-session-api.md` — L3-only design constraint
- `bench/stream-i-cross-session-results.darwin-arm64.json` — re-run at dim=1536

### Stream H round-2 must-fix items (NOT YET COMPLETE)

These were dispatched to sonnet-impl-h but it was killed mid-execution. Inspect `git diff` before redoing.

**Must-fix #1 — H-B3 adoption gap.** Only T01 was migrated to `eval_assert!`/`eval_assert_eq!` in round 1. T02-T18 still use plain `assert!`/`assert_eq!`. The orchestrator's `unwrap_or(1)` fallback at `crates/memorum-eval/src/orchestrator.rs:638` means unmigrated tests report `assertions: 1` — same honesty gap as before.
- At minimum migrate: T02 (`t02_superseded_fact.rs`, multiple assertions), T11 (`t11_self_poisoning.rs`, 9+ assertions), T14 (`t14_merge_driver_semantic_correctness.rs`, 6+ assertions).
- Recipe per test: replace each `assert!`/`assert_eq!` → `eval_assert!`/`eval_assert_eq!`, add `eval_flush_assertion_count();` immediately before each successful return point.
- Default to migrating all 15 unmigrated tests — they're mechanical.

**Must-fix #2 — H-R4/H-B1 stderr→stdout interaction.** T17 and T18 internal skip paths still use `eprintln!` (stderr), but orchestrator's `extract_skip_marker` scans **stdout** for `MEMORUM_EVAL_SKIP:`. T17/T18 internal skips will silently pass — same bug class H-R4 fixed for T16. Fix at:
- `crates/memorum-eval/tests/eval/domain/t17_lease_contention_resolution.rs:31`
- `crates/memorum-eval/tests/eval/domain/t18_encrypted_tier_key_rotation.rs:18`
- `crates/memorum-eval/tests/eval/domain/t18_encrypted_tier_key_rotation.rs:75`
- Change each `eprintln!` to `println!("MEMORUM_EVAL_SKIP:{CONSTANT}: ...")` using the appropriate skip-reason constant (`SEMANTIC_PARTIAL_LEASE_REENTRANCY_NOT_SHIPPED`, `STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED`).

### Stream I review (NEVER RAN)

opus-reviewer was queued to review Stream I but was killed before responding. The Stream I impl summary is in T5 of the tracking notes below. Three things to verify when reviewing:

1. **I-R3 baseline update in `dream_recall_integration.rs`** — `entities=""` → `entities="dream-project,proj_dream"`. Verify the new baseline is *correct* (these are actual salients from the test fixture's recall selection), not just new-wrong.
2. **I-R5 cool-down sharing** — does extract-from-return-value + clone-pre-seeding actually preserve the SessionContext-mutation closeout invariant? Trace it.
3. **I-R4 holder preservation** — verify renamed/rewritten tests in `claim_lock_unit.rs` actually catch the bug class (contender silently replacing holder), not just the surface change.

### Pre-existing concerns to resolve

- **`dream_harness_cli` PoisonError on `SUBPROCESS_TEST_LOCK`** — Stream I agent flagged this as pre-existing, but the snapshot commit gate ran clean. **Suspicious.** Either I-agent broke it or this is real test-isolation flakiness. Coordinator gate after revisions land will tell.
- **`unused_import: TestInjectEventResponse` in `crates/memoryd/src/handlers.rs:49`** — pre-existing from Stream H round 1. Will fail clippy `-D warnings` in coordinator gate. Trivial fix when resuming.

### How to resume

1. `git status` — see what's actually on disk.
2. `git diff` — inspect any partial round-2 H work from the killed agent.
3. Decide: finish H round-2 manually (~30 min of mechanical edits per the recipe above) OR re-spawn `sonnet-impl-h` with the same round-2 brief.
4. Once H round-2 is done, run opus review on Stream I (re-spawn `opus-reviewer` or do the review yourself).
5. Run `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh` end-to-end once everything is integrated.
6. If gate green: commit per-stream (3 commits: G, H, I). If red: triage failures.
7. After all 3 commits land, update `CLAUDE.md` "Current status" to reflect Stream H is genuinely shipped (not just claim-shipped).

### Key file references

- Plan with full DAG: `docs/plans/2026-05-02-codex-12hr-review-fixes.md` (this file)
- Fresh-eyes reviews (untracked): `docs/reviews/stream-{g,h,i}-claude-fresh-eyes-review.md`
- Codex's self-reviews: `docs/reviews/stream-{g,h,i}-final-gate-report.md`
- Specs: `docs/specs/stream-{g,h,i}-*-v0.1.md`
- Two prior gate-run logs: `/tmp/check-sh-output.log`, `/tmp/check-sh-output-2.log` (may be cleaned by reboot)

---

## Tracking notes

(Coordinator updates this section as work lands.)

- 2026-05-02 T0: plan created, Wave 1 about to launch.
- 2026-05-02 T1: `opus-reviewer` spawned, setup complete (skills loaded, plan + reviews + specs + CLAUDE.md read), waiting for diffs.
- 2026-05-02 T1: `sonnet-impl-h`, `sonnet-impl-g`, `sonnet-impl-i` spawned in parallel (all background).
- 2026-05-02 T2: `sonnet-impl-g` finished. All 4 G items addressed:
  - G-R1: `is_some_and` → `is_none_or` in `reality_check/scheduling.rs:63-67`; test `test_overdue_when_never_completed` added.
  - G-R3: `validate_localhost` accepts `::1` via new `is_localhost` helper; 4 tests added.
  - G-R4: spec §11.8 added documenting `audit_walk` deferral to v1.1+; 501 stub stays.
  - G-R2: `synthetic_caveat` field added to TUI bench entries + spec §12.1 caveat prose.
  - Files: `reality_check/scheduling.rs`, `tests/scheduling.rs`, `memoryd-web/src/config.rs`, `bench/stream-g-observability-results.darwin-arm64.json`, `docs/specs/stream-g-observability-v0.1.md`.
  - Note: `memoryd` does not compile due to pre-existing I-agent in-flight work in `recall/render.rs:149` and `recall/startup.rs:173`; G-agent verified its own changes via stash round-trip. cargo check + clippy + test gates blocked by I-agent state until I-agent lands.
  - Routed to opus-reviewer.
- 2026-05-02 T3: opus-reviewer round-1 verdict on Stream G: **approved, all 4 items, no must-fix**. One nice-to-have (boundary-day test at exactly 21 days) recorded but not actioned per scope discipline.
- 2026-05-02 T4: `sonnet-impl-h` finished. All 7 H items addressed (218 tool calls, 45min runtime). 52/52 tests pass, clippy/fmt clean. Notable surface changes:
  - H-B1: T17/T18 unconditional skip arms removed; both routed via cargo_dispatch.
  - H-B2: `[features] stream-i-deps = ["dep:memorum-coordination"]` with `default = ["stream-i-deps"]` so T19 runs by default.
  - H-B3: Thread-local `EVAL_ASSERTION_COUNTER` + `eval_assert!`/`eval_assert_eq!` macros + `MEMORUM_EVAL_ASSERTIONS=<n>` stdout marker; orchestrator parses via `extract_assertion_count`. **Open question for opus:** were existing t01-t19 test files migrated to use the new macros?
  - H-R1: `sqlite3` CLI replaced with `rusqlite::Connection` + new `RequestPayload::TestInjectEvent` in memoryd protocol gated by `#[cfg(feature = "test-utils")]`. Real protocol surface expansion.
  - H-R2: home-rolled busy-spin replaced with `TokioBuilder::new_current_thread().enable_all().build().block_on()`.
  - H-R4: T16 silent-pass replaced with `MEMORUM_EVAL_SKIP:` stdout marker → `skipped_result`.
  - H-nit: `chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)` replaces `unix-ms:<millis>`.
  - Files touched: `memorum-eval/{Cargo.toml,src/lib.rs,src/orchestrator.rs}`, `memorum-eval/tests/eval/domain/t16_drift_scoring_sanity.rs`, `memorum-eval/tests/orchestrator_integration.rs`, `memorum-eval/src/daemon_scaffold.rs`, `memoryd/{Cargo.toml,src/protocol.rs,src/handlers.rs}`.
  - Routed to opus-reviewer.
- 2026-05-02 T5: `sonnet-impl-i` finished. All 5 I items addressed (156 tool calls, 45min runtime). Agent confirms SessionContext-mutation closeout fix invariant preserved.
  - I-R1: `INVARIANT` comment at `handlers.rs:~594` + named design constraint in `docs/api/stream-i-cross-session-api.md`. Kept L3-only guard with explicit upgrade obligation if L2 heartbeat path is added.
  - I-R2: bench re-run at `--embedding-dimension 1536` (default raised from 16). p95=0.001283ms, budget=5ms, pass=true.
  - I-R3: `<entity-recall entities="...">` populated from `SessionContext.salient_entities` (lexicographic sort + XML escape). 4 new tests. Updated `dream_recall_integration.rs` baseline from `entities=""` to `entities="dream-project,proj_dream"` — **opus to verify this is correct, not just new-wrong**.
  - I-R4: removed `entry.insert(...)` from `Entry::Occupied` arm in `claim_lock.rs::acquire_at`. Original holder preserved on contention. Tests renamed/rewritten in `claim_lock_unit.rs`.
  - I-R5: cool-down sharing via extract-from-return-value + clone-pre-seeding pattern. Original `startup_context` never mutated. 2 new tests in `gate_unit.rs`.
  - Targeted gates: 73/73 memorum-coordination, 18/18 coordination_recall_render, 17/17 claim_lock_unit, 20/20 gate_unit, 13/13 dream_recall_integration, 12/12 coordination_integration. Clippy + fmt clean.
  - **Agent flagged pre-existing failures:** `dream_harness_cli` PoisonError on `SUBPROCESS_TEST_LOCK` — agent claims pre-existing on main; **suspicious because the snapshot commit gate ran clean** — coordinator will verify.
  - **Pre-existing warning from Stream H:** `unused_import: TestInjectEventResponse` in `handlers.rs:49`.
  - Routed to opus-reviewer (queued behind H review).
- 2026-05-02 T6: opus-reviewer round-1 verdict on Stream H: **revisions required, 2 must-fix**:
  1. **H-B3 adoption gap** — only T01 migrated to `eval_assert!`. T02-T18 still use plain `assert!`/`assert_eq!`; fallback `unwrap_or(1)` means unmigrated tests report `assertions: 1` (same honesty gap as before). Must migrate at least T02, T11, T14.
  2. **H-R4/H-B1 interaction** — T17/T18 (newly unblocked) use `eprintln!` for skip paths but orchestrator scans **stdout** for `MEMORUM_EVAL_SKIP:`. T17/T18 internal skips will silently pass. Must change `eprintln!` → `println!("MEMORUM_EVAL_SKIP:...")` at `t17_lease_contention_resolution.rs:31`, `t18_encrypted_tier_key_rotation.rs:18`, `t18_encrypted_tier_key_rotation.rs:75`.
  - One nice-to-have noted on H-R1 (daemon_scaffold cached-binary edge case) — not actioned.
  - Routed to `sonnet-impl-h` for round 2.
