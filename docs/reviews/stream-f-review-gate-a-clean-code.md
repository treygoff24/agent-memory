# Stream F Review Gate A: clean-code / Rust review

## Verdict

Changes requested. Gate A clean-code lane does **not** pass while the S2 findings below remain open.

## Intended outcome

Task 2-3A appears to extend the Stream A substrate/config/events/merge surfaces so Stream F can store repo-synced noncanonical dream files, append/archive substrate fragments, validate those files without canonical-memory frontmatter parsing, keep them out of canonical query/index APIs, and merge them deterministically. The slice should preserve Stream A as the only canonical substrate/index while adding valid-but-noncanonical Stream F file families.

## Executive summary

The implementation is directionally consistent with the Stream F contract: frontmatter-free dream files validate, noncanonical paths are refused by `read_path_envelope`, canonical index queries skip dream/substrate files, config defaults are explicit, and the narrow requested test set passes. However, two integration seams currently break the intended behavior before Task 4: archived substrate files written by the new API do not validate under the new tree validator, and the new JSON/JSONL merge rules are not actually wired through `.gitattributes` for git merges. Both are real Stream A substrate/merge regressions, not style issues. The code otherwise uses deterministic containers/orderings and keeps writes mostly contained.

## Findings

### [S2 must-fix before Task 4] Correctness: substrate archive output is not accepted by Stream A tree validation

- **Evidence:** `archive_expired_substrate_fragments` writes archived fragments to `substrate/archive/<device>/<YYYY-MM>.jsonl` (`crates/memory-substrate/src/api.rs:814-817`). The Stream F spec says expired substrate files move to that same year-month archive path and that the six Stream F path families must validate (`docs/specs/stream-f-dreaming-v0.2.md:64-66`). But `validate_noncanonical_stream_f_file` treats every `substrate/**.jsonl` path as `substrate/<device>/<YYYY-MM-DD>.jsonl` via `validate_device_date_path(relative, "substrate", "jsonl")` (`crates/memory-substrate/src/tree/validate.rs:149-151`). That validator accepts exactly two path segments after `substrate/`, so `substrate/archive/dev_test/2026-04.jsonl` is rejected as an invalid device-date path. The public API docs also list live substrate paths but omit the archive path (`docs/api/stream-a-public-api.md:20-27`).
- **Why it matters:** The new archive API can create a repository state that the same Stream A validator rejects on the next startup, doctor run, daemon-visible validation, or sync preflight. That means the first successful cleanup/archive pass can poison the repo and block later Stream F work.
- **Reasoning:** This is a contract mismatch between the write path and validation path. The tests prove archival writes are idempotent, but they never validate the repository after archival, so the failure is currently unprotected.
- **Recommendation:** Add an explicit validator branch for `substrate/archive/<device_id>/<YYYY-MM>.jsonl` before the generic `substrate/` branch, with a year-month filename validator. Update `is_noncanonical_stream_f_repo_path`/docs as needed so the archive family is intentional rather than accidentally admitted by `value.starts_with("substrate/")`. Add a behavior test that archives an expired fragment and then runs `validate_tree(..., FullySynced)` successfully.
- **Confidence:** High.

### [S2 must-fix before Task 4] Correctness: Stream F JSON/JSONL merge rules are implemented but not invoked by git

- **Evidence:** `merge_stream_f_file` implements special handling for substrate JSONL, dream-question JSONL, `leases/journal.lease`, dream-journal Markdown, and cleanup JSON (`crates/memory-substrate/src/merge/three_way.rs:74-87`). The docs promise those path families route before canonical Markdown parsing (`docs/api/stream-a-public-api.md:31-36`), matching the Stream F merge contract (`docs/specs/stream-f-dreaming-v0.2.md:86-91`). But the bootstrap `.gitattributes` only sends `*.md` to `memory-merge-driver`; `substrate/**/*.jsonl` still uses `merge=union`, and there are no rules for `encrypted/substrate/**/*.jsonl`, `dreams/questions/**/*.jsonl`, `leases/journal.lease`, or `dreams/cleanup/**/*.json` (`crates/memory-substrate/src/tree/layout.rs:8-12`).
- **Why it matters:** In real two-clone git convergence, most of the new Stream F merge rules will not run. Substrate rows will use git union instead of canonical parse/dedupe/sort, dream-question and lease JSONL may fall back to default textual conflict behavior, and cleanup JSON will not use last-writer-wins. The library-level tests can pass while actual sync behavior remains nondeterministic or conflict-prone.
- **Reasoning:** The merge driver binary can only execute `merge_markdown(MergeInput { path, ... })` for paths assigned to `merge=memory-merge-driver` in `.gitattributes`. Adding dispatch in `three_way.rs` is insufficient unless the generated and preflighted attributes route those non-Markdown path families to the driver.
- **Recommendation:** Update `GITATTRIBUTES_BODY` to route every Stream F path family that needs custom semantics to `memory-merge-driver` and leave only truly union-safe streams on `merge=union`. At minimum add coverage that bootstraps a repo and asserts `.gitattributes` contains the Stream F routes; ideally add a two-clone or merge-driver CLI test for one JSONL path to prove git invokes the driver, not just the Rust function directly.
- **Confidence:** High.

## Non-blocking simplifications

- Consider extracting the Stream F path-family classification into one small module or enum used by `model.rs`, `tree/validate.rs`, and `merge/three_way.rs`. The current string-prefix checks are understandable, but they are already duplicated across three surfaces and the archive-path miss shows how easy it is for those contracts to drift.
- Consider separating the new substrate JSONL helpers from `api.rs` once Task 4 expands this area. The current slice is acceptable, but `api.rs` is accumulating storage-format details that would be easier to test and reason about behind a small `substrate_fragments` module.

## Test gaps

- No test archives an expired substrate fragment and then validates the resulting repo tree. This would catch the archive-path validator mismatch.
- No test asserts the generated `.gitattributes` routes Stream F JSON/JSONL/JSON paths to the merge driver, and no two-clone/CLI test proves the new merge rules are used by git rather than only by direct `merge_markdown` unit calls.
- Existing archive tests cover plaintext archival but not the tree/doc contract for `substrate/archive/<device>/<YYYY-MM>.jsonl`.

## Verification run

The requested gates passed:

```bash
cargo test -p memory-substrate --test dream_canonical_isolation
cargo test -p memoryd --test dream_canonical_isolation
cargo test -p memory-substrate --test dream_substrate_primitives
cargo test -p memory-substrate --test config_loading
cargo test -p memory-substrate --test dream_merge_rules
cargo fmt --all -- --check
```

Results observed: 5/5, 3/3, 5/5, 10/10, and 4/4 tests passed respectively; `cargo fmt --all -- --check` exited successfully.

## Questions / uncertainties

- I did not run a full workspace clippy or full test suite because the review prompt named a narrower Gate A command set.
- I did not modify production code or tests because this lane is review-only.

## Positives

- The canonical isolation behavior is covered at both `memory-substrate` and `memoryd` layers, which is the right contract boundary for Stream F noncanonical files.
- The new config defaults/range checks are explicit and deterministic, including the pending-attention cap relationship.
- The merge implementation uses canonical JSON serialization and sorted keys for deterministic direct-library output; once wired into git, that should be a good base for convergence.
