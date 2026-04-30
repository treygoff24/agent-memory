# Stream E Passive Recall Handoff

**Repo:** `/Users/treygoff/Code/agent-memory`
**Date:** 2026-04-30
**Reason for handoff:** Trey is restarting the computer. Stop here; resume from the next uncompleted Stream E plan task.

## Current high-level state

- Autonomous-loop was installed for this repo with `autonomous-loop install-repo --repo "$PWD" --force --prefer-scripts check`.
- `autonomous-loop doctor --cwd "$PWD"` is green.
- Autonomous-loop session remains **active**:
  - `run_id/session_id`: `019ddf9d-f510-7851-a328-1318fb0d4497`
  - objective: `Implement Stream E passive recall from docs/plans/2026-04-30-stream-e-passive-recall.md through final gate, commit, and push`
  - trusted gate: `.codex/autoloop.project.json` -> `pnpm check` -> `bash scripts/check.sh`
- Branch state before shutdown: `main...origin/main [ahead 1]`.
- There is a broad dirty/untracked Stream E tree. **Do not reset or clean it.**

## Source contract

Normative plan/spec files:

- `docs/plans/2026-04-30-stream-e-passive-recall.md`
- `docs/specs/stream-e-passive-recall-v0.5.md`

Historical untracked specs are intentionally present and should not be overwritten unless Trey explicitly asks:

- `docs/specs/stream-e-passive-recall-v0.1.md`
- `docs/specs/stream-e-passive-recall-v0.2.md`
- `docs/specs/stream-e-passive-recall-v0.3.md`
- `docs/specs/stream-e-passive-recall-v0.4.md`
- `docs/specs/stream-e-passive-recall-v0.5.md`

## Completed plan work

### Task 1 — Contract map

Completed.

Created:

- `docs/reviews/stream-e-contract-map.md`

Verified v0.5 contract mapping and deltas:

- URL-form-agnostic git remote canonicalization.
- `RecallOmission.alias` / `colliding_ids` additive fields.
- `passive_recall` and `index_body` index columns.
- `handlers.rs` private `safe_plaintext_fragment` rename is owned later by Task 10.
- No hot-path `Substrate::doctor()` count.

### Tasks 2-3 — Stream A / Stream D foundations

Completed and Review Gate A passed, with one concrete P2 fixed.

Stream A changed:

- `crates/memory-substrate/src/model.rs`
- `crates/memory-substrate/src/error.rs`
- `crates/memory-substrate/src/index/schema.rs`
- `crates/memory-substrate/src/index/migrations.rs`
- `crates/memory-substrate/src/index/query.rs`
- `crates/memory-substrate/src/api.rs`
- `crates/memory-substrate/tests/memory_query_extension.rs`
- `docs/api/stream-a-public-api.md`

Added:

- `MemoryQuery.status`
- `MemoryQuery.namespace_prefix`
- `MemoryQuery.passive_recall_only`
- `MemoryQuery.updated_since`
- `RecallIndexQuery`
- `RecallIndexRow`
- `Substrate::query_recall_index`
- `SubstrateError::InvalidQuery`
- schema migration to v2 for `passive_recall` + `index_body`
- later discovered schema migration to v3 for `human_review_required` + `max_scope`

Important discovered blocker already fixed: Task 8 needed recall-index governance projection fields. Stream A now projects:

- `requires_user_confirmation`
- `review_state`
- `human_review_required`
- `max_scope`

Stream D changed:

- `crates/memory-privacy/src/decision.rs`
- `crates/memory-privacy/src/lib.rs`
- `crates/memory-privacy/tests/safe_plaintext_fragment.rs`
- `docs/api/stream-d-privacy-api.md`

Added:

- `SafeFragmentDecision`
- `safe_plaintext_fragment(classifier, fragment)`

Known note: Review Gate A found a P2 spec/docs ambiguity around final `PrivacyStorageAction::EncryptAtRest` vs no-span/URL/date `Allow`. We did **not** edit the spec. Current code/tests intentionally allow benign no-span/URL/date fragments while omitting private/account/secret fragments.

Review reports:

- `docs/reviews/stream-e-query-extension-review.md`
- `docs/reviews/stream-e-safe-fragment-security-review.md`

### Task 6 — Recall DTOs, errors, budgeting, rendering primitives

Completed.

Created/changed:

