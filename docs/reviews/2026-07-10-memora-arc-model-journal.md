# Model performance journal — Memora-lessons arc (2026-07-10 →)

Per the delegate-agent journal mandate. Coordinator: Fable session.

## 2026-07-10 - gpt-5.6-sol via codex - plan review r2 (round 1)

Command and run: `delegate --group memora-plan-review codex safe --model sol --reasoning-effort xhigh --prompt-file thoughts/memora-build/plan-review-r2-prompt.md`; alias `codex-73`; mode/isolation: safe / worktree-temporary.

Task and expectation: adversarial review of the r2 foundry execution plan against live repo code, specs, and the Memora clone; findings ranked with anchors, scenarios, fixes.

Outcome and verification: delivered 4 BLOCKER + 12 MAJOR + 1 MINOR with verdict "structural rework." Coordinator verified the load-bearing code claims (review-approve lifecycle, classifier field coverage spot-checks, `memoryd remember` nonexistence, thoughts/ gitignore status) — all checked accurate. All four blockers accepted; 12/12 majors accepted (2 with adapted fixes). Zero rejected findings — an unusually high hit rate.

Performance observations: ~25 min wall. Read shipped Rust across import/governance/embedding/recall, the v4 spec, CLI contract, AND the Memora Python clone; caught cross-document collisions (index schema 6 double-claim vs v4 P2) and a governance-surface gap (no atomic merge-approve) that the native Opus reviewer missed. Report format followed exactly. One environmental note: delegate safe isolation replaced the `clean-code` skill symlink with a placeholder (skill unavailable to the lane).

Routing assessment: Sol xhigh is now field-validated as a frontier *plan* reviewer, not just author — deeper cross-artifact collision detection than native Opus on the same brief (Opus won on root-cause code archaeology instead; see below). Use both: they decorrelate beautifully. Confidence: high.

## 2026-07-10 - stale-alias incident (process note, no new run)

`delegate wait codex` + `run-output codex-72` grabbed a 22h-old API-lane review (group `apilane-w3rev`) instead of the just-launched plan review — bare-alias resolution hit the newest *finished* run while ours was still registering. Detection: report content mismatched the task (report-validity check). Rule reinforced: record the numbered alias at launch; never bare-alias wait/run-output. Side effect: codex-72's stale report surfaced possibly-untriaged API-lane findings (doctor drain-error blindness, `config embedding-lane` missing from v1 schema registry, `init --print-only` mutation) — logged for cross-check against main.

## 2026-07-10 - native Opus plan-reviewer (for comparison; not a delegate run)

Parallel same-brief review. Delivered 4 BLOCKER + 6 MAJOR + minors after one idle-nudge (known deliver-failure mode; SendMessage recovered it). Unique catches Sol missed: the verified `ImportState::default()` root cause of the import bug (best single finding of the round — killed a whole misdirected wave), rename-survival via recovered `mem_*` frontmatter id, dream-loop-torn-off dependency of the backfill. Weaker than Sol on: governance merge-op gap, privacy-classifier field coverage, CLI-surface nonexistence. Verdict "executable after blocker fixes" was softer than Sol's "structural rework"; coordinator sided with Sol.

## 2026-07-10 - grok-4.5 via cursor - plan review r3 (round 2)

Command and run: `delegate --group memora-plan-r3 cursor safe --prompt-file thoughts/memora-build/plan-review-r3-prompt.md`; alias `cursor-28`; mode/isolation: safe / isolated copy.

Task and expectation: round-2 adversarial review of the r4-bound r3 plan; instructed not to re-find round-1 items unless the fix was defective.

Outcome and verification: 3 BLOCKER + 4 MAJOR + 2 MINOR + 1 NIT, verdict "another structural pass." All three blockers independently convergent with the native Opus re-attack (authorship orphan, unbacked atomicity, API-lane latency); unique adds: the W0 write-DTO realism finding (`ClassificationOutcome` not client-suppliable — checked, correct), decision-point numbering mismatch, W3-tuning dependency edges, findings-artifact rigor. Constants it cited verified. Zero re-reports of fixed round-1 items — brief compliance excellent.

