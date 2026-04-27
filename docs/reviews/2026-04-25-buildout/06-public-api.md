# Clean-code review — public api / model / error

Reviewer: reviewer-api
Files reviewed:

- `crates/memory-substrate/src/lib.rs`
- `crates/memory-substrate/src/api.rs`
- `crates/memory-substrate/src/model.rs`
- `crates/memory-substrate/src/error.rs`
- `crates/memory-substrate/src/bin/stream_a_bench.rs` (CLI/bin under crate; no `bin/main.rs`)

Spec anchor: `docs/specs/stream-a-core-substrate-v1.1.md` (v1.1).

The five **critical invariants** the brief flagged are all satisfied:
classification is mandatory on every write request (model.rs:405, model.rs:424); `SecretRefused`,
`EncryptionRequired`, and `ClassificationSensitivityMismatch` are distinct typed variants
(error.rs:97–104) and reachable (api.rs:679–693, api.rs:225–230); embedding triple errors
are typed (error.rs:201–214); and `Roots` does not expose any device id (model.rs:13–18).
The blockers below are the next layer in: where the implementation diverges from the
public-API contract Streams B–I are expected to live with.

## Blockers

- **api.rs:1681 / model.rs:349 — `read_memory` / `read_path` return `Memory` instead of `MemoryEnvelope`.** Spec §16.2 says: `read_memory(&self, id: &MemoryId) -> Result<MemoryEnvelope, ReadError>` with `MemoryContent { Plaintext, Ciphertext { bytes, encryption }, MetadataOnly }`. Implementation returns the raw `Memory` struct with a single `body: String`. There is no way for any caller (Stream B/E) to distinguish a plaintext memory from an encrypted-metadata-only memory or a ciphertext envelope. This is the contract Stream E will build recall block assembly on top of. **Fix:** introduce `MemoryEnvelope { metadata: Memory, content: MemoryContent }` and migrate `read_memory`/`read_path` to return it. The `MemoryContent::Ciphertext` variant carries `EncryptionEnvelope` so encrypted callers can route through Stream D without Stream A knowing how to decrypt.

- **model.rs:621–651 — `QueryResult` and `ChunkResult` are too thin to satisfy spec §10.4 / §16.4.** Spec §10.4 mandates `MemoryHit.body_indexability == MetadataOnly` (so callers can tell a metadata-only encrypted memory from a fully-indexed one) and "per-hit `score_breakdown` inputs, not final policy ranking." Implementation gives `QueryResult { id, path, summary }` and `ChunkResult { memory_id, text, score: f64 }`. There is no `body_indexability`, no `content_state`, no `score_breakdown`. Stream E cannot do hybrid-result assembly without inventing types Stream A should have shipped. **Fix:** rename to `MemoryHit`/`ChunkHit` per spec, add `body_indexability: BodyIndexability { Full | MetadataOnly | None }`, replace scalar `score` with `score_breakdown: ScoreBreakdown { fts: Option<f64>, vector: Option<f64>, distance: Option<f64> }`.

- **api.rs:561 — `drop_embedding_model` returns `Result<usize, VectorError>`; spec §16.4 promises `Result<DropTripleReport, VectorError>`.** A `usize` count is information-lossy. Spec §10.2.2 step 3 says drop-triple emits a structured event and verifies "vector queries against triple A return `VectorError::UnknownEmbeddingTriple`." `DropTripleReport { vectors_removed, meta_rows_removed, pending_jobs_dropped, table_dropped }` is what callers need. **Fix:** add `DropTripleReport` and return it.

- **api.rs:604 — `watch` returns `WatchSubscription` directly, but the spec-required `WatchSubscription` API surface is missing.** Spec §16.5 binds three things to the type: `events(&mut self) -> impl Stream<Item = Result<FileEvent, WatchError>>`, `unsubscribe(self)`, and `rescan_now(&self) -> Result<(), WatchError>`. The crate's `WatchSubscription` is re-exported from `crate::watcher` but its public methods (and the spec §11.4 acceptance signal "WatchSubscription outlives Substrate") cannot be verified from this slice — confirm the watcher type implements all three and that dropping `Substrate` does not stop event delivery. If any of those three are absent, that is a blocker.

