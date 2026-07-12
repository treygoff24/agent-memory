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

## 2026-07-10 - grok-4.5-fast-xhigh via cursor - W0 round-3 cap review

Command and run: `delegate --cwd <w0-worktree> --group memora-w0rev3 cursor safe --prompt-file thoughts/memora-build/w0-review-r3-prompt.md`; alias cursor-3; mode/isolation: safe/worktree-temp.

Task and expectation: final-round scoped re-review of fix commit `6721ff1` against G1–G7 hunt areas; must-be-dry cap round.

Outcome and verification: FINDINGS — one HIGH (H1 grandchild-pipe join stall on the timeout path), with a **local OS-level reproduction** (`sh -c 'sleep 4 & wait'` vs plain `sleep 4`) proving the mechanism, and the correct observation that round-2's own fix design *introduced* the stall. Also correctly cleared G2/G3/G7 and honored the G5 waiver. 2m30s. One miss: called the shared-socket-parent teardown safe where Luna found a real (narrow) race — coordinator adjudication sided with Luna after reading `prepare_socket_parent`.

Performance observations: elite again on evidence-computing review — the repro is exactly the behavior that distinguishes a real reviewer from a diff-reader. The G2 miss shows even its "verified OK" claims need coordinator adjudication when another lane dissents.

Routing assessment: keep as first review lane; when two lanes disagree on a specific code path, the coordinator MUST read the disputed code — both lanes were each right once in this round. Confidence: high.

## 2026-07-10 - gpt-5.6 luna (reasoning high) via codex - W0 round-3 cap review

Command and run: `delegate --cwd <w0-worktree> --group memora-w0rev3 codex safe --model luna --reasoning-effort high --prompt-file <same>`; alias codex-3 (W0 scope).

Task and expectation: same cap-round brief as cursor-3.

Outcome and verification: FINDINGS — converged with Cursor on H1 (independent discovery, different framing: "exits without draining a large stdin, or spawns a descendant"), plus two uniques both accepted: H2 socket-parent teardown race (coordinator verified real — prepare→bind window) and H3 the metric fixture's structural inability to discriminate formula bugs (INSUFFICIENT-FIX on G6, correct — Cursor had noted the same weakness but under-severitied it as non-blocking). No false positives this round.

Performance observations: this is the round where Luna beat Cursor on judgment (H2/H3 severity calls) while Cursor beat Luna on evidence (H1 repro). The pair remains disjoint-but-overlapping in exactly the useful way — 6 consecutive rounds now.

Routing assessment: Cursor+Luna stays the standing review pair for this arc. Confidence: high.

## 2026-07-10 - grok-4.5-fast-xhigh via cursor - W0 scoped verify of coordinator fix

Command and run: `delegate --cwd <w0-worktree> --group memora-w0rev3 cursor safe --prompt-file thoughts/memora-build/w0-verify-coordfix-prompt.md`; alias cursor-4; safe/worktree-temp.

Task and expectation: verify the coordinator's uncommitted H1/H2/H3 fix diff only; hunt list included pgid pitfalls and nDCG arithmetic.

Outcome and verification: FINDINGS — 2 MEDIUM + 1 LOW, every one real and precisely the residual risk classes named in the hunt list (deadline stacking across sequential recvs; post-reap pgid recycling; comment drift). Independently re-derived the nDCG hand-math and confirmed `rank_metrics` is a pure move (incl. the full-list-MRR subtlety). No false positives. ~3 min.

Performance observations: as a verifier of *someone else's fix* it's just as sharp as an attacker of author code — the pgid-recycling catch is genuinely expert Unix semantics. Handing it an explicit hunt list of failure classes returns maximum value per minute.

Routing assessment: scoped verify prompts with named residual-risk classes are the highest-leverage way to spend this lane. Confidence: high.

## 2026-07-10 - gpt-5.6 luna (low) via codex call - pinned judge, baseline₀ fleet (240 calls)

