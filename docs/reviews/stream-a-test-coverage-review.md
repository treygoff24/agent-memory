# Stream A test coverage review

Status: release-certification candidate coverage.

The acceptance coverage manifest in `crates/memory-substrate/tests/spec_coverage_manifest.rs` maps every §5-§17 acceptance bullet in `docs/specs/stream-a-core-substrate-v1.1.md` to a named test. The manifest also fails on stale mappings and missing referenced tests.

Covered areas include:

- Tree bootstrap, duplicate IDs, case-fold collisions, supersession cycles, partial/full validation behavior.
- Frontmatter schema/defaults, lifecycle/tombstone/prospective variants, unknown extras, and merge quarantine output validity.
- ID allocation monotonicity, shard separation, high-water repair, exhaustion, and duplicate repair.
- Atomic writes, CAS, durability outcomes, event-after-commit repair outcomes, encrypted writes, and crash matrix states.
- FTS/chunk indexing, mutation cleanup, VACUUM behavior, sqlite-vec vectors, vector reconciliation, active triple switching, and dropped triples.
- Startup reconciliation, offline edit ingestion, invalid edit operator repair, pending index/event replay, duplicate event skip, and duplicate device-log refusal.
- Git adoption/preflight, semantic two-clone convergence, merge-driver schema gates, and add/add quarantine.
- Watcher overflow/rescan, subscription ownership, and hash-based self-event suppression with external-edit delivery.
- Config precedence, public API/error/event-kind contracts, cancellation behavior, release gate scripts, fuzz/perf/durability gates, and final review evidence.

Independent review findings were remediated and rechecked; the final narrow review reported no material issues.
