# Findings triage — W1 import-dedup fix, round 1

Diff under review: Terra's uncommitted worktree diff (codex-76, `delegate/codex-20260710T220306Z_48771d`). Reviewers: coordinator read, Cursor safe (cursor-1), Luna high safe (pending merge below). Coordinator gate on the worktree: `cargo test -p memoryd -- --test-threads=2` → **3 failures** (Terra fixed fixture compile errors but never re-ran): `execute_candidate_with_supersede_next_action_issues_followup_supersede`, `execute_promoted_with_existing_id_counts_as_dedup_not_new`, `second_run_with_unchanged_content_but_wrong_project_bucket_repairs_bucket`.

Verdict: **NEEDS-REWORK** (unanimous). The identity scaffolding is sound; the live root cause is not fixed and the new keying model breaks four adjacent paths.

## Accepted findings (fix round 1 scope)

| # | Sev | Finding | Source | Fix contract |
| --- | --- | --- | --- | --- |
| F1 | BLOCKER | Codex ordinal survives in identity tuple — live bug unfixed. `import_identity` embeds `section` verbatim (`task-group-45-…`) | coordinator + cursor | Strip the `task-group-\d+-` prefix from the section component at identity computation (keep `source_key` verbatim for display). Collision policy: two sections in the same file whose ordinal-free slugs collide → disambiguate by content-hash suffix at parse time and surface both as `ReportAmbiguous` when historical records match ambiguously. Fixture: renumber `task-group-N` between two runs → stable memory_id count |
| F2 | BLOCKER | v1-record migration never happens on `SkipUnchanged` (comment claims it does); ordinal orphans never pruned for anchor-less records | coordinator + cursor | On `SkipUnchanged` with a matched record, rewrite it under `source_identity` and remove the old key (state save is already per-record atomic). Prune superseded/stale keys by identity, not only by `source_memory_id` |
| F3 | BLOCKER | Re-import over the already-duplicated live corpus never reaches `ReportAmbiguous` (legacy records have empty identity fields; only exact source_key compat matches) | cursor | After F1+F2: also match legacy records by ordinal-free identity recomputed from their stored `source_key`; multiple hits → `ReportAmbiguous`. This is what makes report-only real for the live repair pass |
| F4 | MAJOR | Supersession-chain lookup uses `state.imports.get(&source_key)` — misses rekeyed records; W3 lineage silently lost | cursor | Resolve the prior record via the same plan-time match (identity/anchor/compat), thread it through `PlannedWrite` instead of re-looking-up by key |
| F5 | MAJOR | `retain` can leave two map entries for one `memory_id` (daemon-dedup path) and keeps stale ordinal keys forever | cursor | Retain/replace by `source_identity` + matched-prior-key + `memory_id`; explicit prune of the matched record's old key |
| F6 | MAJOR | `alias_to_id` seeded from map keys, which are now identities (`tuple:…`) not source keys — wiki-link resolution degrades post-migration | cursor | Seed from `record.source_key` (and stored aliases), never the map key |
| F7 | MAJOR | Identity's profile component is weak on two axes: anchor matches ignore harness/profile entirely (copied frontmatter ids collapse distinct sources), and the tuple's profile component is a bare directory basename (`/a/.claude/…` vs `/b/.claude/…` collide; symlinked profiles diverge) | cursor + luna | Constrain anchor matches to same harness; derive the tuple's profile component from the canonicalized (symlink-resolved) profile root path, not its basename; tests for multiple explicit roots + symlinked roots |
| F8 | MAJOR | Plan §W1 edge-case + gate tests missing: rename-only pipeline, content+rename, profile-symlink same-file, identical relative paths across profiles, path reuse, double re-import stable-id count, ordinal renumber | coordinator + cursor | Implement all as pipeline-level tests (unit identity tests alone don't satisfy the gate) |
| F9 | MAJOR | 3 failing tests in the worktree (see header) — fixture changes never validated | coordinator | Make the full `cargo test -p memoryd` suite green |
| F10 | MAJOR | Supersede retry can mint multiple replacements: daemon commits the replacement, crash/failure before state save → next run re-supersedes the old id and the substrate creates a fresh replacement per request | luna | Bounded fix (no substrate change): before issuing a Supersede, read the prior memory's `superseded_by` chain from the daemon; if it is already superseded by a memory whose content hash matches the candidate, adopt that id into state instead of writing again. Simulated-crash test (state save suppressed between runs), not true crash injection |
| F11 | MINOR | Stale doc comment: state.rs still says map keyed by portable `source_key` | cursor | Update comments to the identity-keyed model |
| F12 | NIT | ReportAmbiguous absent from harness counters; tuple uses `:` delimiter; `.codex` profile detection is exact-match while `.claude*` is prefix | cursor | Counter + `.codex*` prefix; delimiter left as-is (paths with `:` are already excluded by tree constraints) |

## Rejected / deferred

- Generic "crash between Promoted and save_atomic → stale state" (cursor MAJOR, luna overlap) — the *general* crash-journaling problem is pre-existing and out of W1 scope; the bounded supersede-specific consequence is accepted as F10. DEFERRED beyond F10's mitigation.

Luna round-1 (codex-1, 1 BLOCKER + 4 MAJOR + 2 MINOR, NEEDS-REWORK) merged above: convergent on F1/F2/F3/F4; unique adds folded into F7 (profile-root identity) and F10 (supersede retry duplication).

## Round 2 — Cursor (cursor-2) + Luna (codex-2) on the fix diff `d85f104..0ffde7c`

Coordinator gate re-run GREEN (/tmp/w1-gate-r1.log, 102 suites, 0 failures) — again green while defects remain: the F10 mocks return untruncated bodies and full chains the real daemon never produces. Coordinator read independently found F17 (fail-open) and the truncation half of F14 before reports landed; GET_BODY_MAX=4096 and one-hop chain verified on disk. Verdicts: both FINDINGS. 7/7 accepted:

| # | Sev | Source | Finding → fix contract |
| --- | --- | --- | --- |
| F13 | BLOCKER | L1+C3+C5 | Collision disambiguation not unique (identical content or shared 8-hex prefix → same identity) and suffix-toggle breaks matching (delete sibling → suffix drops; renumber+edit → WriteNew duplicate). **Fix:** identical-content same-slug candidates collapse to ONE plan action (they are the same memory); differing-content collisions get a deterministic unique per-file disambiguator; record matching tries ordinal-free identity with AND without suffix; multi-hit → ReportAmbiguous — never WriteNew solely from a suffix change. Tests: collision set → delete sibling → renumber+edit → Supersede; identical-content collision collapses. |
| F14 | MAJOR | C1 | F10 adoption blind to `GET_BODY_MAX=4096` truncation — any replacement body ≥4KiB skips adoption and mints a duplicate; mocks return full bodies so tests can't see it. **Fix:** additive optional `full_body: bool` on the get request (server honors for import adoption; hash needs candidate frontmatter_hint + full replacement body, so server-side hashing can't substitute); `truncated: true` after that = unknown → fail closed per F17. Test with a >4KiB body through the real handler bound. |
| F15 | MAJOR | C2+L2 | "Chain" is one hop — `get_superseded_by_chain` maps only immediate `superseded_by` links; A→B→C with state on A misses C, mints duplicate; mock fakes a full walk. **Fix:** client-side transitive walk with cycle+depth guard (bound ~16); mock aligned to one-hop-per-call; multi-hop + cycle tests. |
| F16 | MAJOR | L3 | Adoption can select a tombstoned/non-servable replacement (get response carries no lifecycle status) → state points at a dead memory, future runs skip as unchanged. **Fix:** additive status field on the get response; adoption rejects non-`{active,pinned}` replacements. Test: tombstoned replacement in chain → not adopted. |
| F17 | MAJOR | C4+L4+K | F10 fail-open: `if let Ok(chain)` + `_ => continue` swallow daemon errors, then supersede anyway — recreating the duplicate-mint race under transient failure. **Fix:** fail closed — propagate the error via `partial_import_error` for that action; supersede only when the chain was read successfully and had no content match. Test: erroring mock → action errors, no supersede issued. |
| F18 | MINOR | L5 | Alias seeding inserts the stable `tuple:` identity key into the wiki-link alias map, contrary to F6. **Fix:** seed only `record.source_key` + persisted aliases. |
| F19 | NIT | C6 | `prior_record_key` threaded on Supersede/RepairBucket but unused. **Fix:** use it for explicit legacy-key removal or drop it from those variants. |

