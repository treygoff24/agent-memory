1 # Stream G/H/I Combined Plan Review
2
3 **Reviewer:** plan-reviewer (combined pass, post-patch)
4 **Date:** 2026-05-01
5 **Verdict:** BLOCK
6
7 One blocker is hard enough to derail Stream G's entire drift-scoring feature before Task 6 even runs. Three others are correctness problems the patches introduced. The rest are risks, not showstoppers.
8
9 ---
10
11 ## Blockers
12
13 **1. `distinct_sources` recursive CTE over `memories.supersedes_ids` references a column that does not exist.**
14
15 The system spec §16.4 and Stream G plan Task 6 both describe a recursive CTE walking `memories.supersedes_ids` to compute corroboration. That column does not exist. The `memories` table has no `supersedes_ids` column; supersession relationships live only in the Markdown frontmatter (`Frontmatter.supersedes: Vec<MemoryId>`, `Frontmatter.superseded_by: Vec<MemoryId>`) and are explicitly deferred from the index per `/Users/treygoff/Code/agent-memory/crates/memory-substrate/src/index/query.rs:564`: "Deferred: memory*supersession, memory_related, memory_regressions tables." The schema at `/Users/treygoff/Code/agent-memory/crates/memory-substrate/src/index/schema.rs` has no `memory_supersession` table and no `supersedes_ids` column on `memories`.
16
17 Stream G plan Task 6 Step 2 says: "a recursive CTE on `memories` walking `supersedes_ids` to depth 8, computing `COUNT(DISTINCT source_harness)` per leaf memory." This will produce a SQL error at execution time because neither `supersedes_ids` nor the join table exist.
18
19 The `source_harness` column itself does exist as `memories.source_harness` (confirmed in the INSERT path at `/Users/treygoff/Code/agent-memory/crates/memory-substrate/src/index/query.rs:449` and the schema at `/Users/treygoff/Code/agent-memory/crates/memory-substrate/src/index/schema.rs:37`). But the *traversal* path is broken.
20
21 Fix options: (a) add `memory_supersession(memory_id, supersedes_id)` as a derived projection in migration v4 alongside `events_log` and populate it from the frontmatter `supersedes` list — this is straightforward since `sync_auxiliary_tables` already does analogous sync for tags/entities/evidence. (b) Scope the corroboration check to depth 1 only: `SELECT COUNT(DISTINCT m2.source_harness) FROM memories m1 JOIN memory_supersession ms ON m1.id = ms.memory_id JOIN memories m2 ON ms.supersedes_id = m2.id WHERE m1.id = ?`. Option (a) is the right long-term shape; option (b) is a defensible v1 shortcut. Either way, the spec and plan must be updated to acknowledge this before Task 6 starts.
22
23 Files affected: `/Users/treygoff/Code/agent-memory/docs/specs/stream-g-observability-v0.1.md` §5.1, `/Users/treygoff/Code/agent-memory/docs/specs/system-v0.2.md` §16.4, `/Users/treygoff/Code/agent-memory/docs/plans/2026-05-01-stream-g-observability.md` Task 6.
24
25 **2. The `events_log` dual-write failure mode leaves the SQLite mirror silently stale with no detection.**
26
27 Stream G spec §1.3 #2 and plan Task 2 both say: "if SQLite write fails, log the error but do not roll back the JSONL append." This is correct as a durability choice. However, the plan provides no mechanism for the system to know the mirror is behind. `doctor --reindex` is described as the recovery path, but nothing triggers it automatically after a failed SQLite write, and nothing in the `StatusResponse` or `DoctorResponse` shapes exposes a "mirror lag" indicator. The scoring queries in Task 6 run against the mirror. If the mirror is stale by even a few events, `recall_count_30d` is wrong and drift scores are silently incorrect.
28
29 The fix is small: add a `events_log_rows` count to the `DoctorResponse` or expose a `mirror_lag` bool in `DaemonState`. That way `memoryd doctor` surfaces the divergence, and the operator or a future health-check can trigger `--reindex` intentionally. Without this, the mirror's consistency is unobservable.
30
31 Files: `/Users/treygoff/Code/agent-memory/docs/specs/stream-g-observability-v0.1.md` §5.8, plan Task 4 (`DaemonState`), plan Task 6 (`scoring` — must acknowledge that scores from a stale mirror are still valid per the spec's "conservative floor" approach, OR add the health indicator).
32
33 **3. `source_harness` is `Optional` in practice but Stream G's corroboration formula treats it as reliably non-null.**
34
35 Looking at `/Users/treygoff/Code/agent-memory/crates/memory-substrate/src/model.rs:257`, `Source.harness` is `Option<String>`. The INSERT path at `/Users/treygoff/Code/agent-memory/crates/memory-substrate/src/index/query.rs:504` writes `&memory.frontmatter.source.harness` directly — meaning `memories.source_harness` is NULL for any memory whose frontmatter omitted the harness field. This is a real code path: notes written via `memory_note` go through `WriteNote` not `WriteMemory`, and the harness field on those is plausibly absent.
36
37 Stream G plan Task 6 `test_corroboration_requires_two_distinct_harnesses` and `test_corroboration_satisfied_by_two_harnesses` assume `source_harness` is populated. If a supersession chain includes one memory with NULL `source_harness`, `COUNT(DISTINCT source_harness)` with NULL semantics returns a count that excludes NULLs — so two memories where one has `harness=null` and one has `harness='codex'` would return a distinct count of 1, not 2. This is probably correct behavior (NULL is not a distinct harness identity), but it is not documented in the spec or tested explicitly. The test vectors in Task 6 need a case covering `NULL source_harness` to confirm the formula's behavior is intentional.
38
39 Files: `/Users/treygoff/Code/agent-memory/docs/specs/stream-g-observability-v0.1.md` §5.1, plan Task 6 RED tests.
40
41 **4. Stream H Task 20 regression meta-test scans `src/tests/eval/regression/` but the plan places files under `crates/memorum-eval/tests/eval/regression/`.**
42
43 Stream H plan Task 20 Step 1 says: "Test that any file in `src/tests/eval/regression/` named `t<NN>**.rs`contains a`//!`doc-comment block." But the crate layout the plan specifies throughout (and the spec's §2 layout) puts regression tests under`crates/memorum-eval/tests/eval/regression/`— the`tests/`directory, not`src/tests/`. The regression meta-test will scan the wrong path and always pass vacuously. This is a previous-finding regression from the "test files moved from `src/tests/`to`tests/`" fix that the plan summary claims was applied globally: the meta-test's scan path was not updated to match.
44
45 Files: `/Users/treygoff/Code/agent-memory/docs/plans/2026-05-01-stream-h-eval-harness.md`Task 20 Step 1. Fix: change the scanned path in the meta-test to`crates/memorum-eval/tests/eval/regression/`.
46
47 ---
48
49 ## Risks
50
51 **R1. `memoryd-tui`depends on`memoryd`as a library, but`memoryd`'s `Cargo.toml`is the same file Stream G Task 8 modifies (adds`reqwest`, `lettre`) and Stream H Task 5 modifies (adds features). These are sequential, not a parallel risk during execution. But the `memoryd-tui/Cargo.toml`dependency declaration`memoryd = { path = "../memoryd", features = ["test-utils", "stream-g-events"] }`is stated in Stream H's`Cargo.toml`, not `memoryd-tui`'s.** Stream G plan Task 10 says `memoryd-tui`depends on`memoryd`as a library dependency. Pulling`ratatui`/`crossterm`into`memoryd-tui`while`memoryd`itself is a library dep means the entire`memoryd`library's compilation becomes transitively coupled to any feature flags on the binary side. Verify that`memoryd`builds as a library without`test-utils`features leaking into production consumers. This is a cargo feature hygiene issue, not a showstopper, but it will surface as a compilation warning or, worse, a silently-included test surface.
52
53 **R2. Migration v4 backfill correctness under concurrent writes is unspecified.** Stream G plan Task 2 says the backfill runs during migration. The migration holds a`BEGIN IMMEDIATE`transaction, which blocks writes from other connections. On large repos with many JSONL events, this could be a seconds-long hold. The plan does not specify a timeout or a chunked-backfill strategy. This is fine for v1 dogfood scale; flag it as a known limitation in the bench evidence.
54
55 **R3. The`test_corroboration_walks_supersession_chain_depth_bounded`test in Stream G Task 6 asserts "recursion stops at depth 8 without panicking" — but if the`memory_supersession` table does not exist (see Blocker 1), this test cannot pass at all.** Even after the fix for Blocker 1, this test is asserting against the depth-bounding logic of the recursive CTE, which must be explicitly written as a bounded CTE (`WITH RECURSIVE ... WHERE depth < 8`). The plan does not mention writing this CTE shape explicitly; the subagent needs clear instruction to write it as a bounded recursive CTE, not an unbounded walk.
56
57 **R4. Stream H Task 5's `stream-g-events`cargo feature approach for`InjectableEventKind::RecallHit`creates a split test surface.** When`stream-g-events`is disabled,`inject_event_smoke.rs`tests the stub path, not the real injection. When it is enabled, the test runs the real variant. The CI workflow does not specify which feature set the eval crate builds with for the main-branch daily run. If it builds without`stream-g-events`, the inject path is never exercised in CI until Stream G ships. This is probably fine as a sequencing choice, but it should be documented explicitly in the CI workflow YAML.
58
59 \*\*R5. Stream I plan Task 16 uses `RecallIndexRow.source_device`to split same-device from cross-device peer writes. But`source_device`on the`memories` table is the device that *wrote\* the memory, which is populated from `Frontmatter.source.device`. On a single-device setup, this field will be None for memories that predate Stream A's device-id writing.** The plan says "same-device (`source_device == Some(local_device_id) || source_device == None`)" — treating None as same-device. That is a reasonable default but will include legitimately cross-device memories with missing device attribution in the "same-device" bucket. For a v1 local-first use case this is probably acceptable; flag in docs as a known gap.
60
61 **R6. Stream G's bench binary in Task 17 asserts against `bench/stream-g-observability-results.darwin-arm64.json`, which must be created before the assertion mode can succeed.** The plan says the file is "Created" by the task via `--write-output`. But the task's verification plan says to run assertion mode first (`--assert`), which would fail if the file doesn't exist yet. The plan needs to sequence `--write-output` before `--assert` in the first run, or handle a missing baseline with a "bootstrap path emits `.proposed`" fallback as the existing `bench/baseline.linux-x86_64.json` does. Blocker if left unspecified for the release gate.
62
63 **R7. System spec §20.4 says "Eval harness passes 18/18 every day" but Stream H's test catalog is 19 tests.** Test #19 (peer-update framing) is the 19th, and the system spec's dogfood pass criterion says 18. This is a spec inconsistency: either the dogfood criterion should be updated to 19, or test #19 is explicitly counted as a bonus/skip in mock mode. Stream H plan Task 18 says mock mode skips #13, #15, #19 → 16 passed, 3 skipped. The system spec's "18/18" was written before test #19 was added. Update system spec §20.4 to "19 tests; passing criterion: no failures; mock-mode run counts 16 passed, 3 skipped as partial:true."
64
65 ---
66
67 ## Nits
68
69 Stream G's plan Task 2 description says migration v4 adds `original_confidence REAL` to `memories` — but the spec §1.3 #3 says it is added in the same migration, and the `SCHEMA_SQL` template in `schema.rs` has no `original_confidence` column. The plan's Step 2 only mentions adding it to `SCHEMA_SQL`; it does not explicitly show `add_column_if_missing(&tx, "original_confidence", "REAL")` in the migration function. Given that `schema.rs` is for fresh DB creation and the `migrate_vN` functions handle upgrades, this column addition needs to appear explicitly in `migrate_v4`, not just in `SCHEMA_SQL`. Stream G plan Task 2 Step 2 should spell this out.
70
71 Stream I plan Task 7 says `session.rs`'s path-derivation accesses `CoordinationContext.session_paths` "populated by Level 3 heartbeat" but Task 7's invariant also says "Tier 1 salient paths = ... UNION tool-call file paths from `CoordinationContext.session_paths` (populated by Level 3 heartbeat; empty at Level 2 without heartbeat)." Level 2 and Level 3 are `CoordinationLevel` values, not tiers — the text conflates "Level" (coordination level from per-project config) with "Tier" (harness tier from §10). A reader implementing this could infer the wrong thing. Throughout the plan, "Tier 3" means harness tier (Cursor etc.), "Level 3" means collaboration mode. They are different axes. This conflation appears in several places in both Task 7 and Task 17.
72
73 ---
74
75 ## Cross-plan/spec consistency findings
76
77 **Finding 1: System spec §19 authorization table says Stream I "owns daemon-side relevance-gate logic (new module under `crates/memoryd/src/peer/`)".** But Stream I plan Task 4 creates `crates/memorum-coordination/` as a standalone crate, not a module inside `memoryd`. This is the correct technical choice (separate crate keeps `memoryd` free of the scoring logic's dependencies), but the system spec §19 row is wrong. Not a blocker for execution since both the stream spec and plan agree on `crates/memorum-coordination/`; the system spec row is just stale prose.
78
79 **Finding 2: Stream G Task 6 description says "Two aggregate queries prepared once per scoring run" for `distinct_sources`, including "a recursive CTE on `memories` walking `supersedes_ids` to depth 8."** The system spec §16.4 says "joined recursively across `memories.supersedes_ids`." Both the spec and the plan use identical phantom column name `supersedes_ids` — this is not a spec-vs-plan inconsistency; both are wrong in the same way. See Blocker 1.
80
81 **Finding 3: Stream H spec §2 crate layout places test files under `src/tests/eval/`.** But Stream H plan's "Non-blocking coordination" section and all plan tasks place them under `tests/eval/`. These two documents contradict each other. The plan is correct (cargo `--test <name>` resolves from the `tests/` directory, not `src/tests/`). The spec layout diagram at `/Users/treygoff/Code/agent-memory/docs/specs/stream-h-eval-harness-v0.1.md` §2 still shows the `src/tests/eval/` path. This is a documentation inconsistency. The spec layout diagram should be corrected to match the plan, or the plan-reviewer's prior fix that moved tests to `tests/eval/` was only partially applied.
82
83 **Finding 4: Stream H plan Task 7 MockHarness description says: "For test #19: directly call the daemon to construct a `<memory-delta>` containing a synthetic `<peer-update>` and run the Stream I assertion function `assert_framing` on it."** But Stream H plan Task 17 says test #19 is gated behind `cfg(feature = "stream-i-deps")` — meaning when the feature is off (the default until Stream I lands), MockHarness running test #19 would fail to compile if it has an unconditional reference to `assert_framing`. Task 7's MockHarness implementation for test #19 needs to also be behind `#[cfg(feature = "stream-i-deps")]`. This is not mentioned in Task 7's owned-files or invariants.
84
85 **Finding 5: Stream G plan Task 3 says emission is "fire-and-forget" and if the event log write fails, log WARN and continue.\*\* The dual-write semantics in Task 2 say JSONL is canonical and SQLite is derived. But Task 3's `RecallHit` emission goes through `events::log::append`, which (as of the current code at `/Users/treygoff/Code/agent-memory/crates/memory-substrate/src/events/log.rs:172`) writes to JSONL only. After Task 2 adds the dual-write seam, `events::log::append` will also attempt a SQLite write. If the SQLite write fails in the recall hot-path, Task 3's "fire-and-forget" only applies to the outer `WARN` — the JSONL write already succeeded. This is fine. But the test `test_recall_output_xml_unchanged` must be run against the dual-write version of `events::log::append`, not a pre-Task-2 version. The task ordering (Task 2 before Task 3) ensures this. Confirmed: no execution ordering problem here; just documenting the dependency is correctly sequenced.
86
87 ---
88
89 ## What's correct
90
91 The `MethodNotAllowedOnMcp` ownership is clean. Stream G Task 5 adds the variant, Stream H and Stream I reuse it. The protocol error struct is currently a plain `{ code, message, retryable }` tuple at `/Users/treygoff/Code/agent-memory/crates/memoryd/src/protocol.rs:415`, so this variant is genuinely absent today and Task 5 is the right place to introduce it.
92
93 `indexed_at` and `source_device` on `memories` are confirmed real columns in the shipped schema at `/Users/treygoff/Code/agent-memory/crates/memory-substrate/src/index/schema.rs:43-44`. Stream I Task 2's invariant ("No new column. Both already exist in the shipped schema.") is correct.
94
95 The SELECT projection in `query_recall_index` at `/Users/treygoff/Code/agent-memory/crates/memory-substrate/src/index/query.rs:288-292` confirms that `indexed_at` and `source_device` are currently absent from the projection and absent from `RecallIndexRow` at `/Users/treygoff/Code/agent-memory/crates/memory-substrate/src/model.rs:1219-1258`. Stream I Task 2 adds both additively with no schema migration, which matches the spec's claim exactly.
96
97 `INDEX_SUPPORTED_SCHEMA_VERSION` is confirmed at 3 in `/Users/treygoff/Code/agent-memory/crates/memory-substrate/src/index/migrations.rs:15`. Stream G Task 2 bumping it to 4 is the correct single-point-of-truth change.
98
99 The Stream H CI field-name corrections (`.number`, `.failure_detail` replacing `.test_id`, `.failure_reason`) are wired into the `ci_workflow_shape` meta-test in Task 19, which means the fix will hold across regressions. That is the right shape for a brittle YAML assertion.
100
101 Stream H Task 5's `stream-i-deps` feature gate for the `memorum-coordination` dependency is correctly modeled: the compile-time guard prevents build breakage when Stream I hasn't shipped, and the runtime skip-guard communicates clearly to CI. The approach is sound.
102
103 The Stream I stop condition that says "stop if spec §1.1's claim that `memories.indexed_at TEXT NOT NULL` already exists is incorrect" is validated: the column exists, the stop condition will not trigger.
104