- `crates/memoryd/src/lib.rs`
- `crates/memoryd/src/recall/mod.rs`
- `crates/memoryd/src/recall/error.rs`
- `crates/memoryd/src/recall/types.rs`
- `crates/memoryd/src/recall/budget.rs`
- `crates/memoryd/src/recall/render.rs`
- `crates/memoryd/tests/startup_recall_determinism.rs`

Added DTOs/primitives:

- `StartupRequest`
- `StartupResponse`
- `SessionBinding`
- `ProjectBinding`
- `ProjectBindingSource::{YamlOverride, GitRemote}`
- `RecallExplanation`
- `RecallSectionExplanation`
- `RecallOmission` with additive `alias` and `colliding_ids`
- `OmissionReason`
- `RecallSectionName`
- `RecallError`
- exact `ceil(utf8_byte_len / 4)` token estimator
- UTF-8-safe truncation
- hand-rolled XML escaping/rendering

Corrected during review:

- `ProjectBinding.alias` is `Option<String>`.
- `ProjectBindingSource::YamlOverride` serializes as `yaml_override`.
- project-state rendering falls back to `canonical_id` when alias is absent.

### Task 7 — Session/project binding

Completed.

Created/changed:

- `crates/memoryd/Cargo.toml`
- `crates/memoryd/src/recall/binding.rs`
- `crates/memoryd/src/recall/project.rs`
- `crates/memoryd/src/recall/mod.rs`
- `crates/memoryd/tests/startup_recall_project_binding.rs`

Added:

- `validate_startup_request`
- absolute/canonical `cwd` validation
- trim/bounds validation for `session_id`, `harness`, `harness_version`
- `.memory-project.yaml` parsing with fail-closed duplicate/shape validation
- git remote fallback
- URL-form-agnostic git remote canonicalization
- namespace order `me`, `project:<id>`, `agent`

Manual orchestrator correction after subagent: changed default budget constant from `2_000` to `3_600` to match v0.5.

### Task 8 — Candidate collection, entity resolution, ranking

Completed after one blocker and one review-fix loop.

Initial Task 8 stopped because Stream A recall-index lacked governance fields. That was fixed by extending Stream A recall-index to schema v3, then focused rereview approved.

Created/changed:

- `crates/memoryd/src/recall/candidates.rs`
- `crates/memoryd/src/recall/entity.rs`
- `crates/memoryd/src/recall/rank.rs`
- `crates/memoryd/src/recall/mod.rs`
- `crates/memoryd/tests/startup_recall_governance.rs`
- `crates/memoryd/tests/startup_recall_ranking.rs`

Added:

- recall-index-row based candidate filtering
- `RecallIndexReader` trait
- `RecallCollectionRequest`
- `collect_recall_candidates_from_index`
- entity/alias matching and collision omissions
- deterministic ranking and budget selection

Review Gate B found issues in Task 8; all were fixed:

- P1 fixed: ambiguous alias collision no longer drops unrelated valid matches.
- P1 fixed: added Stream A-backed collection entrypoint and fake-backed test proving recall-index query usage/no envelope hydration path in enumeration.
- P2 fixed: governance/privacy filters now reject out-of-max-scope rows, `index_body=false`, confidential/personal rows from factual body recall.

Review report:

- `docs/reviews/stream-e-recall-core-correctness-review.md`

## Latest verification before handoff

The Gate B fix agent reported these passing commands:

```bash
cargo test -p memoryd --test startup_recall_governance       # PASS, 6 tests
cargo test -p memoryd --test startup_recall_ranking          # PASS, 8 tests
cargo test -p memoryd --test startup_recall_determinism      # PASS, 10 tests
cargo clippy -p memoryd --all-targets --all-features -- -D warnings  # PASS
cargo fmt --all -- --check                                  # PASS
git diff --check                                            # PASS
```

Immediately before handoff, I also ran:

```bash
autonomous-loop status --cwd "$PWD"
git status --short --branch
git status --porcelain=v1 --untracked-files=all
```

Status showed autonomous-loop active and repo dirty as expected.

## Next exact step after restart

Resume at **Task 10: Daemon Protocol, Handler Wiring, Status Counters** from `docs/plans/2026-04-30-stream-e-passive-recall.md`.

Before Task 10, do a quick local sanity gate because the computer restarted:

