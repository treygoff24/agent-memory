# Stream A final review

Status: release-certification candidate.

Independent review: no blocking findings after remediation. Review lanes found and verified fixes for encrypted write classification/pathing, safe projection privacy, encrypted pending-index replay durability, event-after-commit outcome semantics, startup reconciliation, watcher suppression, read-path validation, private-credential privacy labels, event-log corruption handling, encrypted-namespace indexing, vector job reconciliation, and recoverable merge parse quarantines.

## Acceptance evidence

- `cargo fmt --all -- --check`: pass via `scripts/check.sh`.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass.
- `cargo test --workspace -- --nocapture`: pass.
- `cargo test --workspace --release`: pass via `scripts/check.sh`.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`: pass via `scripts/check.sh`.
- `pnpm run format:check`: pass via `scripts/check.sh`.
- `pnpm run lint`: pass via `scripts/check.sh`.
- `cargo +nightly-2025-09-18 dylint --path .dylint/custom_lints`: pass via `scripts/check.sh`.
- `specgate validate` and `specgate check --output-mode deterministic`: pass via `scripts/check.sh`.
- `scripts/two-clone-convergence.sh --full`: pass via `scripts/check.sh`.
- `scripts/durability-probe-gate.sh`: pass via `scripts/check.sh`.
- `scripts/bench-gate.sh --tier release`: pass via `scripts/check.sh`.
- `scripts/bench-regression-check.sh`: pass via `scripts/check.sh`.
- `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh`: final release gate command.

## Remediation notes

### Encrypted writes

- `write_encrypted` only accepts `ClassificationOutcome::RequiresEncryption`; `Trusted` and `Secret` are refused before disk effects.
- Ciphertext paths are derived under `encrypted/<original memory path>.bin`; non-memory originals are rejected and existing ciphertext is not overwritten.
- Safe projections are indexed through a sanitized index-only copy. For confidential/personal metadata with `index_body=false`, the indexed copy permits FTS only for the supplied safe projection; the original body is never indexed.
- No-projection encrypted index repair clears the body before writing a pending repair record.
- Pending encrypted index replay verifies the ciphertext hash. A mismatch blocks `Substrate::open` with operator repair and leaves the pending record durable.

### Startup reconciliation

- `Substrate::open` replays pending repair queues and runs a full Markdown reindex before returning.
- Full reindex clears plaintext-derived rows while preserving encrypted metadata rows that cannot be reconstructed from ciphertext.
- Invalid offline edits return `OpenError::OperatorRepairRequired`.
- Non-final malformed event log lines return `OpenError::OperatorRepairRequired`; only a single malformed trailing line is truncated.
- Plaintext Markdown under `encrypted/` returns `OpenError::OperatorRepairRequired` during startup and public reindex.

### Index and merge gates

- Plaintext index upserts enqueue pending embedding jobs for the active embedding triple.
- Startup reindex requeues missing active-triple embedding jobs.
- Chunk IDs are content-derived, so same-offset edits cannot stale-write through an old chunk ID.
- Recoverable merge-side parse failures with identifiable frontmatter delimiters produce valid quarantine files preserving the unparsed raw side.
- The full two-clone convergence gate covers independent same-file edits and add/add same-path quarantine convergence.

### Watcher suppression

- Programmatic writes insert/promote hash-based suppression entries.
- `Substrate::watch` uses the shared suppression ledger, suppressing self-events while still delivering external edits with different hashes.

## §17.7 acceptance mapping

All §17.7 criteria are mapped in `crates/memory-substrate/tests/spec_coverage_manifest.rs` and enforced by `release_certification_has_no_known_spec_coverage_gaps`.
