# Stream F Review Gate A - Contract/API Review

Date: 2026-04-30
Scope: Tasks 1-3A against `docs/specs/stream-f-dreaming-v0.2.md` §1.1, §2 invariants, and §12 Stream A acceptance bullets.
Lane: contract/API review only.

## Verdict

No S1 blocker found.

Gate A contract lane does **not** pass yet because there are S2 findings that should be fixed before Task 4 begins. The core in-process APIs mostly model the intended contract, and the requested tests are green, but the real git merge-driver attachment and archived substrate tree-validity contracts are incomplete.

## Findings

### S2 - Stream F JSON/JSONL merge rules are implemented in `merge_markdown` but are not wired into `.gitattributes`, so real git merges will not use them

Evidence:

- Stream F v0.2 requires Stream A's three-way merge driver to handle substrate JSONL, dream question JSONL, lease JSONL, dream journal Markdown, and cleanup JSON with Stream-F-specific semantics (`docs/specs/stream-f-dreaming-v0.2.md:86-90`).
- Task 3A implements path routing inside `merge_stream_f_file` for substrate JSONL, dream question JSONL, lease JSONL, dream journal Markdown, and cleanup JSON (`crates/memory-substrate/src/merge/three_way.rs:74-87`).
- But repo bootstrap still writes `.gitattributes` as only:
  - `*.md merge=memory-merge-driver`
  - `events/*.jsonl merge=union`
  - `substrate/**/*.jsonl merge=union`
  - `tombstones/*.jsonl merge=union`
    (`crates/memory-substrate/src/tree/layout.rs:8-12`).
- The merge driver binary only runs when git attributes select `memory-merge-driver`; it delegates all semantics to `merge_markdown(MergeInput { ..., path })` (`crates/memory-merge-driver/src/main.rs:27-36`).

Impact:

- `dreams/questions/**/*.jsonl`, `leases/journal.lease`, and `dreams/cleanup/**/*.json` are not assigned to `memory-merge-driver` at all in freshly bootstrapped repos, so Task 3A's implemented merge rules are dead in real git conflict resolution.
- `substrate/**/*.jsonl` is assigned to git's `union` merge, not the new concat/dedup/sort-by-`id` rule. Union merge can preserve duplicate rows and non-deterministic ordering, which breaks the Stream F deterministic merge contract.
- `encrypted/substrate/**/*.jsonl` is not explicitly assigned either, despite the Task 3A router treating it as substrate JSONL.

Required fix:

- Update the bootstrap `.gitattributes` contract so every Stream F noncanonical merge family that depends on Task 3A semantics invokes `memory-merge-driver` or an equivalently deterministic driver path.
- Add a regression test against the emitted `.gitattributes` body, not only direct unit tests against `merge_markdown`. A direct `merge_markdown` test proves algorithm behavior but not real git attachment.
- If substrate should intentionally remain `merge=union`, then the spec/API docs must be amended because the current docs claim Stream F routes through the merge-driver public surface (`docs/api/stream-a-public-api.md:31-36`).

### S2 - The archive primitive creates `substrate/archive/<device>/<YYYY-MM>.jsonl`, but tree validation rejects that same path family

Evidence:

- Stream F v0.2 says expired fragments move to `substrate/archive/<device_id>/<YYYY-MM>.jsonl` (`docs/specs/stream-f-dreaming-v0.2.md:66`, `docs/specs/stream-f-dreaming-v0.2.md:543`, `docs/specs/stream-f-dreaming-v0.2.md:720`).
- Task 3 implements exactly that archive output path (`crates/memory-substrate/src/api.rs:815-817`).
- The noncanonical Stream F path classifier broadly treats any `substrate/**/*.jsonl` path as noncanonical (`crates/memory-substrate/src/model.rs:1098-1109`).
- But tree validation routes all `substrate/` files through `validate_device_date_path(relative, "substrate", "jsonl")` (`crates/memory-substrate/src/tree/validate.rs:149-151`), which only accepts `substrate/<device>/<YYYY-MM-DD>.jsonl` (`crates/memory-substrate/src/tree/validate.rs:180-193`, `crates/memory-substrate/src/tree/validate.rs:196-205`). `substrate/archive/<device>/<YYYY-MM>.jsonl` has one extra segment and a month-only stem, so a repo becomes invalid after the archive primitive runs.
- Existing archive tests assert the archive file is written and sorted (`crates/memory-substrate/tests/dream_substrate_primitives.rs:61-117`) but do not call `validate_tree` after archival.

