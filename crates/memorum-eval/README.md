# memorum-eval

`memorum-eval` is the Stream H eval harness crate for Memorum. It owns the eval orchestrator binary, simulator-driven integration tests, real-harness test runners, and the regression-as-test directory.

## Common commands

```bash
# List the 19-test catalog.
cargo run -p memorum-eval -- --list

# Run the local mock gate and emit JSON.
cargo run -p memorum-eval -- --harness mock --output json

# Run one test by number or name.
cargo run -p memorum-eval -- --filter t01 --output text
cargo run -p memorum-eval -- --filter domain/t15 --harness mock

# Keep temp trees around while debugging.
cargo run -p memorum-eval -- --filter t14 --no-cleanup -v
```

The standard Task 20 validation commands are:

```bash
cargo test -p memorum-eval --test regression_meta
cargo test -p memorum-eval --test ci_workflow_shape --test orchestrator_integration
cargo fmt -p memorum-eval -- --check
```

## CLI output

Use `--output json` for automation. The report includes run metadata, pass/fail/skip counts, `partial`, missing credential names, and one result object per test with `number`, `name`, `group`, `mode`, `status`, assertion counts, `failure_detail`, and `skip_reason`.

Exit codes:

- `0`: all selected tests passed, or only real-harness tests skipped in `--harness mock` mode;
- `1`: test failure, or non-mock run skipped because required auth was missing;
- `2`: orchestrator/internal error;
- `3`: timeout.

See `docs/api/stream-h-eval-api.md` for the full JSON contract and catalog table.

## Real-harness auth

Real-harness tests use Claude and/or Codex CLIs through `HarnessRunner`. Local mock runs do not require auth. Full runs require:

```bash
export MEMORUM_EVAL_CLAUDE_KEY=...
export MEMORUM_EVAL_CODEX_KEY=...
```

The underlying CLI auth must also be configured. If credentials or CLIs are missing, real-harness tests skip with `SKIP_NO_AUTH`; that is acceptable only in `--harness mock` mode.

## Debugging failures

- Re-run with `--filter <test>` and `-v` to isolate output.
- Use `--no-cleanup` to inspect the temp memory tree after a failure.
- For scaffold/socket failures, run the relevant integration test directly with `RUST_BACKTRACE=1`.
- For JSON-shape issues, run `cargo test -p memorum-eval --test orchestrator_integration` before changing the binary output contract.
- For CI-shape issues, run `cargo test -p memorum-eval --test ci_workflow_shape`.

## Adding a regression test

Every production failure that the harness missed becomes a permanent regression test.

1. Pick the next test number and create `crates/memorum-eval/tests/eval/regression/t<NN>_<slug>.rs`.
2. Start the file with a `//!` metadata block containing: test number, incident date, description, root cause, fix commit, and what the test asserts.
3. Write the test so it fails on the buggy code and passes only with the fix.
4. Register the test in `TEST_CATALOG` if it should run through the orchestrator.
5. Update `docs/dev/stream-h-test-catalog.md` and `docs/api/stream-h-eval-api.md` if the public catalog changes.
6. Run `cargo test -p memorum-eval --test regression_meta`.

The metadata check scans `tests/eval/regression/` relative to `CARGO_MANIFEST_DIR`. It intentionally does not scan `src/tests/...`; that path is not part of Cargo integration-test discovery and would allow a vacuous pass.