Command and run: `scripts/eval/pinned-judge.sh` → `delegate --json codex call --read-only --model luna --reasoning-effort low --output-schema <score/rationale> --prompt-file <rubric+record>`; invoked serially by the benchmark harness.

Task and expectation: score retrieved-context support for the gold answer on a frozen 1.0/0.5/0.0 rubric, 240 records.

Outcome and verification: 236/240 scored, 4 timeouts at 120s (recorded as typed judge_error, excluded from judge_mean only). Score distribution exactly on the rubric points — no off-rubric drift across 236 calls. ~8.5s/call calibration held at fleet scale (42m total wall incl. harness). Spot-checked verdicts during calibration were correct.

Performance observations: luna/low is reliable as a high-volume structured judge under --output-schema; the schema constraint appears load-bearing for the zero-drift behavior.

Routing assessment: standing judge for the arc (frozen). For W4's A/B the same 4-timeout tail is expected; treat >2% timeout rate as an anomaly worth diagnosing. Confidence: high.

## 2026-07-11 - grok-4.5-fast-xhigh via cursor - W1 verify round (cursor-4, W1-worktree registry)

Command and run: `delegate --json --cwd <w1-worktree> cursor safe --prompt-file thoughts/memora-build/w1-verify-prompt.md`; alias cursor-4 (W1-worktree registry — distinct from the main-registry cursor-4 used in W0); mode/isolation: safe/isolated copy; run `del_20260711T050445Z_73a967`.

Task and expectation: scoped adversarial verify of coordinator fix commit `cf30e96` (F20–F23) against six named hunt areas; expected DRY or a small number of high-quality findings.