Performance observations: ~12 min. Loaded repo skills (plan-review-loop, clean-code, premortem) unprompted and said so. Grounded in shipped write/recall/governance code. Confirms the ATTACKER role rating on plan artifacts, not just code diffs.

Routing assessment: keep as the standing cross-family plan reviewer alongside Sol. Confidence: high.

## 2026-07-10 - meta/muse-spark-1.1 via opencode - plan review r3 (round 2, comparative slot)

Command and run: `delegate --group memora-plan-r3 opencode safe --model muse --prompt-file thoughts/memora-build/plan-review-r3-prompt.md`; alias `opencode-1`; mode/isolation: safe / config-lockdown isolated copy.

Task and expectation: same brief as cursor-28; comparative test per the Muse journal mandate.

Outcome and verification: 2 BLOCKER + 6 MAJOR + 3 MINOR + 1 NIT, verdict "another structural pass." Convergent on the structural trio AND the deepest code-grounding of the round: unique catches included the missing multi-table KNN query API, the specific entrypoints that bypass classification today (`review approve` and dream orchestration write `Trusted` — spot-checked, correct), the sensitivity-upgrade-must-revoke-API-vectors path, the cue set-union rule violating its own 0–3 cap, and precise W0 grounding mechanics (`explicit_user_context`). Fix suggestions were unusually implementable (concrete method names, migration guard patterns).

Performance observations: ~14 min; no progress events (known opencode buffering); clean single-envelope output; no scope creep; zero re-reports of fixed items. Verification burden low — every spot-checked claim held.

Routing assessment: on this task Muse performed at or above Cursor/Grok-4.5 and near Sol-xhigh depth at a fraction of the ceremony. Second strong data point wanted on a *code diff* review before promoting past test-slot status; for plan/spec review it is already worth a standing slot. Confidence: medium-high (n=1 task type).

## 2026-07-10 - gpt-5.6-sol via codex - plan review r4 convergence re-read (round 3)

Command and run: `delegate --group memora-plan-r4 codex safe --model sol --reasoning-effort xhigh --prompt-file thoughts/memora-build/plan-review-r4-prompt.md`; alias `codex-74`; mode/isolation: safe / worktree-temporary.

Task and expectation: certify-or-refute readiness of r4 with full prior-round context; new/fix-defect findings only.

Outcome and verification: verdict READY-WITH-EDITS — 4 MAJOR + 2 MINOR, zero blockers, zero structural findings, explicit certification of the baseline₁ A/B protocol and journaled merge design. All six findings accepted and applied as r5; spot-checks held (quarantine.rs hardcoded Trusted; supersession primitive writes replacements active-first; desk path receives no vector context). Notably calibrated: it certified what was sound instead of inventing severity, and caught the coordinator's own CPU-discipline slip (`cargo build --locked`) that all prior reviewers and the coordinator missed.

Performance observations: ~14 min. Round-over-round finding decay 16 → 6 → 0-structural = healthy convergence, not reviewer fatigue (round 3 findings were real but small).

Routing assessment: Sol xhigh as bookend reviewer (round 1 deep attack + round N convergence certification) is now a proven pattern. Confidence: high.

## 2026-07-10 - gpt-5.6-luna via codex - W2 spec ratification package review (round 1)

Command and run: `delegate --group memora-w2spec codex safe --model luna --reasoning-effort high --prompt-file thoughts/memora-build/w2-spec-review-prompt.md`; alias `codex-77`; mode/isolation: safe / worktree-temporary.

Task and expectation: adversarial review of the coordinator-drafted W2 spec ratification package against the plan, Stream A v1.1, and shipped code; hunt areas included convergence, lifecycle holes, and classification ordering.

Outcome and verification: 2 BLOCKER + 6 MAJOR + 1 MINOR, verdict NOT-RATIFIABLE; **9/9 accepted** after coordinator spot-checks (the `_extras` silent-ours-wins claim against `field_rules.rs` and the `index_embeddings` policy conflict both verified accurate). Both blockers were real convergence defects in coordinator-authored text: casing-collision dedup order-dependence, and the equal-timestamp tie — the latter generalized into a discovered PRE-EXISTING convergence bug in the shipped `summary` merge rule, now logged in docs/issues.md.

