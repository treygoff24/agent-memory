# Wright dogfood — first run learnings

**Date:** 2026-05-16
**Run:** ship `memoryd export v0.1` via the wright M1 loop, end-to-end, in one session.
**Outcome:** all four acceptance items closed mechanically (cargo test pass → status `implemented`); `wright --root . done` exits 0.

This is the LEARNING document. What worked, what didn't, what to change in
wright M2 before the next overnight run, and what to flag in the export
spec for a possible v0.2.

---

## 1. What shipped (concretely)

| Repo                          | Commits since baseline | Headline |
| ----------------------------- | ---------------------- | -------- |
| `~/Code/wright`               | 9 (above M1 step 1, `95ffe6a`)            | M1 step 2 (`ingest`/`queue`/`claim`/`verify`/`done` + run records + e2e smoke) plus three review-fix passes (opus, opus, codex). |
| `~/Code/agent-memory`         | 5 on `feature/tiered-gate-dashboard-workflow` | Item JSONs ingested, then four `feat(memoryd): wright[<item-id>] ...` commits implementing export end-to-end. |

End state: `wright queue` reports "no items ready (4 total — 4 implemented)". `wright done` exits 0. `memoryd export --repo … --runtime …` works on real fixtures.

---

## 2. What worked

**The shape held.** The handoff's premise — external durable queue + atomic per-item work + verifier-decided closure — held under real work. There was never a moment where the system was unsure whether an item was done; either `cargo test` exited 0 or it didn't, and wright flipped the status to match. No agent self-assessment was load-bearing.

**The context bundle is the right primitive.** `wright claim <id>` emits a single chunk — spec quote, source coordinates, acceptance tests, test command, scope hint, planner note — and that was *exactly* what the implementer needed to start working without re-reading the spec. The bundle is small enough to fit in an LLM context without truncation, structured enough to JSON-parse for tooling. Don't change this surface in M2.

**The dependency gate worked first try.** Item 01 was the gating item; 02/03/04 each declared `depends_on: ["export-json-shape-01"]`. `wright queue --next` correctly returned 01 first; only after 01 flipped to `implemented` did the other three become ready. No special-casing in the implementer was required.

**Iteration on failure is cheap.** Item 04 surfaced a real implementation gap (missing-parent error message lacked the parent path). The test failed, lock released, status returned to `approved`, fix was 6 lines in `export.rs::atomic_write_export`, re-test, re-verify, pass, commit. About six minutes of inner loop. The wright contract made the recovery mechanical.

**Adversarial reviews caught real bugs.** Three reviews ran in this session — two opus clean-code passes, one Codex pass. Findings across them:
- TOCTOU on lock acquire (opus 1)
- Status-vs-record write ordering in verify (opus 2)
- `{name}` substitution silently produces empty filter on empty refs (opus 2)
- `Regressed` items invisible to `wright queue` — `wright done` would never re-flip true (Codex)
- Claim's status-save before lock-persist could orphan a status on RAII drop (Codex)
- Partial-content lock file on post-create write failure (Codex)
- Silent `Ok(None)` on missing `runs/` dir hides telemetry loss (Codex)

Every one of these was a real correctness issue, not a style nit. Three of them would have shown up in an overnight loop and produced bad state. The 30-second cost of running the reviews after each command landed paid for itself many times.

---

## 3. What didn't work / what changed mid-run

**Subagent stream timeout.** The opus subagent dispatched for item 01 hit the Claude Code stream-idle timeout at ~56 minutes / 143 tool uses. It had done the full implementation (302-line `export.rs`, 462-line test, +31 lines on `Substrate` for the additive `iter_memory_envelopes`) and gotten as far as a passing local `cargo test`. It did NOT manage to run `wright verify` or commit. I had to step in, run `wright verify` (passed), review the diff, and commit by hand. **Lesson:** in M2 the wright loop should not assume any one agent will live long enough to finish an item end-to-end. The verify call and commit step should be runnable independently — which they already are, structurally — but the orchestration story should explicitly account for handoff between agent invocations.

**Spec wording on item 04 entry count.** Spec §8.4 says "the temp directory containing the output file has exactly two entries: the target file and a directory entry". The actual invariant is "no `.tmp` sidecar remains after a successful `--out` write" (the load-bearing thing). My test asserts that; a literal "exactly two entries" check doesn't match any natural layout. **Flag for export v0.2:** rewrite as "the output directory contains the target file and no `.tmp` sidecar".

**Byte-for-byte stdout vs `--out` comparison races.** Spec §8.4 says "the two outputs are byte-for-byte identical". Two `memoryd export` invocations produce different `exported_at` stamps in the JSON (different millisecond), so a literal byte-compare fails reliably. The test compares modulo `exported_at`. **Flag for export v0.2:** rewrite as "the two outputs are byte-for-byte identical *modulo the wall-clock-stamped `exported_at` field*".

**Test command substitution token is unused.** Wright supports `{name}` in `test_command` to fill in `acceptance_tests[0].name_pattern`. None of the four items used it (we hand-specified `cargo test -p memoryd --test <file>`). It's still worth keeping the feature, but it's optional — the M1 design assumed it would be the primary mechanism; in practice authors prefer the fully-specified shell command.