- **model.rs:344–346 — `Frontmatter::extras` is `#[serde(skip)]`.** Spec §6.2: "Unknown future fields are preserved in `_extras` by the parser and re-emitted after known fields." With `#[serde(skip)]`, unknown fields parsed into `extras` will _not_ round-trip through serde — they will be silently dropped on serialize. This breaks the spec §6.13 acceptance signal "Unknown v1.x optional fields parse, warn, preserve, and reserialize." **Fix:** remove `#[serde(skip)]` and either (a) flatten `extras` with `#[serde(flatten)]` or (b) drive the canonical YAML serializer manually with explicit extras-after-known-fields ordering. Either way, write a round-trip test that proves an unknown field survives.

- **model.rs:316–336 — `entities`, `evidence`, `tombstone_events` are typed as `Vec<serde_json::Value>`.** Spec §6.4–§6.5 mandates structured types: `Evidence { id, quote, quote_norm_hash, ref, weight, observed_at, source }`, tombstone event objects with typed `actor`, `reason: enum`, `prior_status: MemoryStatus`. Today, every site that touches these — merge driver, validator, frontmatter writer, the bench fixture — has to hand-marshal `serde_json::Value`. This is exactly the "string-bag dumping ground" anti-pattern; only worse, because it is a value-bag. **Fix:** define `Evidence`, `Entity`, `TombstoneEvent` structs in `model.rs` and migrate the fields. The runtime cost is zero; the type-safety win is large.

- **api.rs:78–88 — `read_memory` does a full linear scan over `relative_memory_paths` and disk-reads every file to find one by ID.** This is O(n) filesystem reads to satisfy a query the SQLite index can answer in O(1). Beyond performance, it means the public API's behavior diverges from the index (a memory not yet reindexed but on disk is found; one in the index but not on disk is not found). Spec §10.5 invariant 1 says `memories.id == frontmatter_json.id` — the index is supposed to be the resolver. **Fix:** resolve `MemoryId → RepoPath` through the SQLite index, then fall back to file read for the body. (Yes, this also means `read_memory` becomes the path the spec §10.4 metadata-only case correctly routes through.)

- **events/log.rs:30–47 — only 8 event kinds implemented; spec §12.2 lists ~24.** Missing: `WriteStarted`, `WriteIndexed`, `WriteEventAppendFailed`, `WriteRefused`, `Deleted`, `Superseded`, `IndexUpdated`, `IndexFailed`, `VectorReconciled`, `EmbeddingJobEnqueued`, `EventLogRecovered`, `MergeQuarantined`, `PendingIndexReplayed`, `PendingEventReplayed`, `GitCommitted`, `GitFetched`, `WatcherSuppressed`, `ReconciliationRepaired`. Several are referenced as acceptance signals (e.g. spec §10.6 "an `EmbeddingModelChanged` event was emitted with the correct `chunks_requeued` count" — present; "`VectorReconciled`" — absent; spec §13.5 step 11 "Append `GitFetched`" — absent). Also note `EncryptedWriteCommitted` is in the impl but not in spec §12.2 — diverges in the other direction. **Fix:** reconcile the event-kind enum to the spec list. If the spec is wrong (some of these are appropriate to fold), bump the spec; do not let impl drift be the resolution.