Performance observations: ~13 min. Read shipped parser/serializer/merge-driver/write-path Rust to ground every finding; fix suggestions directly usable. Zero noise findings.

Routing assessment: third consecutive strong Luna×high review outing (after the API-lane and plan-review data points) — it is now the default first-review lane for coordinator-authored contract text, not just code. Confidence: high.

## 2026-07-10 - grok-4.5 via cursor - W1 diff review (round 1)

Command and run: `delegate --cwd <terra-worktree> --group memora-w1rev cursor safe --prompt-file thoughts/memora-build/w1-review-prompt.md`; alias `cursor-1` (worktree-scoped registry); mode/isolation: safe / git-worktree temporary.

Task and expectation: adversarial review of Terra's uncommitted W1 import-dedup diff, primed with three coordinator findings to verify and extend.

Outcome and verification: 3 BLOCKER + 5 MAJOR + 1 MINOR + 1 NIT, NEEDS-REWORK, ~2 min wall. Confirmed all three coordinator findings; unique catches all verified by coordinator read: supersession-chain lookup broken after rekey (W3 lineage dependency!), retain leaving two records per memory_id, alias_to_id seeded from identity keys, anchor matches unconstrained by harness. Traced `sources/codex.rs:108` to prove Codex candidates never carry mem_* anchors — the load-bearing detail that the primary anchor path can't save the live bug.

Performance observations: fastest deep review of the arc; loaded repo skills unprompted; zero re-derivation of primed findings, pure extension. Verification burden low.

Routing assessment: attacker role re-confirmed on Rust diff review. Confidence: high.

## 2026-07-10 - gpt-5.6-luna via codex - W1 diff review (round 1, parallel slot)

Command and run: same brief, `codex safe --model luna --reasoning-effort high`; alias `codex-1` (worktree-scoped registry); mode/isolation: safe / worktree-temporary.

Task and expectation: same as cursor-1; decorrelated same-brief parallel review.

Outcome and verification: 1 BLOCKER + 4 MAJOR + 2 MINOR, NEEDS-REWORK, 5 min. Convergent with Cursor on the ordinal blocker, migration gap, ambiguity unreachability, and chain lookup. Two unique accepted adds Cursor missed: profile identity is a bare directory basename (cross-root collisions, symlink divergence) and supersede-retry can mint multiple replacements via the substrate's fresh-replacement-per-request behavior (read memory-substrate/src/api/write.rs:101-148 to prove it) — the latter is the same defect class W1 exists to fix, accepted as F10.

Performance observations: read into a SECOND crate to ground a finding; both uniques survived triage. Slightly slower than Cursor, deeper on cross-crate consequence tracing.

Routing assessment: Cursor+Luna as the standing parallel review pair is producing disjoint accepted-finding sets — keep it. Confidence: high.

## 2026-07-10 - grok-4.5 via cursor - W2 spec package review (round 2)

Command and run: `delegate --group memora-w2spec cursor safe --prompt-file thoughts/memora-build/w2-spec-review-prompt.md`; alias `cursor-29`; mode/isolation: safe / isolated copy.

Task and expectation: cross-family round-2 attack on package r2 (post-Luna fixes).

Outcome and verification: 2 BLOCKER + 3 MAJOR + 2 MINOR + 1 NIT, 8/8 accepted → package r3. Both blockers were implementation-reality catches Luna's contract-level pass missed: the drop-fields rule was unimplementable as written (outcome stays RequiresEncryption → body refused — required inventing the dual-classify/rebind design), and "same naming discipline" would have routed aux vectors into the chunk table because the shipped digest hashes only the triple. Also surfaced a THIRD pre-existing drift: shipped `_extras` merge is ours-wins while §14.4 says quarantine (logged in issues.md). Ran a convergence proof of the cue rule itself and pinned the case-fold hazard.

Performance observations: ~9 min; read schema.rs/sqlite_vec.rs/upsert.rs/field_rules.rs to ground findings. Luna→Cursor sequencing on spec text is working exactly like the canonical author→attacker pipeline: contract-level pass first, implementation-reality pass second, near-zero overlap.

