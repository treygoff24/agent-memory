# Stream I Cross-Session API

Stream I adds cross-session coordination on top of shipped Streams A-E. It does not add a second persistence layer: peer presence, claim locks, peer-update delivery audit, and cooldown state are daemon-RAM state. Stream A remains the source for indexed memories; Stream E remains the recall XML assembler.

Normative spec: `docs/specs/stream-i-cross-session-v0.1.md`. XML shapes are specified in spec §5; this document records the implemented API surface.

## Daemon protocol additions

`crates/memoryd/src/protocol.rs` exposes Stream I through newline-delimited JSON request/response envelopes, using the existing `RequestEnvelope` / `ResponseEnvelope` framing.

### `PeerHeartbeat` / `PeerHeartbeatAck`

Request variant:

```rust
RequestPayload::PeerHeartbeat(PeerHeartbeat)

pub struct PeerHeartbeat {
    pub session_id: String,
    pub device_id: Option<String>,
    pub harness: String,
    pub project_binding: Option<ProjectBinding>,
    pub namespace: String,
    pub salient_entities: Vec<String>,
    pub salient_paths: Vec<String>,
    pub capabilities: Vec<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub claim_locks_held: Vec<String>,
}
```

Response variant:

```rust
ResponsePayload::PeerHeartbeat(PeerHeartbeatAck)

pub struct PeerHeartbeatAck {
    pub session_id: String,
    pub active_level: u8,
    pub peer_session_count: u32,
    pub active_peers: Vec<ActivePeer>,
    pub conflicting_claim_locks: Vec<ClaimLockInfo>,
}
```

Notes:

- `started_at` is optional on the wire so later heartbeats can omit it. The daemon retains the first non-`None` value for a session.
- Heartbeats update `PresenceRegistry` only when the effective coordination level is Level 3. Lower levels can still receive an acknowledgement.
- Level 3 heartbeat can renew advisory claim locks listed in `claim_locks_held`.
- `conflicting_claim_locks` in `PeerHeartbeatAck` is populated only at Level 3. This is a named design constraint, not a silent gap: the heartbeat path in `handlers.rs` gates population behind `ack.active_level == 3` because in v1 only Level 3 sessions send heartbeats. If a Level 2 heartbeat path is added in a future release, the gate must be removed or explicitly extended so that Level 2 callers receive conflict signals too.

### `PeerStatus`

Request variant:

```rust
RequestPayload::PeerStatus
```

Response variant:

```rust
ResponsePayload::PeerStatus(PeerStatusResponse)

pub struct PeerStatusResponse {
    pub coordination_level: u8,
    pub active_sessions: Vec<PeerSessionStatus>,
    pub claim_locks: Vec<ClaimLockInfo>,
    pub recent_deliveries: Vec<PeerDeliveryAuditEntry>,
}

pub struct PeerSessionStatus {
    pub session_id: String,
    pub harness: String,
    pub namespace: String,
    pub salient_entities: Vec<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub last_heartbeat_age_seconds: u64,
}
```

`PeerStatus` is an admin/status projection of daemon RAM: current default coordination level, active same-device peer sessions, live claim locks, and recent peer-update deliveries.

### `PeerActivity`

Request variant:

```rust
RequestPayload::PeerActivity {
    session: Option<String>,
    since: Option<String>,
    limit: Option<usize>,
    format: PeerActivityFormat,
}
```

Response variant:

```rust
ResponsePayload::PeerActivity(PeerActivityResponse)

pub struct PeerActivityResponse {
    pub entries: Vec<PeerDeliveryAuditEntry>,
    pub limit: usize,
    pub total_recorded: usize,
}

pub struct PeerDeliveryAuditEntry {
    pub delivered_at: DateTime<Utc>,
    pub from_harness: String,
    pub from_session_id: String,
    pub to_harness: String,
    pub to_session_id: String,
    pub memory_id: String,
    pub relevance: f64,
    pub summary: String,
}
```

The audit trail is in memory only and keeps the most recent 200 deliveries across sessions. It resets on daemon restart.

### `PeerReleaseLock`

Request variant:

```rust
RequestPayload::PeerReleaseLock { memory_id: String }
```

Response variant:

```rust
ResponsePayload::PeerReleaseLock(PeerReleaseLockResponse)

pub struct PeerReleaseLockResponse {
    pub memory_id: String,
    pub status: PeerReleaseLockStatus,
    pub released: Option<ClaimLockInfo>,
}

pub enum PeerReleaseLockStatus {
    Released,
    NoLockFound,
}
```

`PeerReleaseLock` is an admin override for advisory claim locks. It is not exposed through MCP.

## Coordination DTO consumed by Stream E

`CoordinationInsertion` lives in `crates/memorum-coordination/src/protocol.rs` and is passed to Stream E recall rendering as an optional parameter.

```rust
pub struct CoordinationInsertion {
    pub peer_updates: Vec<PeerUpdateEntry>,
    pub peer_presence: Vec<PeerPresenceEntry>,
    pub capped_peer_updates: u32,
    pub capped_peer_presence: u32,
}
```

Field meanings:

| Field                  | Meaning                                                                                       |
| ---------------------- | --------------------------------------------------------------------------------------------- |
| `peer_updates`         | Zero to two relevant peer-write summaries sorted by score, then `updated_at`, then memory id. |
| `peer_presence`        | Zero to four Level 3 peer-presence entries. Empty at Level 1/2.                               |
| `capped_peer_updates`  | Count of candidates that cleared the threshold but were omitted by the peer-update cap.       |
| `capped_peer_presence` | Count of active peer sessions omitted by the peer-presence cap.                               |

