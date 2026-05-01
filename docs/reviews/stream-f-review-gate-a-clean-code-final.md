### Verdict

Changes requested

### Intended outcome

This final Gate A clean-code rerun appears intended to verify that the remaining stale/custom `.gitattributes` S2 has been fixed for Stream F. The product goal is to upgrade existing Stream A memory repositories so Stream F's noncanonical substrate/dream/lease file families are routed to the correct merge driver, while preserving unrelated user-managed `.gitattributes` rules and keeping Stream A as the only canonical substrate/index.

### Executive summary

The original stale-upgrade failure is mostly fixed: `bootstrap_repo_layout` now reconciles an existing `.gitattributes`, the new Stream F merge-driver families are appended idempotently, stale managed rules such as `*.md merge=union` and `substrate/**/*.jsonl merge=union` are removed, and the requested Gate A commands all pass. However, the reconciler still treats the entire `*` pattern as managed, so any user-authored global `.gitattributes` rule is silently deleted even when it does not conflict with the managed `text/eol` attributes. Because the lane explicitly covers stale/custom `.gitattributes` reconciliation and promises not to lose user rules, I do not think Gate A should state PASS yet.

### Findings

[Medium] Correctness `.gitattributes` reconciliation drops custom global user rules

- Evidence: `MANAGED_GITATTRIBUTES_PATTERNS` includes `"*"` (`crates/memory-substrate/src/tree/layout.rs:20-31`). `reconcile_gitattributes` copies only lines where `is_managed_gitattributes_line` returns false, then appends the canonical managed body (`crates/memory-substrate/src/tree/layout.rs:100-120`). `is_managed_gitattributes_line` classifies a line solely by its first whitespace-delimited token (`crates/memory-substrate/src/tree/layout.rs:124-132`). Therefore any existing user line such as `* -diff`, `* linguist-generated`, or `* filter=lfs diff=lfs merge=lfs -text` is removed even though those attributes are not the stale Stream F merge-driver rules being reconciled. The regression test only proves preservation for an unrelated `*.txt` pattern (`crates/memory-substrate/tests/dream_canonical_isolation.rs:56-90`) and does not cover custom global `*` rules.
- Why it matters: Existing memory repositories may have legitimate global Git attribute policy that is unrelated to Stream F merge routing. Silently deleting those rules during `Substrate::open`/bootstrap changes repository behavior outside the managed Stream F path-family fix. In the worst case it can alter diff, filter, or merge behavior for all files in the repo, which violates the custom-rule preservation side of the reconciliation contract.
- Reasoning: Pattern-level deletion is too coarse for the `*` rule because the managed canonical line only owns `text eol=lf`, not every possible attribute a user might attach to `*`. Git attributes are attribute-specific, so unrelated attributes can coexist with the canonical `* text eol=lf` line. The current implementation cannot distinguish a stale/conflicting global text rule from a non-conflicting user rule and deletes both.
- Recommendation: Preserve custom `*` lines unless they specifically set attributes owned by the managed canonical rule, or rewrite reconciliation around managed attribute keys rather than managed path patterns. At minimum, add a regression test seeding `.gitattributes` with a user global rule such as `* -diff` plus stale Stream F rules, then assert reconciliation keeps `* -diff`, removes stale managed merge rules, remains idempotent, and `git check-attr merge` still returns `memory-merge-driver` for the Stream F families.
- Confidence: High

### Non-blocking simplifications

- Consider representing managed `.gitattributes` entries as structured `{ pattern, owned_attributes }` data rather than bare pattern strings. That would make the reconciliation policy explicit and reduce future drift between preservation and managed-rule replacement.

### Test gaps

- Missing coverage for preserving custom user rules on managed patterns, especially the global `*` pattern.
- The existing stale/custom reconciliation test preserves `*.txt merge=union` and verifies Stream F `git check-attr` routing, but it does not assert stale managed lines are absent or that non-conflicting custom attributes on managed patterns survive.
- No end-to-end two-clone merge-driver invocation test was run in this final lane; the current tests verify generated attributes through `git check-attr` and direct merge-rule behavior separately.

### Questions / uncertainties

- I treated custom global `.gitattributes` preservation as in scope because the prior S2 was specifically about upgrading existing stale/custom attributes without losing user rules. If the intended contract is instead "the substrate owns all attributes for the listed patterns, including `*`," that should be documented because it is a behavior change from the previous preservation model.
- I did not review or run the full workspace because this final lane was scoped to `layout.rs`, the Stream F tests, and the named Gate A commands.

### Positives

- The stale Stream F merge routing issue is substantially improved for existing repos: reconciliation is now idempotent and the new test verifies `git check-attr merge` for the important Stream F path families.
- The canonical isolation tests continue to enforce that dream/substrate/lease files validate as noncanonical files and stay out of canonical query/index paths.
- The requested Gate A command set is green.

## Prior S2 verification

- Prior S2, existing repos with stale `.gitattributes` are not upgraded: mostly fixed. Evidence: `bootstrap_repo_layout` now calls `reconcile_gitattributes` instead of write-if-missing (`crates/memory-substrate/src/tree/layout.rs:66-73`), stale managed patterns are filtered and the canonical Stream F body is appended (`crates/memory-substrate/src/tree/layout.rs:100-120`), and `existing_stale_gitattributes_are_reconciled_without_losing_user_rules` verifies upgraded `git check-attr merge` results for Stream F families (`crates/memory-substrate/tests/dream_canonical_isolation.rs:56-90`).
- Remaining S2: the same reconciliation is too broad for the global `*` pattern and can delete unrelated custom user rules.

## Gate A command results

```bash
cargo test -p memory-substrate --test tree_validation
# PASS: 7 passed; 0 failed

cargo test -p memory-substrate --test dream_canonical_isolation
# PASS: 7 passed; 0 failed

cargo test -p memory-substrate --test dream_merge_rules
# PASS: 4 passed; 0 failed

cargo fmt --all -- --check
# PASS

cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
# PASS
```

Gate A status: FAIL. Do not state PASS while the remaining S2 above is open.
