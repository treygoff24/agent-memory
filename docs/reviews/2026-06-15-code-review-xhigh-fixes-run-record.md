# Run record — xhigh code-review fixes (Claude orchestrator × Cursor Composer)

**Date:** 2026-06-15. **Branch:** `refactor/desloppify-hardening-elegance`. **Commit:** `e0957ba`. **Process:** `/code-review xhigh` over `git diff main...HEAD` (the desloppify + security/perf hardening branch: 12 commits, 114 files) surfaced candidates via 10 finder angles → per-candidate verifiers → a gap sweep. Claude orchestrated and code-reviewed; both implementation waves were delegated to Cursor Composer 2.5 via `delegate cursor work --prompt-file` (real workspace, no isolation), each wave a bounded per-finding work order with owned-file lists and verification steps. Claude reviewed every diff on disk, ran the gates, and finished the supply-chain item by hand. Correctness gates green; Phase 3 (convergence/durability/bench) deliberately deferred to integration.

## What shipped — 15 findings closed

**Wave 1 (`cursor-15`) — substrate + embedding internals**

| # | File | Fix |
|---|---|---|
| 2 | `memory-substrate/src/api.rs` | `update_embedding`/`update_embeddings_batch` moved onto `spawn_blocking` — writes off the tokio worker, matching the read-side change; symmetry the perf commit had missed |
| 3 | `memoryd/src/embedding/worker.rs` | Batch-embed failure now falls back to per-chunk re-embed (shared `write_and_record_embedded_jobs`) so one bad chunk no longer charges the whole batch's retry budget |
| 11 | `memoryd/src/embedding/worker.rs` | Release-erased `debug_assert` on vector count → real length guard (no silent `zip` truncation) |
| 7 / 13 | `memory-substrate/src/index/mod.rs`, `memoryd/src/util.rs` | IN-clause helpers made `pub` + generic (`AsRef<str>`) in the substrate as the single source; `util.rs` re-exports; pad uses `saturating_sub` |
| 12 | `memory-substrate/src/index/query.rs` | `chunk_texts_by_rowid` gets its own `CHUNK_TEXT_FETCH_BATCH` const instead of borrowing the mirror-health constant |
| 8 | `memory-substrate/src/markdown/atomic.rs` | Extracted `read_memory_bytes_checked` so the path-containment guard lives in one place |

**Wave 2 (`cursor-16`) — daemon / web / supply-chain**

| # | File | Fix |
|---|---|---|
| 1 | `memoryd-web/tests/csrf.rs` | New 14-route `PROTECTED_GET_ROUTES` (adds `/api/recall-hits`, `/api/search`) drives the CSRF-gating tests; "mirror server.rs" comment |
| 5 | `memoryd/src/handlers/status.rs` | Single `count_memories_by_status` scan threaded into both helpers (was scanning twice per poll) |
| 6 | `memoryd/src/reality_check/scoring.rs` | Reuses the live index via `index_handle()` + `_conn` variants; deleted now-dead `open_runtime_index`/`open_runtime_index_at`/`recall_usage_for`/`distinct_sources_for` |
| 10 | `memoryd/src/recall/startup.rs` | `catch_unwind` inside the blocking task — hydration panics degrade to structural-only ranking instead of aborting recall |
| 9 | `memoryd/src/notifications/os.rs` | `--` terminator on the notify-send arm |
| 14 | `memoryd-web/tests/csrf.rs` | Bootstrap test now asserts the SSE-stream token exemption it claimed to cover |
| 15 | `memoryd/src/dynamics/usage.rs` | COUNT casts saturating (`u32::try_from(...).unwrap_or(u32::MAX)`) |
| 4 | `fuzz/Cargo.lock`, `scripts/cargo-audit-gate.sh` | Regenerated fuzz lock off deprecated `serde_yaml 0.9.34` → `serde_yaml_ng 0.10`; CVE gate now scans both lockfiles |

## Refuted in review (correctly NOT changed)

- **auth.rs "no-Host loopback bypass"** — the fail-open absent-Host path is unreachable behind the loopback-only bind and is intentionally tested (`host_guard.rs` `test_missing_host_is_allowed_for_loopback_clients`).
- **reconcile.rs "quarantine drift"** — `file_hash`, `status`, `trust_level` are written by one atomic `INSERT…ON CONFLICT`, so a matching-hash/stale-status window cannot exist.
- **SSE `/api/notifications/stream` un-gated read** — real but low: deliberate `EventSource` bootstrap exemption, payload is fixed-template metadata (counts/timestamps), not memory text. Addressed by documenting + test (#14) rather than gating.

## Verification

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace --features memoryd-web/dev-fixtures,memoryd/dev-fixtures` (**1722 passed, 0 failed, 1 ignored**, 254 binaries) all green. `scripts/cargo-audit-gate.sh` clean on both `Cargo.lock` (659 deps) and `fuzz/Cargo.lock` (177 deps). Targeted Wave tests green per wave. fmt collapsed Cursor's multi-line forms to the repo's wider `max_width` style.

## For the next loop

1. **Phase 3 not run** — `check.sh` two-clone convergence, durability matrix, and bench-regression-vs-baseline were skipped (perf-baseline-sensitive; thermal noise in a contended background job). Run on the integrated trunk before merge.
2. **Stale spec ref** — `docs/specs/memory-dynamics-v0.1.md:82` still names the deleted `recall_usage_for`/`distinct_sources_for` and describes the pre-refactor `score_memories_at` inline-Index path. Left untouched (versioned spec; no mutate-without-ask). Refresh if/when that spec is next revised.
3. Finder/verifier transcripts and the saved diff are in the session job dir (ephemeral); this record is the durable summary.
