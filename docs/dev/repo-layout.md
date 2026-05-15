# Repository layout

Snapshot of what's on disk for Streams A–I as of 2026-04-30 (refreshed with Stream F/G/H/I additions). For current behavior of each crate, consult the live spec/API doc — not this inventory. Spec/plan pairs and current status live in `CLAUDE.md`.

## Crates

- **`crates/memory-substrate/`** (Stream A) — public API (`api.rs` — Stream D added an `allow_encrypted_namespace` flag to `AtomicWrite` so plaintext writes still refuse `encrypted/` while supersede can update encrypted records), model + error taxonomy, frontmatter (parse/validate/serialize/defaults/schema), tree (layout/validate), config, IDs (sequence/repair), events (log/framing/sequence/recovery), index (schema/migrations/chunking/query — query.rs sanitizes FTS5 input, sqlite-vec adapter), git (init/adopt/preflight/commit/sync/command), watcher (subscription/filter/suppression), merge (three_way/quarantine), runtime (reconcile/faults), bench harness binary. ~30 integration test files including `spec_coverage_manifest.rs`, `crash_matrix.rs`, `startup_reconciliation.rs`, `vector_lifecycle.rs`, `fts_query_sanitization.rs`.
- **`crates/memoryd/`** (Streams B/C/D) — `cli.rs`, `client.rs`, `handlers.rs`, `main.rs`, `mcp.rs`, `mcp_stdio.rs`, `protocol.rs`, `server.rs`, `workers.rs`. Daemon serves newline-delimited JSON over a Unix socket with a 64 KiB frame cap, idle-frame timeout, watch::Receiver-driven graceful shutdown, and (Stream D) owner-only socket chmod after bind; `serve_substrate_with(socket, substrate, options, shutdown_rx)` is the supervised entry; SIGINT/SIGTERM wired in `main`. `memoryd mcp --socket <path>` is the launchable stdio MCP server: stdout is JSON-RPC protocol frames, stderr is logs, `tools/list` reflects `mcp::manifest()`, and `tools/call` routes through the daemon forwarder. The MCP forwarder declares nine agent-facing tools (Search/Get/Startup/Note/Observe plus governed Write/Supersede/Forget/Reveal); admin commands (`privacy`, `privacy-filter`, `device`, `review`, `web`, `reality-check`, peer admin) are CLI/socket-only and explicitly rejected from MCP. Stream D's `write_privacy_memory` runs `DeterministicPrivacyClassifier` over body+title+summary+source_ref+tags+privacy_descriptors before any disk effect; high-risk secrets refuse, PII encrypts at rest, and safe descriptors may be indexed for encrypted records. Startup recall is implemented for Stream E via daemon protocol, MCP forwarding, recall CLI hook commands, and additive status counters.
- **`crates/memory-governance/`** (Stream C) — deterministic governance decisions, policy loader/built-in policies, grounding verification, contradiction/tombstone/supersession helpers, and review queue projection.
- **`crates/memory-privacy/`** (Stream D) — Layer 1 deterministic classifier (`classifier.rs`, `regex.rs`, `entropy.rs`), monotonic tier plus storage-action policy (`policy.rs`, `decision.rs`), `PrivacyEncryptor` over `age` X25519 (`crypto.rs`), file-backed key provider with 0700/0600 hardening + symlink rejection (`keys.rs`), in-session `MaskingSession` (`masking.rs`), optional Privacy Filter trait with disabled-by-default + fixture providers (`privacy_filter.rs`). Six contract test files in `tests/`; full crate ~1k LoC. URL/date labels detect without encrypting by default; phone/email/address labels encrypt at rest without tier elevation; SSN/Luhn-valid card/credential-like labels refuse.
- **`crates/memory-merge-driver/`** — CLI + `tests/merge_driver_cli.rs`.
- **`crates/memory-test-support/`** — `convergence.rs`, `perf.rs`, `bin/rust_boundary_check.rs`.

## Other top-level dirs

- **`fuzz/`** — `merge_driver` and `merge_swap_convergence` targets.
- **`scripts/`** — `check.sh` (full release gate), `two-clone-convergence.sh`, `durability-probe-gate.sh`, `bench-gate.sh`, `bench-regression-check.sh`, plus task-worktree helpers.
- **`bench/baseline.darwin-arm64.json`** — re-captured 2026-04-27 under realistic system load (post-query.rs perf fixes), with `captured_at`/`captured_method` provenance fields. Codex's original baseline was set on an idle machine and didn't survive contact with normal load (3x slower SQLite query p95s). **`bench/baseline.linux-x86_64.json`** — still `runs: 0` placeholder; first-release bootstrap path emits a `.proposed` file rather than failing. Per spec §17.6/§18.9, baselines are only updated by explicit human commits; the bench harness never overwrites them.
- **`.github/workflows/`** — `stream-a-ci.yml`, `stream-a-fuzz.yml`, `stream-a-perf.yml`.
- **`.dylint/custom_lints/`**, `.oxlintrc.json`, `.oxfmtrc.json`, `clippy.toml` — installed from agentlinters SHA `91446bb`.
- **`modules/stream-a-*.spec.yml`** — specgate module manifests.

## Docs

- **`docs/api/stream-a-public-api.md`**, **`docs/dev/stream-a-architecture.md`**, **`docs/dev/stream-a-test-matrix.md`** — public surface, architecture, and test matrix references.
- **`docs/reviews/`** — `stream-a-final-review.md`, plus the `2026-04-25-buildout/` lane reviews + adversarial pass + SUMMARY, plus per-domain final review summaries (performance, security, test-coverage), plus the four Stream D reviews (Codex's correctness/performance/security + Claude's adversarial pass).
- **`docs/runbooks/`** — `operator-repair.md`, `privacy-leak-response-placeholder.md`.
- **`docs/specs/stream-e-passive-recall-v0.{1,2,3,4,5}.md`** — Stream E spec history. v0.5 is the live contract; the others are kept on disk per the versioning convention. Each version's "Revision goal" block at the top documents what changed and why. **`docs/plans/2026-04-30-stream-e-passive-recall.md`** — Stream E implementation plan, shipped from plan revision v0.4 (see the file's Plan Revision History block). For current behavior, use the v0.5 spec plus `docs/api/stream-e-passive-recall-api.md`.

## Gate taxonomy

`pnpm run check:fast` for inner-loop validation, `pnpm run check:local` for local confidence before claiming a milestone, and `pnpm run check:full` / `bash scripts/check.sh` for full release validation (with `BENCH_PROFILE=darwin-arm64` on Trey's machine when needed).