Routing assessment: keep Cursor as round-2 on all remaining spec/contract text this arc. Confidence: high.

## 2026-07-10 - stale-alias incident #2 (process note)

`delegate run-output cursor-2` (guessed alias for the round-2 spec review) returned an unrelated old `live_conflicts_count` report. Caught by report-validity check immediately. Cause: coordinator launched the run without recording its numbered alias first — the exact rule violation the round-1 incident established. Correct sequence run thereafter (`runs --group memora-w2spec` → cursor-29). Reinforcement: NEVER issue run-output on a guessed alias; resolve via --group listing first.

## 2026-07-10 - gpt-5.6-luna via codex - W3 merge-proposal spec review (round 1)

Command and run: `delegate --group memora-w3spec codex safe --model luna --reasoning-effort high --prompt-file thoughts/memora-build/w3-spec-review-prompt.md`; alias `codex-78` (main scope); mode/isolation: safe / worktree-temporary.

Task and expectation: contract-level attack on the W3 merge-proposal spec (coordinator-authored), primed with two-clone convergence + shipped-code grounding requirements.

Outcome and verification: 4 BLOCKER + 6 MAJOR + 1 MINOR, NOT-RATIFIABLE, 4m37s; **11/11 accepted** (findings-w3spec-r1.md). Every blocker was grounded in shipped code with exact line anchors: staged candidates servable through `query_chunks` (filters only metadata_only/passive_recall), transitions rejected by `validate.rs:44-55`, no concurrency fence, and the spec citing a per-source supersession event that `events/log.rs` explicitly defers. That last one is the sharpest catch — I wrote "emits the existing per-source supersession event" from the spec's mental model, not the code.

Performance observations: read 6+ source files across 3 crates to ground findings; zero speculative findings; every fix suggestion was implementable as written. Luna×high on coordinator-authored contract text is now 2-for-2 producing accepted-only rounds.

Routing assessment: standing conclusion reinforced — Luna×high is the default first-review lane for contract/spec text. Confidence: high.

## 2026-07-10 - grok-4.5 via cursor - W0 benchmark harness review (round 1)

Command and run: `delegate --group memora-w0rev cursor safe --prompt-file thoughts/memora-build/w0-review-prompt.md` (W0-worktree scope); alias `cursor-1`; mode/isolation: safe / worktree-temporary.

Task and expectation: adversarial review of Sol's uncommitted W0 diff, primed with the strict-AND diagnosis and the 45-gold-id hunt area.

Outcome and verification: 3 BLOCKER + 6 MAJOR + 3 MINOR + 1 NIT in 1m58s; 12/13 accepted (findings-w0-r1.md). Confirmed the coordinator's gold-mapping read independently and went further: the corpus-contamination blocker (one shared daemon corpus across all conversations/questions) was unique to Cursor and is arguably the most consequential finding of the round — nobody else saw it. Also spot-checked split parity with actual sha256 vectors.

Performance observations: fastest reviewer in the fleet again (<2 min); findings arrived with plan-clause citations (caught the sensitivity-injection contract violation against plan r4 text). One finding rejected (feature-gating sha2 — cosmetic).

Routing assessment: attacker role re-confirmed on eval/metrics code; its protocol-faithfulness instincts (dataset isolation) are a distinct strength. Confidence: high.

## 2026-07-10 - gpt-5.6-luna via codex - W0 benchmark harness review (round 1, parallel slot)

Command and run: same brief, `codex safe --model luna --reasoning-effort high` (W0-worktree scope); alias `codex-1`; mode/isolation: safe / worktree-temporary.

Task and expectation: decorrelated same-brief parallel review of the W0 diff.

Outcome and verification: 6 MAJOR + 3 MINOR + 1 NIT in 3m8s; all accepted after merging with Cursor's set. Convergent on gold over-count, judge timeout, streaming load, sensitivity conflation, startup-lane mislabeling, test gaps. Two unique accepted adds: chunk-level hits not collapsed to memory level (duplicate ids consume the top-10 budget and double-count in recall/nDCG — grounded in memory_ops.rs:158-179) and judge-score validation (non-finite/out-of-range scores enter judge_mean unchecked).

