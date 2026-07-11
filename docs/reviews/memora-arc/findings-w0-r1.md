# Findings triage — W0 benchmark harness, round 1

Build: Sol high (codex-75, worktree `delegate/codex-20260710T220304Z_9bac88`). Coordinator gate: GREEN (`/tmp/w0-gate-r0.log`, 31 lib + all integration suites, exit 0) — the gate is green while the scorer is wrong, which is exactly the author-blindness failure mode.

Reviewers: Cursor (cursor-1, W0-worktree scope) and Luna high (codex-1, W0-worktree scope), both NEEDS-REWORK. High convergence with each other and with the coordinator riskiest-read (gold over-count found independently three times). Merged below; C=Cursor, L=Luna, K=coordinator.

## Accepted findings (fix contracts)

| # | Sev | Source | Finding → fix contract |
| --- | --- | --- | --- |
| F1 | BLOCKER | C1+L1+K | LoCoMo gold maps evidence turns (`D1:3`) to entire sessions (45-id gold sets); `dia_id` dropped at ingest. **Fix:** preserve `dia_id` through ingestion (turn→promoted-id map), map evidence to exact turn memory ids; date artifacts never gold. |
| F2 | BLOCKER | C2+K | Recall@10 = fraction over bloated gold — structurally capped, insensitive for W4 A/B. **Fix:** with turn-level gold, report **Hit@10** (any gold in top-10) and Recall@10 with denominator `|gold|` (now evidence-sized); formulas documented in the report schema. |
| F3 | BLOCKER | C3 | One shared daemon corpus across all conversations/questions — later items retrieve earlier items' memories; protocol-unfaithful, poisons baseline₀. **Fix:** fresh scaffold (or full wipe + re-init) per LoCoMo conversation and per LongMemEval question; gold and search scoped to that corpus. |
| F4 | MAJOR | C4+L2 | Report claims a `startup_recall` lane but startup output never reaches metrics or judge; empty `answer_basis` is mislabeling. **Fix (coordinator call):** judge context stays search-only (startup is not query-conditioned — Luna's "include it in judge inputs" rejected on that ground); capture startup block ids + bytes per item and score startup coverage separately (gold∩startup); add `search_hit_count`, `search_empty`, per-lane labels. |
| F5 | MAJOR | C5 | LongMemEval gold includes all session turns + AgentPrimary date stubs. **Fix:** gold = `has_answer` turns when marked, else all turns of answer sessions; date stubs always excluded. |
| F6 | MAJOR | C6+L6+C12 | Harness injects `meta.sensitivity: "internal"` (becomes a CallerSensitivity floor — plan says expected, not injected); observation greps the filesystem and conflates storage disposition with sensitivity. **Fix:** omit sensitivity from write meta; `expected_sensitivity` lives in the report only; observed sensitivity read via daemon `get` (typed), compared expected-vs-observed separately from disposition/encryption; delete the filesystem walk. |
| F7 | MAJOR | C7+L3 | External judge has no timeout — a hung judge blocks the whole run. **Fix:** configurable timeout (default 60s), `try_wait` poll, kill+reap on expiry, typed `judge_timeout` error. |
| F8 | MAJOR | L8 | Judge scores unvalidated — negative/inf values enter `judge_mean`. **Fix:** require finite score within the pinned rubric range; typed error otherwise. |
| F9 | MAJOR | C8+L4 | 277MB cleaned file fully deserialized before filtering. **Fix:** oracle file is the documented default; cleaned path must not retain haystacks of non-selected items (streaming visitor or filter-during-read); memory expectations documented. |
| F10 | MAJOR | L5 | Chunk-level FTS hits not collapsed to memory level — duplicate ids consume the top-10 budget and double-count in recall/nDCG. **Fix:** dedupe by memory id keeping best rank before metrics and judge context; top-10 = top-10 *memories*. |
| F11 | MINOR | L7 | `exact_match`/`contains` compare gold answer to concatenated context — not QA accuracy. **Fix:** rename `context_exact_match`/`context_contains`; documented as evidence metrics (judge covers answer quality; no generation phase this arc). |
| F12 | MINOR | C10 | Split rules correct but untested. **Fix:** parity pinning tests — known question_ids → dev/holdout; LoCoMo even index → dev. |
| F13 | MINOR | C11 | Auto-approve single-shot; failure silently drops the memory from gold. **Fix:** bounded retry (2) + assert final `approved` status; typed give-up counted in the report. |
| F14 | MINOR | L10 | Artifacts lack schema version / dataset identity. **Fix:** `schema_version`, dataset file sha256s, split config, judge identity in every artifact. |
| F15 | MAJOR | C9+L9 | Tests cover helpers, not the load-bearing contracts. **Fix:** tests for F1/F5 gold mapping, F2/F10 metric formulas incl. dup-collapse, F4 lane fields (fixture: search empty + startup populated), F6 accounting, F7 timeout (sleeping fake judge), F3 isolation seam. |
| F16 | MAJOR (product) | K (docs/issues.md) | `memoryd search` FtsOnly degraded path is strict-AND `query_chunks` — one non-matching term zeroes results (root cause of the e2e zero recall). **Fix:** route the degraded path through the same two-stage strict→relaxed-OR fallback the hybrid BM25 lane uses (`query.rs:649-680`); pinning test: multi-term query with one alien term still returns hits in FTS-only mode. Files: `crates/memoryd/src/handlers/memory_ops.rs` (+ export the helper from memory-substrate if needed). |

## Rejected

| Sev | Source | Finding | Reason |
| --- | --- | --- | --- |
| NIT | C13 | Feature-gate `sha2`/`hex` | sha256 split computation is load-bearing for the benchmark module; feature-plumbing churn buys nothing this arc. |
| — | L2 (partial) | Include startup context in judge inputs | Startup recall is not query-conditioned; stuffing it into `answer_basis` would grade the wrong lane. Superseded by F4's separate startup-coverage score. |

Round 2: scoped re-review of the fix diff (Cursor + Luna) until dry.

## Round 2 — Cursor (cursor-2) + Luna (codex-2), W0-worktree scope, on fix r1 + coordinator test rewrites

Both FINDINGS; both explicitly validated the four coordinator test rewrites (candidate-fence contract + relaxed-fallback pin) and found no other caller depending on the leak. Coordinator full gates green pre-review (103 memoryd suites + memorum-eval). 8/8 merged findings accepted:

| # | Sev | Source | Finding → fix contract |
| --- | --- | --- | --- |
| G1 | MAJOR (insuff. F7) | C+L | Judge timeout starts AFTER the synchronous stdin `write_all` — a child that never reads stdin blocks forever. **Fix:** deadline covers spawn + stdin write (bounded/non-blocking write); on expiry always kill+reap and drain pipes; typed Timeout. Test: judge that sleeps without reading stdin. |
| G2 | MAJOR | C+L | Scaffold teardown leaks: Drop never unlinks `/tmp/memd-eval-<pid>/…sock` (debris across per-question scaffolds), and a readiness-poll failure drops the raw child unkilled (daemon leak). **Fix:** wrap child pre-poll (kill+reap on failure); Drop removes socket file + pid dir when empty. Test: teardown leaves no socket path; readiness-failure kills child. |
| G3 | MAJOR (insuff. F9) | C+L | Cleaned loading still `fs::read`s + deserializes the whole 277MB then trims — peak RSS = file size, and all selected haystacks retained for the whole run. **Fix:** `from_reader` + custom array visitor that deserializes elements incrementally and drops non-selected items immediately; retain only the current question's haystack during scoring. Test: loader shape test asserting non-selected items dropped during parse (structure-level). |
| G4 | MINOR (insuff. F1 edge) | C+L | Missing `dia_id` falls back to `Dsession_1:{n}` which can never match evidence `D1:1` → silent empty gold. **Fix:** derive `D{numeric}:{n}` from `session_N`; record unmatched evidence ids on the item as a typed field (never silent). Test: fallback + unmatched-evidence fixtures. |
| G5 | MINOR (insuff. F5 edge) | L | Missing/mismatched `answer_session_ids` silently yields empty gold and a false zero recall. **Fix:** typed dataset-shape error recorded on the item; item excluded from means with an exclusion count in the artifact. Test: fixture with a dangling session id. |
| G6 | MAJOR (insuff. F15) | C+L | Several new tests are compile-presence-only (`let _ = field`), range-only, or single-chunk. **Fix:** hand-computed Hit@10/Recall@10 vectors; multi-chunk collapse fixture at the harness seam; artifact JSON round-trip asserting concrete values. |
| G7 | MINOR | C | FTS-only score shape `1.0/rank` diverges from the fused lane's RRF `1/(k+rank)`. **Fix:** reuse `reciprocal_rank_score(DEFAULT_VECTOR_RECALL_RRF_K, rank)` for parity. |
| G8 | NIT | C | LoCoMo multi-QA items clone conversation-wide dispositions onto every question. **Fix:** attach dispositions once per conversation (or per-item delta), cheap. |

Round 3: scoped re-review of fix diff 2 — MUST be dry (cap). W1's lesson applied: contracts above name the error paths and edge variants explicitly.

## Round 3 — Cursor (cursor-3) + Luna (codex-3), W0-worktree scope, on fix commit `6721ff1` (CAP ROUND)

Both FINDINGS — cap hit, but unlike W1 the residuals sit entirely in eval-harness code (zero production blast radius), so the coordinator fixed them inline rather than halting the wave. All three accepted:

| # | Sev | Source | Finding → coordinator fix |
| --- | --- | --- | --- |
| H1 | HIGH (insuff. G1) | C+L (converged; Cursor reproduced the OS mechanism locally) | Timeout path kills only the direct child then **unconditionally joins** the three pipe-drain threads — a judge wrapper leaving a grandchild holding inherited stdio (`sh -c 'sleep 600 & wait'`, any forking supervisor — our real judge is a delegate CLI call) blocks the joins until the grandchild exits; this stall mode was *introduced* by the round-2 drain/join design. **Fix:** spawn judge with `process_group(0)`; on deadline `kill(-pgid, SIGKILL)` + reap; pipe results delivered over `mpsc` channels with `recv_timeout` so no join is ever unbounded (an escaped-group descendant detaches instead of stalling). Test: `external_command_judge_timeout_not_extended_by_grandchild_holding_pipes` (`sleep 5 & wait`, 100ms budget, asserts <3s wall). |
| H2 | HIGH | L (Cursor read the same code and called it safe; coordinator adjudicated FOR Luna) | Concurrent scaffolds share `/tmp/memd-eval-<pid>`: A's Drop `remove_dir` can land inside B's daemon prepare→bind window (`prepare_socket_parent` recreates a missing dir, but the window between recreate and bind remains) → readiness flake. **Fix:** per-scaffold parent dir `<prefix>-<pid>-<seq>`; Drop cleanup safe by construction. Test: `concurrent_scaffold_socket_parents_are_distinct`. |
| H3 | MAJOR (insuff. G6) | C+L | The hand-computed metric fixture has one relevant item at rank 1 → structurally cannot catch rank-cutoff, partial-recall, averaging, or exclusion-denominator bugs. **Fix:** extracted pure `rank_metrics()` helper; tests `rank_metrics_hand_computed_rank_cutoff_and_partial_recall` (relevant at ranks 2/11 of 12, gold 3 → Hit 1.0, MRR 0.5, Recall 1/3, exact nDCG; past-cut and empty-gold variants) and `finish_metrics_hand_computed_means_exclude_item_errors` (excluded item carries perfect scores that must not leak into means). |

Clean areas (both reviewers): G2 kill-on-drop ordering, G3 visitor memory shape + haystack clear, G7 RRF parity/monotonicity, G4/G5 machinery. G5 dedicated-test waiver stands (recorded in the r3 prompt).

Cap disposition: coordinator inline fix (this section) + full crate gate + one scoped cross-family verify of the coordinator diff. No 4th delegate fix round.

## Scoped verify of the coordinator fix (cursor-4)

Verdict FINDINGS, but all core H1/H2/H3 claims verified (incl. independent nDCG arithmetic and behavior-preservation of `rank_metrics`). Three refinements, all accepted and applied by the coordinator:

| # | Sev | Finding → fix |
| --- | --- | --- |
| V1 | MEDIUM | Sequential `recv_pipe(remaining)` calls stack to ~2× the configured timeout worst-case. **Fix:** one shared absolute `Instant` deadline for both drains. |
| V2 | MEDIUM | Post-reap `kill(-pid)` in the recv-timeout path can hit a recycled pgid when every group member exited (setsid'd descendants). **Fix:** group kill only on the pre-reap wait-loop path; post-reap drain expiry detaches readers with no kill (in-group straggler leaks for its natural lifetime — acceptable in an eval harness). |
| V3 | LOW | `short_socket_path` doc lead-in still said `/tmp/<prefix>-<pid>/`. **Fix:** comment synced. |

Refinements implement cursor-4's own fix directions verbatim; loop closed on the coordinator's re-read + full crate gate (clippy `-D warnings` + full `cargo test -p memorum-eval`) rather than a further delegate round. W0 review loop DRY at this point.
