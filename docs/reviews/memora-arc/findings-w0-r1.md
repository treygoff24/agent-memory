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
