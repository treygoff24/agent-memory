# Codex Core Audit — 2026-06-04

Scope: `crates/memoryd`, `crates/memory-substrate`, `crates/memory-privacy`, `crates/memory-governance`, `crates/memory-merge-driver`.

Read-only audit categories: perf hot paths, security, dead code, and async lock-across-await. I read `CLAUDE.md` first and treated its seven critical invariants as hard gates. Findings marked `INVARIANT-CRITICAL` should be applied by a human, not auto-fixed as drive-by cleanup.

## Findings

### SEC-01 — blocker — INVARIANT-CRITICAL — `memory_reveal` persists unclassified reveal reasons to disk

- **File:line:** `crates/memoryd/src/handlers/memory_ops.rs:174-199`; `crates/memory-substrate/src/api.rs:1581-1582`; `crates/memory-substrate/src/events/log.rs:90-95`; `crates/memoryd/src/handlers/mod.rs:577-579`
- **Issue:** `reveal_response` validates only that `reason` is non-empty and <=512 chars, then calls `record_encrypted_content_revealed(memory_id, bounded(reason, ...))`. That reason is serialized into the JSONL event log as `EventKind::EncryptedContentRevealed { reason }`, and later surfaced verbatim in `event_kind_summary`. A caller can put a token, API key, email, phone number, or other secret/PII in the reveal reason and have it persisted to disk. This directly touches invariant 1: secrets must never persist to disk.
- **Proposed fix:** Treat reveal reasons like forget reasons or stricter: classify before persistence with `safe_plaintext_fragment` plus explicit secret/PII marker checks; either reject unsafe reasons or store a redacted/category-only reason (`[redacted]`, `user_requested_reveal`, etc.). Add a regression test that attempts a reveal reason containing `sk-...` and proves no raw secret appears in `events/*.jsonl`, event mirror rows, or notification summaries.

### SEC-02 — risk — merge-driver config command quotes paths unsafely

- **File:line:** `crates/memory-substrate/src/git/init.rs:29-38`
- **Issue:** `configure_merge_driver` writes the local Git merge driver as `format!("\"{}\" --base %O --ours %A --theirs %B --path %P", merge_driver_binary.display())`. Git merge-driver commands are shell-like command templates; a binary path containing quotes or shell metacharacters can break out of the intended argv and run unintended commands during merge. The path is local/operator-controlled, but this is still a command-injection footgun in repo setup.
- **Proposed fix:** Shell-quote the binary path with a tested POSIX quoting helper, or better, install/configure a small wrapper script with fixed argv and no interpolated shell metacharacters. Add tests for paths with spaces, quotes, `$()`, backticks, and semicolons.

### SEC-03 — risk — search `include_body` returns full plaintext bodies despite bounded-preview contract

- **File:line:** `crates/memoryd/src/handlers/memory_ops.rs:90-109`; contrast bounded `memory_get` at `crates/memoryd/src/handlers/memory_ops.rs:130-135`
- **Issue:** When `include_body` is true, search reads each hit envelope and returns the entire plaintext body. `memory_get` truncates previews to `GET_BODY_MAX`, and the search guidance says search returns bounded chunks, but the body field bypasses that bound. This is both a PII over-disclosure path for broad search queries and an avoidable multi-hit body materialization cost.
- **Proposed fix:** Either remove `include_body` from search or apply the same `bounded_with_truncation` policy used by `memory_get`; prefer returning only bounded snippets and requiring explicit `memory_get`/`memory_reveal` for body access. Add tests for a large plaintext body and for plaintext containing classifier-sensitive canaries that should not be bulk-returned through search.

### SEC-04 — nit — existing socket parent directories are accepted without permission checks

- **File:line:** `crates/memoryd/src/server.rs:160-187`; `crates/memoryd/src/server.rs:393-410`
- **Issue:** The daemon hardens the socket file to `0600` and newly-created parent directories to `0700`, but if the parent directory already exists, `prepare_socket_parent` accepts it as-is. If an operator passes a socket path under a group/world-writable non-sticky directory, another local user may be able to replace or remove the path and cause local DoS or confusing connection behavior. This is not an auth bypass by itself, but it weakens the local-socket trust boundary.
- **Proposed fix:** On Unix, inspect owner and mode for existing parent directories; warn or refuse when the directory is group/world-writable unless it is an explicitly allowed sticky directory. Prefer default runtime paths that are `0700` and document the override risk.

