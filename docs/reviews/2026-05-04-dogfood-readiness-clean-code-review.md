# Dogfood-Readiness Clean-Code Review

**Reviewer:** Claude (general-purpose subagent)
**Date:** 2026-05-05
**Scope:** `scripts/install-memorum.sh`, `crates/memoryd-tui/src/panels/recall.rs` (+ test), `crates/memoryd/src/handlers.rs` (doctor health), `crates/memoryd/src/main.rs` (doctor CLI exit), `crates/memorum-eval/tests/live.rs` (+ T15).
**Verdict:** ⚠️ risks-only

---

## Findings

1. **`scripts/install-memorum.sh`:121** — `stop_existing_daemon` calls `rm -f "$pid_file"` even in dry-run mode when the existing PID is dead (the early `return` on line 110 only fires for a live PID). A dry-run should be fully side-effect-free, but this path silently removes a state file. _Suggested fix:_ Guard the final `rm -f "$pid_file"` with `[ "$dry_run" -eq 0 ]` or check dry_run before calling `rm`.

2. **`scripts/install-memorum.sh`:114,134** — Both wait-loops use `for _ in 1 2 3 4 5` with `_` as the loop variable. In bash `_` is a special variable (last argument of the previous command); assigning it in a loop works but reads as unintentional. _Suggested fix:_ Use `for _i in 1 2 3 4 5` or `seq`-based while loop to signal intent clearly.

3. **`scripts/install-memorum.sh`:147** — There is a TOCTOU window between the readiness probe success and writing `$daemon_pid` to `$pid_file`: if the daemon exits in that interval, a dead PID is recorded. Low probability in practice but the PID write could precede the readiness loop with a dedicated re-check after write, or accept the gap with a comment. _Suggested fix:_ Add a `kill -0 "$daemon_pid" || { … handle orphan … }` check immediately before the write, or document the accepted gap.

4. **`crates/memorum-eval/tests/live.rs`:39-46** — `has_cli` returns `false` after printing `MEMORUM_EVAL_SKIP:SKIP_MISSING_CLI:…` to stderr. The `codex_smoke` caller then returns silently (no additional print, no non-zero exit). The outer test process exits 0 with no output, which the CI harness sees as a passing-but-quiet test rather than an explicit skip. The skip marker is on stderr but `run_domain_filter` is never called, so the SKIP-in-nested-cargo guard also never fires. _Suggested fix:_ After `has_cli` returns false, print a `MEMORUM_EVAL_SKIP:SKIP_MISSING_CLI:…` marker directly from the smoke test body so CI tooling that reads test stdout gets a consistent skip signal.

5. **`crates/memorum-eval/tests/live.rs`:75-79** — `extract_assertion_count` takes the first `MEMORUM_EVAL_ASSERTIONS=` line it finds. If the nested cargo run emits multiple such lines (e.g., from verbose harness setup before the test marker), the count could be wrong. The plan intent is to find the terminal count emitted by `eval_flush_assertion_count`. _Suggested fix:_ Use `rev()` to find the _last_ matching line rather than the first, matching `eval_flush_assertion_count`'s emit-at-end contract.

6. **`crates/memoryd/src/handlers.rs`:1445** — `doctor_is_healthy` is a private module-level function tested by an in-file `#[cfg(test)]` unit test. The function signature and test coverage are exactly what the plan required. No issue with correctness. However, the function name `doctor_is_healthy` is slightly ambiguous — it computes health from three independent booleans and its name does not hint at the harness-count semantics. _Suggested fix:_ No change required; a doc comment noting the zero-registry case (`enabled == 0` is trivially healthy) would future-proof the rule.

---

## Notes

T1 installer: the core lifecycle (nohup+disown, PID file, log path, version-skip, lifecycle stanza) is correctly implemented and matches the plan. Findings 1–3 are edge cases around dry-run fidelity and a low-probability race, not correctness failures under normal use. T3 recall panel: clean removal of all n/a placeholders; tests correctly assert absence rather than presence. T4 doctor health: `doctor_is_healthy` extraction and test coverage are solid; `doctor_cli_exit_code` correctly maps `healthy == false` to exit 1. T2 live eval: the lying-green blocker from the original review is fixed for the main path; findings 4–5 are residual gaps around the missing-CLI skip signal and assertion-count ordering.