Outcome and verification: FINDINGS — 1 HIGH (F23 mixed-version reopen via serde-default `encrypted:false`; convergent with the coordinator's own pre-review flag), 1 MAJOR (corrupt supersession-mirror row mapped to `not_found` for the live parent — coordinator verified on disk at trust_artifact.rs:479-480 before accepting), 2 NITs (backstop tradeoff; one pairing test not solo-red). Also produced a correct per-test "would it fail pre-fix" table and correctly cleared the tombstone path with the actual daemon mapping as evidence. Both real findings fixed in `d28b677`, gate re-ran green (1117/0).

Performance observations: ~8 min wall. Again the standout Cursor behavior: it computed evidence (traced the daemon error mapping through three files; checked live-corpus record shapes against the June bucket-fix commit) rather than pattern-matching the diff. The MAJOR is a finding class delegate fix lanes and the coordinator both missed across four prior rounds — it required reading OUTSIDE the diff (the daemon's error construction) to falsify the diff's core assumption (`not_found` ⇒ provably gone).

Routing assessment: Cursor safe remains the verify lane of choice for fix diffs whose correctness hinges on cross-file contracts; give it the falsifiable assumption explicitly ("is not_found the ONLY code that means gone?") — that framing produced the MAJOR. Confidence: high.

## 2026-07-11 - gpt-5.6-sol (high) via codex - W2 build lane died at finish line (codex-81)

Command and run: `delegate --json --cwd <repo> --isolation worktree codex work --model sol --reasoning-effort high --forbid-commit --prompt-file thoughts/memora-build/w2-implement-prompt.md`; alias codex-81; run `del_20260711T044830Z_1e145a`; worktree `codex-20260711T044830Z_1e145a`.

Task and expectation: full W2 build (six tasks: frontmatter fields, aux embedding lifecycle, privacy composition, schema 5→6, CLI surface, abstraction_compile dream job) from the ratified spec package.

Outcome and verification: **failed (harness_error)** after ~35 min — but the work substantively landed: 118 files, +1579/−66 uncommitted in the worktree, and Sol's own final gate logs (/tmp/w2-gates/) show substrate 418/0, memoryd 1097/0, clippy clean on both crates. The harness transport died before the completion report; stderr shows MCP auth fatals (granola/composio rmcp AuthRequired) at startup and normal apply_patch retry noise, no model failure. Coordinator response: continuation Sol lane launched IN PLACE in the same worktree (--isolation none; local alias codex-1, arc alias codex-82) to audit the tree against the §A5 acceptance signals, close gaps, and deliver the report. No work discarded.

Performance observations: excellent CPU discipline throughout (crate-scoped gates only, logs to files); the failure class is harness/transport, not model. Second observed instance of "codex lane dies after the meaty work, before the report" — treat a dead codex lane's worktree as probably-valuable; always inspect before relaunching from scratch.

Routing assessment: keep Sol high as W2 author; the recovery pattern (new lane, same worktree, isolation none, audit-first prompt) is cheap and preserves sunk work. Confidence: high.

## 2026-07-11 - gpt-5.6-luna (medium) via codex - Stream A v1.2 version splice (arc alias codex-83)

Command and run: `delegate --json --cwd <repo> --isolation none codex work --model luna --reasoning-effort medium --prompt-file thoughts/memora-build/w2-spec-v12-prompt.md`; main-registry alias codex-82 (arc codex-83 — the W2 continuation holds arc codex-82).

Task and expectation: mechanical splice — full v1.1 copy + ratified §A deltas into a new stream-a-core-substrate-v1.2.md, one file only, placement map provided.

Outcome and verification: succeeded, one file created (2,257 lines), no other files touched. Coordinator diff-review (v1.1↔v1.2: 109 additions / 5 version-string replacements) found 2 splice defects — §A1 type-table rows duplicated into the §14.4 merge-rules table, and a dangling "(see deviation flag above)" reference to a package-only section — plus 1 improvement opportunity (the §A5 block landed as one giant bullet; split per-signal for manifest binding). All fixed by coordinator before commit (`v1.2` in `git log`).

Performance observations: fast (~7 min), obedient to the one-file constraint, section placement per the map otherwise correct including the tricky §10.2.1/10.2.2 integration. The two defects are exactly the "context-free paste" class a mechanical lane produces — cheap to catch on a structural diff read.

Routing assessment: Luna medium is the right lane for version-splice work IF the coordinator reviews the version diff (mandatory anyway for spec files). Keep the pattern. Confidence: high.

## 2026-07-11 - grok-4.5-fast-xhigh via cursor - W2 wave review round 1 (cursor-1, W2-worktree registry)

Command and run: `delegate --json --cwd <w2-worktree> cursor safe --prompt-file thoughts/memora-build/w2-review-prompt.md`; safe/isolated copy.

Task and expectation: adversarial review of wave commit `5667eea` (122 files) against the ratified package across 10 named hunt areas.

Outcome and verification: FINDINGS — 1 BLOCKER (generation dual-classify drop dead for Scope::User under the Me-namespace floor; coordinator verified on disk and confirmed it reproduces the June review-reject bug class), 2 HIGH (aux drain starvation behind poisoned chunk path; aux fence TOCTOU + hashless job delete — both coordinator-verified), 2 MAJOR, 2 MINOR, 1 NIT. Also produced a per-hunt-area scorecard that CLEARED the areas the coordinator's own read had cleared (table identity, case folding, §A4 order) — convergent negative evidence is worth as much as the findings.

Performance observations: ~12 min. The BLOCKER required composing the diff with an out-of-diff invariant (PrivacyPolicy floor semantics) — the same out-of-diff reasoning that produced its W1 V-MAJOR. It also correctly validated part of the author's mega-test claim (revocation paths real) instead of reflexively flagging it — low false-positive discipline.

Routing assessment: Cursor stays the primary attack lane. Feed it the falsifiable invariants explicitly (the hunt-area framing keeps paying). Confidence: high.

## 2026-07-11 - gpt-5.6-luna (high) via codex - W2 wave review round 1 (codex-2, W2-worktree registry)

Command and run: `delegate --json --cwd <w2-worktree> codex safe --model luna --reasoning-effort high --prompt-file thoughts/memora-build/w2-review-prompt.md`.

Task and expectation: same brief as cursor-1, decorrelated family.

Outcome and verification: FINDINGS — converged independently on the aux fence TOCTOU, aux starvation, and spawn_blocking findings (3/3 of the shared set), and added 2 unique: the migration test being structurally green (SCHEMA_SQL pre-creates v6 tables, so migrate_v6 is never exercised — sharper than Cursor's fidelity note; upgraded to MAJOR in triage) and the encrypted-candidates-perpetually-skipped churn in abstraction_compile. Its BLOCKER (substrate should classify the combined payload) was REJECTED in triage as a layering misread — Stream A never scans content; classification is supplied per-write (invariant #2) — but its residue (audit every daemon entrypoint's classify input) was kept as fix-round verify work.

Performance observations: ~14 min. One rejected top-severity finding out of six — an acceptable false-positive rate given the two unique real finds. The structurally-green-migration-test catch is exactly the test-fidelity class Luna has now caught twice in this arc (W0 H3 fixtures, this).

Routing assessment: keep Luna high as the standing second review lane; weight its test-fidelity findings heavily, double-check its architecture-boundary findings against the layering contract before accepting. Confidence: high.

## 2026-07-11 - devin swe-1.7 via devin - W2 fix round 1a (devin-1, W2-worktree registry)

Command and run: `delegate --json --cwd <w2-worktree> --isolation none devin work --prompt-file thoughts/memora-build/w2-fix-r1-prompt.md`; run `del_20260711T062103Z_da7da3`, 958s.

Task and expectation: implement accepted findings W2-F1..F10 from the triage artifact with required tests + gate.

Outcome and verification: **partial with empty report** — exited 0 with resultQuality=empty (no completion report, 1 byte stdout). Tree audit showed F1 (Agent-namespace probe + all 3 required tests), F2 (single-txn fence + hash-scoped delete — correct shape), F3/F4 (aux drain on all paths + spawn_blocking), F5 (Archived), F8 (rsplit) landed; F6/F7/F9/F10 NOT started. Its own new code didn't compile (`old_hash` moved-value in vector_lifecycle.rs) and its new starvation test had a wrong expectation (asserted per-pass aux success ≥2 forever; aux jobs are consumed on pass 1) — Devin never ran the gate it was instructed to run. Coordinator fixed both test defects inline (clone + first-pass ≥1 assertion + a sharper post-exhaustion scenario: fresh aux work behind the exhausted head) and re-ran green. Round 1b lane launched for the missing F6/F7/F9/F10 with a mandatory write-report-to-file instruction.

Performance observations: the substantive FIXES it did land are high quality (the F2 transaction restructure is exactly the contract). Failure modes: window cutoff before trailing items (known class), no gate run, no report. 16 min wall.

Routing assessment: keep Devin for findings-list fixes but (a) cap list size ~5-6 items per launch, (b) always require a file-written report, (c) never trust its gate claim — it makes none. Confidence: high.

## 2026-07-11 - devin swe-1.7 via devin - W2 fix round 1b (devin-2, W2-worktree registry)

Command and run: `delegate --json --cwd <w2-worktree> --isolation none devin work --prompt-file thoughts/memora-build/w2-fix-r1b-prompt.md`; scoped to the 4 items round 1a dropped (F6/F7/F9/F10) + the entrypoint-classify audit; report REQUIRED as a file.

Outcome and verification: complete — file report delivered (`thoughts/memora-build/w2-fix-r1b-report.md` in the worktree), all 4 items landed and coordinator-verified on disk: the migration test now genuinely drives `migrate_v6` over a downgraded DB with data (would fail on a no-op); freshness + candidate-exclusion tests added; manifest re-pointed to v1.2 with all 11 signals bound. BONUS: the audit surfaced a REAL gap — the importer's `build_write_meta` never forwarded abstraction/cues into the daemon write meta, so import-path classification saw body-only; fixed with type-guarded forwarding. Coordinator gate: clippy ×2 clean, substrate 423/0, memoryd 1130/0.

Performance observations: ~13 min; the 4-item scope + file-report requirement fixed both round-1a failure modes (cutoff, empty report). The audit finding shows Devin CAN do discovery work when the audit is framed as a checklist with an explicit output table.

Routing assessment: confirms the round-1a lesson — cap Devin lists at ~4-6 items and demand file reports. Keep. Confidence: high.

## 2026-07-11 - grok-4.5-fast-xhigh via cursor - W2 rounds 2+3 (cursor-2, cursor-3, W2-worktree registry)

Two scoped rounds on the W2 fix layer. Round 2 (`c84b2de`): convergent with Luna on the race-test false pin and the migrate_v6 pub leak; recomputed all 11 manifest bullet hashes as verification rather than trusting the report. Round 3 (`037d3ef`, cap verify): caught that the coordinator's own replacement pin was a DUPLICATED SQL literal — deleting the content_hash scope from production alone would have kept every test green. That is the third distinct false-pin catch of this arc (W0 H3 fixtures, W2 r2 race test, this) and the first one aimed at coordinator-authored code — the lane attacks all authors equally, which is exactly the property the loop needs. Remedies prescribed were applied verbatim (`701edec`, shared const binding production+test).

Routing assessment: Cursor safe remains the arc's verify lane; its test-fidelity instincts now have a 3/3 hit rate. Confidence: high.

## 2026-07-11 - gpt-5.6-luna (high) via codex - W2 round 2 (codex-3, W2-worktree registry)

Round 2 on `c84b2de`: convergent on the false pin (filed HIGH with the sharper mechanism statement) and the pub-export leak; unique real find: import forwarded unbounded cue arrays before any validation (accepted, severity adjusted to MAJOR — local self-DoS). Zero rejected this round (1-for-1 improvement over its round-1 layering misread).

Routing assessment: keep as standing second lane. Confidence: high.

## 2026-07-11 - meta/muse-spark-1.1 via opencode - W3 wave review round 1 (opencode-1, W3-worktree registry) — FIRST ARC OUTING

Command and run: `delegate --json --cwd <w3-worktree> opencode safe --model muse --prompt-file thoughts/memora-build/w3-review-prompt.md`; DP6-approved test slot; safe mode (config-lockdown isolation; external skill reads correctly blocked by sandbox).

Task and expectation: adversarial review of `0f680f3` (merge-on-dream) across 10 hunt areas, decorrelated from Cursor; calibration target = Grok 4.5 / GPT-5.5 class.

Outcome and verification: FINDINGS (1 HIGH, 1 MAJOR, 3 MINOR) + an unusually rich verified-negative record: independently traced the availability-gap invariant at all five crash points, all four idempotency dedup keys, proposal exclusivity at generation AND apply, the journal matrix including a novel edge (valid-parse truncation → fail-closed quarantine, correctly judged safe-but-harsh), and reconcile-before-socket-bind ordering. Its MAJOR (central non-servability predicate unused by any read lane) was accepted over Cursor's MINOR framing of the same fact; two unique MINORs accepted (aux rows created at staging contra spec §3.6; over-strict sensitivity fence). Its HIGH (Agent-namespace probe under-classifies Me scope) was REJECTED — scanner-vs-floor conflation, mirror image of the June bug the probe design exists to fix, and unreachable anyway (Me-tier sources are fence-excluded). Its floor-order trace also missed that classify_plaintext_memory ignores frontmatter.sensitivity (Cursor caught the outcome-layer gap it declared correct).

Performance observations: ~19 min wall (slowest lane this arc, no live progress events — expected for opencode). Report structure was the best-organized of any lane: per-hunt-area with file:line evidence, explicit negative results, and honest probability judgments. Weakness profile: privacy-layering subtleties (the one rejected HIGH and the missed outcome-layer gap are both floor/scanner/outcome composition errors) — same weak axis where Luna erred in W2 round 1.

Routing assessment: comfortably Grok-4.5-class on structural/state-machine reasoning; below Cursor on privacy-composition subtlety. Worth keeping in the review rotation for state-machine-heavy waves; pair with Cursor (never solo) on privacy-critical diffs. Next comparison: give Muse and Luna the same scoped fix-diff verify and compare. Confidence: medium-high (one outing).

## 2026-07-11 - batch entries: W4-prep + W3 round-2 lanes

**Terra medium (codex-84, main registry) — W4-prep build.** Full 4-item scope delivered with honest failure reporting (flagged the SUN_LEN env failures as pre-existing). One real defect: an ungated optional-dep reference broke the no-default-features honesty gate — caught by coordinator gate, not Terra's own. Terra remains the everyday-build lane; its self-gate trusts the default feature set, so coordinator gates must include the feature matrix. Confidence: high.

**Devin swe-1.7 — W4-prep fix (WP-F1..F6) + W3 fix rounds B (F5-F13) and C (in flight).** W4-prep round: complete, file report delivered, contracts reconstructed faithfully when the triage file was absent from its tree (coordinator now inlines contracts in every Devin prompt). W3 round B: complete, no-commit honored (after the round-A violation), report recovered from stdout, plus an honest self-found gate fix (test helper lacked seeded vectors for the new F6 preflight). The 4-6-item scoped-list pattern is now 3-for-3. Confidence: high.

**Cursor (grok-4.5-fast-xhigh) — W4-prep rounds 1-3 + W3 round 2.** Four more verified catches: the dream-harness grandchild-stall port gap (W0 H1 class in PRODUCTION dreaming — the single highest-value find of the arc's second half), the early-reap-skips-group-SIGKILL escalate gap in the coordinator's own port, the enrichment structural-fallback cap bypass, and the convergent W3 post-activation-reject BLOCKER with a full pre-fix trace of the pinning test. Also confirmed-or-refuted every attack it was handed (the pin-trace confirmation is as valuable as the findings). Standing: the arc's verify lane, unchallenged. Confidence: high.

**Muse Spark 1.1 (opencode) — W3 round 2 (second outing).** Convergent on the BLOCKER with an independent trace (journal-record-level), unique MAJOR (pinned→superseded open to all actors — a spec-conformance catch Cursor missed), and precise verified-negatives on the retry counter. Two solid outings: state-machine reasoning at Cursor's level; privacy-composition remains its weak axis (round-1 rejected HIGH). Rotation-worthy for governance/state-machine waves. Confidence: medium-high.

## 2026-07-11 - gpt-5.6-luna via codex - B1/B2 encrypted-review-decision diff review

Command and run: `delegate codex safe --model luna --reasoning-effort high --prompt-file thoughts/memora-build/b1b2-review-prompt.md`; alias codex-86; mode/isolation: safe/isolated copy.

Task and expectation: adversarial review of the uncommitted B1 (encrypted review decisions) + B2 (raw-frame exit codes) diff with six named hunt areas; expected it to probe the TOCTOU seam and contract coherence.

Outcome and verification: 3 findings, ALL REAL. F1 HIGH TOCTOU (closure applies onto a fresh read without revalidation — archived→active resurrection is validator-legal; also routes into the plaintext-only quarantine arm) — ACCEPTED, fixed by closure-revalidation (only a still-queued Candidate is decidable), which also collapses its F5 calibration-staleness note. F2/F6 HIGH (covered-command crosswalk not contract-coherent for admin commands, §2 pins them to 1/2-style codes) — ACCEPTED; independently self-addressed minutes before the report landed (exit 1 + contract doc amendment), so convergent confirmation. Hunt areas 2/3/4 explicitly cleared with correct reasoning (ciphertext preserved, W3 actor honored, only review/quarantine/doctor call print_response).

Performance observations: ~20 min wall; read the substrate API, validator, contract doc, and all callers unprompted; zero noise findings; the archived→active validator-gap observation required composing three files. Verification burden low — both findings checked out on first read.

Routing assessment: Luna high remains the default cheap review lane for governance-critical diffs; this is its third field validation catching a real lifecycle defect Opus-family authorship missed. Confidence: high.
