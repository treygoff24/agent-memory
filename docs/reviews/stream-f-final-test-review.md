# Stream F Final Gate E Test / Acceptance Review

**Role:** test-hardening / acceptance review  
**Scope:** Stream F dreaming final acceptance coverage, named prior-blocker regressions, and final gate sufficiency against `docs/specs/stream-f-dreaming-v0.2.md` and `docs/plans/2026-04-30-stream-f-dreaming.md`.  
**Result:** FAIL

## Verdict

Final Gate E is **not releasable yet**. The Stream F targeted acceptance suite, benchmark assertion, workspace tests, clippy, rustfmt, rust-boundary check, `oxlint`, and `git diff --check` are green in this review run. The named Claude prior blockers are covered by deterministic regression tests.

However, two final release gates still fail:

1. `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` fails on invalid Rustdoc HTML tags in `crates/memoryd/src/cli.rs`.
2. `pnpm exec oxfmt --check .` fails on 16 Markdown files, including Stream F API/review docs.

Because Task 17 explicitly requires these gates before declaring done, this review fails until both are fixed and rerun.

## Findings

### S1-1 - Rustdoc final gate fails on unescaped `<id>` scope examples

**Evidence:**

- Task 17 requires `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` as a broader Rust gate (`docs/plans/2026-04-30-stream-f-dreaming.md:966-972`).
- The current CLI doc comments contain bare angle-bracket examples at `crates/memoryd/src/cli.rs:257` and `crates/memoryd/src/cli.rs:288`:
  - `/// Dream scope: me, agent, project:<id>, or org:<id>.`
  - `/// Optional dream scope: me, agent, project:<id>, or org:<id>.`
- The gate failed with `rustdoc::invalid-html-tags` for each bare `<id>` occurrence.

**Why it matters:** This is a required final release gate. With `-D warnings`, the workspace cannot publish clean docs and Task 17 cannot truthfully report a green final gate.

**Required fix:** Escape or backtick the examples, e.g. `project:\<id\>` / `org:\<id\>` or `` `project:<id>` `` / `` `org:<id>` ``, then rerun the rustdoc gate.

### S1-2 - Markdown formatting final gate fails across Stream F documentation/review artifacts

**Evidence:**

- Task 17 requires repo boundary/docs gates including `pnpm exec oxfmt --check .` (`docs/plans/2026-04-30-stream-f-dreaming.md:973-979`).
- `pnpm exec oxfmt --check .` failed and listed 16 files, including Stream F docs/reviews:
  - `docs/api/stream-f-dreaming-api.md`
  - `docs/reviews/stream-f-bench-evidence.md`
  - `docs/reviews/stream-f-contract-map.md`
  - `docs/reviews/stream-f-final-clean-code-review.md`
  - several Stream F Gate A/B/C review files
- The command reported: `Format issues found in above 16 files. Run without --check to fix.`

**Why it matters:** This is also a required final release gate. It does not undermine runtime behavior, but it blocks Task 17's "all commands pass" release criterion.

**Required fix:** Run the formatter intentionally (or manually apply equivalent formatting) for the listed Markdown files, then rerun `pnpm exec oxfmt --check .`.

## Coverage / named blocker review

I did **not** find remaining acceptance-coverage gaps for the specifically named prior blockers:

- **`NotACanonicalMemory` / `read_path` erratum:** `Substrate::read_path_envelope` refuses noncanonical Stream F paths before frontmatter parsing at `crates/memory-substrate/src/api.rs:140-143`; substrate tests assert `ReadError::NotACanonicalMemory` for dream/substrate/lease paths at `crates/memory-substrate/tests/dream_canonical_isolation.rs:159-177`; daemon-visible wrapper coverage exists at `crates/memoryd/tests/dream_canonical_isolation.rs:34-64`.
- **`DreamProseAsSource`:** matrix coverage refuses `dreams/journal`, `dreams/questions`, and `file:` forms in both `source` and `evidence` positions at `crates/memoryd/tests/dream_grounding_rehydration.rs:190-214`.
- **`no_entity_match`:** nonmatching Pass-3 question entities are omitted and counted at `crates/memoryd/tests/dream_recall_integration.rs:63-72`.
- **Exit code 5:** both `lease_held` and `lease_unavailable` are asserted through the real `memoryd dream now` binary at `crates/memoryd/tests/dream_lease_election.rs:32-59` and `crates/memoryd/tests/dream_lease_election.rs:98-117`.
- **`grounding_rehydration_required`:** Pass 2 accepted candidates assert the marker before candidate write at `crates/memoryd/tests/dream_pass_pipeline.rs:100-105`; dream candidate fixtures set the marker at `crates/memoryd/tests/dream_grounding_rehydration.rs:337-340`.
- **Prompt determinism byte equality and runtime-path independence:** Pass 1/2/3 render twice and assert byte equality while CWD is moved away from the repo at `crates/memoryd/tests/dream_scope_and_prompts.rs:102-117`; templates are compile-time embedded with `include_str!` at `crates/memoryd/src/dream/prompts.rs:11-13`.
- **Startup recall unchanged:** no-question startup recall is byte-identical to the Stream E baseline at `crates/memoryd/tests/dream_recall_integration.rs:187-193`.
- **Pass 2 empty evidence regression:** empty evidence is refused before governance and marks Pass 2 skipped at `crates/memoryd/tests/dream_pass_pipeline.rs:159-182`.

