1 # Stream I Cross-Session Coordination — Plan Review
2
3 **VERDICT: RISK**
4
5 No single finding is an outright execution stopper, but three findings together form a pattern that will cause Task 13 or Task 16 to fail mid-stream without warning. Fix the `EventKind::ClaimLockContention` ownership gap before execution starts. The cross-device device-filter problem and the `TimeSource` phantom reference are softer but will still surface as confusion during implementation.
6
7 ---
8
9 ## Blockers
10
11 **1. `EventKind::ClaimLockContention` has no owning task and touches a file Stream G exclusively owns.**
12
13 Spec §7.4 says: "Logs a contention event to the event log with `EventKind::ClaimLockContention { memory_id, holder, contender }`." No task in this plan adds that variant to `crates/memory-substrate/src/events/log.rs`. Task 13 wires the warning response to the caller and the in-memory claim-lock registry, but the event log write is simply not there.
14
15 The inter-stream coordination section explicitly locks `crates/memory-substrate/src/events/log.rs` to Stream G. Stream I adding `EventKind::ClaimLockContention` to that file would be an unauthorized touch by the plan's own rules. The plan must either (a) add `EventKind::ClaimLockContention` to the authorized Stream A surface in the `Inter-Stream Coordination` section and update the corresponding rebase rule, or (b) explicitly defer the event log write as a known gap and note it in Task 13's invariants.
16
17 File: `crates/memory-substrate/src/events/log.rs` (locked to Stream G). Plan section: Task 13, Inter-Stream Coordination.
18
19 Fix: Add a clause to Inter-Stream Coordination noting that `EventKind::ClaimLockContention` is authorized for Stream I in `log.rs` under the same rebase-second rule as Stream G's `RecallHit`, and add a step in Task 13 to add the variant. Or explicitly state the contention event log write is deferred and remove it from §7.4's behavioral contract.
20
21 **2. Task 16's cross-device device filter is architecturally underspecified.**
22
23 Task 16's GREEN implementation says: "separately query for cross-device entries (rows with `device_id != local_device_id` in the event log with `indexed_at` within the relevant window)." The event log is a JSONL file, not the SQLite index. `RecallIndexRow` has no device field — the `memories` table has `source_device TEXT` but `query_recall_index`'s SELECT list at `index/query.rs:288` does not include it, and `RecallIndexRow` at `model.rs:1219` has no `source_device` field.
24
25 To filter by device in the recall index, either `source_device` must be added to `RecallIndexRow` (another Task-2-style struct-field addition that Task 2's invariants and owned files don't cover), or the query must join the event log JSONL (which has no SQL interface). Neither path is planned or included in Task 2's owned files.
26
27 File: `crates/memory-substrate/src/model.rs:1219`, `crates/memory-substrate/src/index/query.rs:288`. Plan section: Task 16, GREEN implementation.
28
29 Fix: Either extend Task 2 to also surface `source_device` on `RecallIndexRow`, add it to Task 2's owned files and invariants, and update Task 16 to use it — or Task 16 must use the JSONL event log and explicitly describe how entity/path metadata is obtained for candidates that came via that path.
30
31 ---
32
33 ## Risks
34
35 **R1. Spec §5.3 and §4.2 disagree on which field the cross-device startup window uses.**
36
37 Spec §4.2 says the recency window uses `local_observed_at` (i.e., `indexed_at`). Spec §5.3 says the cross-device startup 30-minute window uses "the wall-clock time of the peer write (`updated_at` in the event log), not the sync time." These are different fields and opposite semantics. The plan follows §4.2's framing (Task 15 hardwires `indexed_at`; Task 16 inherits that). But the contract test `test_startup_no_cross_device_outside_window` in Task 16 will be written against one interpretation, leaving the other untested and the spec contradiction unresolved. When someone reads §5.3 during review they'll flag it.
38
39 The contradiction should be resolved in the spec before Task 16 executes, not discovered by a reviewer mid-stream.
40
41 **R2. `TimeSource` abstraction referenced in Task 15 does not exist in the codebase.**
42
43 Task 15's invariants say "the `TimeSource` abstraction from Stream E is used so tests can fixture the clock deterministically." Stream E's spec v0.5 mentions `TimeSource` once conceptually (`stream-e-passive-recall-v0.5.md:978`) but no implementation exists — grep across the entire codebase returns zero results. Task 15's subagent will either invent an ad-hoc clock mock, skip clock fixturing, or block on this. The plan should either define `TimeSource` as a Task 15 deliverable or remove the reference and describe the actual test clock strategy.
44
45 **R3. Task 20 writes into `crates/memorum-eval/` which Stream H creates — no cross-plan sequencing gate declared.**
46
47 Task 20 creates `crates/memorum-eval/fixtures/prompts/t19_peer_update_framing.md`. That path only exists after Stream H's Task 2 creates the `memorum-eval` crate. The plan doesn't declare a cross-plan blocking dependency or include a creation step for the directory. If Stream I's Task 20 executes before Stream H has landed, the file write either succeeds (creating a dangling directory outside the workspace) or fails. The final gate in Task 23 runs `cargo test --workspace`, which would fail if `memorum-eval` is a workspace member that Stream I created an orphan file in. The plan should add a step: "if `crates/memorum-eval/` does not yet exist, create directory structure under the `memorum-eval` workspace member; note dependency on Stream H Task 2."
48
49 **R4. System-v0.2 §19 authorization table uses different protocol variant names than the stream-i spec.**
50
51 `system-v0.2.md §19` lists `PeerPresenceHeartbeat`, `PeerClaimAcquire`, `PeerClaimRelease` as authorized `RequestPayload` variants. The stream-i spec §6.1 and §2.2 define `PeerHeartbeat`, `PeerStatus`, `PeerActivity`, `PeerReleaseLock`. These don't match. The plan correctly follows the stream-i spec's names, but the authorization table in the parent document is stale. A strict reading of §19 means `PeerHeartbeat` is unauthorized (not listed). The plan doesn't flag this discrepancy or note it as a pre-execution amendment to the system spec.
52
53 **R5. Task 13's claim-lock acquire placement is before the disk write in the pseudocode, but spec §7.1 says the lock is visible to peers via recall — meaning other sessions need to see the lock while the write is in flight.**
54
55 The plan's pseudocode acquires the lock, does the write, then releases. That's correct. But the sequence means the lock is acquired before governance, then acquired again after governance in the plan's narrative ("after passing Stream C governance checks, calls... `claim_lock_registry.acquire`"). The actual pseudocode in the plan shows the acquire after governance success, which is correct per spec §7.1. However, the test `test_level2_supersede_acquires_lock` checks that the lock is `Some(...)` "immediately after the call returns" — but the call returns only after the write commits and the lock is released. The test description is wrong: the lock should be Some during execution but None after completion per `test_level2_supersede_releases_lock_on_success`. These two tests in Task 13 are contradictory as written unless `test_level2_supersede_acquires_lock` is a concurrent-access test (which it doesn't say it is).
56
57 **R6. Per-project level resolution in Task 17 says "Level 2 is the default when `concurrent_session_mode` is absent" but this requires reading `config.yaml` fallback, which is not wired in Task 17's scope.**
58
59 Task 17 wires level resolution in `handle_delta_block`, `handle_startup`, and `handle_supersede`. The fallback chain is: project-binding `concurrent_session_mode` → `config.yaml coordination.level`. Reading the config value requires the daemon to have loaded `CoordinationConfig`, which Task 11 wires. But Task 17 references this indirectly via "effective-level resolution function." If that function reads from daemon state, it needs the `CoordinationConfig` arc to be threaded from `workers.rs` through the handler context, which Task 11 does. The dependency is implicit and Task 17's owned files don't include `workers.rs` or `server.rs`. This probably works, but the subagent may not realize it needs to read Task 11's handler-state wiring to understand how to implement the resolution.
60
61 ---
62
63 ## Nits
64
65 The plan's stop conditions are well-chosen. The rebase rule and the single-trunk gate are clean. Task 1's contract-map step is the right first move.
66
67 Task 5's `test_tier3_returns_empty` and Task 18's `test_tier3_returns_empty_no_scoring` are the same behavior, just with a spy. The plan notes Task 18 should check "if not already green from Task 5" — that's fine, but the two tasks share `gate_unit.rs` and the parallel marker on Task 18 (parallel with Task 17, blocked by Review Gate C) is correct given the file ownership.
68
69 Task 9's `PresenceRecord::started_at` is typed `DateTime<Utc>` in the spec struct (§6.2) but `started_at` on the heartbeat wire is `Option<DateTime<Utc>>`. The plan correctly handles the conversion in the upsert logic ("started_at on initial record is retained if the incoming record has started_at from the first heartbeat"). But the struct field itself is non-optional while the wire field is optional. Task 10 needs to handle the projection from `Option<DateTime<Utc>>` on the wire to `DateTime<Utc>` on the record — this is a case where the first heartbeat might legitimately send `None` for `started_at` and the daemon would have to store a sentinel. The test `test_heartbeat_started_at_none_first_then_some` covers this, but Task 9's `PresenceRecord` struct should document how it handles the "first heartbeat had no `started_at`" case.
70
71 The crate name `memorum-coordination` (not `memory-coordination`) is consistent throughout the plan. Fine, but worth confirming intentional given every other crate is `memory-*`.
72
73 ---
74
75 ## Cross-Plan Consistency Findings
76
77 Stream G owns `crates/memory-substrate/src/events/log.rs` and the schema-version bump. Stream I's plan respects this — with the exception of the unowned `EventKind::ClaimLockContention` addition noted in Blocker 1.
78
79 Stream H's Task 17 (test #19, peer-update framing) explicitly skips with `"SKIP: memorum-coordination framing_tests not yet shipped (Stream I dependency)"` when Stream I hasn't landed. That guard is correct and present. Stream I's Task 20 writing into `crates/memorum-eval/` without Stream H's crate existing is a softer version of the same problem but in the opposite direction — no analogous guard is present in Task 20.
80
81 Stream G and Stream I both add `RequestPayload` variants to `crates/memoryd/src/protocol.rs`. The plan's parallel-batch ownership check will catch any direct collision, and they're in different waves, so this is managed. The rebase rule in the coordination section is correct and explicit.
82
83 ---
84
85 ## Things That Are Correct
86
87 Task 2's approach to `indexed_at` — additive struct field, no new column, no migration, `NOT NULL` invariant propagated to the Rust type — matches exactly what the shipped schema has. The column is there (`schema.rs:43`), the SELECT list in `query_recall_index` doesn't include it yet (verified at `query.rs:288`), the hydration function `row_to_recall_index_row` uses positional indexing and will need the field appended at the right position. The plan's description of what needs to change is accurate.
88
89 Task 3's two-layer whitelist + serde update in a single commit is exactly right, the tests are well-specified, and the "gibberish" rejection test is present.
90
91 The Tier 3 short-circuit design is well-specified across Tasks 5, 6, and 18. The redundancy between Task 5 and Task 18 is intentional belt-and-suspenders — Task 5 adds the behavior, Task 18 adds an explicit spy test confirming no scoring runs.
92
93 The inter-stream coordination section's rebase rule ("whichever of Stream G and Stream I lands first on `main`, the second must rebase before its own integration") is clear and operational. Task 23's rebase-check step makes it executable.
94
95 The bench fixture design in Task 21 correctly excludes embedding worker latency from the 5ms budget, which matches the spec's measurement contract. The `--write-output` / `--assert` flag split for human-authored-only baseline updates is the right pattern for this repo.
96
97 The `ClaimLockRegistry` advisory semantics (acquire always succeeds, contention returns a warning, never a refusal) are consistently applied across Tasks 12, 13, and the contention tests. That design decision is sound given the daemon-restart bypass argument in §7.1.
