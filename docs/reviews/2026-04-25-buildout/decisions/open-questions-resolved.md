# Open question resolutions — Stream A buildout fix run

**Date:** 2026-04-25
**Authority:** Trey delegated to Claude (architect), guided by two heuristics:

1. Best UX for the **user** (eventually-shipping product, real humans).
2. Best UX, usability, and token efficiency for the **agents** that consume Stream A.

These resolve the 12 open questions in `SUMMARY.md` §6. All implementers MUST treat these as binding for the fix run.

---

## Q1 — `memories` table scope (B-IX-4) → **Fix to spec §10.1 now**

The 6-column reduction is **not** scaffold. Spec §10.1 mandates ~28 columns plus 5 compound indexes; plan v0.3 has no callout deferring this; Task 6 owns the schema and was supposed to land it complete. Codex shipped a stub.

**Why:** Stream E recall block assembly + Stream H eval reproducibility both query against this table; metadata-only reads must hydrate from `frontmatter_json` rather than re-reading files (otherwise "SQLite is the read path" model collapses). Not negotiable.

**Implementer note:** add the 5 compound indexes from §10.1 too. `frontmatter_json TEXT NOT NULL CHECK (json_valid(frontmatter_json))` is the json-validity gate.

---

## Q2 — Async surface (B-API-3, B-RT-7) → **Truly async via `tokio::task::spawn_blocking`**

