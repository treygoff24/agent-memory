# Review-Fix Decision Policy

When review finds a gap between implementation and the live spec, the default response is to fix the code to satisfy the spec. Amending the spec is allowed only when the spec was wrong, the implementation cost is clearly disproportionate to v1 value, or the work is explicitly deferred with tracking.

## Required decision record

Every review-fix PR that changes a contract must state one of:

1. **Code fix** — implementation now satisfies the existing spec. Include the test that would have failed before the fix.
2. **Spec correction** — the spec asserted an impossible or unsafe contract. Include the technical reason and update all API/dev docs that depended on the old text.
3. **Explicit deferral** — the feature is not v1. Include the user-visible behavior today, target version, owner surface, and a tracking issue/doc reference.

Do not silently move the spec to match shipped code. If the code remains partial, the user-visible surface must be honest: return a typed error, mark a benchmark as smoke-only, or report a skip/deferred status rather than implying full coverage.

## Bench and baseline decisions

Canonical benchmark files are release artifacts. Automation may create `.proposed` files and assert against existing canonicals, but canonical promotion requires an explicit human action (`--promote-canonical`) after reviewing the proposed output.

Synthetic benchmarks may stay in a canonical baseline only when their limitation is in the result file and in the relevant spec/API docs. If a synthetic number is not a meaningful regression floor for the shipped path, either add a real-path benchmark or mark/remove that metric from the canonical release gate.
