# Stream F Final Test / Gate Rerun Review

**Result: BLOCK.** Stream F is not releasable under the Task 17 gate contract yet.

The Stream F targeted acceptance suite is green, the prior rustdoc blocker is closed, clippy is green, the Rust boundary check is green, oxlint is green, `git diff --check` is green, and the Stream F benchmark assertion is green. However, two required final gates still block release:

1. `cargo test --workspace --all-targets --all-features` fails in `memory-substrate` release-gate coverage.
2. `pnpm exec oxfmt --check .` still fails on two Markdown files.

I did not modify source files in this pass. I wrote only this report. I did observe source/test/docs mtimes changing during the first rerun, so I reran the impacted checks after the last observed source timestamp; the current blocker evidence below uses those later checks where relevant.

## Findings

### S1 - Full workspace test gate fails before completing the workspace

**Status:** BLOCK.

**Required gate:** `cargo test --workspace --all-targets --all-features`.

**Evidence:** The full workspace rerun failed in `crates/memory-substrate/tests/release_gate_contracts.rs`:

```text
thread 'two_clone_convergence_script_reaches_fixed_point' panicked at crates/memory-substrate/tests/release_gate_contracts.rs:135:5:
script failed: ./scripts/two-clone-convergence.sh --smoke
stderr:
Traceback (most recent call last):
  File "<stdin>", line 4, in <module>
TypeError: write_text() got an unexpected keyword argument 'newline'
```

I reconfirmed the exact failing test after the worktree stabilized:

```bash
cargo test -p memory-substrate --test release_gate_contracts two_clone_convergence_script_reaches_fixed_point
# FAILED with the same TypeError from ./scripts/two-clone-convergence.sh --smoke
```

The failing script call is at `scripts/two-clone-convergence.sh:92-97`, where `path.write_text(..., newline="")` is invoked by the embedded Python. On this machine (`python3 --version` => `Python 3.13.9`), that invocation raises the TypeError above.

**Impact:** Task 17 cannot honestly report a green full workspace test gate. Because the full workspace command stops at this failure, it does not certify any later tests that would have run after this crate in the full command.

**Required fix:** Change the embedded Python write helper to a portable form for this environment, for example opening the file with `path.open("w", newline="")` and writing the text explicitly, then rerun the full workspace test gate.

### S1 - Oxfmt final docs gate still fails

**Status:** BLOCK.

**Required gate:** `pnpm exec oxfmt --check .`.

**Evidence:** The final oxfmt check failed on two files:

```text
docs/api/stream-f-dreaming-api.md
docs/reviews/stream-f-final-api-contract-review-rerun.md

Format issues found in above 2 files. Run without `--check` to fix.
```

This confirms the prior 16-file oxfmt blocker was reduced but not fully closed.

**Impact:** Task 17's repo boundary/docs gates are still not green.

**Required fix:** Format those two Markdown files only, then rerun `pnpm exec oxfmt --check .`.

## Prior blocker coverage

### Rustdoc blocker: closed

The previous test review blocked on bare `<id>` rustdoc examples in `crates/memoryd/src/cli.rs`. The rerun gate now passes:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
# PASS
```

### Oxfmt blocker: partially closed, still blocking

The previous oxfmt failure listed 16 Markdown files. The current failure lists only two files, but the required gate is still red:

```bash
pnpm exec oxfmt --check .
# FAIL: docs/api/stream-f-dreaming-api.md and docs/reviews/stream-f-final-api-contract-review-rerun.md
```

### Claude-review / named prior blockers: covered by deterministic tests

I confirmed the named prior-blocker coverage by inspecting the exact tests and by running the targeted Stream F acceptance suite successfully:

- `NotACanonicalMemory` / `read_path` erratum: covered by `crates/memory-substrate/tests/dream_canonical_isolation.rs` and daemon wrapper coverage in `crates/memoryd/tests/dream_canonical_isolation.rs`.
- `DreamProseAsSource`: covered by `crates/memoryd/tests/dream_grounding_rehydration.rs` matrix cases for journal/question/file refs in source/evidence positions.
- `no_entity_match`: covered by `crates/memoryd/tests/dream_recall_integration.rs`.
- Exit code 5 for lease conflicts/unavailability: covered by `crates/memoryd/tests/dream_lease_election.rs` through the real `memoryd dream now` binary path.
- `grounding_rehydration_required`: covered by `crates/memoryd/tests/dream_pass_pipeline.rs` and dream-candidate fixtures in `crates/memoryd/tests/dream_grounding_rehydration.rs`.
- Prompt determinism and runtime-path independence: covered by `crates/memoryd/tests/dream_scope_and_prompts.rs`.
- Startup recall unchanged without dream questions: covered by `crates/memoryd/tests/dream_recall_integration.rs`.
- Pass 2 empty-evidence regression: covered by `crates/memoryd/tests/dream_pass_pipeline.rs`.

Targeted acceptance rerun:

```bash
cargo test -p memory-substrate --test dream_canonical_isolation --test dream_substrate_primitives
# PASS: 8 + 5 tests

