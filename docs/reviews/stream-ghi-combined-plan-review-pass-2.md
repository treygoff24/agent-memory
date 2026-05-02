1 # Stream G/H/I Combined Plan Review — Pass 2
2
3 **Reviewer:** plan-reviewer (verification-only re-review, post-patch pass)
4 **Date:** 2026-05-01
5 **Verdict:** RISK
6
7 All four hard blockers from Pass 1 are resolved in the patched specs and plans. One new correctness issue was introduced by the patch. Existing risks are partially addressed. Details follow.
8
9 ---
10
11 ## Blocker Resolution Status
12
13 **B1 — phantom `supersedes_ids` column: RESOLVED.**
14
15 System spec §16.4 now explicitly states "There is no `memories.supersedes_ids` column." Stream G spec §1.3 #3 and §5.1 now define `memory_supersession(memory_id, supersedes_id)` as the join table. Stream G plan Task 2 owns the DDL, backfill, and `sync_auxiliary_tables` extension. Task 6 now uses the bounded recursive CTE over `memory_supersession` with `WHERE depth < 8`. The CTE shape is spelled out explicitly in both the Task 6 invariants and Step 2. The deferred comment in `query.rs:564` still says "Deferred: memory*supersession, memory_related, memory_regressions tables" — Task 2 Step 2 explicitly instructs the subagent to update it, so this is fine as a pre-execution state.
16
17 **B2 — stale mirror undetectable: RESOLVED.**
18
19 Stream G spec §1.3 #2 now specifies `events_log_mirror_health()` helper returning `(jsonl_max_seq, sqlite_max_seq, lag)`. Plan Task 2 owns the substrate-side helper export. Plan Task 4 owns wiring `lag > 0` into `DoctorFinding { code: "events_log_mirror_lag", repair: Some("memoryd doctor --reindex") }`. The `DoctorFinding` shape in shipped `protocol.rs:306–309` has `repair: Option<String>` — confirmed. The wiring path is coherent.
20
21 **B3 — NULL `source_harness` untested: RESOLVED.**
22
23 Stream G plan Task 6 now has two explicit NULL test cases:
24 - `test_corroboration_null_source_harness_does_not_count_as_distinct`
25 - `test_corroboration_two_non_null_harnesses_with_one_null_in_chain_yields_corroboration`
26
27 System spec §16.4 and Stream G spec §5.1 both have explicit NULL semantics paragraphs stating this is intentional. The invariant block in Task 6 also calls it out. Resolved.
28
29 **B4 — regression scan wrong path: RESOLVED.**
30
31 Stream H spec §2 now says `tests/eval/` not `src/tests/eval/`. Stream H plan Task 20 Step 1 now explicitly scans `crates/memorum-eval/tests/eval/regression/` with a note referencing the prior wrong path. Stream H plan Task 17 places `t19_peer_update_framing.rs` in `tests/eval/regression/`. Resolved.
32
33 ---
34
35 ## New Issue Introduced By The Patch
36
37 **`add_column_if_missing` call signature mismatch in Task 2 Step 2.**
38
39 The shipped function at `/Users/treygoff/Code/agent-memory/crates/memory-substrate/src/index/migrations.rs:121` has signature:
40
41 ```
42 fn add_column_if_missing(tx: &Transaction<'*>, column: &'static str, definition: &'static str)
43 ``  44 
  45 It takes 3 arguments and is hardcoded to the `memories` table internally.
  46 
  47 Stream G plan Task 2 Step 2 instructs the subagent to call it as:
  48 
  49``
50 add_column_if_missing(&tx, "memories", "original_confidence", "REAL")
51 ```  52 
  53 That is 4 arguments with an explicit table parameter. This will not compile against the shipped helper. The subagent will either (a) fail at compilation and spend time debugging a phantom API, (b) silently rename the call to skip the table argument and get the wrong semantics, or (c) modify the helper to accept a table parameter — which is a valid fix but changes a shared function used by migrations v2 and v3 and requires updating those call sites.
  54 
  55 The correct Task 2 instruction is:`add_column_if_missing(&tx, "original_confidence", "REAL")`— matching the existing 3-argument form. If the subagent needs to add`original_confidence`to a different table, the helper must be extended and existing call sites updated.
  56 
  57 **Fix:** Change Task 2 Step 2 to call`add_column_if_missing(&tx, "original_confidence", "REAL")`, or explicitly instruct the subagent to generalize the helper to accept a table name and update the existing call sites in `migrate_v2`and`migrate_v3`to pass`"memories"`explicitly.
  58 
  59 ---
  60 
  61 ## Risk Status
  62 
  63 **R6 (bench bootstrap) — RESOLVED.** Task 17 Step 2 now explicitly documents the`.proposed`bootstrap path when`--assert`is invoked with a missing baseline. The behavior matches the Stream A precedent. The verification plan's sequencing is correct: first-run emits`.proposed`and exits 0; canonical baseline is committed manually; subsequent`--assert`runs against the canonical file.
  64 
  65 **R7 (18/18 vs 19 tests) — RESOLVED.** System spec §20.4 now reads "19 tests; mock mode skips #13/#15/#19."
  66 
  67 **Finding 1 (Stream I crate name) — RESOLVED.** System spec §19 now says`crates/memorum-coordination/`.
  68 
  69 **Finding 3 (src/tests path) — RESOLVED** (same as B4 above).
  70 
  71 **Finding 4 (MockHarness #19 cfg gate) — RESOLVED.** Stream H Task 7 Step 3 now gates the `MockHarness`test #19 path behind`#[cfg(feature = "stream-i-deps")]`.
  72 
  73 **R1 (cargo feature hygiene) — still open.** The `memoryd-tui/Cargo.toml`dependency on`memoryd`with`features = ["test-utils", "stream-g-events"]`is defined in Stream H's cargo file, not`memoryd-tui`'s. Stream G plan Task 10 says `memoryd-tui`has a`memoryd`library dependency but does not address whether`test-utils`features leak into production consumers. Not a blocker; flag to Codex to verify`memoryd`builds cleanly as a library without test-utils features when consumed by non-test crates.
  74 
  75 **R2 (migration backfill under concurrent writes) — still open.** Migration v4 backfills from JSONL by iterating all events files. No chunked-backfill strategy. Fine for v1 scale; should appear in bench evidence.
  76 
  77 **R3 (depth-bounding logic) — RESOLVED** for the`memory_supersession` table. The bounded CTE shape (`WITH RECURSIVE chain(memory_id, depth) AS (... WHERE depth < 8)`) is now spelled out explicitly in Task 6 invariants and Step 2.
  78 
  79 **R4 (stream-g-events CI build) — still open.** The CI workflow YAML does not specify which feature set the eval crate builds with. The `stream-g-events`feature gate on`InjectableEventKind::RecallHit`means the real injection path may never be exercised in CI until Stream G ships. Acceptable as a sequencing choice; should be documented in the CI workflow.
  80 
  81 **R5 (source_device None treated as same-device) — still open.** Unchanged; acceptable for v1.
  82 
  83 ---
  84 
  85 ## Nits
  86 
  87 The nit from Pass 1 about`add_column_if_missing`being implicit in Task 2 Step 2 has been upgraded to a blocker above — it is not a prose issue anymore, it is a wrong API call.
  88 
  89 The "Level vs. Tier conflation" nit in Stream I plan Tasks 7 and 17 is still present. It will not cause a compilation failure, but an implementer could infer wrong cross-stream dependency ordering. One-line clarification in each invariant block would close it.
  90 
  91 Stream G plan line 24 has a note: "The Stream G spec is the implementation contract; the system spec row is a pre-patch approximation." This is the right call and the right place to document it. No action needed.
  92 
  93 ---
  94 
  95 ## What's Good
  96 
  97 All four blockers are cleanly addressed with the right technical choices —`memory_supersession`as a derived projection is the correct long-term shape (not the depth-1 shortcut),`events_log_mirror_health()` as a substrate helper with the Doctor wiring in a separate task is the correct seam, and the two NULL test cases added to Task 6 precisely match the SQL semantics rather than papering over them. The fact that all fixes are purely additive — no renamed functions, no removed variants, no schema column drops — is exactly the right approach for a substrate this tightly constrained. The cycle-guard documentation (depth-bound IS the cycle guard) is explicit and correct.
98