Round 3: scoped re-review of fix diff 2 — MUST be dry (3-round cap; a non-dry round 3 is a coordinator escalation to Trey, not a round 4).

## Round 3 — Cursor (cursor-3) + Luna (codex-3) on fix commit `1a6481c` — CAP HIT

Round 3 was the must-be-dry final round. It was not dry: 4 new accepted findings, all in the round-2 fix layer, 2 coordinator-verified on disk before triage. **The 3-round cap is hit; the W1 cycle is HALTED pending Trey's decision.** No further delegate fix rounds without authorization.

| # | Sev | Source | Finding (verified?) |
| --- | --- | --- | --- |
| F20 | HIGH | C (r3) | F17's fail-closed scope too broad: a dangling `superseded_by` link (a state `trust_artifact.rs` already models as "unavailable") makes the chain walk error on hop-1 → whole supersede path aborts on EVERY retry → permanent import livelock for that memory. Mock treats missing nodes as empty children (`unwrap_or_default`), production errors — tests structurally green. Fix direction: typed-error discrimination — NotFound/tombstoned chain node = leaf (skip, keep walking); transport/protocol error = fail closed; fail-closed scope = "don't adopt", never "don't supersede" when the unreadable node is provably gone. |
| F21 | HIGH | L (r3) | Namespace fallback cross-project mismatch — **coordinator-verified** at `plan.rs:150,236-242`: legacy-record identity recomputation uses the CANDIDATE's `canonical_namespace_id`, not `record.canonical_namespace_id` (added in round 1, unused here). A project-A record can match a project-B candidate → wrong-memory supersede. |
| F22 | HIGH | L (r3) + C residual | BFS bound semantics: `chain.len() >= 16` bounds collected nodes, checked pre-hop — a single wide fan-out can exceed it, and exactly-16 first-level nodes stop traversal to deeper replacements → missed adoption. Fix: bound traversal depth and cap frontier explicitly. |
| F23 | HIGH | L (r3) | Encrypted replacements unrecoverable: `full_body` still returns the `[encrypted content omitted]` sentinel → hash mismatch → duplicate supersede for encrypted-tier memories — the truncation defect's sibling, one tier over. Fix: adoption for encrypted replacements must compare via a daemon-computed hash of plaintext (or skip adoption + fail closed), never the redacted body. |