- **api.rs:99–104, 223–230, 680–691 — write refusals never emit `WriteRefused` events.** Spec §8.7 step 6: "Stream A logs the classification and the decision in the `WriteCommitted`/`WriteRefused` event payload so audit can confirm Stream D made a positive call on every write." The implementation returns `Err(WriteFailure { kind: SecretRefused | EncryptionRequired | ClassificationSensitivityMismatch | StaleBase })` and never appends an audit event for the refusal. This breaks the "audit can confirm Stream D made a positive call on every write" guarantee — refusals are invisible to the audit log. **Fix:** add `EventKind::WriteRefused { id, kind: WriteFailureKind, classification }` and emit it on every refusal path before returning the error. Refusal events must not require disk-side write durability since there is no canonical file commit; appending to the per-device event log is sufficient.

- **api.rs:96, 219, 384, 517 — every `Substrate` method is `async fn` but the body uses `std::sync::Mutex` and blocking I/O.** Spec §16.5: "All public `async` methods that perform filesystem, SQLite, git, vector, or network work must run blocking sections on Stream A's configured blocking executor or single index thread. The public API must not hide blocking/network behavior behind cheap-looking sync calls." The implementation has zero `spawn_blocking` calls, locks `std::sync::Mutex` (which blocks the runtime if contended), and calls `std::fs`/`std::process::Command`/SQLite directly inside `async fn`. On any tokio multi-thread runtime, a contended `index.lock()` parks a worker thread; on the current-thread runtime, it deadlocks the executor. Spec §16.7 also requires "Async write/index/git/watch APIs can be cancelled without corrupting repo/index/event state" — a half-completed `write_memory` cancelled at any `.await` between durable rename and event append leaves the substrate in an undocumented state because there are no `.await` points inside the actual mutation. **Fix:** either (a) make these methods sync and document the blocking contract explicitly (spec already permits "Stream A itself may be synchronous internally"), or (b) wrap the bodies in `tokio::task::spawn_blocking` with a configured pool. Pick one and be consistent. Right now they look async but are not.

- **api.rs:556, 564, 570 — `update_embedding`, `drop_embedding_model`, `vector_count` map a poisoned `index` mutex to `VectorError::UnknownEmbeddingTriple`.** Lock poisoning is not an unknown triple. This is an error-classification bug that will mislead both callers and operators. **Fix:** add `VectorError::IndexUnavailable(String)` (or surface poisoning at a higher level), and reserve `UnknownEmbeddingTriple` for the actual condition. Same issue at api.rs:117 / api.rs:421 mapping poisoning to `WriteFailureKind::Io(String)`.

- **model.rs:511–520 — `id_type!` macro implements `From<&str>` and `From<String>` for `MemoryId`, `RepoPath`, `OperationId`, `EventId`, `Sha256` with no validation.** The spec is explicit that `MemoryId` matches `^mem_\d{8}_[0-9a-f]{16}_\d{6}$` (§5.3) and `RepoPath` has a real `try_new` (model.rs:531). Yet `MemoryId::new("not_a_memory_id")` and `RepoPath::new("../../../etc/passwd")` both compile and produce values that flow through the public API. The `try_new` on `RepoPath` is a polite suggestion no caller is forced to use; the `From` impls let any `&str` collapse into a `RepoPath` silently. **Fix:** remove the unchecked `From` impls. Make `MemoryId::try_new` validate the regex and `MemoryId::new` either go away or take a `&'static str` validated at compile time. Same for `RepoPath` — keep `try_new`, drop `new` from the public surface.

## Risks

- **api.rs (whole file) — three near-identical 100+ LOC write paths (`write_memory`, `write_encrypted`, `tombstone_memory`) duplicate the same six-step "index → fall back to pending queue → fall back to startup marker → fall back to operator-required" cascade.** Each cascade is open-coded inline, four times for index failures and four times for event-append failures. This is the highest-risk maintenance hazard in the file: any future change to the repair contract has to be made in ~8 places, and a reviewer cannot tell at a glance whether they all match. **Fix:** extract two helpers — `commit_index_or_repair(&self, op_id, build_pending) -> Result<(), WriteFailure>` and `commit_event_or_repair(&self, op_id, event_kind, build_pending) -> WriteOutcome` — with shared knowledge of the fallback ladder. Each write path becomes ~30 LOC of orchestration calling those helpers. The bug surface drops by ~60%.

