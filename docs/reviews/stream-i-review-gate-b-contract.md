# Stream I Review Gate B Contract Review

Verdict: Changes requested

Scope: read-only contract/correctness review of Stream I Tasks 4-8 in `crates/memorum-coordination`, checked against `docs/plans/2026-05-01-stream-i-cross-session.md` Review Gate B / Tasks 4-8 and `docs/specs/stream-i-cross-session-v0.1.md`.

## Findings

### Severity 2 - `PeerUpdateEntry.reference` is populated with the namespace path instead of the memory/substrate id

- Location: `crates/memorum-coordination/src/gate.rs:163-174`
- Contract: spec §5.1 says `<ref>` is the memory id (`mem_...`) or substrate fragment id (`sub_...`).
- Current behavior: `peer_update_entry` sets `reference: candidate.row.path.as_str().to_string()`.
- Impact: any downstream renderer using this DTO will emit a path in `<ref>`, losing the stable write id and duplicating the namespace/path information. This also weakens follow-up mechanics that need the surfaced memory id for claim-lock/cool-down/audit correlation.
- Required change: populate `PeerUpdateEntry.reference` from the candidate's stable write id (`candidate.memory_id` for memory rows; the substrate fragment id for substrate observations once those candidates are represented). Add a unit test asserting the DTO `reference` equals the memory id, not `row.path`.

### Severity 2 - Cool-down is checked but not recorded when entries are surfaced

- Location: `crates/memorum-coordination/src/gate.rs:28-68`; `crates/memorum-coordination/src/session.rs:51`; `crates/memorum-coordination/tests/gate_unit.rs:86-97`
- Contract: spec §4.2 says a peer-write is not surfaced to the same session more than once, and the in-memory per-session cool-down registry tracks surfaced peer-write ids once delivered.
- Current behavior: `evaluate` filters ids already present in `session.surfaced_peer_writes`, but takes `&SessionContext` and never records the ids it selects. The existing test only covers a pre-seeded registry entry; it does not prove that a surfaced id is added after delivery.
- Impact: repeated calls with the same `SessionContext` and candidates can return the same `peer_updates` indefinitely unless an unstated caller mutates `surfaced_peer_writes` out of band. That violates the stable cool-down contract at the gate boundary.
- Required change: either make `evaluate` own the mutation through an explicit `&mut SessionContext`/registry method, or extend the return contract so the caller must atomically record selected ids before rendering. In either design, add a behavior test that calls `evaluate` twice for the same session/candidate and asserts the second call does not return the candidate.

### Severity 3 - `device` is set from `source_device` for all peer updates, but §5.1 reserves it for cross-device startup framing

- Location: `crates/memorum-coordination/src/gate.rs:173`; `crates/memorum-coordination/src/protocol.rs:32-33`
- Contract: spec §5.1 says `device` is conditional and present only in `<memory-recall>` cross-device blocks, with value `"other"`.
- Current behavior: every selected candidate with `row.source_device = Some(...)` gets that raw device id copied into the DTO, even in the same-device delta relevance path covered by Tasks 4-8.
- Impact: when Task 14 renders this DTO, same-device delta peer-updates may incorrectly include a `device` attribute, and cross-device entries may expose a raw device id instead of the spec's `"other"` framing.
- Required change: keep `device` unset for normal same-device gate output. Introduce an explicit cross-device/startup mapping path that sets `device = Some("other")` only for the §5.3 cross-device startup use case.

## Contract checks

- `CoordinationConfig` defaults: pass. Defaults match spec §8.1: level 2, threshold 0.6, recency 1800s, cap 2, cross-device window 86400s, cross-device threshold 0.7, heartbeat 60s, stale-after 300s, claim-lock TTL 300s.
- `CoordinationInsertion` shape: pass for the four §1.1 fields and `empty()` helper.
- DTO shape: partial pass. `PeerUpdateEntry` and `PeerPresenceEntry` carry the required rendering data, but the current gate population has the `reference` and `device` contract issues above.
- Score weights/threshold/cap/sort/recency: pass. Weights are 0.5/0.3/0.2; threshold is config default 0.6; recency uses `local_observed_at`; cap is 2; sort is score desc, `updated_at` desc, memory id asc.
- Tier 3 short-circuit: pass in implementation. `evaluate` checks `session.is_tier3()` before computing embeddings or iterating candidates.
- Entity/path derivation: pass for current scope. Tier 1 entity/path derivation uses startup recall plus supplied FTS5 ids/session paths; Tier 3 binding-derived entities match §4.3; Tier 3 paths come only from startup recall; path matching is exact-string and no prefix matching was found.
- Embedding cache: pass for current scope. Cache is keyed by `(session_id, message_hash)`, lookup is synchronous/non-blocking, cache miss yields no session embedding, and mismatched triples yield `0.0` topic score.
- `memory-governance` dependency: pass. `crates/memorum-coordination/Cargo.toml` has no direct dependency, and `cargo tree -p memorum-coordination | rg "memory-governance"` returned no matches.

## Validation run

- `cargo test -p memorum-coordination --test gate_unit --test session_derivation` - pass (14 gate tests, 7 session derivation tests).
- `cargo test -p memorum-coordination` - pass (all coordination crate tests/doc-tests).
- `cargo clippy -p memorum-coordination --all-targets --all-features -- -D warnings` - pass.
- `cargo fmt --package memorum-coordination -- --check` - pass.
- `cargo fmt --all -- --check` - fail due pre-existing formatting diffs outside `crates/memorum-coordination` (`crates/memoryd/src/handlers.rs`, `crates/memoryd/src/reality_check/*`). I did not edit those files.
- `cargo tree -p memorum-coordination | rg "memory-governance"` - no matches.
