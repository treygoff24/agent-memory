# Stream I Architecture

Stream I implements cross-session coordination without changing Memorum's persistence authority. Stream A remains the canonical substrate and index, Stream B/memoryd owns daemon protocol and in-process state, Stream E owns recall XML assembly, and `crates/memorum-coordination/` owns the reusable coordination primitives.

## Crate layout: `crates/memorum-coordination/`

| Module             | Responsibility                                                                                                                                                                 |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `config.rs`        | `CoordinationConfig` defaults and nested relevance-gate, presence, and claim-lock timing.                                                                                      |
| `protocol.rs`      | Wire/DTO shapes: `PeerHeartbeat`, `PeerHeartbeatAck`, `CoordinationInsertion`, `PeerUpdateEntry`, `PeerPresenceEntry`, `ClaimLockInfo`.                                        |
| `session.rs`       | `SessionContext`, `ConcurrentSessionMode`, project binding vocabulary, startup recall/entity/path derivation, recent-query embedding cache.                                    |
| `gate.rs`          | `RelevanceGate::evaluate`, relevance score, recency filtering by `local_observed_at` / `RecallIndexRow::indexed_at`, per-turn caps, deterministic sorting, cooldown recording. |
| `presence.rs`      | `PresenceRegistry`, heartbeat validation, active-peer queries, stale-session cleanup, heartbeat claim-lock renewal.                                                            |
| `claim_lock.rs`    | RAM-only advisory `ClaimLockRegistry`, acquire/renew/release, contention result, stale-holder cleanup hooks.                                                                   |
| `framing_tests.rs` | Stream H regression helper that asserts peer-update content is treated as third-party context.                                                                                 |

`memorum-coordination` depends on the substrate and privacy surfaces it needs, but it does not depend on `memoryd` or create another persistence layer.

## Data flows

### Heartbeat to presence

1. A Tier 1 Level 3 harness sends `RequestPayload::PeerHeartbeat(PeerHeartbeat)` to memoryd.
2. `memoryd` validates session id, harness, namespace, entity/path limits, capabilities, and `claim_locks_held` bounds through `handle_peer_heartbeat`.
3. Effective Level 3 writes or refreshes a `PresenceRecord` in `PresenceRegistry`; Level 1/2 acknowledge without adding presence.
4. `PresenceRegistry` keeps only daemon-RAM state. The stale cleanup task removes expired sessions and asks `ClaimLockRegistry` to release locks held by stale sessions.
5. Later delta recall at Level 3 queries active peers by namespace, excluding the current session and filtering to entity/path overlap before rendering `<peer-presence>`.

### Supersede to claim locks

1. `memory_supersede` enters the Stream C governed supersession path in memoryd.
2. Memoryd resolves effective coordination level from per-project `concurrent_session_mode` first, then `coordination.level`.
3. Level 1 (`minimal`) skips Stream I claim-lock work entirely.
4. Level 2/3 acquire an advisory claim lock in `ClaimLockRegistry` for the target memory id before the supersede workflow proceeds.
5. Contention produces a warning/telemetry path while preserving liveness; locks remain advisory, RAM-only, and TTL-bound.
6. Level 3 heartbeats can renew locks listed in `claim_locks_held`; Level 2 locks expire unless released by completion/admin override/TTL.
7. `memoryd peer release-lock` can release one lock manually.

### Recall delta to peer-update XML

1. A delta request builds Stream E's normal recall candidate set.
2. Memoryd builds a `SessionContext` from the request/session binding and recent recall signals.
3. Candidate peer writes are projected from Stream A recall-index rows. `RecallIndexRow::indexed_at` is the `local_observed_at` used for the recency window; `source_device` separates same-device from cross-device startup behavior.
4. `RelevanceGate::evaluate` runs only for Tier 1 sessions and effective Level 2/3 projects.
5. The gate filters candidates by local recency, suppresses writes already surfaced to the receiving session, scores entity/path/topic relevance, sorts deterministically, caps to two peer updates, and returns `CoordinationInsertion`.
6. Memoryd attaches active claim-lock metadata to matching peer updates and, at Level 3, attaches capped peer-presence entries.
7. Stream E's recall assembler receives `Option<CoordinationInsertion>` and renders `coordination="stream-i-v0.1"`, `<peer-presence>`, and `<peer-update>` only when entries exist.
8. Rendered peer updates are recorded into the in-memory peer delivery audit used by `memoryd peer activity`.