- **api.rs:124, 154, 394, 647, 769 — hardcoded fallback path `"agent/patterns/{id}.md"` appears five times.** When a write request omits `memory.path`, the implementation invents a path under `agent/patterns/` regardless of the memory's `type` and `scope`. A `MemoryType::Decision` with `Scope::Project` will silently land in `agent/patterns/`, violating spec §5.1's prescribed layout. **Fix:** make `Memory.path` non-optional, or add a single `default_repo_path(&Frontmatter) -> RepoPath` helper that respects `type`/`scope`/`namespace` and use it everywhere. Better: refuse the write if `path` is None and the spec layout cannot derive it deterministically.

- **error.rs:124–129 — `WriteFailureKind::Validation(String)` and `WriteFailureKind::Io(String)` are string-bag variants.** The brief flagged this exact anti-pattern. Callers cannot pattern-match on the underlying validation rule or IO kind; they have to string-sniff. `Validation` should wrap `ValidationError` (already a typed enum at error.rs:133); `Io` should expose at minimum `std::io::ErrorKind`. **Fix:** `Validation(ValidationError)`, `Io { kind: std::io::ErrorKind, context: &'static str }`.

- **error.rs:39–41 — `SubstrateError::Io { path: String, source: std::io::Error }` uses `path: String` not `PathBuf` or `RepoPath`.** Lossy. A path with non-UTF-8 bytes (real on macOS resource forks, possible on Linux) becomes `"<lossy>"`. **Fix:** `path: PathBuf`.

- **api.rs:783–820 — `parse_device_id` and `write_local_device_id` are hand-rolled YAML.** `parse_device_id` matches both `device_id:` and bare `id:` keys (line 802), so a `local-device.yaml` with `paths:\n  id: …` would be misread as the device id. `write_local_device_id` uses `std::fs::write` (non-atomic) — a crash mid-write leaves a truncated YAML, and the next `Substrate::open` mints a new device id, breaking the per-device shard contract. **Fix:** use the existing `serde_yaml` (already in dependency tree per `frontmatter`) for both, and reuse `markdown::atomic_write` (or its byte equivalent) for the write.

- **model.rs:382–385 — `IndexProjection { safe_body: Option<String> }` with one field is anemic.** Spec §8.4 implies a richer "safe projection" surface (masked summary, masked entities). Today the type carries only `safe_body`. Either spec the projection more precisely (masked summary, entities) or rename to `SafeBody(Option<String>)` so the limitation is honest.

- **api.rs:399 (and `WriteRequest`) — `WriteRequest::index_projection: Option<IndexProjection>` exists for plaintext writes but is never used.** Plaintext writes don't consume a projection — only `EncryptedWriteRequest::safe_index_projection` does. Field is dead. **Fix:** delete it from `WriteRequest`.

- **model.rs:373–378 — `EventContext { actor: Option<String>, reason: Option<String> }` is two free-form strings.** Spec §6.4 makes actor structured (`Author { kind, harness, session_id, ...}`). The substrate stores actor as an opaque string blob in the event log via this struct — no validation, no schema. **Fix:** `EventContext { actor: Author, reason: Option<String> }` and let validation reject malformed authors at write time.

- **model.rs:67–78 — `Sensitivity` derives `Ord, PartialOrd` and the variant declaration order is `Public, Internal, Confidential, Personal`, which makes `Personal > Confidential > Internal > Public` for the spec §14.4 "max sensitivity wins" merge rule.** The spec's ordering is correct, the impl agrees by accident of declaration order, but there is no test or comment locking that in. A future renaming or alphabetization in `model.rs` silently breaks the merge driver. **Fix:** add a `const SENSITIVITY_RANK_TEST: () = …` build assert, or write the comparator explicitly and remove the derive. A single short comment "variant order is the merge precedence; do not reorder" is the minimum.