```bash
autonomous-loop status --cwd "$PWD"
autonomous-loop doctor --cwd "$PWD"
cargo test -p memoryd --test startup_recall_determinism
cargo test -p memoryd --test startup_recall_project_binding
cargo test -p memoryd --test startup_recall_governance
cargo test -p memoryd --test startup_recall_ranking
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

Then proceed with Task 10 exactly as written.

Task 10 critical reminders:

1. First rename private `crates/memoryd/src/handlers.rs` helper:
   - from `safe_plaintext_fragment(text: &str) -> bool`
   - to `is_safe_plaintext_for_indexing`
   - update all current call sites found by:
     ```bash
     rg -n "safe_plaintext_fragment" crates/memoryd/src/handlers.rs
     ```
2. Only after that import/use the public `memory_privacy::safe_plaintext_fragment` helper.
3. Add protocol startup request/response and status recall counters.
4. Do **not** call `doctor_response`, `Substrate::doctor()`, fsck, or full repair scan inside startup hot path.
5. `memory_startup` may return `not_implemented` only for non-null `since_event_id`.

## Remaining plan tasks

- Task 10: Daemon protocol, handler wiring, status counters.
- Task 11: MCP manifest, forwarding, CLI startup/delta commands.
- Task 12: Review Gate C protocol/MCP/CLI review.
- Task 14: Privacy/encrypted-memory/pending-attention acceptance.
- Task 13: Security/privacy review.
- Task 15: Full startup recall acceptance/output shape.
- Task 16: Performance probe/release gate fixture.
- Tasks 17-19: Review Gate D performance/API/security reviews.
- Task 20: API docs, README, CLAUDE updates.
- Task 21: final gate + performance gate + completion report.
- Commit and push for Claude review.

## Current dirty/untracked files at handoff

`git status --short --branch` at handoff:

```text
## main...origin/main [ahead 1]
 M CLAUDE.md
 M Cargo.lock
 M crates/memory-privacy/src/decision.rs
 M crates/memory-privacy/src/lib.rs
 M crates/memory-substrate/src/api.rs
 M crates/memory-substrate/src/bin/stream_a_bench.rs
 M crates/memory-substrate/src/error.rs
 M crates/memory-substrate/src/index/migrations.rs
 M crates/memory-substrate/src/index/query.rs
 M crates/memory-substrate/src/index/schema.rs
 M crates/memory-substrate/src/model.rs
 M crates/memory-substrate/tests/api_write_read.rs
 M crates/memory-substrate/tests/async_cancellation.rs
 M crates/memory-substrate/tests/reindex_reconciliation.rs
 M crates/memory-substrate/tests/startup_reconciliation.rs
 M crates/memoryd/Cargo.toml
 M crates/memoryd/src/lib.rs
 M crates/memoryd/tests/privacy_e2e.rs
 M docs/api/stream-a-public-api.md
 M docs/api/stream-d-privacy-api.md
?? .codex/autoloop.project.json
?? crates/memory-privacy/tests/safe_plaintext_fragment.rs
?? crates/memory-substrate/tests/memory_query_extension.rs
?? crates/memoryd/src/recall/
?? crates/memoryd/tests/startup_recall_determinism.rs
?? crates/memoryd/tests/startup_recall_governance.rs
?? crates/memoryd/tests/startup_recall_project_binding.rs
?? crates/memoryd/tests/startup_recall_ranking.rs
?? docs/plans/2026-04-30-stream-e-passive-recall.md
?? docs/reviews/stream-e-contract-map.md
?? docs/reviews/stream-e-query-extension-review.md
?? docs/reviews/stream-e-recall-core-correctness-review.md
?? docs/reviews/stream-e-safe-fragment-security-review.md
?? docs/specs/stream-e-passive-recall-v0.1.md
?? docs/specs/stream-e-passive-recall-v0.2.md
?? docs/specs/stream-e-passive-recall-v0.3.md
?? docs/specs/stream-e-passive-recall-v0.4.md
?? docs/specs/stream-e-passive-recall-v0.5.md
```

This handoff file itself is new and should be included in the eventual commit unless Trey says otherwise.

## Known caveats / watch items

- `CLAUDE.md` was already modified before this handoff path; do not overwrite blindly. Later Task 20 owns README/CLAUDE docs updates.
- `Cargo.lock` is modified because `memoryd` now directly uses existing workspace deps (`serde_yaml`, `sha2`, `hex`). Keep it unless a later gate proves it is wrong.
- Some recall modules are untracked because they are newly created. Do not assume `git diff` shows them; use `git status --untracked-files=all` or `rg`/`sed` to inspect.
- Autonomous-loop remains active. If restarting clears runtime session context, start by running `autonomous-loop status --cwd "$PWD"` and `autonomous-loop doctor --cwd "$PWD"`.
- Do not commit yet. The user’s original goal is commit + push only after the full Stream E plan is complete and final gates pass.
