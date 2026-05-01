# Stream F Review Gate A - Contract/API Rerun

Date: 2026-04-30
Scope: Rerun of Review Gate A contract/API lane after scoped fixes for prior S2 findings.
Contract: `docs/specs/stream-f-dreaming-v0.2.md` Stream A surface additions, canonical isolation, substrate primitives, config/event primitives, and Stream F merge-driver attachment.
Lane: review-only; no implementation files were edited in this rerun.

## Verdict

PASS.

No S1 or S2 contract/API gaps remain in the Gate A rerun scope. The two prior S2 findings are fixed, and the requested Gate A command set is green.

## Findings

No S1/S2 findings.

## Prior S2 verification

### Fixed - archive paths validate after substrate archival

The previous failure mode was: `archive_expired_substrate_fragments` wrote `substrate/archive/<device>/<YYYY-MM>.jsonl`, but Stream F tree validation only accepted live `substrate/<device>/<YYYY-MM-DD>.jsonl` files.

Current evidence:

- `validate_noncanonical_stream_f_file` now routes `substrate/archive/` before the live `substrate/` branch and validates it with the device-month path helper plus JSONL object validation (`crates/memory-substrate/src/tree/validate.rs:153-155`).
- `validate_device_month_path` enforces `substrate/archive/<device>/<YYYY-MM>.jsonl` shape (`crates/memory-substrate/src/tree/validate.rs:204-217`).
- The archival behavior test now reruns full tree validation after the archive file is written (`crates/memory-substrate/tests/dream_substrate_primitives.rs:74-92`).
- The API documentation now lists `substrate/archive/<device_id>/<YYYY-MM>.jsonl` as a valid-but-noncanonical Stream F repo file (`docs/api/stream-a-public-api.md:20-30`).

Assessment: fixed.

### Fixed - generated `.gitattributes` routes Stream F merge families to `memory-merge-driver`

The previous failure mode was: Stream F merge rules existed in Rust, but fresh repo attributes did not invoke the memory merge driver for the JSON/JSONL families that depend on those rules.

Current evidence:

- The canonical generated `.gitattributes` body now routes live substrate, archived substrate via the same glob, encrypted substrate, dream questions, dream cleanup reports, dream journals, and the lease file to `memory-merge-driver` (`crates/memory-substrate/src/tree/layout.rs:8-17`).
- Bootstrap writes that canonical body when `.gitattributes` is absent (`crates/memory-substrate/src/tree/layout.rs:52-59`).
- The regression test initializes a real git repo and verifies `git check-attr merge` resolves `memory-merge-driver` for every Stream F family, including `substrate/archive/dev_local/2026-04.jsonl` (`crates/memory-substrate/tests/dream_canonical_isolation.rs:36-53`).
- The merge router dispatches Stream F paths before canonical Markdown parsing (`crates/memory-substrate/src/merge/three_way.rs:40-88`).
- Direct merge-rule coverage still exercises substrate JSONL, question/lease JSONL, dream journal Markdown, and cleanup JSON semantics (`crates/memory-substrate/tests/dream_merge_rules.rs:6-102`).

Assessment: fixed for fresh/bootstrap-created repos.

## Additional contract checks

- Noncanonical path classification covers `dreams/journal/**.md`, `dreams/questions/**.jsonl`, all `substrate/**.jsonl` including archive paths, `encrypted/substrate/**.jsonl`, `dreams/cleanup/**.json`, and `leases/journal.lease` (`crates/memory-substrate/src/model.rs:1098-1113`).
- `Substrate::read_path_envelope(&RepoPath)` refuses those valid-but-noncanonical paths with `ReadError::NotACanonicalMemory` before frontmatter parsing (`crates/memory-substrate/src/api.rs:139-143`).
- Canonical-isolation tests verify dream/substrate/lease files validate without canonical frontmatter, refuse path-envelope reads, and do not appear through memory/query/recall/chunk APIs (`crates/memory-substrate/tests/dream_canonical_isolation.rs:9-108`).
- Config defaults and validation cover the Stream F v0.2 keys exercised in Gate A, including lease/retry windows and event compaction (`crates/memory-substrate/tests/config_loading.rs`; rerun green below).

## Gate A command results

```text
cargo test -p memory-substrate --test dream_canonical_isolation
  PASS: 6 passed

cargo test -p memoryd --test dream_canonical_isolation
  PASS: 3 passed

cargo test -p memory-substrate --test dream_substrate_primitives
  PASS: 5 passed

cargo test -p memory-substrate --test config_loading
  PASS: 10 passed

cargo test -p memory-substrate --test dream_merge_rules
  PASS: 4 passed

cargo fmt --all -- --check
  PASS

cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
  PASS
```

## Residual risks / follow-up watch items

- Existing already-initialized memory repos with a pre-Stream-F `.gitattributes` remain a migration/watch item because `bootstrap_repo_layout` intentionally writes `.gitattributes` only when absent. This rerun verifies the generated/bootstrap contract and does not require rewriting user-customized attributes.
- The canonical-isolation fixture list does not explicitly include an archive path in its `read_path_envelope` loop, but the classifier is broad over `substrate/**.jsonl`, and the archive primitive now validates the full tree after writing the archive file. This is not an S2 gap, but adding the archive file to `noncanonical_files()` would make the test intent clearer.
- Task 11 remains responsible for `DreamProseAsSource` and grounding rehydration enforcement. Gate A still treats that as correctly deferred, not missing from Tasks 1-3A.