---

## 4. Iteration count per item

| Item                            | Implementations | Verify attempts | Notes |
| ------------------------------- | --------------- | --------------- | ----- |
| `export-json-shape-01`          | 1 (subagent)    | 1 (PASS)        | Gating; subagent timed out post-implementation; verify + commit done by driver. |
| `export-since-filter-02`        | 1               | 1 (PASS)        | --since logic already shipped in 01; this item was purely the test. |
| `export-encrypted-default-03`   | 1               | 1 (PASS)        | One import-path adjustment (`memory_substrate::events::EventKind` not the crate root). |
| `export-out-atomic-write-04`    | 2               | 1 fail + 1 PASS | Surface gap in `atomic_write_export` error message; fix was 6 lines. |

Four items, four passing verifies, zero spec/closure edits. The pattern produced shippable code on first try in three of four cases; the fourth surfaced and recovered from a real bug. None of the four items required >3 implementation attempts (the handoff's escalation threshold).

---

## 5. Wright M2 design issues surfaced

**M2-1. Verify timeout is declared but not enforced.** `Config::default_for_repo()` writes `verifier.timeout_seconds = Some(600)` to `.wright/config.json`. `verify::run` calls `std::process::Command::output()` which blocks forever. A hung `cargo test` in an overnight loop will stall the whole queue. The fix is `wait_timeout` or a manual poll-and-kill — call it M2-priority.

**M2-2. Concurrent verify TOCTOU.** Wright's M1 use model is single-writer. If two implementer processes ever race, `verify` snapshots the item, runs a long test, then writes back the new status without rechecking — and could clobber a concurrent `release`/`reclaim`. M2's "multiple agents in parallel" story needs a verify-time lock-token recheck.

**M2-3. Parent-directory fsync.** Both `wright-core::store::atomic_write_json` and `wright-core::lock::acquire` `fsync` the data file but not the parent directory. On unexpected power loss, a freshly-renamed file might disappear. Not load-bearing under normal SIGKILL recovery (the use model for overnight loops on a Mac laptop), but worth tightening when M2 starts thinking about durability properly.

**M2-4. Stale-lock detection.** Wright has no PID-liveness check on `acquire`. A crashed implementer leaves a lock file forever, recoverable only via `wright release`. The release subcommand now also flips `claimed -> approved` (added in this session's review fixes), so the recovery flow is one command, but the *detection* is still manual. M2 should auto-detect stale locks (cheaper than full PID-liveness — even just "lock older than N hours" would help).

**M2-5. Loop telemetry / postmortem tooling.** Run records under `.wright/runs/` accumulated nicely (8 records per item × 4 items = ~32 records, plus init + ingests = ~45 total). There's no `wright log` / `wright report` command to summarize them. M2 milestone 3 already plans for this; the dogfood validates that the records have enough information.

**M2-6. Subagent-orchestration handoff.** As noted above, an agent that runs out of time mid-item should be cheaply resumable. Today the wright contract is already mechanically resumable (the lock + status are durable), but the orchestration layer (cron'd / scheduled wright drivers, etc.) doesn't exist. M2 could ship a `wright drive` command that owns the inner loop and an external supervisor only needs to keep it alive.

**M2-7. The `Regressed` state needed a fix mid-flight.** Initial implementation excluded `Regressed` items from `ready_set`. Codex caught this: an item that was implemented and later re-verifies fail would drop off the queue forever. Fixed in this session — `Regressed` is now part of the ready set and re-claimable. The state machine in `verify::next_status` is now exhaustive and clean; the unit test for it covers every transition. **The lesson is that the state lattice needs the same scrutiny as the imperative code.** The M1 design diagram in `item.rs` showed the transitions, but the actual code consequences (queue invisibility) weren't in the diagram. M2 should be modeled as a state graph FIRST, with the imperative code generated to match.

---

## 6. Export v0.2 / spec feedback

Nothing structural; v0.1 closed cleanly. Flagged for v0.2:

- §8.4 "exactly two entries: the target file and a directory entry" → "the output directory contains the target file and no `.tmp` sidecar".
- §8.4 "byte-for-byte identical" → add "modulo the wall-clock-stamped `exported_at` field".
- §3 stdout vs stderr: the success summary regex `^memory_count=\d+ bytes=\d+$` is implementable but tightly bound to the stderr-only diagnostic discipline; v0.2 could relax to "stderr contains a line matching this regex" to make room for future progress diagnostics without breaking older test rigs.

The atomic-write inlining call-out (§7) was correct — having each subcommand inline its own atomic-write helper was less ergonomic than a shared workspace helper would be, but the spec was explicit about not adding one. v0.2 may want to revisit.

---

## 7. Headline take

Wright shipped its first feature through its own loop the same day the loop's `claim`/`verify`/`done` surface was implemented. Six commits in `~/Code/wright`, five in `~/Code/agent-memory`. Two of the wright commits were review-driven fixes that landed *before* the dogfood started shipping items — that's the right cadence.

The pattern works. Multi-hour autonomous overnight runs of this shape should now be tractable, conditional on M2-1 (verify timeout) landing before any long-running agent is left unsupervised.
