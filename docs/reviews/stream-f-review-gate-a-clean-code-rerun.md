### Verdict

Changes requested

### Intended outcome

This rerun appears intended to verify the scoped fixes for Stream F Gate A after the prior clean-code/Rust review found two S2 regressions: archived substrate paths were rejected by Stream A tree validation, and Stream F noncanonical file families were not routed to `memory-merge-driver`. The business/product goal is to keep Stream A as the single canonical substrate/index while safely adding Stream F's noncanonical dream/substrate paths, validators, and merge behavior without breaking startup, sync, or git convergence.

### Executive summary

The archive-path S2 is fixed: `substrate/archive/<device>/<YYYY-MM>.jsonl` now has an explicit validator branch, the archive test validates the resulting tree, and the Gate A tests pass. The generated `.gitattributes` body is also fixed for freshly bootstrapped repos and now has test coverage. However, the implementation still leaves already-initialized memory repositories with stale `.gitattributes` because bootstrap/open only writes `.gitattributes` if it is missing. That means existing Stream A repos upgraded to Stream F can still fail to route the new Stream F JSON/JSONL/JSON path families to `memory-merge-driver`, so Gate A should not be marked PASS yet.

### Findings

[Medium] Correctness Existing repos with stale `.gitattributes` are not upgraded to Stream F merge rules

- Evidence: `GITATTRIBUTES_BODY` now contains the required Stream F routes for new bootstrap output (`crates/memory-substrate/src/tree/layout.rs:8-17`), and the new generated-attributes test asserts those routes in a fresh temp repo (`crates/memory-substrate/tests/dream_canonical_isolation.rs:37-53`). But `bootstrap_repo_layout` calls `write_if_missing(&root.join(".gitattributes"), GITATTRIBUTES_BODY)` (`crates/memory-substrate/src/tree/layout.rs:54-59`), and `write_if_missing` returns without changing an existing file (`crates/memory-substrate/src/tree/layout.rs:79-83`). `Substrate::open_with_options` calls this same bootstrap path for existing repos (`crates/memory-substrate/src/api.rs:1144-1146`), so stale pre-Stream-F attributes are preserved. A manual check with an old-style `.gitattributes` leaves `substrate/archive/...` on `merge=union` and leaves `dreams/questions`, `dreams/cleanup`, and `leases/journal.lease` unspecified.
- Why it matters: Stream F is being added to already-existing Stream A repositories, not only brand-new ones. If an existing repo keeps the old attributes, git will not invoke the custom merge driver for the new noncanonical path families. Substrate archive JSONL can use union instead of canonical dedupe/sort, dream question/lease JSONL can textual-conflict or merge nondeterministically, and cleanup JSON will not use last-writer-wins. That is the same product failure the prior S2 was trying to eliminate for real sync behavior.
- Reasoning: Updating the generated template is necessary but not sufficient. The runtime path that opens/adopts an existing substrate repo intentionally avoids overwriting `.gitattributes`, so the fix only applies to repos where the file is absent or newly initialized after this change. Existing `.gitattributes` files with old rules remain stale indefinitely unless there is an explicit additive reconciliation/migration.
- Recommendation: Add an idempotent `.gitattributes` reconciliation step that preserves unrelated/user rules but ensures the canonical Stream F merge rules are present or corrected for the managed path families. Run it during init/open/adopt or during a documented repair/preflight path, and add a behavior test that seeds an old `.gitattributes`, opens/bootstraps the repo, then verifies `git check-attr merge` returns `memory-merge-driver` for `substrate/archive`, `encrypted/substrate`, `dreams/questions`, `dreams/cleanup`, `dreams/journal`, and `leases/journal.lease`.
- Confidence: High

### Non-blocking simplifications

- The Stream F path-family checks now appear in model validation, tree validation, merge dispatch, tests, and `.gitattributes`. A small shared path-family classifier would reduce drift risk as later Stream F tasks add more dream paths.

### Test gaps

- Missing regression test for upgrading an existing/stale `.gitattributes`. Current coverage proves fresh bootstrap output only.
- No end-to-end git merge/CLI test proves an existing upgraded repo invokes `memory-merge-driver` for a Stream F JSONL/JSON path after reconciliation.

### Questions / uncertainties

- I did not run the full workspace test suite because the prompt scoped this rerun to the Gate A command set.
- I treated existing Stream A repositories as in-scope because Stream F is an additive substrate upgrade and `Substrate::open_with_options` explicitly bootstraps existing repos. If the intended contract is "new repos only," this should be stated in the Stream F rollout notes, but that would be a risky product constraint.

### Positives

- The archive-path fix is well targeted: validation now handles `substrate/archive/` before the generic `substrate/` branch, and the archive test validates the tree after archival.
- Fresh bootstrap `.gitattributes` now covers the Stream F path families directly and has a focused `git check-attr` test.
- The requested Gate A commands are green, including fmt and clippy.

## Prior S2 verification

- Prior S2 #1, archive path rejected by validator: fixed for the implemented API path. Evidence: `validate_noncanonical_stream_f_file` now handles `substrate/archive/` with a year-month validator before generic substrate date validation (`crates/memory-substrate/src/tree/validate.rs:153-159`), and `archive_expired_plaintext_fragments_idempotently` validates the tree after writing `substrate/archive/dev_test/2026-04.jsonl`.
- Prior S2 #2, `.gitattributes` did not route Stream F files: partially fixed. Freshly generated `.gitattributes` now routes the Stream F families to `memory-merge-driver`, but existing stale `.gitattributes` files are not reconciled, leaving a remaining S2 for upgraded repos.

## Gate A command results

```bash
cargo test -p memory-substrate --test dream_canonical_isolation
# PASS: 6 passed; 0 failed

cargo test -p memoryd --test dream_canonical_isolation
# PASS: 3 passed; 0 failed

cargo test -p memory-substrate --test dream_substrate_primitives
# PASS: 5 passed; 0 failed

cargo test -p memory-substrate --test config_loading
# PASS: 10 passed; 0 failed

cargo test -p memory-substrate --test dream_merge_rules
# PASS: 4 passed; 0 failed

cargo fmt --all -- --check
# PASS

cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
# PASS
```

Gate A status: FAIL. Do not state PASS while the remaining S2 above is open.
