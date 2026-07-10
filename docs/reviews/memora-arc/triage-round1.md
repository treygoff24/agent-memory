# Round-1 triage — plan r2 → r3 (2026-07-10)

Reviewers: Sol xhigh (`codex-73`, cross-family) + native Opus plan-reviewer (`plan-reviewer-r2`). Convergent verdict axis: Sol "structural rework"; Opus "executable after blocker fixes." Coordinator ruling: Sol is right — the W2↔W4 inversion plus the missing governance merge operation change the wave graph, not just wording. r3 restructures.

Disposition key: ACCEPT (plan changed), ACCEPT-ADAPTED (changed, different fix than suggested), REJECT (reason given).

## Blockers

| ID | Finding | Disposition |
| --- | --- | --- |
| S-B1 / O-B1 | W2 needs abstraction embeddings W4 creates (dependency inversion) | ACCEPT — new wave order: abstraction/cue substrate foundation (new W2) lands before merge-on-dream (new W3) |
| S-B2 | Stream C review approve can't atomically supersede merge sources | ACCEPT — new W3 includes a spec'd merge-proposal operation (approval transaction = activate replacement + supersede all sources atomically); ratified before implementation |
| S-B3 | Write-time cues bypass v4 §5 machine-verification firewall; migration 5→6 owned by v4 P2 | ACCEPT — cues are **vector-only** in this plan; zero trigger-index writes; trigger registration deferred wholesale to v4 P2. Schema-6 ownership escalated to Trey decision point |
| S-B4 | Privacy classifier doesn't scan abstraction/cues | ACCEPT — new W2 requires combined body/title/summary/abstraction/cues classification on every write path (write, supersede, import, promotion, backfill); strictest outcome controls; secret-in-cue tests mandatory |
| O-B2 | issues.md misdiagnosed the import bug; machinery exists, `ImportState::default()` starves it | ACCEPT — verified live (`cli/import.rs:30`, `execute.rs:369-380`). issues.md corrected; W1 re-scoped to root-cause-first |
| O-B3 | Path+profile identity breaks on rename / path+content change | ACCEPT — W1 keys on recovered canonical `mem_*` id when present, portable tuple as fallback; edge cases enumerated (rename, move, symlinked profiles, path reuse); ambiguous historical repair is report-only |
| O-B4 | Index schema bump unbudgeted; collides with v4 P2's reserved 5→6 | ACCEPT — explicit migration task in new W2 (bump + doctor + pre-migration copy); schema-number ownership = Trey decision point 5 |

## Majors

| ID | Finding | Disposition |
| --- | --- | --- |
| S-M1 / O-B3 overlap | W1 portable identity contract | ACCEPT (folded into W1 rewrite) |
| S-M2 | W3 merge candidates unfenced (cross-namespace loss/leak) | ACCEPT — hard fences: active lifecycle, same scope + canonical namespace, same memory type, privacy-compatible; W1 lineages excluded |
| S-M3 / O-R1 | Benchmark writes vs governance | ACCEPT — W0 specifies ingestion adapter: real daemon writes, pinned dataset artifact as provenance, explicit classification, asserted final status, disposition counts reported; governance-drag measured and separated |
| S-M4 / O-R6 | Judge not comparable to published numbers | ACCEPT-ADAPTED — internal-only scoring, pinned judge identity+prompt, paired A/B deltas; comparison with published numbers prohibited in reports (chose Sol's option B over buying gpt-4o-mini parity) |
| S-M5 / O-R5 | Stream A frontmatter is not safely "additive amendment" | ACCEPT — reclassified: Stream A **version-bump decision routed to Trey** (decision point 1 rewritten); merge/convergence/normalization semantics specified in W2 spec task |
| S-M6 | `memoryd remember` doesn't exist | ACCEPT — surface = `memoryd write`/`write-note` meta fields + protocol DTO + schema + envelope tests + skill update |
| S-M7 | Row kinds need full embedding lifecycle enumeration | ACCEPT — W2 spec task enumerates identity (row kind + memory/cue id + content hash), stale fence, enqueue/delete/reconcile/switch/drop/reindex/doctor per kind |
| S-M8 / O-NIT | RRF formula ambiguous; per-cue contribution gameable | ACCEPT — exact formula over named primitive lanes in W4; cue hits collapse to best-rank-per-memory before fusion; deterministic tie-break |
| S-M9 / O-R3 | No latency/fail-open gate | ACCEPT — W4 gate adds p50/p95/p99 for search + prompt/desk/work-stream cues on both lanes; single query-embed reuse verified; timeout → documented degraded result |
| S-M10 / O-R2 | Train=test loss function | ACCEPT — pre-registered dev/holdout split; tune on dev, freeze, score holdout once; per-dataset regression check |
| S-M11 | Backfill lacks migration rigor + approval | ACCEPT — backfill extracted to its own wave (W5): separately approved, dry-run manifest, rehearsal on copy of ~/memorum, resumable, disposition counts, export/import round-trip, two-clone tests |
| S-M12 | Workflow can't host unscripted coordinator triage | ACCEPT-ADAPTED — review rounds = plain parallel delegate runs; my triage produces a findings artifact; Devin fix = plain run. delegate-workflows reserved for W0 benchmark scoring fan-out and mechanical sweeps where the graph is fully deterministic |
| S-M13 | W0∥W1 worktree ff-integration undefined; thoughts/ not gitignored; BUILD-STATE sync | ACCEPT — integration order contract (first-done integrates, second rebases before coordinator commit); `--forbid-commit`; thoughts/ added to .gitignore; lane prompts get scoped artifacts, not dirty-tree copies |
| O-R4 | W4 dark on live corpus while dream loop torn off | ACCEPT — backfill (W5) runs via manual `memoryd dream now` invocations; scheduler re-wire not required but noted as open dogfood decision |

## Minors / nits

- S-MIN1 placeholder gate crates → ACCEPT: real names (`memoryd`, `memory-governance`, `memory-substrate`, `memorum-eval`).
- O-MIN doctor coverage of new row kinds → ACCEPT (W2 gate).
- O-MIN export/import round-trip → ACCEPT (W2 + W5 gates).
- O-MIN two-clone cue merge semantics → ACCEPT (W2 spec task; proposal: cues = set-union, abstraction = ours-wins + dream repair; final call in spec review).
- O-MIN backfill embed volume (~3k API calls) → ACCEPT (noted in W5; pennies).
- O-NIT weight-count mismatch → ACCEPT (four named lanes, four weights).

## Rejections

None outright. Two suggested fixes were substituted with alternatives (S-M4 judge parity spend → internal-only scoring; S-M12 gate-artifact workflow → plain runs + artifact protocol), reasons above.
