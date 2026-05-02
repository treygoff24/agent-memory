1 # Stream H v0.1 spec review
2
3 **Reviewer:** plan-reviewer (Claude Sonnet 4.6, fresh context)
4 **Date:** 2026-05-01
5 **Spec:** docs/specs/stream-h-eval-harness-v0.1.md (1182 lines)
6 **Verdict:** BLOCK
7
8 ---
9
10 ## Blockers
11
12 ### 1. Test #16 asserts on a field that doesn't exist
13
14 Test #16 (drift scoring sanity) requires writing memories with a controlled `recall_count_30d` value to exercise the inverse-recall-frequency component of the drift formula. Step 1 says "Write the three memories via `memory_write` with metadata reflecting the above characteristics" including "recall count in last 30 days = high (simulated by writing recall-count metadata)."
15
16 There is no such writable metadata field. Stream E §15 explicitly defers "persistent recall-count and last-recalled mutation" — it is not implemented. `recall_count_30d` does not appear in Stream A's frontmatter schema, the governance metadata accepted by `memory_write`, or anywhere in the shipped codebase. Stream G's scoring formula references `m.recall_count_30d` as an index field, but Stream G's spec §5.1 says it comes from the passive-recall index — which Stream E deferred.
17
18 The consequence: the inverse-recall-frequency weight (0.20) cannot be exercised at all. Test #16's assertion 5 ("Assert each component is computed independently") is unverifiable. Memory A and Memory B will produce identical `recall_frequency_norm(m)` values (both zero, since no recall events have occurred), which means assertion 1's ordering may still hold (Memory B beats Memory C beats Memory A), but for the wrong reason — only the staleness, corroboration, confidence, and sensitivity components will differentiate them, not all five.
19
20 More specifically: Stream G §5.1 defines `recall_frequency_norm(m) = m.recall_count_30d / max(max_recall_30d_active, 1)`. If `max_recall_30d_active = 0` (all memories have zero recall, because the counter isn't tracked), then `max(0, 1) = 1` and the formula gives `0 / 1 = 0` for all memories. The recall component contributes identically 0.20 to all scores, which is the maximal contribution (not a varied one). This doesn't necessarily break the ordering between A, B, and C — but it does mean the test is not actually testing what it claims to test.
21
22 **Fix:** either defer test #16 until Stream E ships persistent recall counts (which means the test catalog needs a placeholder and the CI count stays at 17 until then), or redefine test #16 to test only the four components that are verifiable today (staleness, corroboration, confidence decay, sensitivity) with an explicit note that the recall-frequency component is untested. The latter is acceptable as a v1 spec choice but the spec must state it explicitly instead of claiming the test "asserts each component is computed independently."
23
24 **Section reference:** §3.2 test #16, step 1; Stream E spec §15; Stream G spec §5.1.
25
26 ---
27
28 ### 2. The CI gate script breaks in mock-only mode
29
30 The "Fail if eval did not pass" step in §7.2 uses:
31
32 `bash
  33 STATUS=$(jq -r '.passed == .total' eval-results-mock.json)
  34 [ "$STATUS" = "true" ] || (echo "Eval harness failed" && exit 1)
  35 `
36
37 In mock mode with 2 real-harness tests skipped, the JSON output has `total: 18, passed: 16, skipped: 2`. The expression `.passed == .total` evaluates to `false`. The step exits 1 and fails the job — even though §6.4 says "A `partial: true` run exits 0 in mock harness mode."
38
39 There is a direct contradiction between the exit-code spec (exit 0 on partial-mock) and the gate script logic (fails when passed != total). Daily cron runs in mock mode will fail this gate every day, permanently, until fixed.
40
41 The correct expression for mock mode should be something like `.failed == 0` or `.partial == false or .skipped == 0`, not `.passed == .total`. For RC tags (full run), `.passed == .total` is correct because §7.5 already guarantees the full run errors out before this step if secrets are missing (via the binary's own exit 1). But the script doesn't distinguish between the two cases.
42
43 **Fix:** change the gate script to check `(.failed == 0 and .partial == false) or (.partial == true and .harness_mode == "mock" and .failed == 0)`. Or split into two separate steps: one for mock validation (`failed == 0`), one for full validation only when the full results file exists (`passed == total`).
44
45 **Section reference:** §6.2, §6.4, §7.2.
46
47 ---
48
49 ### 3. Test #18 forward-secrecy assertion requires key rotation semantics that aren't specified in Stream D
50
51 Test #18 step 5 asserts "the old key cannot decrypt new content" — a forward secrecy property. Stream D's spec has no section on key rotation semantics. `memoryd device rotate-keys` appears in the Stream D API doc (docs/api/stream-d-privacy-api.md) as a CLI command but is never specified: what does it do cryptographically? Does it re-encrypt existing ciphertext with the new key? Does it decommission the old key immediately? Does the daemon retain the old key for decrypt-only access to pre-rotation content?
52
53 The test's assertion 3 assumes the daemon can still read old content after rotation ("Key rotation does not make existing memories unreadable"). The test's assertion 4 assumes the old key has been decommissioned for encryption purposes ("decommissioned key has no access to post-rotation content"). These two behaviors together define a specific key rotation model — forward secrecy with re-encryption of existing content, or forward secrecy with retained old key for decrypt but not encrypt. Neither model is specified anywhere.
54
55 This is not a "defer to implementation" detail. The test is asserting a cryptographic invariant, which means the spec needs to state what invariant holds. Without that, the implementation could make any choice (e.g., just swap the active key pointer with no decommissioning at all, which would make assertion 4 fail silently), and test #18 step 5 would be implementing undefined behavior.
56
57 **Fix:** Stream D's spec (or the Stream H spec itself, in §3.2 test #18's preamble) must state the key rotation contract: what `memoryd device rotate-keys` does to existing ciphertext, whether the old key is retained for decrypt-only, and whether new writes are guaranteed to use the new key atomically after the rotate command returns. Without this, test #18 step 5 is asserting against an unspecified wall.
58
59 **Section reference:** §3.2 test #18, steps 3–5; docs/specs/stream-d-privacy-v0.1.md (no rotation section exists).
60
61 ---
62
63 ### 4. Test #16 step 2 names a command that doesn't exist in the daemon protocol
64
65 Step 2 of test #16 says "Trigger a Reality Check via `memoryd reality-check run` (CLI equivalent in daemon protocol)." Then step 3 says "Retrieve the drift-scored results for the temp tree's namespaces."
66
67 The problem is that test #16 runs in simulator mode, which means it drives `memoryd` via Unix socket protocol directly (§4.3, §1.1). But `memoryd reality-check run` is a CLI command, not a daemon protocol `RequestPayload` variant. Stream G defines `GET /api/reality-check` over the web API and defines `memoryd reality-check run --json` as a CLI command. Neither of these is a daemon socket protocol variant.
68
69 Stream G's daemon protocol section (its §3, covering which `RequestPayload` variants it adds) does not add a `RealityCheckRun` variant to the daemon protocol. The CLI command calls the daemon indirectly. A simulator-driven test cannot call a CLI command — it can only send `RequestPayload` frames to the Unix socket.
70
71 Test #16 either needs to be redesigned as a unit test against the scoring library directly (bypassing the daemon entirely and calling the scoring function in `crates/memoryd-ui/` or wherever Stream G puts it), or the spec needs to acknowledge that Stream G must expose a `RequestPayload::RealityCheckRun` variant. The spec currently says "Stream H does not own MCP tool additions, daemon protocol extensions, or CLI command additions" (§1). If Stream G hasn't shipped yet and Stream G doesn't define a daemon protocol variant for this, test #16 has no executable path to invoke reality-check from a simulator.
72
73 **Fix:** either (a) declare test #16 as a unit test that calls the scoring function directly (without going through the daemon), or (b) file a cross-stream requirement on Stream G to expose `RequestPayload::RealityCheckRun` as part of its daemon protocol surface, or (c) redesign test #16 to use `memoryd reality-check run --json` via subprocess (which would put it in the real-harness mode, not simulator mode, and requires a different execution group).
74
75 **Section reference:** §3.2 test #16 step 2; §1 (ownership boundaries); §4.3 (simulator drives daemon socket only); docs/specs/stream-g-observability-v0.1.md §5.
76
77 ---
78
79 ### 5. Stream I framing tests are claimed by Stream I but there's no test slot for them in Stream H's catalog
80
81 Stream I §10.1 says: "The framing suite is designed to be run by Stream H's eval harness against real harnesses. Stream I owns the test design, the prompt fixtures, the pass criteria, and the assertion logic. Stream H owns the runtime that invokes the harnesses and collects results."
82
83 Stream I §10.4 defines `assert_framing` in `crates/memorum-coordination/src/framing_tests.rs` and the full six-case sampling matrix (6 harness × temperature combinations, each run 3 times for majority vote). Stream H's §10.1 (open questions) says the peer-update framing tests "which crate owns them?" is deferred to Stream I's contract — and now Stream I's contract is written and has a clear answer.
84
85 But Stream H's test catalog has 18 tests and explicitly declares the count locked. Stream I's framing test suite is a real-harness test suite requiring `claude -p` and `codex exec`, running 18 total invocations (6 cases × 3 runs), with a structured assertion function. This is functionally a set of additional tests in Stream H's eval runtime — but the catalog doesn't include them and the orchestrator binary doesn't enumerate them.
86
87 This is not a post-v1 concern. System-v0.2 §20.4 dogfood pass criteria explicitly includes "Cross-session peer-update fires at least once and is correctly framed as third-party." Stream I §10.5 says "for temperature 0.0: the failure is a spec-level correctness defect. Stream I must not ship until it is resolved." Stream I's tests must run somewhere before 1.0.0.
88
89 The spec acknowledges the open question was deferred (§10.1) but now that Stream I is written, this is resolved: the framing tests need a home in the eval harness. Either the catalog is expanded beyond 18 (with an explicit exception in the locking convention), or Stream I's acceptance tests (§11) include them and they're run separately from Stream H. The current state leaves the framing tests in a documented gap.
90
91 **Fix:** update §10.1 to state the resolved answer from Stream I: Stream I ships framing tests as test #19+ in the eval harness, with Stream I owning the prompt fixtures and assertion logic and Stream H owning the runner. Update §6.1 and §9.1 (orchestrator smoke test) to account for the expanded count. Or explicitly document that framing tests live in a separate binary in `crates/memorum-coordination/` and are gated independently.
92
93 **Section reference:** §10.1; docs/specs/stream-i-cross-session-v0.1.md §10.1, §10.4.
94
95 ---
96
97 ## Risks
98
99 ### R1. The CI "Fail if eval did not pass" check silently passes a partial RC run
100
101 There's a related bug to Blocker #2 but in the opposite direction. §7.5 says: "If the eval workflow passes but `partial: true` (real-harness tests skipped due to missing secrets), the workflow fails with a message: 'RC eval run is partial.'"
102
103 But look at the gate script: it reads `eval-results-full.json` when that file exists. The full-run step only fires when the tag trigger or workflow_dispatch with non-mock mode is set. If the secrets are missing but the step runs anyway (which is possible if someone adds the real-harness step to the cron trigger), `--harness all` will exit 1 from the binary (per §6.4), which will fail the step and halt before the "Fail if eval did not pass" step even runs — and the job fails for the wrong reason, with no mention of "partial." The spec's §7.5 message would never appear.
104
105 The actual enforcement of the "RC partial is a failure" rule is in the binary's exit behavior, not in the gate script. That's fine, but the spec claims the message "RC eval run is partial — MEMORUM_EVAL_CLAUDE_KEY..." appears in the workflow. It can't appear from a jq check; it would have to come from within the binary's stderr or a separate check step. The spec needs to clarify where this message is emitted.
106
107 ### R2. Test #13 asserts on Claude's structured JSON output, but the prompt template doesn't specify how to wire the MCP server
108
109 Test #13 step 3 says Claude is invoked with a prompt template that "instructs it to call `memory_startup` and then `memory_search`... and to output a JSON object." The prompt template delivers the socket path via `{{SOCKET_PATH}}` substitution. But `claude -p` needs to be configured to use a specific MCP server before it can call `memory_startup` as a tool.
110
111 §5.1 says "The prompt template for each test includes an instruction to configure the MCP connection accordingly — or the `HarnessRunner` injects a harness-specific MCP configuration into the environment before spawning." This is an unresolved "or": either the prompt template configures it (by instructing the agent to configure MCP at runtime, which isn't how `claude -p` MCP configuration works — it's done via config files, not runtime prompts), or the HarnessRunner injects a config file.
112
113 The "or" here isn't a style choice — it's two fundamentally different implementation approaches. `claude -p` reads MCP server configuration from a settings file or `--mcp-server` flag. A prompt template cannot configure the MCP transport layer at the protocol level. The HarnessRunner must inject a config or pass a flag. The spec needs to commit to one approach. If it's config injection, the spec needs to define where the config file is written (temp path, same as the temp tree) and what format. If it's a flag, `--mcp-server` needs to be in the invocation spec.
114
115 ### R3. MockHarness parity test (§9.5) only runs when auth secrets are present, making it expensive for contributors to validate
116
117 The MockHarness parity test requires running both `--harness mock` and `--harness claude` and comparing outcomes. This requires auth on the developer's machine. Any contributor who can't authenticate Claude locally cannot run the parity test, which means they can't verify that a MockHarness change doesn't diverge from real behavior.
118
119 This is a structural problem: the test that validates the mock is correct only runs where the mock's purpose is least needed. The spec should acknowledge this and either (a) provide a cached baseline of "what real harness produced" (committed to the repo as a golden file) that the mock can be diffed against, or (b) weaken the parity test to only check mock/real consistency on daemon-state assertions (not on full JSON output), which is runnable without auth.
120
121 ### R4. Test #3 cross-project entity collision setup requires fixture-injected project bindings
122
123 Test #3 setup says: "simulated by two separate `memory_startup` calls with different `cwd` values that resolve to different canonical project ids via the git-remote canonicalization path, or by fixture-injecting project bindings into the temp tree's `config.yaml`."
124
125 The "or" here matters. The git-remote canonicalization path requires two actual git repos with different remotes. A fresh temp tree from `DaemonScaffold::fresh()` won't have git remotes — it's just an empty directory. The fixture-injection path means the test bakes in project id values and bypasses the actual canonicalization logic, which is the exact behavior the test is supposed to exercise.
126
127 If the test uses fixture injection, it's not testing cross-project collision at the canonicalization layer; it's testing that the namespace filter works when given two hard-coded project ids. That's still a useful test, but the spec should commit to which approach and explain why the git-remote path is or isn't used. The "or" reads like the author wasn't sure.
128
129 ### R5. Test #17 races with real time: two `memoryd` processes sharing a git remote need coordination the spec doesn't define
130
131 Test #17 steps 1 and 2 send `DreamNow` to two separate daemon instances and say "immediately (before Device A's dream run completes)." Dream pass 1 involves git operations (`git fetch origin`, file reads, LLM call). These are not instantaneous. The test relies on the two `DreamNow` calls being close enough in time that the lease hasn't been written and pushed before Device B sends its request.
132
133 On a loaded CI machine, the first daemon might acquire the lease and push it before the second daemon sends its `DreamNow`. The test would then see Device A succeed and Device B get `lease_held` (which is the expected outcome) but for a correct reason — except the test was trying to exercise the contention detection, not the already-committed lease case. These are different code paths.
134
135 The spec says this test is serial (§6.3) but doesn't say how it handles the timing gap. If the two `DreamNow` calls happen sequentially in the test code (which they would, since Rust async but single-test-thread), there's always a race between "write the first request" and "Device A has pushed the lease." The spec should define a setup step that pre-seeds Device A's daemon with a dream already in-progress (e.g., using `force: false` on a scope that already has an active lease in the git remote fixture), eliminating the timing dependency.
136
137 ### R6. Test #11 assertion 3 may be policy-version brittle in a way the spec doesn't acknowledge
138
139 Test #11 assertion 3 says: "Step 5 (agent re-asserts the same incorrect claim with higher confidence, using itself as source) returns `status: 'refused'` with `reason: 'grounding'`, or `status: 'candidate'` with a `next_actions` note that the self-referencing grounding requires human review."
140
141 The spec acknowledges in §10.2 that test #7's assertion could break if a future policy changes the subagent-write gate. The same fragility exists for test #11 assertion 3: whether a self-referencing source_ref returns "refused" or "candidate" depends on policy. Specifically, does `project-standard` policy recognize self-referential circular grounding as a refusal condition, or does it put it in review? The shipped Stream C policy's behavior on this specific input isn't quoted here — it's asserted as expected behavior.
142
143 If Stream C's governance engine doesn't specifically detect "this source_ref points back to a memory by the same session that wrote the candidate," the assertion is testing against governance's general grounding check, which may not catch circular self-reference as a distinct case.
144
145 ### R7. Daily main run non-blocking rationale is unargued
146
147 §7.1 says the daily cron run on main is non-blocking. The comment in §7.6 explains what happens when it fails (Slack notification, dogfood gate extension) but never explains why it's non-blocking. The handbook (v2.2) says to "run these on every release" and "track pass rates over time" — that's compatible with non-blocking if you trust developers to respond to Slack notifications. But the spec should state the tradeoff explicitly: non-blocking daily runs mean a regression can sit for hours before someone notices, but blocking daily runs mean main is unusable during legitimate infrastructure or rate-limit failures. Right now the spec just says "non-blocking" with no reasoning, which means the next person to own CI will consider making it blocking without understanding why it was non-blocking.
148
149 ---
150
151 ## Nits
152
153 **N1.** §6.1 documents `--filter <PATTERN>` as "glob pattern on test name or number" with examples like `--filter "t01"` and `--filter "handbook/*"`. "Glob" isn't specified further — does `*` match slashes? Is it case-sensitive? Rust glob crates differ on these. One sentence specifying the exact pattern dialect (e.g., "glob syntax per the `glob` crate v0.3") would save implementer questions.
154
155 **N2.** §6.3 parallel group includes test #15 (real-harness) alongside simulator tests. Test #15 uses `claude -p` with a 180-second timeout. Running 14 parallel tests with a real harness invocation in the mix means the parallel group could stall on the auth timeout. The spec notes default workers is 4, which mitigates this, but test #15 should probably be in the serial group alongside #13, or at least noted as the rate-limiting element in the parallel group.
156
157 **N3.** §8.5 (flaky test quarantine policy) says "A quarantined test may not remain quarantined for more than one release cycle without a resolution plan." "One release cycle" is undefined. Is that a v1.x.y release (could be days) or a v2.0.0 release (could be years)? Needs a concrete time bound.
158
159 **N4.** §9.3 (dream path exclusion test) says the simulator has "test-only substrate access" to call `Substrate::read_memory_envelope` directly. This is not documented in §4 (SimulatorAgent architecture) and doesn't appear in the `SimulatorAction` vocabulary. If the simulator uses only the daemon socket protocol (as §4.3 states), it can't call substrate methods directly. The test as written requires a different access path than the simulator provides.
160
161 **N5.** "memoryd-eval" appears zero times in the spec, which is good since §2 establishes "memorum-eval" as the canonical name. But system-v0.2 §19 says "Stream H owns `crates/memorum-eval/` (new), CI workflow, and a test orchestrator binary" — consistent. This is just confirming naming is locked correctly.
162
163 ---
164
165 ## Cross-spec consistency findings
166
167 **C1. Stream G does not define a daemon protocol `RequestPayload` variant for triggering reality-check.** Stream H test #16 step 2 calls "CLI equivalent in daemon protocol" — but Stream G defines reality-check as a CLI command and a web API route, not a daemon socket protocol variant. Stream H §1 forbids adding daemon protocol extensions. Either Stream G needs to add `RequestPayload::RealityCheckRun` or test #16 can't use the simulator mode it declares. (This is also Blocker #4.)
168
169 **C2. Stream G §9.5 documents `memoryd reality-check run --json | jq '.items[0]'` but never defines the JSON schema.** The `items` array contains scored memory objects but the field names (id, score, score_breakdown, namespace, etc.) are not specified. Stream H test #16 assertion 5 asserts on individual score components. Without a defined field schema for the JSON output, assertion 5 is asserting against an undefined wire shape. Stream G §10.3 (Reality Check tests) shows `test_reality_check_panel_renders_score_breakdown` asserting on rendered TUI output but not on the `--json` schema. The `--json` output shape needs to be defined in Stream G before Stream H can write test #16 assertions against it.
170
171 **C3. Stream I's framing test suite is a real-harness test suite requiring Stream H's eval runner but is not in Stream H's 18-test catalog.** Stream I §10.4 defines `assert_framing` in `crates/memorum-coordination/src/framing_tests.rs` with a 6-case sampling matrix. Stream I §10.1 explicitly states "Stream H owns the runtime that invokes the harnesses and collects results." This isn't an open question anymore — Stream I is written. The 18-test count needs to be revisited. (This is also Blocker #5.)
172
173 **C4. Stream F's `DreamRunReport.pass_1.status` uses the enum `PassStatus { Success, Skipped, Failed }`, not a string `"success"`.** Test #17 assertion 1 references `pass_1.status: "success"` as a string literal. This will work at the JSON layer (serde serializes to lowercase) but the spec's Rust-flavored assertion language should use the enum form `PassStatus::Success` or note that the assertion is on the serialized JSON value `"success"` to be precise.
174
175 **C5. The 12 handbook tests are accurately transcribed.** Test names match handbook v2.2 pp. 474–487 verbatim. The spec's claim that they "quote or closely paraphrase the handbook's language" is verified. I spot-checked tests #1, #5, and #11 against the handbook's "Minimum tests to include" section and all three are correctly named. The descriptions expand on the one-liners with concrete setup and assertions. No discrepancy found.
176
177 **C6. Stream D's `memory_reveal` API requires a non-empty reason string and emits an audit event.** Test #18 step 3 correctly specifies a reason string and assertion 6 correctly asserts the audit event. The `memory_reveal` shape in the Stream C governance API (MCP boundary section) confirms reveal is MCP tool #8 and requires bounded reason validation. Test #18 is correctly specified here.
178
179 ---
180
181 ## Things I checked and found correct
182
183 The 12 handbook tests accurately represent the handbook v2.2 "Minimum tests to include" list (pp. 474–487). The test names, descriptions, and regression targets are faithful to the handbook's intent and expanded appropriately for the Memorum-specific protocol.
184
185 The `SimulatorAgent` architecture (§4) is a sound design. Using the daemon's public socket protocol rather than test-internal substrate access keeps the tests honest about the system boundary. The `DaemonScaffold` isolated-instance-per-test pattern avoids cross-test state contamination.
186
187 The `MockHarness` design is honest about what it tests and what it doesn't. The spec explicitly marks mock runs as `"mode: mock — agent reasoning not exercised"` and blocks RC gates on partial runs. This is the right call; a mock that silently replaces real LLM behavior without disclosure would be misleading.
188
189 The real-harness auth handling (§5.3) is safe with respect to the stated threat model: keys are in environment variables, never embedded in prompt text or temp tree files, and the sandbox tree isolation (§5.5) prevents the test harness from touching the user's production memory tree. The `MEMORUM_EVAL_SOCKET_PATH` environment variable approach for MCP wiring is correct in principle.
190
191 Test #5 (poisoned candidate) and test #8 (deletion and tombstone) are among the most precisely specified tests in the catalog. The tombstone filesystem assertion (test #8 assertion 6) is the right kind of cross-layer check that would catch a governance layer that marks the record but forgets to write the tombstone file. Test #5's review queue assertion (assertion 5) correctly ensures the quarantined item is auditable, not silently dropped.
192
193 The regression-as-test workflow (§8) is well-specified. The required metadata block (§8.3) with incident date, root cause, and fix commit is exactly the right discipline to prevent regression tests from becoming disconnected from the failures they guard. The "PRs that fix a production failure without adding a regression test fail code review" policy (§8.4) is correctly forceful.
194
195 The serial/parallel test grouping (§6.3) correctly identifies which tests share mutable global state (git remotes, key provider, multi-daemon coordination) and serializes those. Tests #14 and #17 sharing the serial group is correct.
196
197 The test #2 (superseded fact handling) assertion on `memory_write` of a third write with the same claim body returning `duplicate` or `candidate` with `existing_id` is a genuinely useful regression guard that most eval suites miss — the duplicate-detection path is the one most likely to be broken by a governance refactor.