Pipeline summary:

```text
recall delta-block
  -> SessionContext
  -> RelevanceGate::evaluate
  -> CoordinationInsertion
  -> Stream E assembler
  -> <memory-delta coordination="stream-i-v0.1"> ... <peer-update> ...
```

## Relevance gate and assembly contract

The score is:

```text
0.5 * entity_overlap + 0.3 * path_overlap + 0.2 * topic_similarity
```

Defaults are threshold `0.6`, recency window `1800` seconds, cap `2`, cross-device startup window `86400` seconds, and cross-device startup threshold `0.7`.

Important invariants:

- Empty-empty entity sets score `0.0`, not `1.0`.
- Missing or embedding-triple-mismatched topic vectors score `0.0`, not an error.
- Candidates are sorted by descending score, descending `updated_at`, then ascending memory id.
- Overflow counts become `CoordinationInsertion.capped_peer_updates` / `capped_peer_presence` and feed pending-attention accounting.
- Stream E does not decide relevance; it only renders the already-capped `CoordinationInsertion`.

## Tier 1 / Tier 3 divergence

Stream I distinguishes coordination tiers from coordination levels.

### Tier 1 harnesses

Tier 1 currently means `codex`, `codex-cli`, or `claude-code`. These sessions can supply enough session context for Level 2/3 coordination:

- startup/delta context becomes `SessionContext`;
- salient entities and paths are extracted from startup recall, last-three-turn FTS5 ids, and session paths;
- recent-query embeddings can contribute topic similarity;
- Level 2/3 sessions may receive `<peer-update>`;
- Level 3 sessions may send heartbeat and receive `<peer-presence>`.

### Tier 3 harnesses

Tier 3 means any non-Tier-1 harness. `SessionContext::is_tier3()` returns true, and `RelevanceGate::evaluate` immediately returns `CoordinationInsertion::empty()` before querying/scoring candidates. Tier 3 can still benefit from baseline Stream E recall, but Stream I peer-update/presence is intentionally absent because the daemon cannot rely on rich session context or framing behavior.

## Level 1 / 2 / 3 contracts

Levels are project behavior modes. They are resolved from `.memory-project.yaml` `concurrent_session_mode` first, then daemon `coordination.level`.

| Level | Mode            | Contract                                                                                                                                                                    |
| ----: | --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
|     1 | `minimal`       | Baseline Stream E recall only. No Stream I `CoordinationInsertion`, no claim-lock acquisition, no `coordination="stream-i-v0.1"`, no `<peer-update>`, no `<peer-presence>`. |
|     2 | `default`       | Default after Stream I. Relevance-gated peer updates for candidates/notes/observations and advisory claim locks on supersede. No presence rendering or heartbeat renewal.   |
|     3 | `collaborative` | Level 2 plus heartbeat-backed presence, `<peer-presence>`, and claim-lock renewal through heartbeat.                                                                        |

The floor is Level 1 because peer sessions' promoted memories can still surface through ordinary Stream E recall. That is not Stream I state; it is normal shared-index recall.

## Runtime state and restart behavior

Stream I runtime state is intentionally volatile:

- `PresenceRegistry` resets on daemon restart.
- `ClaimLockRegistry` resets on daemon restart.
- Peer delivery audit resets on daemon restart.
- Per-session surfaced-peer-write cooldown is RAM-only and scoped to the receiving daemon/session state.

Canonical memory files, frontmatter, and Stream A indexes do not store presence, claim-lock state, or delivery audit entries. Stream I observes Stream A's indexed rows and emits framed recall XML; it does not become a persistence authority.