When the optional insertion is `None` or empty, the recall assembler keeps Stream E output unchanged: no `coordination="stream-i-v0.1"`, no `<peer-update>`, and no `<peer-presence>`.

## Recall XML additions

Stream I XML is inserted by Stream E's assembler only when coordination entries are present. The root `<memory-delta>` or `<memory-recall>` gets `coordination="stream-i-v0.1"` in that case.

### `<peer-update>`

Shape:

```xml
<peer-update from="codex" session="peer1234" ts="09:45" relevance="0.78" claim_locked="claude-code:sess_def" device="other">
  <summary>Safe summary text.</summary>
  <ref>mem_20260501_021</ref>
  <namespace>project:proj_agent_memory</namespace>
</peer-update>
```

Attributes:

| Attribute      | Required | Meaning                                                               |
| -------------- | -------- | --------------------------------------------------------------------- |
| `from`         | yes      | Peer harness name.                                                    |
| `session`      | yes      | Truncated peer session id used for third-party framing.               |
| `ts`           | yes      | Display timestamp for the peer update.                                |
| `relevance`    | yes      | Clamped score in `[0.0, 1.0]`, rendered to two decimals.              |
| `claim_locked` | no       | Present when the memory has an active advisory lock.                  |
| `device`       | no       | Used for cross-device startup updates, currently rendered as `other`. |

Children:

| Child         | Meaning                                                                                                 |
| ------------- | ------------------------------------------------------------------------------------------------------- |
| `<summary>`   | Privacy-filtered summary from the peer write. Unsafe summaries are replaced with a privacy placeholder. |
| `<ref>`       | Stable memory or substrate fragment id.                                                                 |
| `<namespace>` | Namespace/path context carried separately from `<ref>`.                                                 |

### `<peer-presence>`

Shape:

```xml
<peer-presence>
  <session harness="claude-code" id="sess12" entities="ent_auth,ent_users" started="09:12" />
</peer-presence>
```

`<peer-presence>` appears in delta blocks only at Level 3, immediately before `<peer-update>`. It is not inserted into startup recall. Each `<session>` row includes `harness`, truncated `id`, up to five salient `entities`, and display `started` time.

## Per-project mode: `concurrent_session_mode`

`.memory-project.yaml` accepts an optional `concurrent_session_mode` key. The pre-parse whitelist and serde layer both know this key; unknown values are rejected instead of silently defaulting.

| Value           | Effective behavior                                                                                                                   |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `minimal`       | Level 1. Stream I coordination is skipped: no `CoordinationInsertion`, no claim-lock acquisition, no `coordination="stream-i-v0.1"`. |
| `default`       | Level 2. Peer-update relevance gate and advisory claim locks are active.                                                             |
| `collaborative` | Level 3. Level 2 plus heartbeat-backed peer presence and claim-lock renewal.                                                         |
| absent          | Falls back to `coordination.level` from daemon config, defaulting to Level 2.                                                        |

## CLI reference

All `memoryd peer` commands are admin CLI surfaces and are explicitly rejected from MCP forwarding.

### `memoryd peer status`

Shows the daemon's coordination state.

```text
memoryd peer status [--socket /tmp/memoryd.sock]
```

Output includes the current coordination level, active peer sessions, active claim locks, and recent peer-update deliveries.

Exit codes:

| Code | Meaning                  |
| ---: | ------------------------ |
|    0 | Success.                 |
|    1 | Daemon not reachable.    |
|    2 | Protocol/internal error. |

### `memoryd peer activity`

Shows the in-memory peer-update delivery audit.

```text
memoryd peer activity [--socket /tmp/memoryd.sock] [--session <id>] [--since <HH:MM|YYYY-MM-DD|RFC3339>] [--limit <n>] [--format human|json]
```

Exit codes:

| Code | Meaning                                            |
| ---: | -------------------------------------------------- |
|    0 | Success.                                           |
|    1 | Daemon not reachable.                              |
|    2 | Protocol/internal error or JSON rendering failure. |

### `memoryd peer release-lock`

Forcibly releases a claim lock by memory id.

```text
memoryd peer release-lock <memory_id> [--socket /tmp/memoryd.sock] [--yes]
```

Without `--yes`, the CLI first fetches `PeerStatus`, prompts `y/N`, and aborts unless confirmed.

Exit codes:

| Code | Meaning                                          |
| ---: | ------------------------------------------------ |
|    0 | Lock released.                                   |
|    1 | No lock found, or operator declined the prompt.  |
|    2 | Daemon not reachable or protocol/internal error. |

## `coordination:` config defaults

Implemented defaults come from `CoordinationConfig::default()` in `crates/memorum-coordination/src/config.rs`:

```yaml
coordination:
  level: 2

  relevance_gate:
    threshold: 0.6
    recency_window_seconds: 1800
    per_turn_cap: 2
    cross_device_startup_window_seconds: 86400
    cross_device_startup_threshold: 0.7

  presence:
    heartbeat_seconds: 60
    stale_after_seconds: 300

  claim_lock:
    ttl_seconds: 300
```

`RecallIndexRow::indexed_at` is Stream I's `local_observed_at` clock for peer-update recency. The relevance gate uses the local index-ingest timestamp, not an author-supplied peer clock, to decide whether a candidate is inside `recency_window_seconds`.