cargo test -p memory-substrate --test dream_merge_rules
# PASS: 4 tests

cargo test -p memoryd --test dream_canonical_isolation
# PASS: 3 tests

cargo test -p memoryd --test dream_substrate_fragments --test dream_lease_election --test dream_lease_scheduled_retry --test dream_pass_pipeline --test dream_grounding_rehydration --test dream_harness_cli --test dream_cleanup --test dream_recall_integration --test dream_cli --test dream_scope_and_prompts
# PASS
```

## Gate evidence

| Gate                                                                                                                                                                                                                                                                                                                | Result | Evidence / notes                                                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -----: | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cargo test -p memory-substrate --test dream_canonical_isolation --test dream_substrate_primitives`                                                                                                                                                                                                                 |   PASS | 8 + 5 tests passed.                                                                                                                                     |
| `cargo test -p memory-substrate --test dream_merge_rules`                                                                                                                                                                                                                                                           |   PASS | 4 tests passed.                                                                                                                                         |
| `cargo test -p memoryd --test dream_canonical_isolation`                                                                                                                                                                                                                                                            |   PASS | 3 tests passed.                                                                                                                                         |
| `cargo test -p memoryd --test dream_substrate_fragments --test dream_lease_election --test dream_lease_scheduled_retry --test dream_pass_pipeline --test dream_grounding_rehydration --test dream_harness_cli --test dream_cleanup --test dream_recall_integration --test dream_cli --test dream_scope_and_prompts` |   PASS | Targeted Stream F acceptance suite passed.                                                                                                              |
| `cargo test --workspace --all-targets --all-features`                                                                                                                                                                                                                                                               |   FAIL | `release_gate_contracts::two_clone_convergence_script_reaches_fixed_point` fails because `./scripts/two-clone-convergence.sh --smoke` raises TypeError. |
| `cargo fmt --all -- --check`                                                                                                                                                                                                                                                                                        |   PASS | Final stabilization rerun passed.                                                                                                                       |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings`                                                                                                                                                                                                                                              |   PASS | Final stabilization rerun passed after last observed source timestamp.                                                                                  |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`                                                                                                                                                                                                                                                        |   PASS | Prior rustdoc blocker is closed.                                                                                                                        |
| `./scripts/rust-boundary-check.sh`                                                                                                                                                                                                                                                                                  |   PASS | Boundary check passed.                                                                                                                                  |
| `pnpm exec oxfmt --check .`                                                                                                                                                                                                                                                                                         |   FAIL | `docs/api/stream-f-dreaming-api.md` and `docs/reviews/stream-f-final-api-contract-review-rerun.md` need formatting.                                     |
| `pnpm exec oxlint .`                                                                                                                                                                                                                                                                                                |   PASS | 0 warnings, 0 errors.                                                                                                                                   |
| `git diff --check`                                                                                                                                                                                                                                                                                                  |   PASS | No whitespace diff errors.                                                                                                                              |
| `cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --assert --baseline bench/stream-f-dreaming-results.darwin-arm64.json`                                                                                                                                                                   |   PASS | Bench assertion passed: observe write p95 0.294ms, cleanup p95 33,995.823ms, pending-attention overhead p95 2.578ms.                                    |

## Residual risks

- The full workspace test command is not certifying the entire workspace until the two-clone script failure is fixed.
- The bench assert still uses the documented deterministic fixture limits: local bare git for lease overhead, best-effort substrate durability for stable local write measurements, and no real harness/LLM latency.
- The worktree is very large and was not stable during the first rerun. I reran the affected gates after the last observed source timestamp, but final release should rerun the full Task 17 sequence once the two blockers above are fixed.