Performance observations: the unique finds were both quantitative-integrity defects — Luna keeps catching the "numbers lie silently" class. Ran `git diff --check` + fixture JSON validation + `cargo metadata` as read-only verification.

Routing assessment: Cursor+Luna parallel pair produced disjoint accepted uniques for the third consecutive round — this is now the standing review configuration for the arc. Confidence: high.

## 2026-07-10 - gpt-5.6-sol via codex - W2+W3 convergence bookend (round 2/3)

Command and run: `delegate --group memora-conv codex safe --model sol --reasoning-effort high --prompt-file thoughts/memora-build/w2w3-convergence-review-prompt.md`; alias `codex-79` (main scope); mode/isolation: safe / worktree-temporary.

Task and expectation: cross-document convergence re-read over W2 package r3 + W3 spec r2 together, explicitly forbidden from re-litigating the 28 settled findings; expected residual cross-spec seams.

Outcome and verification: 1 BLOCKER + 3 MAJOR + 1 NIT in 2m40s, NOT-RATIFIABLE; 5/5 accepted → W3 r3 + W2 r4. The blocker was exactly the class the bookend exists for: triage row 2's fix (journal the tuple) was *individually* accepted in round 1 but is unimplementable against the shipped lifecycle validator once you compose it — superseded+pinned trust is invalid, the restore path wasn't authorized, and "minimum trust excluding pinned" had no defined ordering. Sol checked the accepted disposition against validate.rs and the TrustLevel enum rather than taking the triage table's word. Same pattern on aux state: round-1 row 7 fixed staging suppression, Sol saw that rollback restore re-enters the servable set with no aux re-materialization rule anywhere in either doc. Also caught my r2 journal-tail rule contradicting the row-8 disposition I'd recorded hours earlier.

Performance observations: the "restates settled row N because the applied fix is insufficient" framing was used correctly twice — precise, zero noise, no re-litigation of genuinely settled items. Cross-referenced four code files + both triage tables + the plan in under 3 minutes.

Routing assessment: the convergence bookend (Sol high, both docs + triage tables together, after per-doc rounds run dry) is validated — it found composition defects no per-document round could see. Keep as the standing final gate before human ratification on multi-spec arcs. Confidence: high.

## 2026-07-10 - grok-4.5 via cursor - W1 fix-diff re-review (round 2)

Command and run: `delegate --group memora-w1rev2 cursor safe --prompt-file thoughts/memora-build/w1-review-r2-prompt.md` (W1-worktree scope); alias `cursor-2`.

Outcome: 4 MAJOR + 1 MINOR + 1 NIT on Devin's fix layer, all accepted (findings-w1-r1.md round 2). Star finds: F10 adoption is blind to the real handler's GET_BODY_MAX=4096 truncation (mocks return full bodies, so the pinning test structurally cannot fail — the exact author-blindness class again, one layer down) and the production "chain" is one hop while the mock fakes a full walk. Both verified on disk by coordinator before triage.

Routing assessment: probing the REAL surface against the mocks' assumptions is Cursor's durable edge. Confidence: high.

## 2026-07-10 - gpt-5.6-luna via codex - W1 fix-diff re-review (round 2, parallel slot)

Command and run: same brief, `codex safe --model luna --reasoning-effort high` (W1-worktree scope); alias `codex-2`.

Outcome: 1 BLOCKER + 3 MAJOR + 1 MINOR, all accepted after merge. Convergent with Cursor on collision uniqueness, one-hop chain, fail-open. Unique adds: adoption can select a TOMBSTONED replacement (get response carries no lifecycle status — quantitative-integrity class again) and alias-map seeding violates F6's contract. Sandbox blocked its cargo check; honest about it.

Routing assessment: fourth consecutive round of disjoint accepted uniques from the Cursor+Luna pair. Standing config confirmed. Confidence: high.

## 2026-07-10 - gpt-5.6-sol via codex - W6 memo read (xhigh, plan-mandated)