### SEC-05 — risk — claim-lock conflict checks fail open when the entity index is unavailable

- **File:line:** `crates/memoryd/src/handlers/peer.rs:175-183`
- **Issue:** `conflicting_claim_locks_for_heartbeat` batches entity lookup for active candidate locks, but if `substrate.entities_for_memories` fails, it logs a warning and returns `Vec::new()`. The inline comment correctly notes that reporting "no conflicts" is fail-open: a peer heartbeat proceeds without conflict warnings exactly when the coordination index needed to compute intersections is unavailable.
- **Proposed fix:** Return a typed degradation/error to the heartbeat response, or conservatively report candidate locks as unresolved conflicts when entity lookup fails. Do not collapse index failure into the same result as "no conflicts"; add a regression test that injects an entity-index failure with active candidate locks and verifies the response surfaces degraded/conflict state.

### PERF-01 — risk — every `Substrate::open` performs a full memory reindex and full event-log mirror rebuild

- **File:line:** `crates/memory-substrate/src/api.rs:1585-1625`; `crates/memory-substrate/src/api.rs:1790-1813`; `crates/memory-substrate/src/api.rs:2262-2323`; `crates/memory-substrate/src/tree/layout.rs:223-236`
- **Issue:** Opening the substrate always walks every canonical markdown file, clears/rebuilds the plaintext index, reconciles embedding jobs, reads all `events/*.jsonl`, sorts all events, and rebuilds the event-log mirror. This makes daemon startup/open O(number of memories + total event-log size) even when the derived SQLite state is current.
- **Proposed fix:** Add an incremental open path keyed by stored `file_hash`, `file_mtime_ns`, pending repair markers, and event-log high-water marks `(device, seq/event_id)`. Keep full reindex/rebuild for explicit `doctor`/repair flows and for verified index-corruption states. Preserve the embedding triple identity invariant when deciding whether vector metadata is current.

### PERF-02 — risk — id resolution falls back to full repo walks on stale/missing index rows

- **File:line:** `crates/memory-substrate/src/api.rs:130-167`; `crates/memory-substrate/src/api.rs:281-300`; `crates/memory-substrate/src/tree/layout.rs:223-236`
- **Issue:** `read_memory_with_hash` and `resolve_memory_id_to_path` prefer the index but fall back to `relative_memory_paths`, which walks the entire repo, when the index misses, is stale, or lookup fails. Because open currently hydrates the index, most misses should be authoritative; the fallback turns a normal not-found, stale row, or index error into an O(tree) path.
- **Proposed fix:** Distinguish `IndexHit`, `IndexMissAfterHydration`, and `IndexUnavailable/Unhydrated`. Only disk-walk in explicit repair/unhydrated modes; for stale hits, read exactly the candidate path and queue/index-repair rather than scanning every markdown file.

### PERF-03 — risk — governance writes load and parse every active plaintext memory

- **File:line:** `crates/memoryd/src/handlers/governance.rs:46`; `crates/memoryd/src/handlers/governance.rs:842-864`
- **Issue:** Every governance write calls `active_memory_summaries`, which walks all canonical memory paths, reads/parses each envelope, filters to active plaintext memories, and clones the full body into `ExistingMemorySummary`. Duplicate and contradiction checks therefore scale with the whole repo rather than candidate-relevant rows.
- **Proposed fix:** Add an index-backed active-summary query for id, namespace, canonical hashes, entity ids, and only the fields needed by the governance engine. Use stored body/canonical-claim hashes for exact duplicate checks and FTS/vector/entity prefilters for top-k candidate selection before hydrating full bodies.

### PERF-04 — risk — review queue materializes every envelope before truncating

- **File:line:** `crates/memoryd/src/handlers/review.rs:36-55`
- **Issue:** `review_queue_response` walks every canonical memory, reads every envelope, builds a full `ReviewQueue`, emits threshold notifications, and only then truncates to the caller's limit. Review state, status, confirmation flags, and summary are already mirrored in the index/frontmatter columns, so this endpoint is full-scan bound as the repo grows.
- **Proposed fix:** Add an index query for review candidates (`review_state`, `requires_user_confirmation`, `status`, `human_review_required`) with count + limit support. Hydrate full files only when applying a review decision, not for queue display.

