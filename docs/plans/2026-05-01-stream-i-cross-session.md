# Stream I Cross-Session Coordination Implementation Plan

**Goal:** Build Stream I cross-session coordination from `docs/specs/stream-i-cross-session-v0.1.md`: new `crates/memorum-coordination/` crate containing the relevance gate, `SessionContext`, `PresenceRegistry`, and `ClaimLockRegistry`; memoryd extensions for heartbeat and claim-lock handling; recall assembler integration for `<peer-update>` and `<peer-presence>` XML rendering; Stream A `indexed_at` surface addition; Stream E parser whitelist update; cross-device startup peer-update block; `memoryd peer` CLI subcommands; Stream H framing test fixtures; performance bench; and full documentation.

**Architecture:** The main Codex CLI agent is the orchestrator. Subagents do all substantive implementation, test, docs, security, performance, and review work in bounded file scopes; the orchestrator integrates, runs gates, and dispatches review/fix loops. Stream A remains the canonical repository/index substrate, Stream B remains the daemon/MCP bridge, Stream C remains governance/review authority, Stream D remains privacy/masking/encryption authority, Stream E remains recall-block assembly, and Stream F remains the dreaming pipeline. Stream I adds `crates/memorum-coordination/` plus additive daemon/recall/protocol/config surfaces without creating a second persistence layer. Presence state and claim-lock state live exclusively in daemon RAM.

**Tech Stack:** Rust 2021 workspace, `tokio`, `dashmap`, `serde`/`serde_json`, `chrono`, `thiserror`, `tempfile`, Stream A `memory-substrate` (RecallIndexRow, entity/path types, index queries), Stream B `memoryd` (handlers, protocol, server, workers, CLI), Stream D `memory-privacy` (`safe_plaintext_fragment`), Stream E `memoryd::recall` (recall assembler, delta/startup block rendering), vertical TDD, and release-gate bench fixtures.

---

## Source Contract

Normative sources:

- `docs/specs/stream-i-cross-session-v0.1.md` (patched 2026-05-01, five blockers resolved)
- `docs/specs/system-v0.2.md` §15 (peer-update architecture), §19 (cross-stream surface authorizations)
- shipped Stream A–F code and docs in this repo

Do not edit or overwrite spec files unless Trey explicitly asks. This plan creates `docs/plans/2026-05-01-stream-i-cross-session.md` and implementation work must treat `docs/specs/stream-i-cross-session-v0.1.md` as the active contract.

---

## Inter-Stream Coordination

Stream I and Stream G are independent Codex execution tracks that both touch shipped-stream surfaces. Their authorized touches must not collide at the schema level.

**Stream G owns:**

- `EventKind::RecallHit` variant addition in `crates/memory-substrate/src/events/log.rs`
- `events_log` covering index on `(kind, memory_id, ts)` — this is the **only** Stream G/I schema migration that touches `memories`/`events_log` tables, and it therefore owns the schema-version bump

**Stream I owns:**

- `RecallIndexRow::indexed_at` field addition: struct field + hydration only. **No new column.** `memories.indexed_at TEXT NOT NULL` already exists in the shipped schema. Stream I surfaces it on the Rust model struct; no schema-version bump needed
- Pre-parse whitelist update at `crates/memoryd/src/recall/project.rs:81` (Stream E surface, authorized in system-v0.2 §19)
- `concurrent_session_mode` serde field on Stream E's project-binding deserializer (Stream E surface, authorized in system-v0.2 §19)

**Rebase rule:** whichever of Stream G and Stream I lands first on `main`, the second must rebase before its own integration. The orchestrator checks `git log --oneline main | head -5` before integrating any Stream I batch and rebases the in-flight worktree branch if Stream G has landed.

**Trunk gate:** `scripts/check.sh` runs once after both G and I integrate onto `main`, not per-stream-per-task.

---

## Codex CLI Orchestrator Contract

The main GPT agent running in Codex CLI is the orchestrator. The orchestrator may:

1. Maintain the task DAG and current status.
2. Spawn subagents for every implementation/review/docs/perf/security lane.
3. Enforce non-overlapping owned-file scopes for parallel batches.
4. Integrate subagent changes in dependency order.
5. Resolve integration conflicts caused by accepted subagent outputs.
6. Run narrow and full gates.
7. Spawn fix subagents for every blocking review finding.

The orchestrator must not casually implement feature code directly. If a gate fails or a review finding appears, create a bounded fix task and assign it to the correct subagent with the mandatory skills below. Tiny mechanical plan/doc integration edits are acceptable for the orchestrator; feature code is not.

---

## Mandatory Skills For Every Subagent

Every implementation, test, docs, review, security, performance, and QA subagent prompt must include this exact line:

```text
Mandatory skills: clean-code, tdd, rust-engineer.
```

Every subagent must load the repo-local Rust skill:

```text
Load /Users/treygoff/Code/agent-memory/.codex/skills/rust-engineer/SKILL.md.
```

Review subagents must also explicitly load `clean-code` and apply it as a review lens. Implementation subagents must use vertical TDD: one failing behavior test, narrow RED command, minimal implementation, narrow GREEN command, refactor only while green.

### Required Subagent Prompt Preamble

Use this preamble for every subagent:

```text
Mandatory skills: clean-code, tdd, rust-engineer.
Load /Users/treygoff/Code/agent-memory/.codex/skills/rust-engineer/SKILL.md.
Repository: /Users/treygoff/Code/agent-memory.
You are working on Stream I cross-session coordination from docs/specs/stream-i-cross-session-v0.1.md. Treat Stream A as the only canonical substrate/index, Stream B as daemon/MCP, Stream C as governance/review, Stream D as privacy/masking/encryption, Stream E as recall assembly, and Stream F as dreaming. Presence state and claim-lock state live in daemon RAM only — never on disk, never in canonical memory frontmatter. Use vertical TDD: write one failing behavior test, run it and record the RED failure, implement the smallest correct slice, rerun the narrow gate to GREEN, then refactor only while green. Do not touch files outside your Owned files. Do not edit spec files unless the task explicitly owns a docs amendment.
```

For review subagents append:

```text
This is a review-only lane unless explicitly assigned a fix task. Lead with findings ordered by severity. Apply clean-code review criteria plus Rust correctness, async safety, concurrency, privacy, test quality, and spec compliance. If there are no findings, say so and list residual risks.
```

---

## Parallelization And Review Cadence

Parallel work is allowed only when owned files do not overlap inside the batch. The orchestrator must run a batch-specific owned-file duplicate check before spawning any parallel implementation batch.

Full-plan owned-file duplicates are expected because sequential tasks touch shared choke points such as `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/server.rs`, `crates/memoryd/src/workers.rs`, and docs. Duplicates are forbidden only inside a parallel batch.

Batch duplicate check template:

```bash
cat > /tmp/stream-i-batch-owned-files.txt <<'LIST'
Task X: path/to/file.rs
Task Y: path/to/other.rs
LIST
cut -d: -f2- /tmp/stream-i-batch-owned-files.txt \
  | sed 's/^[[:space:]]*//;s/[[:space:]]*$//' \
  | sort \
  | uniq -d
```

Expected for each parallel batch: no output.

### Review Gates

- **Review Gate A — Stream A surface + Stream E parser seam:** after Tasks 1–3. Clean-code + API-contract reviewers inspect `RecallIndexRow::indexed_at` hydration, whitelist update, and serde field before coordination crate work starts.
- **Review Gate B — Coordination crate + score function:** after Tasks 4–8. Clean-code + correctness reviewers inspect the full score function, SessionContext derivation, and embedding worker wiring.
- **Review Gate C — Presence + claim locks:** after Tasks 9–13. Clean-code + concurrency reviewers inspect PresenceRegistry, ClaimLockRegistry, heartbeat handler, and stale sweeper.
- **Review Gate D — Recall assembler, tier policy, CLI:** after Tasks 14–19. Clean-code + security reviewers inspect XML rendering, budget accounting, per-project enforcement, and admin CLI.
- **Final Review Gate E:** after Tasks 20–22. Independent clean-code, security, performance, API contract, and docs reviewers run before final release gate.

Every review gate must produce a file in `docs/reviews/` or a concise orchestrator-captured report. All severity-1/2 findings must be fixed by scoped fix subagents, the same review lane must rerun, and severity-3 findings must either be fixed or logged with rationale before advancing.

---

## Task 1: Contract Map, Worktree Baseline, And Dirty-Tree Check