Command and run: `delegate --group memora-w6 codex safe --model sol --reasoning-effort xhigh --prompt-file thoughts/memora-build/w6-memo-read-prompt.md`; alias `codex-80` (main scope); 3m13s.

Task and expectation: judgment read of the finished W6 memo per plan §W6; expected concur-with-nits.

Outcome and verification: **DISSENT**, and it was right. Five verdicts: agreed W3-value-moot; disputed the inversion framing (unnecessary — the abstraction transits as plaintext anyway; my vec2text claim was uncited), disputed the classification framing (Stream D is raise-only, so abstraction-only classification = unauthorized declassification — the strongest anti-Option-2 argument, which the memo had missed), disputed the 28/786 denominator (28 = held-local chunk JOBS; sensitive records default index_embeddings=false so even the local lane excludes them; abstraction_compile can't reach encrypted memories through refused supersession — the r1 "local-lane parity" claim was flat wrong), disputed Option-3 dominance as under-specified (second query embedding + second provider lifecycle vs W4's one-embedding contract and the 11-17MB win), and expanded the fail-closed list (declassification authority, four ingress paths, revocation breadth, batch races, zero-request tests). All folded into memo r2; recommendation direction survived but on corrected grounds.

Performance observations: xhigh spent its effort exactly where briefed — checked the memo's citations against source, caught an uncited empirical claim, and found the two contract interactions (Stream D raise-only; W4 single-query-embedding) that both the Opus researcher and I missed. Honest about its own limit (no network to verify vec2text literature).

Routing assessment: Sol xhigh as the final judgment read on coordinator-authored analysis is validated at the strongest level yet — it materially corrected a memo two other passes had grounded. Keep for every judgment-dense deliverable this arc. Confidence: high.

## 2026-07-10 - devin swe-1.7 - W0 fix round 1 (16 findings) + W1 fix round 2 (7 findings)

Two work-mode runs, --isolation none in the respective worktrees (W0: `devin-1` in memora-w0fix, ~75m; W1: `devin-2`... actually `memora-w1fix2`'s run, ~15m). Both executed their findings lists fully and honestly reported scope boundaries. Both committed despite instructions (W0 round said do-not-commit; W1 round 2 was granted one labeled commit) — treat "Devin will commit" as its standing behavior and plan the diff-review accordingly.

Verification outcomes: W0 — its `--lib`-scoped memoryd gate hid 3 integration-test failures my full gate caught one at a time (candidate-leak pins in daemon_e2e, mcp_forward, mcp_stdio + the vector_recall_fusion strict-AND premise). All four were tests pinning pre-fix behavior, not defects in the fix — but the round-1 lesson stands: Devin's self-selected gate is narrower than the coordinator's. W1 round 2 — all 7 landed as specced and my full gate ran green, but round-3 review found 4 new HIGHs in its fix layer (livelock scope, namespace fallback, BFS bound, encrypted adoption). Pattern: Devin executes contracts perfectly and does not reason beyond them — every residual defect was in a case the findings contract failed to name.

Routing assessment: keep Devin for well-specified fix lists; the coordinator must budget a full-gate + adversarial re-review per round, and cap-aware waves should write tighter contracts up front (name the error-path and encrypted/truncated variants explicitly). Confidence: high.

## 2026-07-10 - grok-4.5 via cursor + gpt-5.6-luna via codex - W1 round 3 (cap round)

`cursor-3` + `codex-3`, group memora-w1rev3, same brief. Cursor: 1 merge-blocking finding (fail-closed overreach → permanent import livelock on dangling supersession links; diagnosed the mock/prod divergence exactly — mock `unwrap_or_default` vs production error). Luna: 3 HIGHs (cross-project namespace fallback — coordinator-verified at plan.rs:150; BFS bound semantics; encrypted-replacement adoption hash mismatch). Zero overlap between the two reports — fifth consecutive disjoint round from this pair.

Process note: round 3 was the cap round and was NOT dry → W1 halted and escalated per loop discipline. The pair's throughput (each round <6 min, all findings verified real so far) is not the bottleneck; the fix-contract completeness is.
