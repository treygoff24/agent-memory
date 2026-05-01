# Stream F Gate A Clean-Code Verification

## Verdict

FAIL — one S2 clean-code/correctness finding remains.

## Intended outcome

Gate A is verifying that Stream F's additive tree-layout changes keep Stream A as the only canonical substrate/index while adding noncanonical Stream F paths and managed merge-driver rules. The focused fix under review should reconcile Stream A/F-managed `.gitattributes` entries without deleting non-conflicting user-authored global `*` rules.

## Executive summary

The requested Cargo gates all pass, and the implementation preserves custom global rules when they are on their own `*` line. However, it still deletes custom user attributes when they share a global `*` line with managed `text`/`eol` attributes. That is a destructive config rewrite during bootstrap/open and means Gate A should not be marked PASS yet.

## Findings

[S2] [Correctness] Global `*` reconciliation still drops co-located custom user attributes

- Evidence: `crates/memory-substrate/src/tree/layout.rs:124-145` classifies any `*` line containing a managed `text` or `eol` attribute as fully managed. `reconcile_gitattributes` then removes that whole line before appending the canonical body. A focused check with an existing `.gitattributes` of `* text=auto eol=crlf -diff` produced output that retained only the appended managed `* text eol=lf` line and dropped the user-owned `-diff` attribute. The committed regression test covers `* -diff` on a separate line, but not the common combined-line form.
- Why it matters: Opening or bootstrapping an existing repo can silently delete non-conflicting user `.gitattributes` behavior. That violates the stated Gate A goal of preserving custom global user rules while reconciling managed Stream A/F rules, and it can change diff/merge/filter behavior outside the memory system's owned rules.
- Reasoning: `.gitattributes` attributes are additive per line, and users commonly combine multiple attributes for the same pattern. The current predicate operates at line granularity: once it sees managed `text`/`eol` on pattern `*`, it treats the entire line as replaceable. The fix needs attribute-level reconciliation for `*` lines, preserving unmanaged attributes such as `-diff`, `filter=...`, `working-tree-encoding=...`, etc., while replacing only managed `text`/`eol` settings.
- Recommendation: For global `*` lines, strip only managed `text`/`eol` attributes and preserve the remaining attributes as a user-owned `* ...` line when any remain. Add a regression test for a combined line such as `* text=auto eol=crlf -diff` and assert `* -diff` remains after reconciliation plus idempotent re-open.
- Confidence: High

## Non-blocking simplifications

- Consider extracting `.gitattributes` reconciliation into a small line/attribute transformer with explicit tests. The current private helpers are readable, but attribute-level reconciliation will be easier to reason about if the parse/keep/drop behavior is isolated.

## Test gaps

- Missing regression coverage for combined global `*` lines containing both managed attributes and custom attributes, e.g. `* text=auto eol=crlf -diff`.
- Existing tests verify separate custom global lines and managed Stream F merge rules, but they would not catch destructive deletion of custom attributes on a partially managed line.

## Verification run

```bash
cargo test -p memory-substrate --test dream_canonical_isolation
# PASS: 7 passed

cargo test -p memory-substrate --test tree_validation
# PASS: 7 passed

cargo test -p memory-substrate --test dream_merge_rules
# PASS: 4 passed

cargo fmt --all -- --check
# PASS

cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
# PASS
```

Additional focused reviewer check:

```text
input .gitattributes:
# user
* text=auto eol=crlf -diff

observed after bootstrap_repo_tree:
# user
* text eol=lf
*.md merge=memory-merge-driver
...
```

The user-owned `-diff` attribute was dropped.

## Questions / uncertainties

- I did not review unrelated Stream F implementation files beyond the Gate A-focused tree layout, canonical isolation, merge-rule tests, and requested gates.

## Positives

- The managed Stream F merge-driver rules are now generated and covered by `git check-attr` tests.
- The reconciliation path is idempotent for the cases currently covered by tests.
- The canonical-memory walker exclusion for `dreams/journal/**.md` aligns with the Stream F noncanonical-file contract.