Non-blocking round-3 confirmations: F14 additive + correctly scoped; F13 over-match resolves to ReportAmbiguous; walk duplication acceptable IF error semantics are aligned (they are not — F20).

**Escalation state:** wave is otherwise strong — gate green (102 suites), root cause (ordinal renumbering) fixed and pinned, 19 prior findings landed and verified. The residual 4 are all in the crash-recovery adoption path added by rounds 1–2. Options for Trey: (a) coordinator-owned inline fix pass for F20–F23 + pinning tests + own gate + ONE scoped verify read (recommended — the fixes are small, well-understood, and the fix-lane loop is demonstrably generating new defects at this depth); (b) authorized 4th Devin round; (c) park W1 unmerged, integrate W0 alone, revisit W1 next session.

## Round 4 — coordinator inline fix (Trey authorized option (a), 2026-07-10 AskUserQuestion; BUILD-STATE approval record #7)

Fix commit `cf30e96` on the W1 worktree branch, coordinator-authored:

- **F20** — typed discrimination in `SocketDaemonClient`: TrustArtifact/Get daemon error `not_found` = provably-gone leaf (walk continues; `get_memory` returns `Ok(None)`); all other daemon errors fail closed. Fail-closed scope narrowed per the finding: a gone node never blocks the supersede.
- **F21** — `record_identity_matches` recomputes with `record.canonical_namespace_id` (candidate-side fallback kept only for legacy records without the persisted field).
- **F22** — new `ChainWalker` (depth bound 16, explicit 256-node backstop) shared by production client AND test mock, so bound semantics cannot drift; the pre-hop collected-count check is gone.
- **F23** — additive `GetResponse.encrypted` flag (set from `MemoryContent::Ciphertext` in the get handler); adoption fails closed on an encrypted replacement with an actionable error instead of hashing the redaction sentinel. Check order status→encrypted→truncated so non-servable encrypted intermediates are skipped as non-candidates (minimal fail-closed surface).

