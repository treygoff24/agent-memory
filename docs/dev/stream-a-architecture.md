# Stream A architecture

The repository is a Rust workspace with three crates:

- `memory-substrate`: library crate for frontmatter, tree/config/IDs, durable Markdown writes, events, SQLite index, watcher, git, merge, and public API seams.
- `memory-merge-driver`: path-local Git merge driver binary.
- `memory-test-support`: convergence, perf, and boundary-check helpers.

Canonical state is Markdown/YAML plus durable JSONL audit. SQLite/FTS/vector rows are derived and rebuildable. Git is used as sync transport through explicit argv wrappers.

Specgate is used for ownership/config checks. Rust import/boundary checks are covered separately by `scripts/rust-boundary-check.sh` because the installed Specgate resolver is TS-oriented.