## Required fixes

1. Fix the two Rustdoc comments in `crates/memoryd/src/cli.rs` so bare `<id>` is not parsed as HTML.
2. Format the 16 Markdown files reported by `pnpm exec oxfmt --check .`.
3. Rerun at least:
   - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`
   - `pnpm exec oxfmt --check .`
4. Then rerun the full Task 17 gate set before writing the final gate report.

## Residual risks

- `cargo test --workspace --all-targets --all-features` passed, but it is slow; `id_sequence` alone took ~231s. Keep the broad gate in Task 17 despite the cost because it catches cross-stream regressions outside the targeted Stream F suite.
- The bench assertion passes, but the substrate-fragment write fixture records that durable fsync is not included in the throughput measurement. This is already disclosed in bench evidence and should remain a documented performance caveat, not a release blocker for this test review.
- While this review was running, the dirty/untracked Stream F tree continued to change. I reran the affected lease test and clippy after observing the new lease-release test/function, but the final release owner should run the complete Task 17 sequence once more from a stable worktree.

## Commands run

| Command                                                                                                                                                                                                                                                                              | Result | Notes                                                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -----: | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cargo test -p memory-substrate --test dream_canonical_isolation --test dream_substrate_primitives`                                                                                                                                                                                  |   PASS | 8 + 5 tests passed.                                                                                                                                             |
| `cargo test -p memory-substrate --test dream_merge_rules`                                                                                                                                                                                                                            |   PASS | 4 tests passed.                                                                                                                                                 |
| `cargo test -p memoryd --test dream_canonical_isolation`                                                                                                                                                                                                                             |   PASS | 3 tests passed.                                                                                                                                                 |
| `cargo test -p memoryd --test dream_substrate_fragments --test dream_lease_election --test dream_lease_scheduled_retry --test dream_pass_pipeline --test dream_grounding_rehydration --test dream_harness_cli --test dream_cleanup --test dream_recall_integration --test dream_cli` |   PASS | Stream F acceptance suite passed: 10 cleanup, 7 CLI, 11 rehydration, 8 harness, 8 lease, 6 scheduled retry, 13 pipeline, 9 recall, 13 substrate-fragment tests. |
| `cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --assert --baseline bench/stream-f-dreaming-results.darwin-arm64.json`                                                                                                                                    |   PASS | All benchmark assertions passed; cleanup p95 43,982.652ms < 60s, recall overhead p95 1.976ms <= 5ms.                                                            |
| `cargo test --workspace --all-targets --all-features`                                                                                                                                                                                                                                |   PASS | Full workspace passed. Slowest observed test file: `id_sequence` finished in 231.56s.                                                                           |
| `cargo test -p memoryd --test dream_lease_election`                                                                                                                                                                                                                                  |   PASS | Rerun after the lease-release test appeared; 10 tests passed.                                                                                                   |
| `cargo fmt --all -- --check`                                                                                                                                                                                                                                                         |   PASS | Final rerun passed.                                                                                                                                             |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings`                                                                                                                                                                                                               |   PASS | Final rerun passed.                                                                                                                                             |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`                                                                                                                                                                                                                         |   FAIL | Invalid Rustdoc HTML tags from bare `<id>` examples in `crates/memoryd/src/cli.rs:257` and `crates/memoryd/src/cli.rs:288`.                                     |
| `./scripts/rust-boundary-check.sh`                                                                                                                                                                                                                                                   |   PASS | Boundary check passed.                                                                                                                                          |
| `pnpm exec oxfmt --check .`                                                                                                                                                                                                                                                          |   FAIL | 16 Markdown files need formatting.                                                                                                                              |
| `pnpm exec oxlint .`                                                                                                                                                                                                                                                                 |   PASS | 0 warnings, 0 errors.                                                                                                                                           |
| `git diff --check`                                                                                                                                                                                                                                                                   |   PASS | No whitespace diff errors.                                                                                                                                      |
