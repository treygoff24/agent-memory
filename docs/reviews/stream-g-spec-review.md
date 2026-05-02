1 # Stream G v0.1 spec review
2
3 **Reviewer:** plan-reviewer (Claude Sonnet 4.6, fresh context)
4 **Date:** 2026-05-01
5 **Spec:** docs/specs/stream-g-observability-v0.1.md (1573 lines)
6 **Verdict:** BLOCK
7
8 ---
9
10 ## Blockers
11
12 ### B1. `source_count`, `recall_count_30d`, and `last_recalled_at` do not exist in the shipped index schema
13
14 The spec's entire drift-risk scoring implementation depends on three index columns that were never added to the `memories` table. Specifically:
15
16 The spec says in §12.3: "The provenance source-count is the only field not currently indexed — Stream G must add a `source_count INTEGER NOT NULL DEFAULT 1` column to the `memories` table."
17
18 But it also says in §7.1 (Trust Artifact data sources): `recall_count_total` comes from `SELECT recall_count, recall_count_30d FROM memories WHERE id = ?` and `last_recalled_at` from `SELECT last_recalled_at FROM memories WHERE id = ?`.
19
20 The shipped schema at `/Users/treygoff/Code/agent-memory/crates/memory-substrate/src/index/schema.rs` has no `recall_count`, `recall_count_30d`, or `last_recalled_at` columns. The current schema version is 3. These columns do not exist in any migration. The scoring formula in §5.1 requires `m.recall_count_30d` to compute `recall_frequency_norm(m)` and `m.observed_at` to compute `days_since_observed_norm(m)`.
21
22 `observed_at` does exist in the schema. `recall_count_30d` and `last_recalled_at` do not.
23
24 The spec partially acknowledges the `source_count` gap in §12.3 but treats recall count as already present (§7.1 query, §10.3 scoring tests). This is not a documentation gap — the scoring tests in §10.3 (`test_score_formula_all_components`, `test_top_n_selection_respects_cap`) will fail to compile because the structs they operate on do not have these fields.
25
26 **Fix:** The spec must explicitly document that Stream G requires a schema migration (version 4) that adds `source_count INTEGER NOT NULL DEFAULT 1`, `recall_count INTEGER NOT NULL DEFAULT 0`, `recall_count_30d INTEGER NOT NULL DEFAULT 0`, and `last_recalled_at TEXT`. It must also specify how `recall_count` and `recall_count_30d` get updated — the daemon must update them on every recall event. Neither the daemon protocol handlers nor Stream A's `AtomicWrite` path does this today. This is not just an index migration; it requires a new event-driven counter update path through Stream B.
27
28 **Section reference:** §5.1, §7.1, §10.3, §12.3.
29
30 ---
31
32 ### B2. The `source_count` column migration is authorized in §12.3 prose only, not in §1.3 (cross-stream surface changes)
33
34 §1.3 is the spec's formal section for cross-stream surface changes. It carefully documents two additive additions: the `<pending-attention>` recall line and the `NotificationEvent` broadcast channel. The `source_count` column addition — which requires a Stream A schema migration, a new Stream A indexer upsert path, and a bump to `INDEX_SUPPORTED_SCHEMA_VERSION` in `migrations.rs` — is buried in a performance implementation note in §12.3.
35
36 The prior streams that added Stream A surface changes (Stream E's `passive_recall` column, Stream D's `allow_encrypted_namespace` flag) declared them in the `§1.1 Cross-stream surface changes` section of their respective specs. Stream G breaks this convention for the most structurally significant change it makes to the shipped codebase.
37
38 **Fix:** §1.3 must enumerate every Stream A surface addition including the schema migration, the new indexer columns, `INDEX_SUPPORTED_SCHEMA_VERSION` bump from 3 to 4, and the daemon path that increments `recall_count` on each recall event. Without this, the plan executor cannot correctly scope the work.
39
40 **Section reference:** §1.3, §12.3.
41
42 ---
43
44 ### B3. The `RequestPayload::RealityCheckRespond` daemon protocol variant is used but never defined
45
46 §5.4 says confirm action sends `RequestPayload::RealityCheckRespond { action: Confirm }` to the daemon. The existing daemon protocol (`crates/memoryd/src/protocol.rs`) has no such variant. `RequestPayload` variants are enumerated in the shipped Stream B/C/D/E/F contract — they are the daemon's wire format, not an implementation detail Stream G can quietly add.
47
48 The spec treats daemon protocol extensions as internal plumbing to be figured out during implementation. But the Stream C governance API document is explicit that protocol changes require explicit contract definition (shapes, error codes, retryable flags). Stream G adds: `RealityCheckRespond`, `RealityCheckStatus`, and `RequestPayload` variants for all five response actions. None of these are defined with concrete field types or error shapes anywhere in the spec.
49
50 The same gap exists for `RequestPayload::Startup` variants referenced in §5.3 (the spec implies the daemon exposes a session-resumable reality check state, but no protocol message for "get pending session" is defined).
51
52 **Fix:** §5 must include a protocol section analogous to Stream C's governance API doc: wire shapes for each new `RequestPayload` variant, corresponding `ResponsePayload` variants, and error codes for refusal cases (governance refusal of a correction, reason too short, session not found).
53
54 **Section reference:** §5.3, §5.4.
55
56 ---
57
58 ### B4. The `NotificationEvent::RealityCheckOverdue` variant is referenced but not in the enum
59
60 §5.5 says "NotificationEvent::RealityCheckOverdue is fired" when 3+ weeks are skipped. The `NotificationEvent` enum defined in §1.3 has exactly six variants: `LeakedSecretDetected`, `BlockingMergeConflict`, `ReviewQueueOverThreshold`, `DreamRunCompleted`, `RealityCheckDue`, `DailySynthesisSummaryReady`. `RealityCheckOverdue` is not among them.
61
62 The notification dispatcher tests in §10.4 test `test_passive_queue_receives_all_events` by firing each `NotificationEvent` variant, but the overdue event has no representation in the enum. The §6.2 trigger table references `reality_check_overdue` as a trigger name. The §8 config schema lists `reality_check_overdue` in `external.triggers`. These references are internally inconsistent — three sections say the event exists, the enum definition says it does not.
63
64 **Fix:** Add `RealityCheckOverdue` to the enum in §1.3. Then verify the test in §10.4 covers this variant.
65
66 **Section reference:** §1.3, §5.5, §6.2, §8.
67
68 ---
69
70 ### B5. Stream G declares itself a read-only consumer of shipped surfaces in §1.2 but then adds daemon state files that are written by its own code
71
72 §1.2 states "Stream G does not own: Canonical memory mutation — all mutations route through daemon protocol." But §5.2 and §5.3 specify that Stream G's code writes `~/.memoryd/state.json` (updates `reality_check.last_completed_at`), `~/.memoryd/reality-check-pending.json`, and `~/.memoryd/reality-check-session.json`. These are daemon-local state files, not canonical memory files, but they are new persistent state that the daemon now owns and maintains — and they were not in the daemon's runtime state layout defined in Stream A §5.2.
73
74 More importantly, `~/.memoryd/state.json` is a new file that appears to aggregate multiple pieces of runtime state. This file is not defined in Stream A's `~/.memoryd/` layout (which lists `local-device.yaml`, `seq.json`, `event-seq.json`, `pending/`, `index.sqlite`, `socket`, `pid`, `logs/`, `tmp/`). Either this is a new file that Stream G adds to the daemon runtime layout (which requires documenting as an additive Stream A surface change in §1.3) or the spec expects this to be an undocumented addition.
75
76 The session-state file `reality-check-session.json` also has no crash-recovery semantics defined. If the daemon crashes while mid-session, the file persists. §5.3 says "the interrupted session is offered for resumption" — but there is no specification of what happens if the file is corrupt or written by an older daemon version.
77
78 **Fix:** §1.3 must explicitly list the new daemon state files as a Stream B/A additive surface change. §5.3 must define crash-safety semantics for the session state file (what happens on parse failure, what happens on schema mismatch after daemon upgrade).
79
80 **Section reference:** §1.2, §1.3, §5.2, §5.3.
81
82 ---
83
84 ## Risks
85
86 ### R1. The 500ms scoring budget for 10,000 memories is likely unachievable without the recall columns
87
88 §12.3 budgets "score computation for 10,000 memories in ≤500ms" and correctly identifies that the scoring loop must avoid `Substrate::read_memory` per item. But `cross_source_corroboration(m)` requires counting distinct `(harness, session_id)` pairs in the memory's provenance chain. The provenance chain is in the event JSONL — not the SQLite index.
89
90 The `source_count` column is intended to solve this, but the spec does not define how it gets populated. The provenance chain is assembled by scanning the event log for all events related to a given memory ID — this is not a single-row lookup. If `source_count` is populated at write time (when a new write adds a harness to the chain), it is an approximation that misses supersessions from different harnesses written after the initial promotion. If it is computed from the event log at scoring time, it negates the entire point of the indexed column.
91
92 The spec says "all scoring inputs must come from the SQLite index or pre-aggregated caches" but does not say how `source_count` is maintained as subsequent supersessions arrive. This is a correctness gap (stale source counts) as much as a performance concern.
93
94 **Likely failure mode:** either `cross_source_corroboration` is computed stale (scoring gives wrong answers for memories with late-arriving corroboration) or the 500ms budget is blown by event-log scans.
95
96 **Section reference:** §5.1 (`cross_source_corroboration`), §12.3.
97
98 ---
99
100 ### R2. The 50 KB bundle size claim is plausible but the 500ms paint budget depends on conditions not specified
101
102 §4.1 claims the asset bundle stays under 50 KB gzipped. §12.2 budgets initial page load at ≤500ms cold on localhost. The 50 KB claim is credible for Preact + HTM + hand-written CSS. But §4.6 adds D3 for force-directed graphs. D3's force simulation module alone is ~30 KB gzipped. Combined with the rest of D3, the bundle grows substantially past 50 KB unless only the specific force-layout module is imported (tree-shaken at the module level, not the whole library).
103
104 The spec says "bundled to a single `app.js` + `style.css` at build time via `esbuild`" — esbuild does dead-code elimination, but D3 v7's submodule structure requires intentional selective import to get tree-shaking to work. If the implementer does `import * as d3 from 'd3'`, the bundle will be 200+ KB gzipped.
105
106 The measurement methodology in §12.2 — "`curl` time to first byte + browser paint from Lighthouse in localhost mode" — conflates server response time (nearly zero for localhost) with actual browser render time (dominated by JS parse + D3 simulation startup). Lighthouse in localhost mode is not how users experience this dashboard; it tends to report optimistic numbers.
107
108 **Likely failure mode:** asset bundle silently exceeds 50 KB once D3 is included with a non-selective import. Paint budget passes measurement but fails in practice on lower-end machines due to D3 simulation initialization.
109
110 **Section reference:** §4.1, §4.6, §12.2.
111
112 ---
113
114 ### R3. The CSRF model is sound for the stated threat but the token durability across tab lifecycle is underdefined
115
116 §4.4 correctly identifies the threat: cross-origin POST from a malicious page to localhost. The mitigation (random 32-byte token in initial HTML `<meta>` tag, required on mutating routes) is correct for this threat model. This is not security theater.
117
118 However, §4.4 says "Token rotates on server restart... open browser tabs that cached the old token get a 403 and must refresh." It does not say how the Preact frontend is supposed to recover from this. If the user has the dashboard open, does a background poll to `/api/status` fail with 403? Does the UI show an error? Does it automatically re-fetch the CSRF token from a GET `/` request?
119
120 More practically: the token is loaded from the initial `index.html` response. If the SPA is served with `static_assets_cache_secs: 3600` (the config default), the browser caches `index.html` and subsequent navigations may use a stale CSRF token without the user refreshing. The spec needs to either set `Cache-Control: no-store` on `index.html` specifically, or the frontend needs to re-fetch the token on every session start from a dedicated `/api/csrf` endpoint.
121
122 **Likely failure mode:** 403 errors on POST routes in long-running browser sessions after daemon restart, with no user-visible recovery path.
123
124 **Section reference:** §4.4, §8 (`static_assets_cache_secs`).
125
126 ---
127
128 ### R4. Reality Check schedule timezone handling has a subtle gap
129
130 §5.2 says "Default schedule: Sunday, 09:00 local time, weekly. Configurable as `reality_check.schedule` in `config.yaml` (cron expression string)." The daemon checks "once per hour" whether a reality check is due.
131
132 Cron expressions are timezone-agnostic — they describe a time-of-day in whatever timezone the evaluator uses. The spec says "09:00 local time" but does not say how the daemon knows what "local time" is. The daemon is a background process; `localtime()` on the daemon process may differ from the user's current timezone if they travel between time zones, work across DST transitions, or run the daemon on a NixOS/systemd machine where the system timezone differs from the user's session timezone.
133
134 More concretely: if the daemon is running on a server (even a developer's own desktop with `memoryd` launched at login) and the system timezone is UTC, "Sunday 09:00" fires at 09:00 UTC, not 09:00 in the user's location. The Slack notification — "Your weekly Memorum Reality Check is ready" — arrives at a potentially unintuitive time.
135
136 This is not a blocker (the feature works) but it is a usability gap that will surface in dogfood. The spec should either explicitly say the daemon uses the system local timezone, or add a `reality_check.timezone` config key.
137
138 **Section reference:** §5.2, §8.
139
140 ---
141
142 ### R5. Panel 2's 1-second undo window architecture has a race condition in TUI event handling
143
144 §3.2 (Panel 2) and §3.3 say: "All mutating actions are confirmed in the footer... with a 1-second undo window: 'Approved mem\_… — press u to undo' before the daemon call fires." The undo window is measured in wall-clock time before the daemon call fires.
145
146 The TUI is tick-driven at 16ms. The undo timer runs for ~62 ticks. But the spec also says the TUI polls the daemon every 250ms and updates state. If the daemon state (review queue contents) changes during the undo window — because another session approved or rejected the same memory via the web dashboard or CLI — the TUI's in-memory pre-fire state is stale, and the eventual daemon call may act on a memory that has already changed state.
147
148 The spec handles the post-fire 409 case correctly (§4.4 web dashboard, "If two browser tabs attempt to POST simultaneously, the first succeeds and the second receives 409"). But the TUI's undo window is purely local-delay — there is no re-validation of memory state before the delayed daemon call fires.
149
150 **Likely failure mode:** rare double-action if the user approves from TUI and a concurrent session also approves via CLI within the 1-second window. The second approval hits the daemon with a stale action. The daemon returns a well-defined error, but the TUI has no handler for "action fired after undo window but the daemon rejected it" — the spec does not define the TUI's behavior in this case.
151
152 **Section reference:** §3.2, §3.4.
153
154 ---
155
156 ### R6. The crate naming breaks the `memorum-` prefix convention established by Stream H
157
158 Stream H's spec (§2, Note on crate naming) explicitly states: "only crates new in Stream H get the `memorum-` prefix." More precisely, it states that crates published to crates.io follow `memorum-`. Stream G introduces `crates/memoryd-tui/` and `crates/memoryd-web/` — the `memoryd-` prefix is used instead of `memorum-`.
159
160 System-v0.2 §20.6 says published crates are `memorum`, `memorum-substrate`, etc. Stream H uses `memorum-eval`. If the TUI and web crates are published, they should be `memorum-tui` and `memorum-web`. If they are workspace-internal only (not published), this is a non-issue, but the spec is silent on whether these crates get published.
161
162 **Likely failure mode:** if both Stream G and Stream H implementers follow their respective specs without cross-checking, the workspace will have mixed naming conventions that create confusing release artifacts.
163
164 **Section reference:** §2, system-v0.2 §20.6, Stream H §2.
165
166 ---
167
168 ### R7. Stream I peer-presence display in TUI Panel 1 and Trust Artifacts is not covered but also not excluded
169
170 §7.1's Trust Artifact data source table has one row for "Claim-lock status (Stream I)" that reads `GET /api/peer/claim-lock?id=<id>`. This endpoint does not exist in Stream G's own route table (§4.3) — it would have to be a Stream I daemon protocol call routed through Stream G's web server. The note says "if Stream I is active" as a conditional.
171
172 More importantly, Panel 1's overview shows "Active sessions: 2 (claude-code, codex-cli)" sourced from `StatusResponse`. Stream I's `PresenceRegistry` is the authoritative source for active peer sessions. But the `StatusResponse` shape is defined by Stream B/E, not Stream I or Stream G. Does `StatusResponse` already return active sessions? Or does Stream G need to add this field? The spec says this comes from the daemon's `RequestPayload::Status` response but `active_sessions` is not in the Stream B API as documented.
173
174 **Likely failure mode:** the claim-lock display in trust artifacts silently shows nothing when Stream I is not active; implementer adds `GET /api/peer/claim-lock` to Stream G's web server without realizing it belongs in Stream I's daemon extension; or `active_sessions` in `StatusResponse` does not exist and Panel 1's session count is always 0.
175
176 **Section reference:** §4.3, §7.1, Panel 1 data sources.
177
178 ---
179
180 ## Nits
181
182 The spec says "TUI Tier 1 only" for `/memory-reality-check` in §9.8 but the body acknowledges it calls a CLI command (`memoryd reality-check run --json`) available to all tiers. The constraint is correct but confusingly stated — the slash command is Tier 1; the CLI it invokes is tier-agnostic. One sentence clarifying this would eliminate ambiguity during implementation.
183
184 §10.1's `test_get_status_returns_correct_shape` is labeled "POST to `GET /api/status`" — copy-paste error. It should read `GET /api/status`.
185
186 The "500ms initial-paint budget" framing in §4.1 is reasonable as a constraint but the measurement methodology in §12.2 conflates two different things: server-side TTFB (time to first byte from curl) and browser paint time (from Lighthouse). These should be stated as two separate metrics with separate measurement methods.
187
188 §11.2 and §11.3 are labeled "open questions" but both are answered inline ("v1.x deferred," "users edit via $EDITOR"). The section heading should be "Deferred items" since they are not open.
 189 
 190 The active run panel layout in §3.2 shows the score breakdown formula with arithmetic displayed inline ("62/90 days = 0.69 → contributes 0.24"). The 0.24 is wrong: 0.35 × 0.69 = 0.2415, and the display says staleness weight is 0.35. The spec has an arithmetic error in the illustrative example. Not fatal — the formula in §5.1 is correct — but someone will notice.
 191 
 192 ---
 193 
 194 ## Cross-spec consistency findings
 195 
 196 **Stream H test #16 vs Stream G drift scoring:**
 197 
 198 Stream H §3.2 test #16 asserts that "Memory B's drift score is ≥ 0.65 (90-day saturation + zero recall + zero corroboration + personal sensitivity pushes all five weight components high)." Working through the formula:
 199 
 200 ```
 201 0.35 * 1.0   (90+ days, saturated)
 202 + 0.20 * 1.0  (zero recall)  
 203 + 0.20 * 1.0  (single source)
 204 + 0.15 * 0.25 (0.95 - 0.70 = 0.25 decay)
 205 + 0.10 * 1.0  (personal sensitivity)
 206 = 0.35 + 0.20 + 0.20 + 0.0375 + 0.10
 207 = 0.8875
 208 ```
 209 
 210 This is ≥ 0.65, so the assertion passes. But Stream H §3.2 also writes Memory B with `confidence: 0.70` and no specified `original_confidence`. If `original_confidence == confidence` (because it was just written), `confidence_decay = max(0, 0.70 - 0.70) = 0.0`, which gives a score of 0.85. Still passes. The test is not brittle here, but the spec does not define whether `original_confidence` is set at write time or whether it defaults to `current_confidence`. Stream G §5.1 says "`original_confidence` is the confidence at initial promotion" — but how the indexer knows the initial promotion confidence is not specified. It would have to be stored in the index (another missing column, or extracted from the event log).
 211 
 212 **Stream I peer-presence vs Stream G Panel 1:**
 213 
 214 Stream I §3.3 (Level 3) specifies presence heartbeats as the source of active session data. Stream G Panel 1 shows "Active sessions 2 (claude-code, codex-cli)" as if this is a standard Status response field. Stream I §6.2 says `PresenceRegistry` lives in daemon RAM and is exposed only to Level 3 sessions. For Level 2 sessions (the default), other sessions are not tracked via heartbeat. Stream G's Panel 1 may show zero active sessions for the default coordination level, which would be confusing. Stream G does not address this.
 215 
 216 **Stream E `<pending-attention>` cap vs Stream G `reality_check_due` item:**
 217 
 218 Stream G §1.3 correctly notes the item counts against the 6-total cap. Stream E's spec §5 (via system-v0.2 §12) sets caps at 2/scope and 6 total. Stream G's item is not scoped to a namespace — it is a global daemon event. The spec says it counts against the 6-total cap but says nothing about which scope bucket it falls into. If the cap accounting code expects every `<pending-attention>` item to carry a scope, a nil-scope `reality_check_due` item could cause an off-by-one or a panic depending on how the Stream E assembler counts items. This is a protocol ambiguity at the insertion point, not a stream G spec defect per se, but Stream G owns the insertion and should define the scope value (likely `"daemon"` or `"global"`).
 219 
 220 ---
 221 
 222 ## Things I checked and found correct
 223 
 224 The drift-risk weight vector `(0.35, 0.20, 0.20, 0.15, 0.10)` in §5.1 matches system-v0.2 §16.4 exactly. The sensitivity weight mapping `(public=0.0, internal=0.3, confidential=0.6, personal=1.0)` also matches. The cap behavior is specified correctly — weights sum to 1.0 and the formula is bounded.
 225 
 226 The crate split rationale in §2 is sound. TUI depends on `ratatui`/`crossterm`; the web server depends on `axum` and asset embedding. Merging them creates a fat binary with conditional-compilation gymnastics. The split is the right call.
 227 
 228 The CSRF threat model in §4.4 is correctly scoped: localhost binding plus a token in the initial HTML response, not in a cookie, is the right primitive for a single-user local tool. The analysis of what the attack is (cross-origin fetch to localhost) and why the mitigation works is accurate.
 229 
 230 The notification dispatcher architecture in §6.3 — a `tokio::sync::broadcast` channel, in-process only, not persisted to event JSONL — is consistent with System-v0.2's explicit statement that the event JSONL is the durable audit log and the notification channel is advisory. The distinction is clear and the lagged-receiver handling is correct.
 231 
 232 The `not_relevant` action semantics in §5.4 are clean: sets `passive_recall = false`, adds a tag, does not tombstone, reversible via `memoryd pin`. This correctly threads the needle between "I don't want to see this again" and "this is wrong and should be deleted."
 233 
 234 Stream H test #16's scoring sanity test correctly exercises the formula ordering invariant (B > C > A) without hardcoding exact scores beyond the boundary conditions. The assertions are robust to reasonable implementation variance.
 235 
 236 The deferred sections (policy editor §11.2, sync status dashboard §11.3) are genuinely clean deferrals. The four shipped sections do not depend on the two deferred ones. The `$EDITOR` escape in Panel 7 covers the write path for policy editing without requiring the deferred web UI.
