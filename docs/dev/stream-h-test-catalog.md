# Stream H Test Catalog

This catalog is the developer-facing index for the Stream H eval harness. The authoritative runtime list is `TEST_CATALOG` in `crates/memorum-eval/src/orchestrator.rs`; this document mirrors it with spec anchors and the regression each test guards.

|   # | Name                               | Mode         | Group      | Spec section                                            | Regression guarded                                                                                             |
| --: | ---------------------------------- | ------------ | ---------- | ------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
|  01 | Exact identifier recall            | simulator    | handbook   | Stream H spec §3.1 test #1                              | Passive recall index staleness after high write volume; exact sentinel lost after repeated session boundaries. |
|  02 | Superseded fact handling           | simulator    | handbook   | Stream H spec §3.1 test #2                              | Superseded facts resurfacing in search or startup recall; broken supersession chain direction.                 |
|  03 | Cross-project entity collision     | simulator    | handbook   | Stream H spec §3.1 test #3                              | Namespace leakage when two projects share entity names or aliases.                                             |
|  04 | Abstention                         | simulator    | handbook   | Stream H spec §3.1 test #4                              | Fabricated memories or malformed empty recall blocks for topics with no memory.                                |
|  05 | Poisoned candidate                 | simulator    | handbook   | Stream H spec §3.1 test #5                              | Low-confidence ungrounded writes bypassing governance and appearing as trusted recall.                         |
|  06 | Tool-output preservation           | simulator    | handbook   | Stream H spec §3.1 test #6                              | Artifact handles or diagnostic tool evidence stripped during write/index/recall.                               |
|  07 | Subagent writeback                 | simulator    | handbook   | Stream H spec §3.1 test #7                              | Subagent-authored useful memories being refused, ungrounded, or omitted from later recall.                     |
|  08 | Deletion and tombstone             | simulator    | handbook   | Stream H spec §3.1 test #8                              | Forgotten memories remaining searchable or recallable after deletion/tombstone.                                |
|  09 | Recall budget pressure             | simulator    | handbook   | Stream H spec §3.1 test #9                              | Important memories dropped when recall has many competing candidates.                                          |
|  10 | Compaction resumption              | simulator    | handbook   | Stream H spec §3.1 test #10                             | Session handoff losing pending tasks or continuity after compaction-like restarts.                             |
|  11 | Self-poisoning                     | simulator    | handbook   | Stream H spec §3.1 test #11                             | Agent-generated false claims becoming trusted memory and influencing later answers.                            |
|  12 | Temporal validity                  | simulator    | handbook   | Stream H spec §3.1 test #12                             | Expired or time-bounded facts being treated as current facts.                                                  |
|  13 | Cross-harness substrate sharing    | real_harness | domain     | Stream H spec §3.2 test #13                             | Codex-observed memory not being available to Claude through the same substrate.                                |
|  14 | Merge driver semantic correctness  | simulator    | domain     | Stream H spec §3.2 test #14                             | Multi-device git merges losing memory semantics, frontmatter, or tombstone intent.                             |
|  15 | Privacy filter refusal retry       | real_harness | domain     | Stream H spec §3.2 test #15                             | Harness retry path failing to strip PII after a privacy refusal.                                               |
|  16 | Reality Check drift scoring sanity | simulator    | domain     | Stream H spec §3.2 test #16; Stream G §5.7              | Recall-hit and reality-check event signals failing to create sensible drift scores.                            |
|  17 | Lease contention resolution        | simulator    | domain     | Stream H spec §3.2 test #17                             | Two daemon instances concurrently claiming the same work without correct contention handling.                  |
|  18 | Encrypted tier key rotation        | simulator    | domain     | Stream H spec §3.2 test #18; Stream D rotation contract | Rotated encrypted-tier keys breaking fallback reads or leaving decommissioned keys active.                     |
|  19 | Peer-update framing correctness    | real_harness | regression | Stream H spec §10.1; Stream I §10.4                     | Harnesses treating peer-update context as a direct user instruction instead of attributed third-party state.   |

## Groups and scheduling

- `handbook`: the twelve handbook minimum tests. All are simulator-driven and parallel-safe.
- `domain`: Memorum-specific behavioral tests. Tests #13 and #15 are real-harness tests; #14, #17, and #18 are serial because they mutate shared temp git/key state; #16 is parallel-safe.
- `regression`: permanent tests added when a production failure escapes the harness. Test #19 is the initial Stream I framing regression slot.

The orchestrator runs the parallel group first with `--workers`, then serial tests one at a time.

## Adding to the catalog

1. Add the test file under the appropriate `crates/memorum-eval/tests/eval/<group>/` directory. Production-failure regressions go under `tests/eval/regression/t<NN>_<slug>.rs`.
2. For regression files, add the required leading `//!` metadata block: test number, incident date, description, root cause, fix commit, and asserted behavior.
3. Register the test in `TEST_CATALOG` with number, name, group, mode, and execution group.
4. Update this table and `docs/api/stream-h-eval-api.md`.
5. Run `cargo test -p memorum-eval --test regression_meta` and the orchestrator/meta tests that cover the affected group.
