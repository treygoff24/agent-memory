# Dogfood-Readiness Desloppify Review

**Reviewer:** Claude (general-purpose subagent)
**Date:** 2026-05-05
**Scope:** Touched files in the dogfood-readiness fix-plan (Tasks 1–6C).
**Verdict:** ⚠️ risks-only

## Findings

1. **`crates/memorum-coordination/src/session.rs`:111–115** — The allowlist invariant comment sits on `is_observe_only_harness` rather than on `is_full_coordination_harness` or the `FULL_COORDINATION_HARNESSES` constant above it. A developer adding a new harness will navigate to `is_full_coordination_harness` or the constant definition — not to the negation wrapper — so the invariant is in the wrong place to catch future drift. _Suggested fix:_ Move the comment block to sit immediately above `FULL_COORDINATION_HARNESSES` on line 60 (or at minimum repeat a one-line cross-reference there); leave a brief doc-comment on `is_observe_only_harness` pointing at the constant.

2. **`crates/memoryd-tui/tests/panic_restore.rs`:52–61** — The test `inject_panic_flag_is_hidden_from_help` asserts `!stdout.contains("inject-panic")`. The string `"inject-panic"` is a prefix of both `--inject-panic` and `--inject-panic-mid-render`, so a single `contains` check catches both. That's fine, but the assertion is silent about the mid-render flag's visibility — and both flags are `#[cfg(debug_assertions)]`, which means the test only passes in debug builds (the binary compiled for test is always debug). If this test ever runs against a `--release` binary the flags won't be present and the test will vacuously pass. This is a minor test-contract gap: the test verifies what hides in debug builds, not what hides in release builds, and the comment does not document that. _Suggested fix:_ Add a brief comment noting the `cfg(debug_assertions)` scope of the flags so the next reader understands the test's bounds; or rename the test to `inject_panic_flags_are_hidden_from_help_in_debug_builds`.

3. **`crates/memoryd/tests/dream_cli.rs`:404–409** — `commit_all` is `#[cfg(feature = "dev-fixtures")]` gated, which is correct; however the helper's body runs three unconditional `command_in` calls with no error surface beyond panic. This matches the pattern of the surrounding helpers and is not new slop from this diff, but the diff introduced `commit_all` in its gated form specifically for Task 6B. Not a blocker — noting for completeness.

4. **`scripts/install-memorum.sh`:185–193** — The harness-CLI detection block uses three separate `if … fi` guards (check `claude`, check `codex`, check neither) with duplicated `command -v` calls. The neither-check at line 191 re-evaluates `command -v claude` and `command -v codex` that were already evaluated in lines 185–190. On any shell with `set -euo pipefail` and a slow PATH, this is a harmless double-probe, but the logic is duplicated. _Suggested fix:_ Assign boolean results to local variables before the block; no behavioral change needed, just removes the repeated probes.

## Notes

The implementation overall is clean for a 28-file fix-plan landing. The EchoCli compile-time gate (`#[cfg(any(test, feature = "dev-fixtures"))]`) is applied consistently across `harness.rs`, `run.rs`, `orchestration.rs`, and both test files. The installer's variable quoting is correct throughout: all `$repo`, `$runtime`, `$socket`, `$pid_file`, `$log_file`, and `$existing_pid` references are properly quoted in command positions. The `FULL_COORDINATION_HARNESSES` constant extraction and the session derivation tests for known/unknown harness names are well-structured. Finding 1 is the only one with real forward-risk; the rest are minor comment/test-scope clarity issues.