- **model.rs:610–618 — `MemoryQuery { id, tag, include_metadata_only }` is far thinner than spec §10.4 demands.** Spec lists "by namespace/scope/status/type/sensitivity/time" filters plus chunk FTS plus vector. Today only `id` and `tag` are pluggable. Stream B/E will need namespace, scope, status, type, sensitivity, updated-at range, plus pagination. **Fix:** flesh out the query struct now, before downstream code calcifies around the thin shape. `MemoryQuery` becomes a builder or struct with `Option<>` filters.

- **model.rs:632–640 — `ChunkQuery { text, triple, vector }` lets callers construct invalid combinations.** `text + triple + None vector`, `triple + vector + None text` — neither is a valid hybrid. The struct shape allows wrong states. **Fix:** make `ChunkQuery` an enum: `Fts { text, filters }`, `Vector { triple, vector, filters }`, `Hybrid { text, triple, vector, filters }`. The compiler then forces callers into a valid shape.

- **api.rs:608–611 — `events()` returns `std::io::Result<Vec<Event>>`.** Spec §16.5: `read_events(&self, query: EventQuery) -> Result<impl Stream<Item = Result<Event, EventReadError>>, EventError>`. Loading the entire event history into memory is fine for the current toy size but is not the contract. **Fix:** swap to a streaming reader (the file is JSONL — well-suited) and accept an `EventQuery` filter. `EventReadError` distinguishes IO from corrupt-line.

- **error.rs:85–91 — `WriteFailure`'s `Display` impl renders only `{kind}`, not the `outcome`.** Operators reading log lines lose the critical "was the write committed" bit unless they happen to format with `{:?}`. Spec §16.6 anchors callers' ability to "distinguish recoverable committed states from non-committed failures" — losing committed state in `Display` undermines that. **Fix:** include `committed=true|false` in the `Display` string.

