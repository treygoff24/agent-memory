# Findings triage — W2 spec ratification package, round 1

Reviewer: Luna high (codex-77, safe). Package at commit `f24ec23`; fixes applied as package r2. Verdict was NOT-RATIFIABLE; all nine findings **accepted** (coordinator spot-checked the `_extras` ours-wins claim against `field_rules.rs` and the `index_embeddings` default against §6.2 — both held).

| # | Sev | Finding | Disposition |
| --- | --- | --- | --- |
| 1 | BLOCKER | Cue casing-collision dedup is insertion-order-dependent → side-dependent | ACCEPTED — strict total order `(case_fold(NFC), NFC bytes)`; canonical casing = byte-smaller spelling; casing-collision fixtures required |
| 2 | BLOCKER | Abstraction `updated_at`-newer-wins undefined at equality (shipped `summary` rule shares the defect) | ACCEPTED — sha256(NFC(value)) tie-break; pre-existing `summary` hole logged in `docs/issues.md` as a separate follow-up, not fixed in this arc |
| 3 | MAJOR | Aux hash-refresh/invalidation unspecified (stale vectors servable until reconcile) | ACCEPTED — atomic hash-change invalidation rule + query/reconcile reject stale hashes + ordinal-shift cleanup |
| 4 | MAJOR | Tombstone/supersede "cascade" inaccurate; no status-lifecycle matrix | ACCEPTED — matrix added (edit/supersede/tombstone/quarantine/delete/reindex) |
| 5 | MAJOR | "Re-enqueue held-local" on sensitivity upgrade contradicts `index_embeddings=false` default | ACCEPTED — default-delete everything; override-only re-enqueue; lane switches never resurrect |
| 6 | MAJOR | Caps-before-classification pipeline not stated as universal across entrypoints | ACCEPTED — single fixed-order pipeline for all nine entrypoints; shipped `api/write.rs` ordering must be reconciled |
| 7 | MAJOR | Mixed-version wart wrong: `_extras` add/add is silent ours-wins, not quarantine | ACCEPTED — text corrected |
| 8 | MAJOR | Hash algorithms undefined; abstraction freshness conflated with embedded-content hash | ACCEPTED — canonical sha256 definitions + `source_body_hash` column |
| 9 | MINOR | SKILL.md cue-guidance update omitted from §C | ACCEPTED — added with acceptance signal |

Round 2: Cursor safe re-review of package r2 (different family; re-review until dry).