Pinning tests (all six confirmed in the gate log `/tmp/w1-gate-r4.log`): `execute_supersede_dangling_chain_link_does_not_livelock`, `execute_supersede_chain_transport_error_fails_closed`, `execute_supersede_encrypted_replacement_fails_closed`, `chain_walk_is_depth_bounded_not_sibling_bounded`, `chain_walk_bounds_depth_and_total_nodes`, `record_identity_recompute_uses_record_namespace_not_candidates`. Gate: `cargo clippy -p memoryd -D warnings` exit 0; `cargo test -p memoryd -- --test-threads=2` 1115 passed / 0 failed.

Scoped verify: Cursor safe (cursor-4, W1-worktree registry) on the `cf30e96` diff only — hunt areas: not_found completeness (tombstoned mapping), fail-closed scope leaks, ChainWalker off-by-ones/mock divergence, F21 legacy-fallback against real live-corpus record shapes, F23 mixed-version skew (`encrypted` defaults false from an old daemon), test structural-greenness.

### Verify results (cursor-4) — 2 accepted findings, both fixed in `d28b677`

| # | Sev | Finding | Disposition |
| --- | --- | --- | --- |
| V-HIGH | HIGH | F23 mixed-version reopen: old daemon emits no `encrypted` field → serde default `false` → sentinel body hashed → duplicate supersede. (The coordinator had independently flagged this pre-review — convergent.) | FIXED: shared `ENCRYPTED_BODY_SENTINEL` wire constant in protocol.rs; adoption fails closed on flag OR sentinel body; pinned by `execute_supersede_sentinel_body_fails_closed_without_encrypted_flag` |
| V-MAJOR | MAJOR | F20 over-acceptance: `trust_artifact.rs` mapped a corrupt `memory_supersession` row id to `MemoryNotFound(parent)` → protocol `not_found` → leaf-skip treats the LIVE parent as gone → incomplete chain → duplicate supersede. Coordinator-verified on disk at `trust_artifact.rs:479-480` before triage. | FIXED: new `TrustArtifactError::CorruptSupersessionRow` maps to fail-closed `trust_artifact_error`; pinned by `corrupt_supersession_row_is_not_not_found` |
| V-NIT | NIT | 256-node backstop can abandon deeper adoptable replacements under pathological fan-out | ACCEPTED AS-IS: explicit documented runaway backstop; a >256-node supersession graph is already pathological |
| V-NIT | NIT | transport-error pinning test would also pass pre-fix | ACCEPTED AS-IS: it pins the pair-wise contract (skip leg + fail-closed leg together); solo pre-fix redness is not required of a pairing test |

Verified-OK areas: F20 scope (no swallowed transport errors; tombstoned Get still succeeds with status and is skipped by the status check — no livelock vector), ChainWalker depth/backstop/cycle semantics + mock parity, F21 including live-corpus record shapes (post-6/12 records persist `canonical_namespace_id`; user-scope None + "me" candidate id has no cross-project vector), remaining four pinning tests genuinely pin.

**W1 DRY at this point.** The verify round was the ONE scoped round Trey authorized; its two findings were coordinator-fixed (`d28b677`, 6 files, +132/−4), self-verified on disk, both pinned, and the full gate re-ran green (`cargo test -p memoryd --no-fail-fast`: 1117 passed / 0 failed; clippy -D warnings exit 0). Per the W0 precedent, verify-finding fixes applied by the coordinator with pinning tests and a green gate close the loop without an additional delegate round — the V-fix diff is small (an error-variant mapping + a one-line boolean guard + two tests) and was read line-by-line at commit time. Interim-run note: one `scoring` p95 perf test flaked while a sibling worktree compiled concurrently; it passed in isolation (2.28s) and in the final full run — environmental, same class as the known bench-corpus flake.
