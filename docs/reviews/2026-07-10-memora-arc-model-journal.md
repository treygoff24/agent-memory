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
