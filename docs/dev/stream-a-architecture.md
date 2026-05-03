# Stream A architecture

The repository is a Rust workspace with three crates:

- `memory-substrate`: library crate for frontmatter, tree/config/IDs, durable Markdown writes, events, SQLite index, watcher, git, merge, and public API seams.
- `memory-merge-driver`: path-local Git merge driver binary.
- `memory-test-support`: convergence, perf, and boundary-check helpers.

Canonical state is Markdown/YAML plus durable JSONL audit. SQLite/FTS/vector rows are derived and rebuildable. Git is used as sync transport through explicit argv wrappers.

Specgate is used for ownership/config checks. Rust import/boundary checks are covered separately by `scripts/rust-boundary-check.sh` because the installed Specgate resolver is TS-oriented.

## Specgate ownership doctor note

The installed Specgate CLI identifies itself as "Machine-checkable architectural intent for TypeScript projects" (`specgate --help`, tool version `0.3.1`). In this Rust workspace, `specgate validate` and `specgate check --output-mode deterministic` are meaningful green gates for config/spec policy, but `specgate doctor ownership --project-root . --format json` discovers only TypeScript/JavaScript-style source files. As of 2026-05-02 it reports `total_source_files: 1` (`crates/memoryd-web/static/app.js`) and marks the six Stream A Rust ownership specs as `orphaned_specs` even though their Rust globs exist.

Those six `orphaned_specs` warnings are therefore expected and harmless until Specgate gains Rust source discovery. Keep them as warnings, not release blockers, and use `scripts/rust-boundary-check.sh` plus Rust tests for Stream A code ownership. If Specgate later adds Rust discovery, this note should be deleted and `specgate doctor ownership` should be promoted back to a zero-warning gate.
