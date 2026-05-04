# Stream G TUI Recall panel design

## Goal

Add a dedicated ninth TUI panel, `Recall`, for human visibility into recent recall-hit events. This closes the gap where the web dashboard can show `/api/recall-hits` but the terminal dashboard cannot.

## Data contract

Use the existing daemon socket protocol:

```rust
RequestPayload::RecallHits { since: None, limit: Some(100) }
ResponsePayload::RecallHits(RecallHitsResponse { hits, .. })
```

`DaemonClient` should expose a `recall_hits(limit)` helper and the app should load it only while `PanelId::Recall` is active.

## Layout

Use ratatui primitives already used by the other panels: `Block`, `Borders`, and `Paragraph`. The panel body is split conceptually into two vertical regions, rendered as text rows in one bordered panel for now:

1. **Histogram top**
   - bucket by recalled hour using `recalled_at`;
   - render newest six buckets;
   - draw proportional ASCII bars (`████`) so density is visible without custom widgets.
2. **Scrollable hit list bottom**
   - rows show `recalled_at`, `mem_id`, `score`, `harness_source_id`, and `surfaced_in_session` when known;
   - current protocol has no score/session fields, so v1 renders `score:n/a`, `harness:n/a`, `session:n/a` explicitly instead of inventing values;
   - include `device`, `seq`, and summary as available provenance.

## Refresh model

Use poll-on-active-panel with the existing daemon poll interval. That gives the desired operational behavior without introducing background subscriber work:

- no recall-hit socket calls while other panels are active;
- refreshes continue every daemon poll tick while the Recall panel is visible;
- `Ctrl-r` still forces the generic status refresh path, and the next active-panel poll refreshes recall rows.

A future version can add a dedicated 5s timer if the global daemon poll interval diverges from the desired panel interval.

## Empty and error states

- Empty events log: `No recall hits yet - try startup recall or a delta block.`
- Daemon unreachable: reuse the existing red daemon-unreachable screen/footer from `App::mark_socket_unreachable`.
- Protocol error: mark socket unreachable with the daemon error; this matches the TUI convention for daemon-backed panels.

## Test plan

- Unit/render test: sample snapshot with recall hits renders the panel title, histogram, memory id, summary, and explicit `score:n/a` fields.
- Keymap test: `9` switches to `PanelId::Recall` through `PanelId::all()`.
- Client behavior is covered by existing socket request helpers plus the app-level render test; daemon protocol coverage stays in `memoryd` tests for `RecallHits`.