Impact:

- Cleanup/archival can produce a repository state that fails Stream A tree validation.
- Later cleanup and daemon tasks will build on an invalid-on-disk substrate family unless this is fixed now.
- This also weakens the acceptance coverage claim for Stream A tree/config/events/substrate primitives: archived substrate is part of the Stream F substrate lifecycle but is not represented in the canonical-isolation validation tests.

Required fix:

- Teach `validate_noncanonical_stream_f_file` a distinct `substrate/archive/<device>/<YYYY-MM>.jsonl` branch with JSONL-object validation and month-stem validation.
- Add canonical-isolation coverage for archive paths: valid tree file, `read_path_envelope(&RepoPath)` returns `ReadError::NotACanonicalMemory`, and queries/indexing skip it.
- Add `validate_tree(...).expect(...)` to the archive primitive test after `archive_expired_substrate_fragments` writes the archive file.

## Positive checks

- The `read_memory_envelope(&MemoryId)` erratum is handled correctly. The implementation did not add a bogus path-addressed `read_memory_envelope`; it uses existing `read_path_envelope(&RepoPath)` and returns `ReadError::NotACanonicalMemory` before frontmatter parsing (`crates/memory-substrate/src/api.rs:134-143`). The contract map documents this explicitly (`docs/reviews/stream-f-contract-map.md:24-32`).
- Canonical isolation is represented for the six live noncanonical families listed in Task 2 tests: journal Markdown, question JSONL, cleanup JSON, plaintext substrate JSONL, encrypted substrate JSONL, and lease JSONL (`crates/memory-substrate/tests/dream_canonical_isolation.rs:8-155`; `crates/memoryd/tests/dream_canonical_isolation.rs:8-105`). The archive path gap above is the missing lifecycle variant.
- Config keys named in v0.2 are present with defaults and validation ranges, including `lease_window_seconds`, `pass_1_window_days`, `candidate_stale_days`, `pass_2_drift_threshold`, `events.compaction_days`, and `dream_retry_window_minutes` (`crates/memory-substrate/src/config/mod.rs:77-163`, `crates/memory-substrate/src/config/mod.rs:230-315`; tests in `crates/memory-substrate/tests/config_loading.rs`).
- `SubstrateFragmentWritten` is added as a typed event and has schema fixture coverage (`crates/memory-substrate/src/events/log.rs:97-105`; `crates/memory-substrate/tests/event_kind_schema.rs`).
- Task 2-3A does not claim `DreamProseAsSource` or grounding rehydration complete. The plan leaves that to Task 11 (`docs/plans/2026-04-30-stream-f-dreaming.md:654-676`), and no current write-failure variant exists for it in the reviewed code (`crates/memory-substrate/src/error.rs:143-175`). This is acceptable at Gate A as long as Task 11 remains mandatory.

## Requested gate results

```text
cargo test -p memory-substrate --test dream_canonical_isolation
  PASS: 5 passed

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
```

## Residual risks / should-log items

- The substrate append API currently maps `ClassificationOutcome::Secret` to a validation failure string inside `append_substrate_fragment` (`crates/memory-substrate/src/api.rs:1296-1329`). Task 7 can still satisfy the public `memory_observe` `SecretRefused` contract by refusing before calling the substrate append primitive, but reviewers should verify that exact error mapping when Task 7 lands.
- The direct merge tests exercise `merge_markdown` as an in-process function, not a git-level conflict using emitted attributes and the `memory-merge-driver` binary. The S2 above should be closed with attachment-level coverage.