### PERF-05 — risk — startup peer recall scans all active/pinned rows across namespaces

- **File:line:** `crates/memoryd/src/recall/startup.rs:261-287`; `crates/memoryd/src/recall/startup.rs:290-300`
- **Issue:** `startup_peer_candidate_rows` queries the recall index with `namespace_prefix: None` for all active/pinned rows, then filters session scope and peer-write status in Rust. At coordination levels that enable peer recall, startup work becomes global-scan bound even when the current session has a small namespace scope.
- **Proposed fix:** Query per namespace in `session_binding.namespaces_in_scope`, or extend `RecallIndexQuery` to accept multiple namespace/scope predicates. Push peer-write/source-device predicates into SQL where possible, then hydrate entities only for the already-filtered candidate set.

### PERF-06 — nit — trust artifact builder opens a second SQLite connection per request

- **File:line:** `crates/memoryd/src/trust_artifact.rs:154-164`
- **Issue:** `TrustArtifactBuilder::build` reads the memory via `Substrate`, then opens `runtime/index.sqlite` with a fresh `rusqlite::Connection::open` for mirror queries. This pays connection setup on every trust-artifact request, bypasses the existing `open_index` setup/pragmas/extension path, and adds another SQLite handle racing the long-lived substrate index connection.
- **Proposed fix:** Add a bounded `Substrate::with_index`/read-only index accessor for trust-artifact queries, or centralize connection creation through `open_index`/read-only URI with the same pragmas and sqlite-vec registration.

### LOCK-01 — risk — reality-check mutation mutex is held across async substrate/governance IO

- **File:line:** `crates/memoryd/src/handlers/reality_check.rs:42-45`; awaited work at `crates/memoryd/src/handlers/reality_check.rs:59-72`, `crates/memoryd/src/handlers/reality_check.rs:117-122`, `crates/memoryd/src/handlers/reality_check.rs:203-222`, and `crates/memoryd/src/handlers/reality_check.rs:313-333`
- **Issue:** `reality_check_response` acquires `state.reality_check_lock.lock().await` and then awaits the whole mutating response path. That path can run sessions, confirm/correct items, supersede through governance, read/write substrate state, and update encrypted/plaintext metadata. The mutex therefore serializes all mutations across long IO and creates head-of-line blocking; it also risks future deadlock if any awaited path re-enters a reality-check mutation.
- **Proposed fix:** Move serialization into small critical sections or an actor/queue. Load/advance session state under the lock, drop it before substrate/governance IO, then reacquire for version-checked state commit. If broad serialization is intentionally required, document it and add timeout/non-reentrant safeguards.

### DEAD-01 — nit — reserved merge diagnostic enum variants are dead behind `allow(dead_code)`

- **File:line:** `crates/memory-substrate/src/merge/field_rules.rs:27-39`
- **Issue:** `DiagnosticBucket::{LifecycleNotes, EvidenceNearDuplicates}` are explicitly reserved for future per-field diagnostics and suppressed with `#[allow(dead_code)]`; current orchestration populates those buckets through other direct vectors. This is small, but it keeps unused production variants alive and broadens the allowed dead-code surface.
- **Proposed fix:** Remove the unused variants until they are wired, or add a targeted tracking issue/test that exercises them through the actual merge diagnostic path. If they must remain, narrow the allow to the specific variants and document the planned consumer.

## Checked non-findings

- The privacy classifier's encryption-disabled path looked suspicious, but plaintext writes still pass `privacy.tier.classification()`, and `Substrate::enforce_plaintext_classification` rejects `RequiresEncryption` and `Secret` classifications before plaintext persistence.
- Merge schema-version gating uses `MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION` in the actual merge path; the remaining `schema_version: 1` hits observed in this pass were fixtures or config seeds, not a second merge-driver schema gate.
- Dynamic SQL around sqlite-vec table names is not directly user-interpolated: table names are deterministic hashes of the embedding triple.