- **error.rs:248–262 — `MergeError::Parse(String)` is a string bag.** The merge driver is the most adversarial code path in Stream A; callers (Stream B's diagnostics, the merge-driver binary itself) need to discriminate "yaml parse" from "frontmatter delimiters absent" from "schema conflict." The spec already names these conditions. **Fix:** `MergeError::Parse { side: MergeSide { Base, Ours, Theirs }, source: serde_yaml::Error }` and similar for the related conditions.

## Nits

- **lib.rs:23 — `pub const STREAM_A_SPEC_VERSION: &str = "1.1"`** is a fine forward-compat tag, but it is a string not a `(major, minor)` tuple. Next time someone wants to gate behavior on `>= 1.1`, they will be string-comparing.

- **lib.rs:20–21 — `pub use error::*; pub use model::*;`** re-export the entire crate. Some types here (e.g. macro-generated newtypes' `From<String>` impls) are unintentionally exported. A targeted re-export list is more honest about what is the public API surface.

- **api.rs:9 — `use chrono::Utc;`** is fine, but `Utc::now()` is sprinkled across the file (12+ call sites) in ways that make tests harder. A single `Clock` trait/parameter would let tests freeze time. Not a v1 blocker; flag for v1.x test ergonomics.

- **api.rs:707–763 — `BinaryWrite` / `atomic_write_bytes` is duplicating logic already in `markdown::atomic_write`.** The differences (no frontmatter, `.bin` extension) could collapse into a `markdown::atomic_write_raw` taking a `Bytes` parameter. As-is, two atomic-write code paths must stay in sync (e.g. the §8.3 step 11 `fsync(parent_dir_fd)` — both have it, but if one drifts the contract diverges silently).

- **api.rs:737 — `std::fs::hard_link` then `std::fs::remove_file` instead of `std::fs::rename`.** Spec §8.3 step 10 calls for `rename(temp, final)`. The hardlink+remove sequence is not atomic the same way; if the process dies between hard_link and remove_file, both paths exist. The plaintext `markdown::atomic_write` likely uses `rename` — verify and unify.

- **model.rs:283–347 — `Frontmatter` has 30+ public fields with no builder.** Every test/construction site (see `bin/stream_a_bench.rs` `sample_memory`, ~60 lines of struct-literal noise) has to spell out every field. A `FrontmatterBuilder` with sensible defaults plus required-field checks would shrink fixture code by an order of magnitude and make the required vs nullable spec contract enforced by the type system.

- **error.rs:43 — `SubstrateError::Sqlite(#[from] rusqlite::Error)`** leaks the SQLite dependency into the public API. A consumer who wants to swap SQLite for another store now has to also depend on `rusqlite` to match against this variant. **Fix:** wrap as `Sqlite(String)` or define a `IndexBackendError` enum.

- **model.rs:565–581 — `validate_repo_relative_path` has a hardcoded `allowed` list of top-level prefixes.** That list does not match spec §5.1 perfectly: it is missing `<empty>` (root files like `.gitattributes`) — handled below by a separate `matches!`, OK — but it also accepts `events/` for plaintext writes which spec §13.1 only allows for the merge-union JSONL files. The validator does not enforce file-type-by-tree. Not a blocker today since it is path-shape only, but worth a comment that the deeper "encrypted/ requires .bin, events/ requires .jsonl" rule lives elsewhere.

- **model.rs:343 — `merge_diagnostics: Option<serde_json::Value>`** — same `serde_json::Value` escape as `entities`/`evidence`. Spec §6.10 has it fully structured. Same fix.

- **api.rs:67–75 — `doctor()` is `async fn` but does no async work.** Make it sync, or document why.

- **api.rs:531 — `reindex(&self) -> SubstrateResult<usize>`** — spec §16.4 promises `Result<ReindexReport, IndexError>`. A bare `usize` is a count; `ReindexReport { count, errors, duration }` is what callers need. Add the type.

- **api.rs:41 — `std::env::current_exe().unwrap_or_else(|_| PathBuf::from("memory-merge-driver"))`** — silent fallback to a relative `PATH`-resolved binary. Spec §13.1 step 6: "The driver command uses an absolute path or a stable shim path managed by installation. Ambient `PATH` is not sufficient for unattended merges." This violates that spec rule on the error path. **Fix:** propagate the error instead of fabricating a relative name.

- **api.rs:161, 325, 458, 698 — `EventId::new(format!("evt_{}", uuid::Uuid::new_v4()))`** is duplicated four times. Extract `EventId::generate()`.

- **api.rs:823 — `new_operation_id() -> OperationId` exists; `new_event_id` does not. Inconsistent.**

- **error.rs:131–170 — `ValidationError::Other(String)`** is the same string-bag pattern. The other variants are typed; this one is the "I gave up" hatch. It is reachable from `validate_frontmatter` (frontmatter module) and quietly subsumes anything not covered by the other variants. **Fix:** delete it; force every validator path to a typed variant.

- **api.rs:167–181 / 289–296 / 425–432 — `PendingEventOp { last_error: Some(err.to_string()) }`** loses error type information in the same way the brief warned about for `From` impls.

- **error.rs:83 — `#[error("write failed: {kind}")]` on `WriteFailure`** could include `outcome.committed` for log clarity.

- **model.rs:1 — `#![allow(unknown_lints, file_too_long)]`** at the top of `model.rs` silences a lint that exists for exactly this case; api.rs:1 has the same. The "until Task 10 seam split" comment is honest, but the suppression reads as "we know this is too big and we have not split it."

- **api.rs:96–215 (write_memory): the function is ~120 LOC of nested `.map_err` cascades and inline outcome construction.** It does fewer things than its size suggests but the noise hides them. After the helpers in the first Risk above land, this function should be ~30 LOC of straight-line orchestration.

## Strengths worth keeping

- **`ClassificationOutcome` is required, `Copy`, and serde-snake_case.** Exactly the spec contract — no defaults, no `Option`. Good.

- **`WriteFailure` carries `outcome: WriteOutcome`** so callers can always distinguish committed-but-incomplete from not-committed. Spec §16.6 demands this and the impl honors it. The cascade of repair states in `RepairRequired` (api.rs:447–458) maps cleanly to spec §8.3.

- **`MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION`** is a single named constant (merge.rs:11) referenced from the merge logic (three_way.rs:38–62). Spec §14.2 invariant satisfied — no magic numbers.

- **Newtypes for `MemoryId`, `RepoPath`, `OperationId`, `EventId`, `Sha256`** are the right call (vs. raw `String`). The `id_type!` macro keeps the boilerplate consistent. See the validation blocker above for what to fix; the _shape_ is right.

- **`error.rs` taxonomy is well-stratified.** Open / Read / Write / Validation / Id / Vector / Git / Watch / Merge are separate enums, each `thiserror`-backed, each `transparent`-flattened into `SubstrateError`. The brief warned about "string-bag dumping grounds" — most of these enums are _not_ that (the ones that are, are flagged above).

- **`DurabilityTier` is exposed via `Substrate::durability_tier()`** so Stream B can refuse to start under `BestEffort` policy. Spec §3.1 contract honored.

- **`open_with_options` orchestrates startup reconciliation** (api.rs:613–641) before returning the `Substrate` handle — spec §13.5.1 "Substrate must not return from `open` until startup reconciliation completes" is honored at this seam.

- **`enforce_plaintext_classification` (api.rs:674–694) is the one place classification is enforced for the plaintext path** and it is tight: explicit match on every variant, no `_` wildcard, the `Trusted + sensitive` mismatch is caught. Use this as the model for the encrypted path's classification enforcement (see `write_encrypted`, lines 223–231, which inlines its own version).

## Open questions for Trey

1. **Async surface.** Spec §16.5 mandates "blocking sections [run] on Stream A's configured blocking executor or single index thread" but the impl has zero `spawn_blocking` and uses `std::sync::Mutex`. Was the intent to ship Substrate as sync-internal-with-async-facade (the spec allows this — "Stream A itself may be synchronous internally"), or to actually run on a tokio runtime with proper blocking offload? Either is defensible; current state is neither.

2. **`MemoryEnvelope` and `MemoryHit`/`ChunkHit`.** Spec §16.2 / §16.4 specify these by name with structured content states. Implementation ships `Memory` and `QueryResult`/`ChunkResult` instead. Is the plan to add the envelope/hit types in a Task 10 seam split, or has the spec intent been deliberately deferred? Streams B/E will lock in around whatever shape ships.

3. **Event-kind set.** The implementation has 8 event kinds; the spec lists ~24. Some of the missing ones are critical for §17.8 "spec acceptance signals have named tests" coverage. Is the plan to backfill before release, or to bump §12.2 down to the implemented set?

4. **`WriteRefused` audit event.** Spec §8.7 step 6 says refusals are audited. Today they are not. Is that intentional (refusals don't go through git-synced state, so maybe the spec is over-specifying) or a gap to close?

5. **`extras` round-trip.** The `#[serde(skip)]` on `Frontmatter::extras` looks like an oversight rather than a deliberate choice — confirm? If deliberate, spec §6.2 needs a v1.2 carve-out; if oversight, the round-trip test belongs in the spec coverage manifest.

6. **CLI binary status.** The brief asked about `bin/main.rs`. The crate has `bin/stream_a_bench.rs` (the perf gate), no general CLI. Is a `memoryd-substrate` admin CLI (for the spec §17.7 doctor surface, repair commands, etc.) planned for Task 12+, or is the current "it's an embeddable library only" the v1.1 contract? The README would benefit either way.
