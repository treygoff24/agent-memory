# Recall-hit event flow

This note maps the Stream G recall-hit data path that the TUI Recall panel consumes.

## Emission path

Recall hits are emitted when startup or delta recall renders a memory into a harness-facing recall block:

- `crates/memoryd/src/recall/startup.rs` builds startup recall sections and calls `emit_recall_hits(substrate, included_memory_ids.iter().map(String::as_str))` after rendering.
- `crates/memoryd/src/recall/delta.rs` does the same for delta recall responses.
- `crates/memoryd/src/recall/render.rs` owns `emit_recall_hits`, validates each raw id as a `MemoryId`, and appends `EventKind::RecallHit { id, recalled_at }` through the substrate event API.

Invalid ids are skipped with a warning; valid ids are appended as ordinary substrate events, so they share the same event ordering and mirror projection as other Stream A/Stream G events.

## `events_log` mirror projection

The daemon reads recall-hit summaries from the SQLite mirror at `<runtime>/index.sqlite`:

- table: `events_log`
- relevant columns: `event_id`, `device`, `seq`, `kind`, `memory_id`, `ts`, `payload_json`
- recall rows use `kind = 'recall_hit'`
- ordering is `ORDER BY e.ts DESC, e.event_id DESC`, matching newest-first dashboard expectations.

`seq` remains device-local event sequence. `event_id` is the stable tiebreaker for rows with the same timestamp. The TUI should treat `(event_id, device, seq)` as display metadata, not as a cursor protocol.

The query also left-joins `memories` on `m.id = e.memory_id` to obtain the current safe summary shown in the panel.

## Existing daemon/web query path

The reusable query implementation already lives in `crates/memoryd/src/recall_hits.rs`:

- `recent_recall_hits(substrate, since, limit)` opens the runtime index mirror.
- `query_recent_recall_hits` clamps the limit to `1..=500`, filters optional `since`, validates `memory_id`, and returns `RecallHitsResponse`.

The daemon protocol exposes it through:

- `RequestPayload::RecallHits { since, limit }`
- `ResponsePayload::RecallHits(RecallHitsResponse)`
- handler branch in `crates/memoryd/src/handlers.rs`

The web dashboard route at `crates/memoryd-web/src/routes/recall_hits.rs` is only a socket-forwarder when a daemon socket is configured. In fixture mode, it serves fixture data from `WebState`. It does not own duplicate recall-hit query logic.

## TUI reuse shape

The TUI should consume the existing daemon protocol via `crates/memoryd-tui/src/client.rs::DaemonClient`, the same way it consumes `Status`, `TrustArtifact`, review actions, and Reality Check actions.

Do not lift or duplicate the query into `memoryd-tui`; keep the single owner in `memoryd/src/recall_hits.rs` and render the daemon response.

## TUI refresh invariant

The Recall panel can poll on active use only:

- when the active panel is `Recall`, the normal daemon poll refresh should request `RecallHits` with a bounded limit;
- when any other panel is active, no recall-hit request is needed;
- unreachable daemon errors should flow through the existing red unreachable surface/footer.