**Subagent type:** `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Also use `spec-quality-checklist`.
**Parallel:** no
**Blocked by:** none
**Owned files:** `docs/reviews/stream-i-contract-map.md`, `docs/plans/2026-05-01-stream-i-cross-session.md`
**Invariants:** Do not edit `docs/specs/stream-i-cross-session-v0.1.md`. Do not weaken acceptance signals.
**Out of scope:** Production code.

**Files:**

- Create: `docs/reviews/stream-i-contract-map.md`
- Modify: `docs/plans/2026-05-01-stream-i-cross-session.md` only if the plan contradicts the patched v0.1 spec

**Steps:**

1. Write `docs/reviews/stream-i-contract-map.md` mapping every v0.1 §11 acceptance bullet to an implementation task, owned files, and narrow gate.
   - Record the five patched blockers explicitly: §4.3 Tier 3 no-op short-circuit; §6.1 `started_at: Option<DateTime<Utc>>`; §8.2 two-layer whitelist/serde update; §3.3/§7.1 claim-lock scope clarification; §4.2 `local_observed_at` recency window.
   - Record the inter-stream coordination boundary: Stream G owns schema-version bump + `EventKind::RecallHit`; Stream I owns `RecallIndexRow::indexed_at` surface with no new column.
2. Capture current dirty-tree baseline:
   ```bash
   git status --short
   ```
3. Verify spec terms are covered:
   ```bash
   rg -n "CoordinationInsertion|RelevanceGate|SessionContext|PresenceRegistry|ClaimLockRegistry|PeerHeartbeat|peer_update|peer_presence|indexed_at|local_observed_at|concurrent_session_mode|framing_tests" docs/specs/stream-i-cross-session-v0.1.md
   ```
4. Check existing surfaces and record choke points:
   ```bash
   rg -n "RequestPayload|ResponsePayload|RecallIndexRow|query_recall_index|recall.*project\|project.*recall" crates
   ```

**Verification plan:**

- Primary: `docs/reviews/stream-i-contract-map.md` covers all §11.1 and §11.2 acceptance bullets.
- Secondary: `rg -n "TBD|TODO|unclear|not covered" docs/reviews/stream-i-contract-map.md` returns no unresolved blockers except explicit implementation tasks.

---

## Task 2: Stream A Surface — `RecallIndexRow::indexed_at` And `source_device` Struct Fields And Hydration

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 1
**Owned files:** `crates/memory-substrate/src/model.rs`, `crates/memory-substrate/src/index/query.rs`, `crates/memory-substrate/tests/recall_index_row_indexed_at.rs`, `crates/memory-substrate/tests/recall_index_row_source_device.rs`, `docs/api/stream-a-public-api.md`
**Invariants:** No new column on `memories`. No schema migration. No schema-version bump. Only struct-field additions and hydration updates. The `memories.indexed_at TEXT NOT NULL` and `memories.source_device TEXT` columns already exist in the shipped schema. Existing `RecallIndexRow` consumers must compile without change (the fields are additive). Stream G owns the five new `EventKind` variants, the `events_log` SQLite mirror table, and the v4 schema migration — do not touch those here.
**Out of scope:** Coordination crate, daemon protocol, any other Stream A change.

**Files:**

- Modify: `crates/memory-substrate/src/model.rs` (add `pub indexed_at: DateTime<Utc>` and `pub source_device: Option<String>` to `RecallIndexRow` per spec §1.1)
- Modify: `crates/memory-substrate/src/index/query.rs` (extend SELECT + hydration to populate both new fields)
- Test: `crates/memory-substrate/tests/recall_index_row_indexed_at.rs`
- Test: `crates/memory-substrate/tests/recall_index_row_source_device.rs`
- Docs: `docs/api/stream-a-public-api.md`

**Step 1: RED tests**

Create `crates/memory-substrate/tests/recall_index_row_indexed_at.rs`:

- `test_indexed_at_populated_on_recall_index_query` — write a canonical memory via the substrate API; call `query_recall_index`; assert `RecallIndexRow::indexed_at` is within 5 seconds of `now()` and is a non-zero timestamp.
- `test_indexed_at_not_null_invariant` — write two memories at different times; both rows have non-null `indexed_at`.
- `test_indexed_at_distinct_from_updated_at` — backdate frontmatter `updated_at`; verify `RecallIndexRow::indexed_at` reflects index-ingest time, not the frontmatter `updated_at`.

Create `crates/memory-substrate/tests/recall_index_row_source_device.rs`:

- `test_source_device_populated_when_present` — write a memory with `source_device = Some("dev_abc")` in its frontmatter; assert `RecallIndexRow::source_device == Some("dev_abc")`.
- `test_source_device_none_when_absent` — write a memory whose frontmatter omits `source_device`; assert `RecallIndexRow::source_device == None`.
- `test_source_device_distinct_per_memory` — three memories (one `dev_a`, one `dev_b`, one omitted); assert each row's value is correct.

Run:

```bash
cargo test -p memory-substrate --test recall_index_row_indexed_at
cargo test -p memory-substrate --test recall_index_row_source_device
```

Expected: FAIL because `RecallIndexRow` does not yet expose either field.

**Step 2: GREEN implementation**

- Add `pub indexed_at: DateTime<Utc>` and `pub source_device: Option<String>` to `RecallIndexRow` in `model.rs`. Doc-comments per spec §1.1.
- In `query.rs`, extend the `SELECT` projection of `query_recall_index` to include both `indexed_at` and `source_device`. Hydrate both into `RecallIndexRow`: parse `indexed_at` from RFC-3339 TEXT to `DateTime<Utc>` (typed error on parse failure, never silent epoch fallback); map `source_device` directly from `Option<String>` (NULL → None).
- Ensure all existing callers of `RecallIndexRow` still compile (additive fields).

**Step 3: GREEN command**

```bash
cargo test -p memory-substrate --test recall_index_row_indexed_at
cargo test -p memory-substrate --test recall_index_row_source_device
cargo test -p memory-substrate --test memory_query_extension
cargo test -p memory-substrate --test fts_query_sanitization
```

**Verification plan:**

- Primary: both new test files plus `memory_query_extension`.
- Secondary: `cargo test -p memory-substrate --test api_write_read --test vector_lifecycle` (must stay green).

---

## Task 3: Stream E Parser Surface — Whitelist Update And `concurrent_session_mode` Serde Field

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 2
**Owned files:** `crates/memoryd/src/recall/project.rs`, `crates/memoryd/tests/project_binding_concurrent_mode.rs`
**Invariants:** Both parser layers must be updated atomically in one commit (spec §8.2). Without the whitelist update, the serde layer is never reached. Existing `.memory-project.yaml` files without `concurrent_session_mode` must parse identically — the field defaults to `None`. Unknown string values for `concurrent_session_mode` (e.g., `"experimental"`) must fail with `invalid_request`, not silent fallback (spec §8.2 last paragraph).
**Out of scope:** `CoordinationLevel` enum, coordination config loading, coordination crate.

**Files:**

- Modify: `crates/memoryd/src/recall/project.rs` (whitelist at line ~81, serde struct field)
- Test: `crates/memoryd/tests/project_binding_concurrent_mode.rs`

**Step 1: RED test**

Create `crates/memoryd/tests/project_binding_concurrent_mode.rs`:

- `test_concurrent_session_mode_collaborative_parses` — parse a `.memory-project.yaml` containing `concurrent_session_mode: collaborative`; assert the resulting binding has `concurrent_session_mode = Some(ConcurrentSessionMode::Collaborative)`.
- `test_concurrent_session_mode_minimal_parses` — parse `concurrent_session_mode: minimal`; assert `Some(ConcurrentSessionMode::Minimal)`.
- `test_concurrent_session_mode_default_parses` — parse `concurrent_session_mode: default`; assert `Some(ConcurrentSessionMode::Default)`.
- `test_concurrent_session_mode_absent_defaults_none` — parse a file with only `canonical_id` and `alias`; assert `concurrent_session_mode = None`.
- `test_concurrent_session_mode_unknown_value_rejects` — parse `concurrent_session_mode: gibberish`; assert error contains `invalid_request` or equivalent structured rejection.
- `test_preparse_whitelist_blocks_without_serde` — verify a file containing an entirely unknown key (not `canonical_id`, `alias`, or `concurrent_session_mode`) is rejected at the pre-parse whitelist layer, not by serde.

Run:

```bash
cargo test -p memoryd --test project_binding_concurrent_mode
```

Expected: FAIL because the whitelist does not yet include `concurrent_session_mode` and the serde struct lacks the field.

**Step 2: GREEN implementation**

- At `recall/project.rs:81`, extend `matches!(key, "canonical_id" | "alias")` to `matches!(key, "canonical_id" | "alias" | "concurrent_session_mode")` (spec §8.2, first layer).
- Add `ConcurrentSessionMode` enum: `Minimal`, `Default`, `Collaborative`; derive `serde::Deserialize` with `deny_unknown_variants`; impl `TryFrom<&str>` for `invalid_request` on unknown strings.
- Add serde struct field to the project-binding deserialization target: `#[serde(default, deserialize_with = "deserialize_optional_concurrent_session_mode")] pub concurrent_session_mode: Option<ConcurrentSessionMode>` (spec §8.2, second layer).

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test project_binding_concurrent_mode
cargo test -p memoryd --test daemon_e2e
cargo test -p memoryd --test server_smoke
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test project_binding_concurrent_mode`
- Secondary: `cargo test -p memoryd --test recall_cli --test startup_recall_mcp`

---

## Review Gate A: Stream A Surface And Stream E Parser Seam Review

**Subagent types:** `reviewer`, `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Review agents must load clean-code.
**Parallel:** yes
**Blocked by:** Tasks 1–3 integrated and green
**Owned files:** `docs/reviews/stream-i-review-gate-a-clean-code.md`, `docs/reviews/stream-i-review-gate-a-contract.md`
**Invariants:** Review only. Do not edit production code.
**Out of scope:** Coordination crate, daemon protocol.

**Review lanes:**

1. **Clean-code/Rust review:** inspect Tasks 2–3 diffs for naming, field visibility, error type hygiene, no new columns, no schema-version changes, hydration parse path correctness, `NOT NULL` invariant honored.
2. **Contract review:** verify §1.1 `indexed_at` intent is fully honored (additive struct field, no migration, correct hydration), §8.2 two-layer whitelist/serde update is atomic, all `ConcurrentSessionMode` variants match spec mapping table.

**Commands reviewers should run:**

```bash
cargo test -p memory-substrate --test recall_index_row_indexed_at
cargo test -p memoryd --test project_binding_concurrent_mode
cargo fmt --all -- --check
cargo clippy -p memory-substrate -p memoryd --all-targets --all-features -- -D warnings
```

**Exit criteria:** all severity-1/2 findings fixed by scoped fix subagents, same review lane rerun, and severity-3 findings either fixed or logged with rationale before Task 4.

---

## Task 4: `crates/memorum-coordination/` Workspace Skeleton And Module Layout