`async fn` facade with `std::sync::Mutex` and direct blocking I/O is broken: parks tokio worker threads, deadlocks current-thread runtimes (which Stream B's MCP server will use). Two correct shapes; we pick the one that wins for agents.

**Decision:** Every public method in `api.rs` wraps its blocking body in `tokio::task::spawn_blocking` against a configured pool (size from `RuntimeConfig`). Locks become `parking_lot::Mutex` (no poisoning to swallow). Index handle moves to a dedicated single thread (per spec §16.5) accessed via channel.

**Why agents:** Stream B will run inside a tokio current-thread runtime in many MCP transport adapters. A sync-blocking call inside `async fn` deadlocks them. Real `spawn_blocking` is the only shape that "just works" for agent consumers without them having to know our internals.

**Why users:** users never see this directly, but a blocking `read_memory` that stalls a chat UI (because Stream E is awaiting it on a current-thread runtime) is a brutal regression. Real async is the only safe contract.

---

## Q3 — Merge driver name (B-IO-3) → **`memory-merge-driver` (current code); update spec**

The driver merges full Markdown documents (frontmatter + body), not just frontmatter. `memory-merge-driver` is more accurate. Code is already consistent across `Cargo.toml`, `git/init.rs`, and `tree/layout.rs`.

**Action:** spec amendment to §13.1 step 1 and step 2 to read `memory-merge-driver`. No code rename. Bump spec to v1.2 with revision-goal entry.

---

## Q4 — Device-identity authority (B-API-12, B-FT R7) → **`git::adopt_clone` mints; `Substrate::open` fails if missing**

Spec invariant 4 is right, and the current code's auto-mint is a footgun. A fresh clone that never runs `adopt_clone` could either (a) write under a stale device id, or (b) collide with an existing device's shard namespace. Both are silent corruption.

**Decision:** `Substrate::open` fails with `OpenError::DeviceIdentityMissing { repair: AdoptClone }` when `local-device.yaml` is absent. The CLI / Stream B daemon calls `git::adopt_clone` once on first run, which atomically mints + writes (`tempfile::persist`). `parse_device_id` hand-rolled YAML parser deletes; route through `serde_yaml::from_str::<LocalDeviceConfig>(...)`.

---

## Q5 — Event-log CRC location (B-IO-2) → **In-JSON per spec §12.1**

Out-of-band hex prefix is faster but pushes a custom framing parser onto every consumer. In-JSON CRC means any agent reading event logs can use `serde_json::from_str` without a custom decoder.

**Decision:** `Event` struct gets `schema: u32`, `device_id: DeviceId`, `seq: u64`, `crc32c: u32` (4 hex bytes inside the JSON object, not 8 hex bytes prefix). `seq` persists to `~/.memoryd/event-seq.json` under exclusive lock. Add 64-KiB line bound on append per §12.3.

**Why agents:** Stream B's daemon, Stream E's recall assembly, Stream G's UI all read event logs. Forcing them to write a custom hex-prefix parser is a tax we'd pay every time a new stream lands. Standard JSON wins.

---

## Q6 — Plaintext-under-`encrypted/` detection (B-FT-4) → **Stream A enforces; `encryption:` frontmatter required**

Stream A is the substrate; defense-in-depth here catches Stream D bugs cheaply. Validator rejects any file under `encrypted/` that lacks an `encryption:` frontmatter block (or whose body parses as non-base64 plaintext markdown).

**Decision rule:** under `encrypted/`, the validator runs `frontmatter_has_encryption_envelope(path)` after parse. Missing → `ValidationError::PlaintextUnderEncryptedTier { path }`. Write path refuses similarly via `ClassificationOutcome` machinery (already required per spec §8.7).

---

## Q7 — `roots_converged` semantics (R-RT-1) → **Implement §13.6.1 in `memory-test-support`**

A test-support helper that's actually canonical-content equality is the right shape. Renaming to `roots_byte_equal` is doc theater; the bug is that the convergence check doesn't check what it claims to check.

**Decision:** `memory-test-support::convergence::roots_converged(a, b) -> ConvergenceReport` that:

1. Walks both trees deterministically.
2. For each `.md`: parses, re-serializes via `frontmatter::canonical_serialize`, byte-compares.
3. For each JSONL union path: parses, sorts by spec §13.6.1 ordering tuple, byte-compares.
4. Reports first divergent path with structured diff (not opaque `false`).

`scripts/two-clone-convergence.sh` continues to exist as a CI gate; it calls into the helper.

---

## Q8 — `merge_body` diff3 (R-MG-2) → **Literal diff3 via `imara-diff`**

User UX matters here: two devices each editing a memory body in different paragraphs should produce a clean merge, not a conflict. Whole-blob 3-way is a regression vs. what users get from any modern git workflow.

**Decision:** add `imara-diff` (or `diffy`'s diff3 — pick whichever has fewer transitive deps; verify with `cargo tree`). Implement `merge_body_diff3(base, ours, theirs) -> BodyMergeOutcome` returning `Clean(String) | Conflict(String_with_markers)`. On conflict, mark and continue; quarantine path is unchanged.

**Token-efficiency note for agents:** when a clean diff3 merge succeeds, the resulting body is the union — no recall-assembly cost spent unwinding `<<<<<<<` markers. Agents reading merged memories see clean prose.

---

## Q9 — `Sensitivity::Secret` variant (B-MG-14) → **Don't add it; rely on textual prefilter + serde rejection**

`Secret` should never exist as a parsed value in code — by spec, it's a runtime `ClassificationOutcome` only, never a frontmatter field value. Adding it to the enum invites future drift where someone accidentally constructs `Sensitivity::Secret` in a write path.

**Decision:** keep `Sensitivity` enum as-is (no `Secret`). Merge driver runs a textual prefilter scanning for `^sensitivity:\s*secret\s*$` (case-insensitive, YAML-context-aware) before YAML parse, exits 1 with `merge-driver: secret sensitivity refused`. Belt: serde would reject `secret` anyway since the variant doesn't exist. Suspenders: prefilter catches it before parser even runs.

---

## Q10 — Bench fixture corpus shape (R-RT-3) → **Pin a Stream-E-like namespacing pattern now**

Baselines are immutable absent explicit human commits (CLAUDE.md invariant 7). Pin the corpus shape **before** first real baseline lands, or we eat the cost of a bogus baseline forever.

**Decision:** `memory-test-support::perf::build_corpus(seed, size)` produces a deterministic tree across `me/notes/`, `me/preferences/`, `projects/<3 projects>/decisions/`, `projects/<3 projects>/conventions/`, `agent/patterns/`, `dreams/<dated>/` — proportional to what Stream E's namespacing produces in real use. Use `tempfile::TempDir`, not PID-based paths. Record `corpus_sha256` in every bench result.

---

## Q11 — `.compacted.jsonl` artefacts (B-RT-2) → **Delete after replay**

Per-op `PendingIndexReplayed` / `PendingEventReplayed` events are the durable forensic record. File accumulation on disk is dead weight that no one will ever look at.

**Decision:** `replay_pending_repairs` deletes the source file after successful replay (and after the per-op events are fsync'd to the event log). On failure, the file stays in place with `.failed.jsonl` rename + a `PendingReplayFailed` event for operator inspection.

---

## Q12 — `runtime::blocking::run_blocking` (B-RT-7) → **Delete it; use `tokio::task::spawn_blocking` directly**

Coordinated with Q2. Custom `run_blocking` shim adds a level of indirection without buying anything; `tokio::task::spawn_blocking` is the standard primitive every Rust async dev knows.

**Decision:** delete `runtime::blocking::run_blocking` and the `runtime/blocking.rs` file. Refactor every wrapped `Substrate` method in `api.rs` to use `tokio::task::spawn_blocking` directly. Index handle accesses go through a `flume`-channel actor (single thread owns `Connection`) instead.

---

## Cross-cutting follow-ups (not their own questions, but resolutions imply these)

- **Spec bump to v1.2:** name change in §13.1 (Q3), explicit endorsement of `Sensitivity` enum without `Secret` variant (Q9), `OpenError::DeviceIdentityMissing` enumerated (Q4).
- **No silent fallbacks anywhere.** Q1, Q4, Q5, Q9 all have the same root cause: prefer typed errors over magic defaults. Reviewers should grep for `unwrap_or`, `or_default`, `Default::default()` in fix-touched code paths and challenge each one.
- **Newtype validation invariants.** `MemoryId::new` and `RepoPath::new` MUST validate (B-API-11). All `From<&str>` / `From<String>` impls on these newtypes are deleted, not "fixed" — we want no path that bypasses validation.