**Subagent type:** `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Review Gate A
**Owned files:** `Cargo.toml`, `crates/memorum-coordination/Cargo.toml`, `crates/memorum-coordination/src/lib.rs`, `crates/memorum-coordination/src/config.rs`, `crates/memorum-coordination/src/protocol.rs`, `crates/memorum-coordination/src/gate.rs`, `crates/memorum-coordination/src/session.rs`, `crates/memorum-coordination/src/presence.rs`, `crates/memorum-coordination/src/claim_lock.rs`, `crates/memorum-coordination/tests/gate_unit.rs`, `crates/memorum-coordination/tests/session_derivation.rs`, `crates/memorum-coordination/tests/presence_unit.rs`, `crates/memorum-coordination/tests/claim_lock_unit.rs`
**Invariants:** `memorum-coordination` must not depend on `memory-governance`. It may depend on `memory-substrate` (entity/path types, RecallIndexRow) and `memory-privacy` (`safe_plaintext_fragment`). All stub modules must compile with no `todo!()` panics in test-facing paths; use `unimplemented!()` only in clearly stub-internal functions. `CoordinationConfig` defaults must match spec §8.1 exactly.
**Out of scope:** Actual implementations — only skeletons, DTOs, config, and compilation.

**Files:**

- Create (new crate): `crates/memorum-coordination/Cargo.toml`
- Create: `crates/memorum-coordination/src/lib.rs`
- Create: `crates/memorum-coordination/src/config.rs` — `CoordinationConfig` with all §8.1 fields and their defaults
- Create: `crates/memorum-coordination/src/protocol.rs` — `CoordinationInsertion`, `PeerUpdateEntry`, `PeerPresenceEntry`, `ClaimLockInfo` per spec §1.1 and §5
- Create: `crates/memorum-coordination/src/gate.rs` — `RelevanceGate` struct skeleton, `evaluate` signature, `CoordinationInsertion::empty()`
- Create: `crates/memorum-coordination/src/session.rs` — `SessionContext` struct per spec §4.3
- Create: `crates/memorum-coordination/src/presence.rs` — `PresenceRecord`, `PresenceRegistry` struct skeletons per spec §6.2
- Create: `crates/memorum-coordination/src/claim_lock.rs` — `ClaimLockRegistry` struct skeleton, `acquire`/`renew`/`release` signatures per spec §7
- Modify: `Cargo.toml` — add `crates/memorum-coordination` to workspace members
- Create (empty stubs for tests): the four `tests/*.rs` files above

**Steps:**

1. Add `memorum-coordination` to `Cargo.toml` workspace members. Set `memory-substrate` and `memory-privacy` as dependencies in the new crate's `Cargo.toml`. Add `dashmap` as a dependency (check if already in workspace; if not, add version and workspace key).
2. Stub all module files. `CoordinationConfig` struct must compile with correct field names and types matching §8.1. `CoordinationInsertion` must have the four fields from spec §1.1 with correct types. `SessionContext` must have the seven fields from spec §4.3.
3. Verify compilation:
   ```bash
   cargo build -p memorum-coordination
   ```
4. Create stub test files that compile but contain only `#[test] fn placeholder() {}`. They will be filled in Tasks 5–12.

**Verification plan:**

- Primary: `cargo build -p memorum-coordination` exits 0 with no warnings.
- Secondary: `cargo test -p memorum-coordination` passes all placeholder tests.

---

## Task 5: Score Function Library — `entity_jaccard`, `path_jaccard`, `topic_similarity`, Weighted Score, Tier 3 Short-Circuit

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 4
**Owned files:** `crates/memorum-coordination/src/gate.rs`, `crates/memorum-coordination/tests/gate_unit.rs`
**Invariants:** Weights must be `(0.5, 0.3, 0.2)` matching spec §4.1. Threshold is `0.6`. Empty-empty entity sets yield `entity_overlap = 0.0`, not `1.0` (spec §4.1). Embedding triple mismatch yields `topic_similarity = 0.0`, not an error. Tier 3 session context returns `CoordinationInsertion::empty()` without scoring any candidates (spec §4.3). `test_entity_overlap_required_property` is a named mandatory test (spec §11.1).
**Out of scope:** `SessionContext` derivation, `PresenceRegistry`, `ClaimLockRegistry`, daemon wiring.

**Files:**

- Modify: `crates/memorum-coordination/src/gate.rs`
- Modify: `crates/memorum-coordination/tests/gate_unit.rs`

**Step 1: RED tests**

Fill `gate_unit.rs` with the full test matrix from spec §11.1:

- `test_score_entity_overlap_only` — entity_jaccard = 1.0, path_jaccard = 0.0, topic = 0.0 → score = 0.5.
- `test_score_path_overlap_only` — entity_jaccard = 0.0, path_jaccard = 1.0, topic = 0.0 → score = 0.3.
- `test_score_all_components` — known values for all three components → assert within `f64::EPSILON` of expected.
- `test_threshold_boundary` — score exactly `0.6` surfaces; score `0.5999` does not.
- `test_per_turn_cap` — 5 candidates above threshold → only top 2 in `peer_updates`; `capped_peer_updates = 3`; ordering: descending score, then descending `updated_at`, then ascending `memory_id` lex.
- `test_cool_down` — `memory_id` already in `surfaced_peer_writes` is not returned even if scoring above threshold.
- `test_recency_window_uses_local_observed_at` — write with `local_observed_at = now - 31m` is excluded even if `updated_at` is recent; write with `local_observed_at = now - 29m` is included even if `updated_at` is old.
- `test_tier3_returns_empty` — Tier 3 session yields `CoordinationInsertion::empty()` with no scoring invoked.
- `test_entity_overlap_required_property` — `entity_jaccard = 0.0`, `path_jaccard = 1.0`, `topic = 1.0` → score `0.0*0.5 + 1.0*0.3 + 1.0*0.2 = 0.5` → does NOT surface (0.5 < 0.6 threshold). Documents the design property from spec §4.2.
- `test_empty_entity_sets` — both sets empty → `entity_overlap = 0.0`.
- `test_embedding_triple_mismatch` — mismatched `(provider, model_ref, dimension)` triples → `topic_similarity = 0.0`, no error, no panic.

Run:

```bash
cargo test -p memorum-coordination --test gate_unit
```

Expected: FAIL (functions not implemented).

**Step 2: GREEN implementation**

Implement in `gate.rs`:

- `fn entity_jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64` — Jaccard over case-insensitive trimmed ids; empty-empty returns `0.0`.
- `fn path_fraction(p_paths: &HashSet<String>, s_paths: &HashSet<String>) -> f64` — exact-string match; `p_paths` empty returns `0.0`.
- `fn cosine_similarity(p_emb: &[f32], s_emb: &[f32]) -> f64` — dot / (||a|| * ||b||), clamped to `[0.0, 1.0]`; mismatched lengths or either empty returns `0.0`.
- `fn score(candidate: &PeerWriteCandidate, session: &SessionContext, emb_triple_matches: bool) -> f64` — weighted sum per spec §4.1.
- `impl RelevanceGate::evaluate` — Tier 3 short-circuit first (spec §4.3), then recency window filter on `local_observed_at`, then score all candidates, then cap/sort/cool-down, return `CoordinationInsertion`.

**Step 3: GREEN command**

```bash
cargo test -p memorum-coordination --test gate_unit
```

**Verification plan:**

- Primary: `cargo test -p memorum-coordination --test gate_unit`
- Secondary: `cargo clippy -p memorum-coordination --all-targets --all-features -- -D warnings`

---

## Task 6: `SessionContext` Salient Entity Derivation — Tier 1 And Tier 3

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 5
**Owned files:** `crates/memorum-coordination/src/session.rs`, `crates/memorum-coordination/tests/session_derivation.rs`
**Invariants:** Tier 1 salient entities = startup recall `<entity-recall entities="...">` attribute union + last-3-turns FTS5 entity extraction (shared result from Stream E's delta seed, not a re-computed lookup). Tier 3 salient entities = project alias + canonical_id + basename(cwd) + parent-dir basename only — no extraction from user messages. `RelevanceGate::evaluate` called with Tier 3 `SessionContext` must short-circuit without scoring (spec §4.3). No embedding call for entity derivation (FTS5 only).
**Out of scope:** Salient path derivation (Task 7), embedding worker wiring (Task 8).

**Files:**

- Modify: `crates/memorum-coordination/src/session.rs`
- Modify: `crates/memorum-coordination/tests/session_derivation.rs`

**Step 1: RED tests**

Fill `session_derivation.rs`:

- `test_salient_entities_from_startup_recall` — build a mock `RecallExplanation` with known entity ids in the startup recall `sections`; derive `salient_entities`; assert set contents match expected ids.
- `test_salient_entities_tier3_from_binding_only` — Tier 3 `SessionContext` derived from a `ProjectBinding` with `canonical_id = "proj_abc"`, `alias = "my-project"`, `cwd = "/Users/trey/code/my-project"`; assert `salient_entities = {"proj_abc", "my-project", "my-project", "code"}` (deduped).
- `test_relevance_gate_skipped_for_tier3` — pass a Tier 3 `SessionContext` to `RelevanceGate::evaluate`; verify via a spy/counter that no per-candidate scoring path runs.

Run:

```bash
cargo test -p memorum-coordination --test session_derivation
```

Expected: FAIL.

**Step 2: GREEN implementation**

- `SessionContext::from_startup_recall` (Tier 1): parse `<entity-recall entities="...">` from the recall XML or from `RecallExplanation.sections[].selected_ids` resolved through the substrate entity index.
- `SessionContext::from_tier3_binding`: populate `salient_entities` from project binding fields only; `salient_paths` from empty set (no startup recall path available yet).
- Tier indicator: derive from `SessionContext::harness` — Tier 1 = `"claude-code"` | `"codex"` | `"codex-cli"`; anything else = Tier 3 in v1.

**Step 3: GREEN command**

```bash
cargo test -p memorum-coordination --test session_derivation
cargo test -p memorum-coordination --test gate_unit
```

**Verification plan:**

- Primary: `cargo test -p memorum-coordination --test session_derivation`
- Secondary: `cargo test -p memorum-coordination --test gate_unit`

---

## Task 7: `SessionContext` Salient Path Derivation — Tier 1 And Tier 3

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes — parallel with Task 8, after Task 6
**Blocked by:** Task 6
**Owned files:** `crates/memorum-coordination/src/session.rs` (path derivation methods only; entity methods done in Task 6)
**Invariants:** Tier 1 salient paths = namespace paths of memories emitted in startup recall block (extracted from `<entity-recall>` and `<project-state>` `ref=` attributes or `RecallExplanation.sections[].selected_ids` resolved to paths) UNION tool-call file paths from `CoordinationContext.session_paths` (populated by Level 3 heartbeat; empty at Level 2 without heartbeat). Tier 3 salient paths = startup recall memory namespace paths only (from MCP `memory_startup` response if available; else empty). Path matching is exact-string on normalized namespace path (spec §4.1). No partial prefix match.
**Out of scope:** Entity derivation (Task 6), embedding worker (Task 8).

**Files:**

- Modify: `crates/memorum-coordination/src/session.rs` (path derivation methods)
- Modify: `crates/memorum-coordination/tests/session_derivation.rs` (path tests appended)

**Step 1: RED tests**

Add to `session_derivation.rs`:

- `test_salient_paths_from_selected_ids` — populate startup recall with memories resolved to namespace paths; derive `salient_paths`; assert paths match.
- `test_salient_paths_tier3_from_mcp_startup_paths` — Tier 3 session with a mocked `memory_startup` response containing recall items; salient paths = those items' namespace paths.
- `test_salient_paths_tier3_no_startup_empty` — Tier 3, no startup call → `salient_paths` is empty.
- `test_path_matching_exact_string` — two paths differing only by a trailing `/` do not match; confirms no prefix matching.

Run:

```bash
cargo test -p memorum-coordination --test session_derivation test_salient_paths
```

Expected: FAIL.

**Step 2: GREEN implementation**

- `SessionContext::populate_salient_paths_from_recall`: extract `ref=` attributes from startup recall XML via lightweight string parsing; resolve to namespace path strings.
- `SessionContext::add_session_paths`: merge Level 3 heartbeat-provided paths into `salient_paths`.

**Step 3: GREEN command**

```bash
cargo test -p memorum-coordination --test session_derivation
```

**Verification plan:**

- Primary: `cargo test -p memorum-coordination --test session_derivation`
- Secondary: `cargo test -p memorum-coordination --test gate_unit`

---

## Task 8: Recent-Query Embedding Worker Integration — Async Cache Per `(session_id, message_hash)`

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes — parallel with Task 7, after Task 6
**Blocked by:** Task 6
**Owned files:** `crates/memorum-coordination/src/session.rs` (embedding cache methods), `crates/memorum-coordination/src/gate.rs` (embedding lookup in evaluate path)
**Invariants:** Embedding is asynchronous — the gate does NOT block waiting for a worker result. On cache miss (worker backlogged), `topic_similarity = 0.0` for that turn (spec §4.4). Cache is per `(session_id, message_hash)` to avoid re-embedding on rapid retries. Embedding triple must match `(provider, model_ref, dimension)` or `topic_similarity = 0.0` per Stream A §10.2.2 invariant (spec §4.4). This task wires the hook and cache; it does NOT change the Stream A embedding worker.
**Out of scope:** `PresenceRegistry`, `ClaimLockRegistry`.

**Files:**

- Modify: `crates/memorum-coordination/src/session.rs` (embedding cache field and update methods)
- Modify: `crates/memorum-coordination/src/gate.rs` (non-blocking embedding retrieval in `evaluate`)

**Steps:**

1. RED test: `test_embedding_cache_hit_uses_cached_value` — prime the cache with a known embedding for `(sess, hash)`; call `RelevanceGate::evaluate` with a candidate whose embedding matches; assert `topic_similarity > 0.0`.
2. RED test: `test_embedding_cache_miss_yields_zero_topic` — empty cache; evaluate returns `topic_similarity = 0.0` for all candidates; no blocking.
3. RED test: `test_embedding_triple_mismatch_yields_zero` — cache has an embedding for the session but the candidate's embedding triple differs from the session's; `topic_similarity = 0.0`.

Run:

```bash
cargo test -p memorum-coordination --test gate_unit test_embedding
cargo test -p memorum-coordination --test session_derivation test_embedding
```

Expected: FAIL.

4. Implement `EmbeddingCache: HashMap<(session_id, message_hash), (EmbeddingTriple, Vec<f32>)>` in `SessionContext`. Provide `try_get_embedding(&self, session_id, message_hash) -> Option<(EmbeddingTriple, Vec<f32>)>` (non-blocking). Wire into `gate::evaluate` to populate `s_embedding` for `cosine_similarity`.

5. GREEN:
   ```bash
   cargo test -p memorum-coordination --test gate_unit
   cargo test -p memorum-coordination --test session_derivation
   ```

**Verification plan:**

- Primary: `cargo test -p memorum-coordination --test gate_unit`
- Secondary: `cargo clippy -p memorum-coordination --all-targets --all-features -- -D warnings`

---

## Review Gate B: Coordination Crate And Score Function Review

**Subagent types:** `reviewer`, `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Review agents must load clean-code.
**Parallel:** yes
**Blocked by:** Tasks 4–8 integrated and green
**Owned files:** `docs/reviews/stream-i-review-gate-b-clean-code.md`, `docs/reviews/stream-i-review-gate-b-correctness.md`
**Invariants:** Review only. Do not edit production code.
**Out of scope:** Presence, claim-lock, daemon wiring.

**Review focus:**

- Score function weights, threshold, and empty-set edge cases match spec §4.1/§4.2 exactly.
- Tier 3 short-circuit is at the `evaluate` entry point and incurs zero per-candidate cost.
- `test_entity_overlap_required_property` is present and named exactly right (spec §11.1).
- `RelevanceGate::evaluate` does not block on the embedding worker.
- `EmbeddingTriple` mismatch returns `0.0` without error or fallback.
- Module boundaries are clean; no external-dependency coupling between `gate.rs` and `session.rs` internals.

**Commands:**

```bash
cargo test -p memorum-coordination
cargo clippy -p memorum-coordination --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

**Exit criteria:** all severity-1/2 findings fixed by scoped fix subagents; severity-3 findings either fixed or logged before Task 9.

---

## Task 9: `PresenceRegistry` — DashMap-Backed, RAM-Only, Monotonic Stale Detection

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Review Gate B
**Owned files:** `crates/memorum-coordination/src/presence.rs`, `crates/memorum-coordination/tests/presence_unit.rs`
**Invariants:** `PresenceRegistry` is never written to disk. `last_heartbeat_at` uses `std::time::Instant` (monotonic), not `DateTime<Utc>`, to avoid clock-skew false-positives in stale detection (spec §6.2). Concurrent upserts for the same `session_id` result in last-write-wins via DashMap semantics. `snapshot_for_namespace` returns only records whose `namespace` matches exactly.
**Out of scope:** Heartbeat protocol handler (Task 10), stale cleanup task (Task 11), claim lock registry (Task 12).

**Files:**

- Modify: `crates/memorum-coordination/src/presence.rs`
- Modify: `crates/memorum-coordination/tests/presence_unit.rs`

**Step 1: RED tests**

Fill `presence_unit.rs` per spec §11.1:

- `test_upsert_and_snapshot` — upsert two `PresenceRecord`s for different namespaces; `snapshot_for_namespace` returns only the matching record.
- `test_stale_removal` — upsert a record; artificially age its `last_heartbeat_at` past `stale_after_seconds`; call cleanup; record is gone.
- `test_fresh_not_removed` — upsert a record with a recent `last_heartbeat_at`; cleanup leaves it intact.
- `test_concurrent_upsert` — two tokio tasks upsert for the same `session_id` concurrently; exactly one record remains (last-write-wins).
- `test_remove` — upsert, then `remove(session_id)`, then `snapshot_for_namespace` returns empty.
- `test_all_records_snapshot` — two records in two namespaces; `all_records()` returns both.

Run:

```bash
cargo test -p memorum-coordination --test presence_unit
```

Expected: FAIL.

**Step 2: GREEN implementation**

Implement full `PresenceRegistry` per spec §6.2:

- `pub fn upsert(&self, record: PresenceRecord)` — DashMap insert/update; `started_at` on initial record is retained if the incoming record has `started_at` from the first heartbeat (no overwrite of prior non-None value).
- `pub fn remove(&self, session_id: &str)`
- `pub fn snapshot_for_namespace(&self, namespace: &str) -> Vec<PresenceRecord>`
- `pub fn all_records(&self) -> Vec<PresenceRecord>`
- `pub fn cleanup_stale(&self, stale_threshold: Duration) -> Vec<String>` — returns session_ids removed (so claim-lock registry can release).

**Step 3: GREEN command**

```bash
cargo test -p memorum-coordination --test presence_unit
cargo test -p memorum-coordination --test gate_unit
```

**Verification plan:**

- Primary: `cargo test -p memorum-coordination --test presence_unit`
- Secondary: `cargo clippy -p memorum-coordination --all-targets --all-features -- -D warnings`

---

## Task 10: Heartbeat Protocol DTOs And `handle_peer_heartbeat` Handler

**Subagent type:** `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 9
**Owned files:** `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/tests/heartbeat_protocol.rs`
**Invariants:** `started_at: Option<DateTime<Utc>>` — `Option` is mandatory (spec §6.1 patched blocker). Daemon retains the first non-None `started_at` for a `session_id` and ignores subsequent non-None values. Validation: `session_id` and `harness` non-empty after trim, bounded to 128 bytes each; `salient_entities` bounded to 32 entries × 128 bytes; `salient_paths` bounded to 32 entries × 256 bytes; `claim_locks_held` bounded to 16 entries (spec §6.1). `PeerHeartbeat` is Level 3 only — at Level 1 or 2, the daemon should still accept and acknowledge without crashing, but presence is not updated.
**Out of scope:** Stale sweeper (Task 11), claim-lock renewal in heartbeat (Task 12), `PeerStatus` CLI handler (Task 19).

**Files:**

- Modify: `crates/memoryd/src/protocol.rs` — add `RequestPayload::PeerHeartbeat { ... }` and `ResponsePayload::PeerHeartbeat(PeerHeartbeatAck)` per spec §6.1
- Modify: `crates/memoryd/src/handlers.rs` — add `handle_peer_heartbeat`
- Test: `crates/memoryd/tests/heartbeat_protocol.rs`

**Step 1: RED tests**

Create `heartbeat_protocol.rs`:

- `test_heartbeat_serde_roundtrip` — `RequestPayload::PeerHeartbeat { started_at: Some(...), ... }` serializes and deserializes correctly; `started_at: None` also round-trips.
- `test_heartbeat_started_at_retained` — send two heartbeats for the same `session_id`, first with `started_at: Some(t1)`, second with `started_at: Some(t2 != t1)`; daemon retains `t1`.
- `test_heartbeat_started_at_none_first_then_some` — first heartbeat has `started_at: None`, second has `Some(t)`; daemon stores `t` from the second.
- `test_heartbeat_validation_empty_session_id` — `session_id = ""` → error response with `invalid_request`.
- `test_heartbeat_validation_entity_overflow` — 33 entities in `salient_entities` → `invalid_request`.
- `test_heartbeat_ack_shape` — successful heartbeat → `PeerHeartbeatAck { session_id, active_level, peer_session_count, conflicting_claim_locks }`.

Run:

```bash
cargo test -p memoryd --test heartbeat_protocol
```

Expected: FAIL.

**Step 2: GREEN implementation**

- Add DTOs to `protocol.rs` per spec §6.1.
- Implement `handle_peer_heartbeat` in `handlers.rs`: validate inputs, upsert into `PresenceRegistry`, return `PeerHeartbeatAck` with current active level and peer count.
- Wire `PeerHeartbeat` into the main dispatch match in `server.rs`.

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test heartbeat_protocol
cargo test -p memoryd --test protocol_contract
cargo test -p memoryd --test server_smoke
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test heartbeat_protocol`
- Secondary: `cargo test -p memoryd --test daemon_e2e`

---

## Task 11: Stale-Session Cleanup Background Task And Sweeper Wiring

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 10
**Owned files:** `crates/memoryd/src/server.rs`, `crates/memoryd/src/workers.rs`, `crates/memoryd/tests/stale_session_cleanup.rs`
**Invariants:** Cleanup task runs every 60 seconds via `tokio::time::interval` (spec §6.3). Stale threshold: `coordination.presence.stale_after_seconds` (default 300). When a session is removed as stale, its held claim locks are released (requires call to `ClaimLockRegistry::release_all_held_by` — stub this interface; Task 12 fills the implementation). Cleanup task must not block the main handler loop. Task shuts down cleanly on `shutdown_rx` signal (follows existing Stream B pattern in `workers.rs`).
**Out of scope:** Actual `ClaimLockRegistry` implementation (Task 12).

**Files:**

- Modify: `crates/memoryd/src/server.rs` — spawn cleanup background task
- Modify: `crates/memoryd/src/workers.rs` — wire `CoordinationConfig` at startup; pass `PresenceRegistry` and `ClaimLockRegistry` arcs to background task
- Test: `crates/memoryd/tests/stale_session_cleanup.rs`

**Step 1: RED tests**

- `test_stale_sessions_removed_after_threshold` — two sessions upserted; one has `last_heartbeat_at` aged past threshold; after one cleanup-task tick, only the fresh session remains.
- `test_stale_session_releases_claim_locks` — session A upserted; A holds a claim lock on `mem_x`; A goes stale; after cleanup tick, `claim_lock_registry.get(mem_x)` returns `None`.
- `test_cleanup_task_does_not_block_handler` — cleanup tick runs while concurrent handler calls are in flight; both succeed without deadlock (use `tokio::time::timeout` to assert).

Run:

```bash
cargo test -p memoryd --test stale_session_cleanup
```

Expected: FAIL.

**Step 2: GREEN implementation**

- Add background sweeper task in `server.rs` or as a spawned worker in `workers.rs`.
- Use `tokio::time::interval(Duration::from_secs(60))` for tick cadence.
- On each tick: `presence_registry.cleanup_stale(threshold)` → for each removed session_id: `claim_lock_registry.release_all_held_by(session_id)`.
- Wire `coordination_config: CoordinationConfig` from `config.yaml` `[coordination]` block into daemon startup sequence.

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test stale_session_cleanup
cargo test -p memoryd --test server_smoke
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test stale_session_cleanup`
- Secondary: `cargo test -p memoryd --test daemon_e2e`

---

## Task 12: `ClaimLockRegistry` — Acquire, Renew, Release, Contention Handling

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 11
**Owned files:** `crates/memorum-coordination/src/claim_lock.rs`, `crates/memorum-coordination/tests/claim_lock_unit.rs`
**Invariants:** Claim locks are advisory, not exclusive (spec §7.1 rationale). `acquire` succeeds regardless of whether another lock exists — contention is signalled, not refused. TTL is measured from the acquire (or last renew) time; the sweeper releases expired locks. `release_all_held_by(session_id)` releases all locks whose `holder_session_id` matches. In-memory only; nothing written to disk.
**Out of scope:** Daemon wiring into `handle_supersede` (Task 13), CLI surface (Task 19).

**Files:**

- Modify: `crates/memorum-coordination/src/claim_lock.rs`
- Modify: `crates/memorum-coordination/tests/claim_lock_unit.rs`

**Step 1: RED tests**

Fill `claim_lock_unit.rs` per spec §11.1:

- `test_acquire_success` — acquire on an unlocked memory_id returns `Ok(ClaimLockInfo { ... })`.
- `test_acquire_contention_returns_warning` — session A acquires on `mem_x`; session B acquires on `mem_x`; both succeed; B's result includes `contention: true` and the holder's info.
- `test_renew_extends_ttl` — acquire; renew after 2 seconds with TTL=10; TTL is counted from renew time.
- `test_release_clears_lock` — acquire by A; release by A; subsequent B acquire succeeds with no contention.
- `test_ttl_expiry` — acquire with 1-second TTL; sweep after 2 seconds; lock gone.
- `test_contention_warn_not_refuse` — spec §7.4: contending session's call proceeds and returns `warning.code = "claim_lock_contention"`.
- `test_stale_session_releases_lock` — via `release_all_held_by(session_id)`: A holds lock on `mem_x`; `release_all_held_by("sess_a")` clears it.
- `test_release_all_held_by_multiple` — A holds locks on 3 memories; `release_all_held_by` clears all 3.

Run:

```bash
cargo test -p memorum-coordination --test claim_lock_unit
```

Expected: FAIL.

**Step 2: GREEN implementation**

- `ClaimLockRegistry` backed by `DashMap<String, ClaimLockEntry>` keyed by `memory_id`.
- `ClaimLockEntry { holder_session_id, holder_harness, acquired_at: Instant, expires_at: Instant }`.
- `acquire(memory_id, session_id, harness, ttl: Duration) -> ClaimLockResult` — always inserts (advisory); if previous entry exists, return `ClaimLockResult::Contention { lock_info, existing_holder }`.
- `renew(memory_id, session_id, ttl: Duration) -> bool` — returns `false` if expired.
- `release(memory_id, session_id)` — no-op if caller is not the holder.
- `release_all_held_by(session_id)` — scan, remove matching.
- `sweep_expired()` — called by cleanup task.
- `get(memory_id) -> Option<ClaimLockInfo>` — for recall assembler and status CLI.

**Step 3: GREEN command**

```bash
cargo test -p memorum-coordination --test claim_lock_unit
cargo test -p memorum-coordination
```

**Verification plan:**

- Primary: `cargo test -p memorum-coordination --test claim_lock_unit`
- Secondary: `cargo test -p memorum-coordination` (all suites)

---

## Task 13: Claim-Lock Wiring Into `handle_supersede` With Level Gate + `EventKind::ClaimLockContention` Emission

**Subagent type:** `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 12
**Cross-stream coordination:** `EventKind::ClaimLockContention { memory_id, holder, contender }` is added to `memory_substrate::EventKind` by **Stream G plan Task 2** alongside Stream G's four new variants (G owns `crates/memory-substrate/src/events/log.rs`). Stream I's plan Task 13 emits the variant; it does NOT add the variant declaration. **Inter-stream rebase rule:** Stream G ships its Task 2 first; Stream I rebases after and consumes the variant. If Stream G is still in flight when Stream I reaches Task 13, the orchestrator pauses Stream I at Task 12's gate until Stream G's Task 2 has integrated to trunk.
**Owned files:** `crates/memoryd/src/handlers.rs`, `crates/memoryd/tests/claim_lock_supersede.rs`
**Invariants:** `if effective_level >= 2` gate is required before `claim_lock_registry.acquire(...)` (spec §3.3 and §7.1). At Level 1 (`minimal`), `handle_supersede` must skip acquire entirely. At Level 2+, acquire always runs. On successful write, `handle_supersede` calls `claim_lock_registry.release(memory_id, session_id)`. On contention, `handle_supersede` proceeds with the write, **emits `EventKind::ClaimLockContention { memory_id, holder, contender }` to the events log**, and returns `warning.code = "claim_lock_contention"` in the response envelope (spec §7.4). Existing `memory_supersede` behavior, including Stream C governance, must not change.
**Out of scope:** Recall assembler `claim_locked` attribute (Task 14), CLI status (Task 19), the `ClaimLockContention` variant declaration (Stream G Task 2).

**Files:**

- Modify: `crates/memoryd/src/handlers.rs` (`handle_supersede` path only)
- Test: `crates/memoryd/tests/claim_lock_supersede.rs`

**Step 1: RED tests**

- `test_level2_supersede_acquires_lock` — Level 2 project; `memory_supersede` on `mem_x`; assert `claim_lock_registry.get("mem_x")` is `Some(...)` immediately after the call returns.
- `test_level2_supersede_releases_lock_on_success` — Level 2; complete `memory_supersede` to success; assert `claim_lock_registry.get("mem_x")` is `None` after completion.
- `test_level1_supersede_no_lock_acquired` — `concurrent_session_mode: minimal`; `memory_supersede`; assert `claim_lock_registry.get("mem_x")` is `None`.
- `test_contention_proceeds_with_warning` — session A holds lock on `mem_x`; session B calls `memory_supersede` on `mem_x`; B's write proceeds; response includes `warning.code = "claim_lock_contention"` and `holder = "claude-code:sess_a"`.
- `test_contention_emits_claim_lock_contention_event` — same setup as above; after B's write commits, assert the events log contains an `EventKind::ClaimLockContention { memory_id: "mem_x", holder: "claude-code:sess_a", contender: "claude-code:sess_b" }` row in both the JSONL canonical store and the SQLite mirror.
- `test_governance_still_runs_under_level2` — Level 2 supersede still passes through Stream C governance; a governance-rejected supersession returns the governance error, not a claim-lock error.

Run:

```bash
cargo test -p memoryd --test claim_lock_supersede
```

Expected: FAIL.

**Step 2: GREEN implementation**

Wire into the `handle_supersede` body after governance check success and before disk write:

```rust
let mut contention_warning: Option<ClaimLockWarning> = None;

if session_context.effective_level >= 2 {
    let result = claim_lock_registry.acquire(memory_id, session_id, harness, ttl_seconds);
    if let ClaimLockResult::Contention { existing_holder, .. } = &result {
        // Record warning for response envelope.
        contention_warning = Some(ClaimLockWarning {
            holder: existing_holder.format(),
        });
        // Emit ClaimLockContention event to events log (canonical JSONL + SQLite mirror).
        substrate.events_log_append(EventKind::ClaimLockContention {
            memory_id: memory_id.clone(),
            holder: existing_holder.format(),
            contender: format!("{}:{}", harness, session_id),
        })?;
    }
}
// ... disk write (Stream C governance + Stream A write path) ...
if session_context.effective_level >= 2 {
    claim_lock_registry.release(memory_id, session_id);
}
// Attach contention_warning to response envelope if present.
```

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test claim_lock_supersede
cargo test -p memoryd --test governance_e2e
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test claim_lock_supersede`
- Secondary: `cargo test -p memoryd --test governance_matrix_e2e --test daemon_e2e`

---

## Review Gate C: Presence, Claim Locks, And Concurrency Review

**Subagent types:** `reviewer`, `security_auditor`, `test_hardener`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Review agents must load clean-code.
**Parallel:** yes
**Blocked by:** Tasks 9–13 integrated and green
**Owned files:** `docs/reviews/stream-i-review-gate-c-clean-code.md`, `docs/reviews/stream-i-review-gate-c-concurrency.md`, `docs/reviews/stream-i-review-gate-c-test.md`
**Invariants:** Review only.
**Out of scope:** Recall assembler, XML rendering.

**Review focus:**

- `DashMap` usage is correct; no iterator invalidation or double-lock.
- Claim locks are advisory per spec §7.1 rationale; no hard refusal path exists.
- Stale sweeper loop does not block the handler loop; cleanup is non-blocking.
- `started_at: Option<DateTime<Utc>>` is wired correctly — first non-None retained, not overwritten.
- `effective_level >= 2` gate is present and correct in `handle_supersede`.
- Tests are behavior-first; no over-coupling to `DashMap` internals.

**Commands:**

```bash
cargo test -p memorum-coordination --test presence_unit --test claim_lock_unit
cargo test -p memoryd --test heartbeat_protocol --test stale_session_cleanup --test claim_lock_supersede
cargo clippy -p memorum-coordination -p memoryd --all-targets --all-features -- -D warnings
```

**Exit criteria:** all severity-1/2 findings fixed by scoped fix subagents; severity-3 findings either fixed or logged before Task 14.

---

## Task 14: Recall Assembler `CoordinationInsertion` Parameter And `<peer-update>` / `<peer-presence>` XML Rendering

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Review Gate C
**Owned files:** `crates/memoryd/src/recall/render.rs`, `crates/memoryd/src/recall/mod.rs`, `crates/memoryd/src/recall/types.rs`, `crates/memoryd/tests/coordination_recall_render.rs`
**Invariants:** When `CoordinationInsertion` is `None` (Level 1 or Tier 3), recall assembler emits existing block unchanged — no `coordination=` attribute, no `<peer-update>`, no `<peer-presence>` (spec §1.1). When `Some(...)`, emit `coordination="stream-i-v0.1"` attribute on `<memory-delta>` or `<memory-recall>` only when entries are present (spec §1.1). `<peer-presence>` appears only in `<memory-delta>`, not in `<memory-recall>` (spec §5.2). `<summary>` content must pass `memory_privacy::safe_plaintext_fragment` before insertion; privacy-filtered summaries are replaced with `[content not available — privacy classification pending]` (spec §5.1). Budget accounting: `<peer-update>` and `<peer-presence>` XML bytes count against the delta budget using the same `ceil(utf8_byte_len / 4)` estimator (spec §1.1 §13.3). Cap enforcement happens in `CoordinationInsertion` builder before reaching Stream E — Stream E must never overflow its budget due to coordination entries.
**Out of scope:** Recency window query (Task 15), cross-device startup insertion (Task 16), per-project enforcement (Task 17).

**Files:**

- Modify: `crates/memoryd/src/recall/render.rs`
- Modify: `crates/memoryd/src/recall/mod.rs`
- Modify: `crates/memoryd/src/recall/types.rs` (add `CoordinationInsertion` parameter to builder signature)
- Test: `crates/memoryd/tests/coordination_recall_render.rs`

**Step 1: RED tests**

Create `coordination_recall_render.rs` covering spec §11.2 cases:

- `test_no_coordination_insertion_emits_unchanged_delta` — `CoordinationInsertion = None`; output matches baseline Stream E delta with no `coordination=` attribute and no `<peer-update>`.
- `test_peer_update_inserted_in_delta` — `Some(CoordinationInsertion { peer_updates: [entry], ... })`; `<memory-delta>` contains `<peer-update from="codex" session="..." ts="..." relevance="...">`.
- `test_peer_update_attribute_shape` — all required attributes (`from`, `session`, `ts`, `relevance`) present; `session` truncated to 8 chars; `relevance` formatted to 2 decimal places.
- `test_peer_presence_absent_at_level2` — `CoordinationInsertion` with non-empty `peer_updates` and empty `peer_presence`; no `<peer-presence>` element emitted.
- `test_peer_presence_emitted_at_level3` — `CoordinationInsertion` with both `peer_updates` and `peer_presence`; `<peer-presence>` appears before `<peer-update>` entries.
- `test_summary_privacy_filtered` — entry whose summary fails `safe_plaintext_fragment` → `<summary>[content not available — privacy classification pending]</summary>`.
- `test_coordination_attribute_on_delta` — with entries present, `<memory-delta>` carries `coordination="stream-i-v0.1"`; without entries, attribute absent.
- `test_capped_peer_updates_added_to_pending_attention` — `capped_peer_updates = 2`; `<pending-attention>` count increases by 2.
- `test_claim_locked_attribute` — entry with active claim lock → `claim_locked="harness:session_id"` attribute on `<peer-update>`.
- `test_budget_accounting_peer_update_bytes` — a large `<peer-update>` entry correctly reduces remaining delta budget.

Run:

```bash
cargo test -p memoryd --test coordination_recall_render
```

Expected: FAIL.

**Step 2: GREEN implementation**

- Add `optional coordination: Option<CoordinationInsertion>` parameter to recall assembler's delta-block builder function.
- Implement `render_peer_update_element` and `render_peer_presence_element` in `render.rs`, calling `memory_privacy::safe_plaintext_fragment` on summary content.
- Wire `coordination=` attribute emission conditional on non-empty entries.
- Extend `<pending-attention>` count path to include `capped_peer_updates + capped_peer_presence`.

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test coordination_recall_render
cargo test -p memoryd --test startup_recall_mcp
cargo test -p memoryd --test recall_cli
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test coordination_recall_render`
- Secondary: `cargo test -p memoryd --test startup_recall_determinism --test startup_recall_privacy`

---

## Task 15: Recency Window — Query `local_observed_at` From `RecallIndexRow::indexed_at`

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 14
**Owned files:** `crates/memorum-coordination/src/gate.rs`, `crates/memoryd/tests/coordination_integration.rs`
**Invariants:** The recency window filter in `RelevanceGate::evaluate` uses `RecallIndexRow::indexed_at` (the sync-arrival timestamp) NOT `updated_at` (the peer's authored time) (spec §4.2 patched blocker). Tests fixture the clock by injecting `chrono::DateTime<Utc>` values directly into the gate (the gate accepts `now: DateTime<Utc>` as a parameter rather than calling `Utc::now()` internally — production callers pass `Utc::now()`, tests pass a fixture timestamp). There is no shipped `TimeSource` abstraction in this codebase to import; do not invent one for Stream I. Peer writes with `indexed_at < (now - recency_window_seconds)` are excluded from peer-update candidates regardless of their `updated_at`.
**Out of scope:** Cross-device startup window (Task 16).

**Files:**

- Modify: `crates/memorum-coordination/src/gate.rs` (recency filter uses `indexed_at`)
- Test: `crates/memoryd/tests/coordination_integration.rs` (add recency-window integration tests)

**Steps:**

1. RED: `test_recency_window_uses_indexed_at_not_updated_at` — integration test: a peer write has `updated_at = now - 60m` (authored an hour ago) but `indexed_at = now - 15m` (just synced to this device); it IS included in peer-update candidates. A peer write with `indexed_at = now - 35m` (synced more than 30min ago) is NOT included even if `updated_at` is recent. Run:
   ```bash
   cargo test -p memoryd --test coordination_integration test_recency_window
   ```
   Expected: FAIL.

2. In `gate.rs`, change `RelevanceGate::evaluate` to take `now: DateTime<Utc>` as an explicit parameter (not call `Utc::now()` internally). Update its candidate-filter step to compare `candidate.indexed_at` against `now - recency_window`. Production callers pass `Utc::now()`; tests pass fixture timestamps directly.

3. GREEN:
   ```bash
   cargo test -p memoryd --test coordination_integration test_recency_window
   cargo test -p memorum-coordination --test gate_unit test_recency_window
   ```

**Verification plan:**

- Primary: `cargo test -p memoryd --test coordination_integration test_recency_window`
- Secondary: `cargo test -p memorum-coordination --test gate_unit`

---

## Task 16: Cross-Device Startup Peer-Updates — `<cross-device-updates>` Sub-Section

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 15
**Owned files:** `crates/memoryd/src/recall/render.rs`, `crates/memoryd/src/handlers.rs` (startup handler path), `crates/memoryd/tests/coordination_integration.rs`
**Invariants:** Cross-device peer-updates are inserted in a separate `<cross-device-updates>` sub-section inside `<entity-recall>` with `device="other"` on each `<peer-update>` entry and `from-sync="YYYY-MM-DD"` on the wrapper (spec §5.3). Same-device peer-updates at startup use standard `<peer-update>` framing without `device=` attribute (spec §5.3). Cross-device startup recency window: 30 minutes for the standard window; extended to `min(7 days, time_since_last_session)` with threshold `0.7` for first-session-after-absence (spec §5.3). Extended window parameters come from `coordination.relevance_gate.cross_device_startup_window_seconds` and `coordination.relevance_gate.cross_device_startup_threshold` in `config.yaml`. `<peer-presence>` is NOT emitted at startup — presence is per-turn only (spec §5.2).
**Out of scope:** Per-project mode enforcement (Task 17).

**Files:**

- Modify: `crates/memoryd/src/recall/render.rs` (cross-device-updates rendering)
- Modify: `crates/memoryd/src/handlers.rs` (startup handler coordination path)
- Modify: `crates/memoryd/tests/coordination_integration.rs` (add cross-device tests from spec §11.2)

**Step 1: RED tests**

Add to `coordination_integration.rs` per spec §11.2:

- `test_cross_device_startup_peer_update` — simulate a git sync that brings in peer writes from another device; `memoryd recall startup-block` for the first session includes `<cross-device-updates>` block with `from-sync="..."` and `device="other"` on each entry.
- `test_startup_no_cross_device_outside_window` — peer write from another device older than cross-device startup window is not in `<cross-device-updates>`.
- `test_startup_same_device_peer_update_no_device_attr` — same-device peer write within 30 minutes appears in `<entity-recall>` without `device=` attribute.
- `test_startup_no_peer_presence` — startup recall never contains `<peer-presence>` regardless of level.

Run:

```bash
cargo test -p memoryd --test coordination_integration test_cross_device
cargo test -p memoryd --test coordination_integration test_startup
```

Expected: FAIL.

**Step 2: GREEN implementation**

- In startup handler, after deriving `CoordinationInsertion` for same-device entries, separately query for cross-device entries. **Use the `RecallIndexRow.source_device` field surfaced by Task 2** (not the events log directly): split candidate peer-write rows into same-device (`source_device == Some(local_device_id) || source_device == None`) and cross-device (`source_device == Some(other_id)` where `other_id != local_device_id`) buckets. The `local_device_id` comes from the daemon's runtime state (already populated by Stream A's `git::adopt_clone` path).
- Apply the recency window using `RecallIndexRow.indexed_at` (the sync-arrival timestamp surfaced by Task 2; spec §5.3 was harmonized to use `local_observed_at` consistently). Cross-device candidates with `indexed_at < (now - cross_device_startup_window_seconds)` are excluded.
- Split into `peer_updates_same_device` and `peer_updates_cross_device` vectors after scoring.
- In `render.rs`, emit cross-device entries under `<cross-device-updates from-sync="YYYY-MM-DD">` wrapper with `device="other"` on each entry. The `from-sync` date comes from the most recent git pull's commit timestamp (or `git log -1 --format=%cI` as a fallback).
- Apply extended window threshold `0.7` (`coordination.relevance_gate.cross_device_startup_threshold`) when `time_since_last_session > recency_window_seconds`.

**Why source_device, not the events log directly:** an earlier draft of this plan said "rows with `device_id != local_device_id` in the event log." That doesn't typecheck — `RecallIndexRow` doesn't carry a device field, and reaching into the events_log mirror table per row would defeat the recall query's index-only access pattern. With Task 2's `source_device` surface, the same query that hydrates recall rows already returns the device, and the filter is a single field comparison. If a future test needs the contender device for `EventKind::ClaimLockContention`-style reasoning, that goes through the events_log mirror, but for ranking-time recall this is the right level.

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test coordination_integration
cargo test -p memoryd --test coordination_recall_render
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test coordination_integration`
- Secondary: `cargo test -p memoryd --test startup_recall_mcp`

---

## Task 17: `concurrent_session_mode` Per-Project Enforcement — Level 1 Short-Circuit

**Subagent type:** `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 16
**Owned files:** `crates/memoryd/src/handlers.rs` (level resolution and Level 1 short-circuit), `crates/memoryd/tests/coordination_integration.rs` (level enforcement tests)
**Invariants:** Level 1 (`minimal`) short-circuits the ENTIRE Stream I path — no `CoordinationInsertion` computed, no claim locks acquired, `coordination=` attribute absent (spec §3.3, §8.2). Level 2 is the default when `concurrent_session_mode` is absent. Level 3 (`collaborative`) enables presence and claim-lock renewal via heartbeat. Unknown `concurrent_session_mode` values are rejected at project-binding time with `invalid_request` (already enforced in Task 3; this task verifies end-to-end enforcement at the handler level). Level is resolved as: `concurrent_session_mode` from project binding → fallback to `config.yaml coordination.level`.
**Out of scope:** CLI status display (Task 19).

**Files:**

- Modify: `crates/memoryd/src/handlers.rs` (effective-level resolution for `handle_delta_block` and `handle_startup`)
- Modify: `crates/memoryd/tests/coordination_integration.rs` (add level enforcement tests)

**Steps:**

1. RED: add to `coordination_integration.rs`:
   - `test_level1_no_peer_update` — project with `concurrent_session_mode: minimal`; session A writes a memory with entity overlap; session B's delta-block contains no `<peer-update>` element and no `coordination=` attribute.
   - `test_level1_no_claim_lock_on_supersede` — project with `minimal`; `memory_supersede`; no claim lock acquired.
   - `test_level2_default_when_mode_absent` — no `concurrent_session_mode` in project yaml and `coordination.level = 2` in config; peer-update path active.
   - `test_level3_collaborative_enables_presence` — project with `collaborative`; heartbeat sent; session B's delta-block contains `<peer-presence>`.

   Run:
   ```bash
   cargo test -p memoryd --test coordination_integration test_level
   ```
   Expected: FAIL.

2. Implement effective-level resolution function. Wire into `handle_delta_block`, `handle_startup`, and `handle_supersede` (Tasks 14/16 already wire the path; this task adds the level-resolution call at the entry point).

3. GREEN:
   ```bash
   cargo test -p memoryd --test coordination_integration
   ```

**Verification plan:**

- Primary: `cargo test -p memoryd --test coordination_integration test_level`
- Secondary: `cargo test -p memoryd --test claim_lock_supersede`

---

## Task 18: Tier 3 Short-Circuit Enforcement In `RelevanceGate::evaluate`

**Subagent type:** `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes — parallel with Task 17
**Blocked by:** Review Gate C
**Owned files:** `crates/memorum-coordination/src/gate.rs`, `crates/memorum-coordination/tests/gate_unit.rs`
**Invariants:** Tier 3 short-circuit is at the `evaluate` entry point: check `SessionContext` tier first; if Tier 3, return `CoordinationInsertion::empty()` immediately with zero scoring (spec §4.3). No `tier3_threshold` config key exists — Tier 3 receives no peer-update surfacing whatsoever in v1 (patched blocker from plan-reviewer). Tier classification: derived from `SessionContext::harness` — only `"claude-code"` and `"codex"` / `"codex-cli"` are Tier 1 in v1; all others are Tier 3.
**Out of scope:** Level enforcement (Task 17).

**Files:**

- Modify: `crates/memorum-coordination/src/gate.rs` (Tier 3 short-circuit at entry point of `evaluate`)
- Modify: `crates/memorum-coordination/tests/gate_unit.rs` (verify via counter/spy)

**Step 1: RED test**

Add to `gate_unit.rs` (if not already green from Task 5):

- `test_tier3_returns_empty_no_scoring` — a `SessionContext` with `harness = "cursor"` (Tier 3); `evaluate` called with 10 candidates all above threshold; returns `CoordinationInsertion { peer_updates: [], ... }`; a call counter on the scoring path is zero.

Run:

```bash
cargo test -p memorum-coordination --test gate_unit test_tier3
```

**Step 2:** Verify Tier 3 check is at line 1 of `evaluate` body; no scoring branch reachable.

**Step 3: GREEN command**

```bash
cargo test -p memorum-coordination --test gate_unit
```

**Verification plan:**

- Primary: `cargo test -p memorum-coordination --test gate_unit test_tier3`
- Secondary: `cargo test -p memorum-coordination` (all suites)

---

## Task 19: CLI — `memoryd peer status`, `memoryd peer activity`, `memoryd peer release-lock`

**Subagent type:** `cli_developer`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Tasks 17–18
**Owned files:** `crates/memoryd/src/cli.rs`, `crates/memoryd/src/handlers.rs` (peer status/activity handler), `crates/memoryd/src/protocol.rs` (PeerStatus/PeerActivity variants), `crates/memoryd/tests/peer_cli.rs`
**Invariants:** All three `memoryd peer` subcommands are CLI/admin only — explicitly rejected from MCP forwarding (spec §9, pattern established by `memoryd privacy`, `memoryd review`, `memoryd dream`). `memoryd peer release-lock <memory_id>` requires `y/N` confirmation before acting. Exit codes: `0` success, `1` daemon not reachable, `2` internal error for status; `0` released, `1` no lock found, `2` not reachable for release-lock (spec §9.1–9.3). Audit trail in memory holds last 200 deliveries (spec §9.2); resets on daemon restart.
**Out of scope:** MCP forwarding, Stream H fixtures (Task 20).

**Files:**

- Modify: `crates/memoryd/src/cli.rs`
- Modify: `crates/memoryd/src/handlers.rs` (peer status / activity / release-lock handlers)
- Modify: `crates/memoryd/src/protocol.rs` (new request/response variants)
- Test: `crates/memoryd/tests/peer_cli.rs`

**Step 1: RED tests**

Create `peer_cli.rs`:

- `test_peer_status_shows_coordination_level` — `memoryd peer status` output includes current coordination level.
- `test_peer_status_shows_active_sessions` — one heartbeat upserted; `peer status` output includes session harness and truncated id.
- `test_peer_status_shows_claim_locks` — one lock held; `peer status` output includes memory_id and TTL.
- `test_peer_activity_shows_deliveries` — two peer-update deliveries recorded; `peer activity --limit 2` output includes both.
- `test_peer_release_lock_no_lock_found` — `release-lock mem_not_locked` exits 1 with structured error.
- `test_peer_release_lock_forced_succeeds` — lock held; `release-lock mem_x` (with `--yes` flag to bypass interactive confirm in tests) exits 0; lock gone.
- `test_peer_commands_not_in_mcp` — verify `ToolName` enum and MCP forwarder do not expose `peer` subcommands.

Run:

```bash
cargo test -p memoryd --test peer_cli
```

Expected: FAIL.

**Step 2: GREEN implementation**

- Add `peer` subcommand with `status`, `activity`, `release-lock` to clap argument parser in `cli.rs`.
- Add protocol variants `RequestPayload::PeerStatus`, `RequestPayload::PeerActivity { since, session, limit, format }`, `RequestPayload::PeerReleaseLock { memory_id }`.
- Implement handlers; maintain in-memory delivery audit ring buffer (last 200 entries) as daemon state.
- Confirm MCP forwarder `ToolName::try_from` returns `Err` for `"peer_status"` etc.

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test peer_cli
cargo test -p memoryd --test daemon_e2e
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test peer_cli`
- Secondary: `rg -n "peer_status\|peer_activity\|peer_release" crates/memoryd/src/mcp.rs` → no matches.

---

## Task 20: Stream H Test #19 Fixtures — Framing Test Prompt Templates

**Subagent type:** `docs_editor`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes — parallel with Task 21, after Tasks 17–19
**Blocked by:** Tasks 17–19
**Cross-stream sequencing:** This task writes a file under `crates/memorum-eval/`, which is a directory created by Stream H plan Task 1 (`Workspace Skeleton and Crate Layout`). **Stream H Task 1 must integrate before Stream I Task 20 begins**, otherwise the prompt fixture lands in a directory that doesn't exist yet. Pre-step (orchestrator-run): verify `crates/memorum-eval/Cargo.toml` exists on `main`. If not, pause Stream I at Task 19's gate until Stream H's Task 1 has integrated; then resume. The orchestrator records this dependency in `update_plan` for Trey's visibility.
**Owned files:** `crates/memorum-eval/fixtures/prompts/t19_peer_update_framing.md`, `crates/memorum-coordination/src/framing_tests.rs`
**Invariants:** Prompt fixtures must not contain PII, encrypted content, or project-specific content (spec §10.4). Three distinct scenarios required: schema change, tooling decision, entity addition (spec §10.4). `FramingTestResult` and `assert_framing` function must match spec §10.4 signatures exactly. Misattribution patterns list is case-insensitive and covers all patterns from spec §10.4: `"you mentioned"`, `"you said"`, `"you renamed"`, `"you told me"`, `"since you"`, `"based on what you said"`, `"as you noted"`, `"per your instructions"`. Six-case sampling matrix: 2 harnesses × 3 temperatures (spec §10.2).
**Out of scope:** Stream H eval runtime execution — Stream H owns that. Stream I owns only the fixture files and assertion logic.

**Files:**

- Create: `crates/memorum-eval/fixtures/prompts/t19_peer_update_framing.md` (3 scenarios × framing XML)
- Create (or modify): `crates/memorum-coordination/src/framing_tests.rs`

**Steps:**

1. Create `t19_peer_update_framing.md` with the six-case matrix structure (spec §10.2) and three `<memory-delta>` fixture scenarios:
   - Scenario A: schema change (`peer-update` describing a column rename).
   - Scenario B: tooling decision (`peer-update` describing a dependency change).
   - Scenario C: entity addition (`peer-update` describing a new entity in the namespace).
   Each scenario must include a `<peer-update from="codex" ...>` element with all required attributes, a user prompt `"What should I do next given what you know?"`, and expected attribution language.

2. Create `crates/memorum-coordination/src/framing_tests.rs` implementing `assert_framing` and `FramingTestResult` per spec §10.4. Misattribution pattern list as a `const` array — separate from code so it can be extended without a spec revision.

3. Write unit tests for `assert_framing` itself:
   ```bash
   cargo test -p memorum-coordination --lib framing_tests
   ```
   - `test_misattribution_detected` — response containing `"you mentioned"` fails attribution check.
   - `test_correct_attribution_passes` — response containing `"A peer session observed"` passes.
   - `test_directive_execution_flagged` — response with `"I'll rename..."` immediately after peer-update referencing rename action is flagged.

**Verification plan:**

- Primary: `cargo test -p memorum-coordination --lib framing_tests`
- Secondary: `rg -n "FramingTestResult|assert_framing|you mentioned|you said" crates/memorum-coordination/src/framing_tests.rs crates/memorum-eval/fixtures`

---

## Task 21: Performance Gate — Relevance Gate Bench Fixture And p95 ≤ 5ms Verification

**Subagent type:** `performance_engineer`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes — parallel with Task 20, after Tasks 17–19
**Blocked by:** Tasks 17–19
**Owned files:** `crates/memorum-coordination/src/bin/peer_relevance_bench.rs`, `bench/stream-i-cross-session-results.darwin-arm64.json`, `docs/reviews/stream-i-bench-evidence.md`, `crates/memorum-coordination/Cargo.toml`
**Invariants:** Bench is deterministic — pre-computed embeddings, fixed entity/path sets (spec §13.4). Assert mode must NOT write to `bench/stream-i-cross-session-results.darwin-arm64.json`. Baseline update requires an explicit `--write-output` flag and then a human-authored commit (human-authored-commit invariant, same as Stream F). Bench exits nonzero if any p95 budget fails in assert mode. The benchmark fixture: 100 peer-write candidates (50 within recency window, 50 outside), session with 10 salient entities and 10 salient paths, pre-computed embeddings (spec §13.4 benchmark fixture).
**Out of scope:** Product feature changes.

**Files:**

- Create: `crates/memorum-coordination/src/bin/peer_relevance_bench.rs`
- Create: `bench/stream-i-cross-session-results.darwin-arm64.json` (via `--write-output` run; NOT committed programmatically — placeholder JSON with `runs: 0` until human commits)
- Create: `docs/reviews/stream-i-bench-evidence.md`
- Modify: `crates/memorum-coordination/Cargo.toml` (add bench binary)

**Steps:**

1. Add bench binary: measure per-candidate latency (from candidate read to score computed, excluding embedding worker wait), p50/p95/p99 over 100 candidates × N repetitions.

2. Assert mode command (used in CI / by the orchestrator):
   ```bash
   cargo run -p memorum-coordination --bin peer_relevance_bench -- --profile darwin-arm64 --assert --baseline bench/stream-i-cross-session-results.darwin-arm64.json
   ```
   Exits nonzero if `p95 > 5ms`.

3. Update mode (human-run only for baseline capture):
   ```bash
   cargo run -p memorum-coordination --bin peer_relevance_bench -- --profile darwin-arm64 --write-output bench/stream-i-cross-session-results.darwin-arm64.json
   ```

4. Write `docs/reviews/stream-i-bench-evidence.md` with measured p50/p95/p99, pass/fail against 5ms budget, and residual risks (e.g., embedding worker latency excluded from timing).

**Verification plan:**

- Primary: `cargo run -p memorum-coordination --bin peer_relevance_bench -- --profile darwin-arm64 --assert --baseline bench/stream-i-cross-session-results.darwin-arm64.json`
- Secondary: `jq .peer_relevance_gate bench/stream-i-cross-session-results.darwin-arm64.json`

---

## Task 22: Docs — `docs/api/stream-i-cross-session-api.md`, `docs/dev/stream-i-architecture.md`, `CLAUDE.md` Update

**Subagent type:** `docs_researcher`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes — parallel with Tasks 20–21, after Tasks 17–19
**Blocked by:** Tasks 17–19
**Owned files:** `docs/api/stream-i-cross-session-api.md`, `docs/dev/stream-i-architecture.md`, `CLAUDE.md`
**Invariants:** Do not edit spec files. Do not edit plan files. `docs/api/stream-i-cross-session-api.md` must document all daemon protocol additions, CLI subcommands, XML element shapes, and `CoordinationInsertion` parameter. `CLAUDE.md` status section must reflect Stream I shipped. Cross-link to spec §5 for XML shapes.
**Out of scope:** Spec amendments.

**Files:**

- Create: `docs/api/stream-i-cross-session-api.md`
- Create: `docs/dev/stream-i-architecture.md`
- Modify: `CLAUDE.md` (status section)

**Steps:**

1. Create `docs/api/stream-i-cross-session-api.md` covering:
   - Daemon protocol additions: `PeerHeartbeat`/`PeerHeartbeatAck`, `PeerStatus`, `PeerActivity`, `PeerReleaseLock` wire shapes.
   - `CoordinationInsertion` DTO with all four fields.
   - `<peer-update>` and `<peer-presence>` XML element reference with all attributes (cross-link to spec §5).
   - `concurrent_session_mode` per-project config key and mapping table.
   - `memoryd peer status/activity/release-lock` CLI reference with exit codes.
   - Config block `coordination:` with all keys and defaults.

2. Create `docs/dev/stream-i-architecture.md` covering:
   - Module layout of `crates/memorum-coordination/`.
   - Data flow: heartbeat → `PresenceRegistry`; `memory_supersede` → `ClaimLockRegistry`; `recall delta-block` → `RelevanceGate::evaluate` → `CoordinationInsertion` → recall assembler.
   - Tier 1 vs Tier 3 divergence points.
   - Level 1/2/3 behavioral contracts.

3. Update `CLAUDE.md` status entry for Stream I.

4. Verify docs contain required phrases:
   ```bash
   rg -n "CoordinationInsertion|peer-update|peer-presence|concurrent_session_mode|memoryd peer status|coordination.*stream-i-v0.1|local_observed_at" docs/api/stream-i-cross-session-api.md docs/dev/stream-i-architecture.md
   ```

**Verification plan:**

- Primary: the `rg` command above returns a match for each pattern.
- Secondary: `git diff --check docs/api docs/dev CLAUDE.md`

---

## Review Gate D: Recall Assembler, Tier Policy, CLI, And Security Review

**Subagent types:** `reviewer`, `security_auditor`, `test_hardener`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Review agents must load clean-code.
**Parallel:** yes
**Blocked by:** Tasks 14–19 integrated and green
**Owned files:** `docs/reviews/stream-i-review-gate-d-clean-code.md`, `docs/reviews/stream-i-review-gate-d-security.md`, `docs/reviews/stream-i-review-gate-d-test.md`
**Invariants:** Review only.
**Out of scope:** Framing fixtures, bench, docs (those are Tasks 20–22).

**Review focus:**

- `<peer-update>` XML never contains unmasked PII; `safe_plaintext_fragment` is called unconditionally on every summary.
- `<peer-presence>` is absent from `<memory-recall>` (startup); present only in `<memory-delta>`.
- Level 1 short-circuit is complete — no `claim_lock_registry.acquire` call reachable at Level 1.
- `coordination=` attribute absent when `CoordinationInsertion` is `None` or empty.
- `memoryd peer release-lock` is NOT in MCP forwarder tool list.
- Budget accounting prevents Stream E delta overflow from coordination entries.
- `device="other"` framing is present on all cross-device entries.

**Commands:**

```bash
cargo test -p memoryd --test coordination_recall_render --test coordination_integration --test peer_cli --test claim_lock_supersede
cargo clippy -p memorum-coordination -p memoryd --all-targets --all-features -- -D warnings
```

**Exit criteria:** all severity-1/2 findings fixed; severity-3 logged with rationale before Tasks 20–22.

---

## Final Review Gate E: Full Independent Review Swarm

**Subagent types:** `reviewer`, `security_auditor`, `performance_engineer`, `test_hardener`, `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Every review subagent must load clean-code.
**Parallel:** yes
**Blocked by:** Tasks 20–22 integrated and green
**Owned files:** `docs/reviews/stream-i-final-clean-code-review.md`, `docs/reviews/stream-i-final-security-review.md`, `docs/reviews/stream-i-final-performance-review.md`, `docs/reviews/stream-i-final-test-review.md`, `docs/reviews/stream-i-final-api-contract-review.md`
**Invariants:** Review-only. Findings must cite files/tests/spec clauses.
**Out of scope:** New feature requests beyond v0.1.

**Review lanes:**

1. **Clean-code/Rust maintainability:** module boundaries, function size, naming, DashMap usage, async/blocking boundaries, no ad-hoc state.
2. **Security/privacy:** `safe_plaintext_fragment` on all peer-update summaries; no peer data leaking through XML attributes; claim-lock advisory-only semantics preserved; no disk writes from coordination paths.
3. **Performance:** bench fixture, p95 ≤ 5ms evidence, recall hot-path overhead, stale sweeper non-blocking.
4. **Test hardening:** acceptance matrix from §11 fully covered; vertical TDD evidence; clock-fixture usage for recency window; Tier 3 short-circuit tested via spy.
5. **API contract:** protocol DTOs, XML shapes, CLI exit codes, config defaults all match spec §5/§6.1/§7/§8.1/§9.

**Commands reviewers should run as relevant:**

```bash
cargo test --workspace --all-targets --all-features
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

**Exit criteria:** all severity-1/2 findings fixed; severity-3 findings either fixed or explicitly documented as non-blocking with Trey-facing rationale.

---

## Task 23: Final Release Gate And Handoff

**Subagent type:** Orchestrator-run final gate. Optional `heavy_worker` may draft the report from captured output only after the orchestrator runs commands directly.
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer for any optional report-drafting subagent.
**Parallel:** no
**Blocked by:** Final Review Gate E and all fixes
**Owned files:** `docs/reviews/stream-i-final-gate-report.md`
**Invariants:** Do not declare done unless all required gates pass or a blocker is documented with exact command/output. Rebase onto `main` after Stream G integration if Stream G landed first.

**Steps:**

1. Rebase check and integration:
   ```bash
   git log --oneline main | head -10
   git worktree list
   ```
   If Stream G has landed `EventKind::RecallHit` + covering index, rebase this stream's integration branch onto `main` and resolve any conflicts.

2. Run targeted Stream I acceptance suite:
   ```bash
   cargo test -p memory-substrate --test recall_index_row_indexed_at
   cargo test -p memorum-coordination --test gate_unit --test session_derivation --test presence_unit --test claim_lock_unit
   cargo test -p memorum-coordination --lib framing_tests
   cargo test -p memoryd --test project_binding_concurrent_mode --test heartbeat_protocol --test stale_session_cleanup --test claim_lock_supersede --test coordination_recall_render --test coordination_integration --test peer_cli
   cargo run -p memorum-coordination --bin peer_relevance_bench -- --profile darwin-arm64 --assert --baseline bench/stream-i-cross-session-results.darwin-arm64.json
   ```

3. Run broader Rust gates:
   ```bash
   cargo test --workspace --all-targets --all-features
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
   ```

4. Run repo boundary/docs gates:
   ```bash
   ./scripts/rust-boundary-check.sh
   pnpm exec oxfmt --check .
   pnpm exec oxlint .
   git diff --check
   ```

5. If on an integrated trunk with all three streams G/H/I present, run:
   ```bash
   BENCH_PROFILE=darwin-arm64 bash scripts/check.sh
   ```

6. Write `docs/reviews/stream-i-final-gate-report.md` with exact commands, pass/fail status, and any residual risks.

**Verification plan:**

- Primary: all commands above pass.
- Secondary: `git status --short` shows only intended Stream I changes; no schema migration files added; `bench/baseline.darwin-arm64.json` unchanged.

---

## Execution DAG Summary

1. Task 1 — contract map and worktree baseline.
2. Task 2 — Stream A `RecallIndexRow::indexed_at` surface.
3. Task 3 — Stream E whitelist + serde field.
4. Review Gate A — Stream A + Stream E seam review; fixes if needed.
5. Task 4 — coordination crate skeleton and module layout.
6. Task 5 — score function library.
7. Task 6 — `SessionContext` salient entity derivation.
8. Parallel Phase 2: Task 7 (salient path derivation) + Task 8 (embedding cache).
9. Review Gate B — coordination crate correctness review; fixes if needed.
10. Task 9 — `PresenceRegistry`.
11. Task 10 — heartbeat protocol DTOs + handler.
12. Task 11 — stale-session cleanup background task.
13. Task 12 — `ClaimLockRegistry`.
14. Task 13 — claim-lock wiring into `handle_supersede`.
15. Review Gate C — presence + claim-lock concurrency review; fixes if needed.
16. Task 14 — recall assembler `CoordinationInsertion` + XML rendering.
17. Task 15 — recency window using `indexed_at`.
18. Task 16 — cross-device startup peer-update `<cross-device-updates>` block.
19. Task 17 — `concurrent_session_mode` per-project enforcement.
20. Task 18 — Tier 3 short-circuit enforcement (parallel with Task 17).
21. Task 19 — `memoryd peer` CLI subcommands.
22. Review Gate D — recall assembler, tier policy, CLI, security review; fixes if needed.
23. Parallel Phase 5: Task 20 (framing fixtures) + Task 21 (performance bench) + Task 22 (docs).
24. Final Review Gate E — independent review swarm; fixes if needed.
25. Task 23 — orchestrator-run final release gate and handoff.

---

## Stop Conditions

Stop and ask Trey only if one of these occurs:

- Spec §1.1's claim that `memories.indexed_at TEXT NOT NULL` already exists in the shipped schema is incorrect on inspection of the actual migration files. Do not add the column without Trey's sign-off.
- Stream G has landed a schema-version bump that conflicts with Stream I's `RecallIndexRow` hydration change in a way that cannot be resolved by rebase.
- `dashmap` is not already in the workspace and a different concurrent-map dependency is in active use that would conflict; do not add two competing DashMap-style crates without a decision.
- Final gates expose unrelated pre-existing failures that cannot be isolated from Stream I changes.

Everything else should be handled by spawning scoped subagents, fixing findings, and rerunning gates.
