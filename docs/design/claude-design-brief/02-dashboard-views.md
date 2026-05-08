# Memorum Dashboard — Views to Design

The dashboard ships **seven primary views** plus a settings surface. Each view below specifies its purpose, the data it shows (see `03-data-shapes.md` for real JSON), key actions, and edge states.

The shell that wraps all views:

- **Top bar:** brand `◆ memorum` (left), global search (center, `/` focus), command palette trigger (right, `:`), notification bell (right). Status dots for daemon/sync/dream-schedule trail the bell.
- **Left sidebar (collapsible):** seven nav items + settings. Compact mode shows icons only. Active item has a 2px accent inset on the left edge.
- **Main content area:** the active view. Persistent footer with current keymap hints, daemon status, and a tiny live recall counter.

Persistent across views: `:` opens command palette; `?` opens keyboard help; `g` then any letter jumps to that view (`gi` inbox, `gr` reality check, `gd` dreams, etc.).

---

## View 1 — Inbox (default landing view)

**Purpose:** unified queue of "things that need your attention." This is the dashboard's homepage and the most-used view.

**Layout:** filter pills (top) + two-pane (list left ~40%, inspector right ~60%). Pills filter the list; inspector adapts to selected item kind.

**Data feeding it:**

- Review queue items (`GET /api/review`) — candidate, quarantined, dream-low-confidence
- Reality-check-due items (`GET /api/reality-check`) — pending list with scores
- Recent recall hits (`GET /api/recall-hits`) — last N events worth surfacing
- Conflicts (subset of review where `reason = merge_conflict`)

**Filter pills:** `all (N)`, `review (N)`, `conflicts (N)`, `recall`, `dreams (N)`, `due (N)`. Active pill has `--surface` background and accent count. Pills are keyboard-navigable with `1-6`.

**List item rows:** glyph (role-colored) + title + scope + sub-metadata (source, age, confidence) + right-aligned relative time. Selected row shows a 2px `--accent` inset on the left.

**Inspector panel** (adapts to item kind):

- For review item: title, namespace, sensitivity badge, body, recent recall, **provenance card** (written, session, grounding, confidence, device, peers), policy card (privacy, governance, tombstone). Action bar at bottom: `a` accept, `r` reject, `e` edit, `f` forget. CSRF-protected POSTs.
- For recall hit: which memory got recalled, when, by which session, with what surrounding context.
- For conflict: side-by-side diff (local | remote), each side with provenance and timestamps, plus three resolution affordances (keep local, keep remote, custom merge).
- For dream output: pattern body, evidence list (memories the dream used as inputs), confidence, "promote / queue / dismiss" actions.

**Empty state:** "Inbox is clear." Single icon `circle-half` in `--fg-3`, body text "All review items processed. Last activity: 2 hours ago." Subtle, not celebratory.

**Edge cases:**

- Daemon down: full-page banner at top in `--bad`, list dims to `--fg-3`. Actions disabled.
- Loading: dim list with "loading…" placeholder; do not skeleton-pulse.
- Stale mutation (409): toast "This item changed elsewhere. Refresh to see latest." with refresh button.

---

## View 2 — Reality Check (focus mode)

**Purpose:** opt-in, ritualistic, one-memory-at-a-time review. The user enters this mode deliberately. It dissolves the dashboard chrome.

**Layout:** full-bleed; sidebar collapses; top bar shrinks to `◆ memorum · reality check` + progress text. Center-stage is **one memory question**, with answer affordances below.

**Stage:**

- Scope line (small, `--fg-3`, italic): `personal/family · written 4 months ago · last verified 92d`
- Question heading (`--text-xl`, `--fg`): "Does Maeve still attend Pacific Crest Montessori?"
- "What memorum thinks" body (`--text-md`, `--fg-2`, max 60ch): the current memory body verbatim, with source.
- Four answer cards stacked vertically:
  - **Confirm — still true** (`y`) — primary, accent border
  - **Correct — replace with…** (`k`) — opens inline textarea on activate
  - **Forget** (`f`) — destructive variant, no accent
  - **Skip — ask later** (`s`) — secondary

**Right rail (220px):** "Session" list — done items (`--ok` checkmark), current item (`--accent` arrow), upcoming items (`--fg-4` bullets). `esc` pauses session.

**Footer:** progress bar (2px, `--accent` filled / `--border-soft` track) showing `3 of 12`.

**Score breakdown** (collapsed by default, expandable below the question): five component scores with mini-bars (`days_since_observed_norm`, `recall_frequency_norm`, `cross_source_corroboration`, `confidence_decay`, `sensitivity_weight`).

**Empty state:** "No items due." Icon `check-circle` in `--ok`, body "Last completed: 4 days ago. Next due: in 3 days." Single button "Run anyway".

**Edge cases:**

- Encrypted item: body shows "encrypted memory · score 0.78 · reveal externally to confirm/correct" — `Confirm` is disabled with tooltip.
- Refused (governance/tombstone): inline error card replaces actions: "This memory cannot be confirmed because its tombstone match is pending." Single `next` button.

---

## View 3 — Recall Ledger

**Purpose:** "what did Memorum surface to me, when, and why?" Read-only timeline.

**Layout:** vertical timeline grouped by day. Each row: timestamp (mono, tabular) + memory title + which session triggered the recall + scope tag. Hover row → highlight the corresponding memory in a side preview pane.

**Filters (top bar):** time range (`24h`, `7d`, `30d`, `all`), namespace, session id, memory id. Free-text search across summary.

**Data:** `GET /api/recall-hits?since=&limit=` returns `{ hits: [{ event_id, device, seq, memory_id, recalled_at, summary }] }`.

**Sparkline (header):** 30-day recall volume bar chart, ~100px tall, accent-soft fill. Hover a bar → tooltip with date + count.

**Empty state:** "No recall events yet." Icon `clock`, body "Recall events appear here once an agent retrieves a memory."

---

## View 4 — Dreams

**Purpose:** review what background dreaming pulled out — patterns, contradictions, questions, cleanup proposals. **Three sub-tabs**: Journal, Questions, Cleanup.

**Sub-tab 1: Journal** — a feed of dream entries by date. Each entry: scope, summary, evidence count, confidence, "promote / dismiss" affordance. Each entry expandable to show evidence list (memory ids, titles, scores) and pass-by-pass output (Pass 1 fragments, Pass 2 candidates, Pass 3 promotions).

**Sub-tab 2: Questions** — questions the dream pass surfaced for the user to answer. Each: scope, question text, "answer" button (opens the appropriate review or correction flow), "dismiss" button.

**Sub-tab 3: Cleanup** — superseded/forgotten memories the dream pass identified. List with "approve cleanup" / "preserve" actions.

**Header:** "Last dream: 14:00 today · scope: all · promoted 3 / queued 1 / dropped 0 · next scheduled 03:00." Run-now button (admin).

**Empty state per tab:** "No dream output for this scope yet. Dreams run nightly at 03:00 by default."

---

## View 5 — Peers (Stream I)

**Purpose:** see other devices in the user's Memorum network — laptop, desktop, etc. Single-user, multi-device.

**Layout:** card per peer device. Card shows:

- Device label (`mbp`, `mini`, `desktop`)
- Status dot (online / offline / stale-heartbeat)
- Last heartbeat timestamp
- Active session info (if any) — which agent, since when, what scope
- Active claim locks held by this peer
- Recent peer-updates received from this peer (last 24h, count)

**Coordination level indicator** (top of view): one of three modes (observe-only / soft-claims / strict-claims) with a brief description.

**Recent peer-update feed** (right rail or below cards): stream of `<peer-update>` deliveries with timestamp, scope, and excerpt.

**Empty state:** "No peers yet." Icon `users`, body "Add a peer device by cloning your Memorum git remote on another machine and running `memoryd init --adopt`."

---

## View 6 — Governance Review

**Purpose:** dedicated review queue surface (Inbox shows mixed items; this view is filtered to governance-only with richer affordances).

**Layout:** left list (paginated), right inspector. Same row pattern as Inbox but with a top-of-list **batch action bar**: "Select all" + "Approve selected" / "Reject selected".

**Filters:** status (`candidate`, `quarantined`, `dream_low_confidence`), namespace, reason code, since, free-text.

**Inspector additions** beyond Inbox:

- **Policy decision trace** — every governance check that ran, pass/fail, why
- **Privacy scan** — labels detected, storage action chosen
- **Provenance chain** — full `provenance_chain[]` rendering, each entry with timestamp + actor + action
- **Supersession history** — chronological chain of supersedes/superseded_by

**Edit affordance:** opens the body in a modal with a textarea. Shows a diff preview before commit. Sends `memory_supersede`. (Trivial textareas are OK here because governance review explicitly requires user oversight of edits.)

---

## View 7 — Trust Artifact (route, not a sidebar item)

**Purpose:** deep-dive on a single memory. Linked-to from any other view (clicking a memory id navigates here).

**Route:** `/audit/:memory_id`

**Layout:** single column, scroll. Sections (in order):

1. **Header** — title, namespace, status badge, encryption badge, sensitivity badge, "open in editor" / "supersede" / "forget" actions
2. **Body** — full text with code/inline-code rendered properly. For encrypted memories, shows redaction notice + reveal-externally instructions.
3. **Confidence** — score (`0.95`), reason text (`"deterministic web fallback fixture"`)
4. **Recall** — total count, 30-day count, last recalled timestamp, mini-timeline of recent recalls
5. **Provenance chain** — vertical chain of provenance entries with arrows
6. **Policy decisions** — list of governance decisions that ran
7. **Privacy scan** — labels detected, storage action
8. **Supersession history** — bidirectional list (supersedes / superseded_by)
9. **Sync state** — devices that have this memory, merge status, claim lock status

**Walk affordance:** "Walk provenance graph" button (top right). Opens a sub-route `/audit/:id/walk` that renders a graph (see View 8 below).

---

## View 8 — Entity Graph (route)

**Purpose:** visualize entity relationships (entities + co-mentions + supersession chains).

**Route:** `/entities` and `/entities/:entity_id`

**Layout:** SVG graph in the main area, controls in a left rail.

**Graph data:** `GET /api/entity-graph?namespace=&depth=&focus=` returns `{ nodes, edges }`. Render with d3-force or similar (your choice).

**Controls (left rail):** namespace filter, depth (1–5), focus entity, density toggle, color-by toggle (sensitivity, recall frequency, confidence).

**Detail card** (right rail when entity clicked): entity name + memory list + supersession chain.

**Empty state:** "No entities mapped for this namespace yet."

---

## View 9 — Settings

**Purpose:** theme, density, keymap reference, notification preferences. Single settings page; tabs along top.

**Tabs:**

1. **Appearance** — theme picker (six presets shown as small swatches), density toggle (comfortable / compact), reduced-motion toggle (respects OS setting by default), font-size slider.
2. **Theme editor** — for each of the 23 color tokens, an OKLCH picker (L slider, C slider, H slider). Live preview pane on right showing one inbox row + inspector. "Save as…" button creates a custom theme; "Reset to default" reverts.
3. **Keyboard** — full keymap reference table (action / key / context). Read-only in v1.
4. **Notifications** — channel toggles (passive only / OS notification / Slack webhook / email). Threshold inputs.
5. **About** — daemon version, dashboard version, build commit, link to docs.

---

## Cross-cutting: Command Palette

Triggered by `:` from any view. Modal centered, ~520px wide.

- Top: input with cursor.
- Below: filtered command list, ranked by fuzzy match (`nucleo-matcher` semantics).
- Each result: command name + scope tag + (optional) keyboard shortcut.
- Categories: **Navigate** (`go inbox`, `go reality check`, …), **Theme** (`theme switch warm-light`, `theme save-as <name>`, …), **Action** (`approve selected`, `run reality check`, `run dream now`, …), **Help** (`help <topic>`).
- `Enter` executes; `Esc` closes; `Tab` cycles category; `↑/↓` navigate.

## Cross-cutting: Notification UI

Bell icon in top bar with a tiny dot when there are unread notifications. Click → dropdown with last 10 events: `LeakedSecretDetected`, `BlockingMergeConflict`, `ReviewQueueOverThreshold`, `DreamRunCompleted`, `RealityCheckDue`, `RealityCheckOverdue`, `DailySynthesisSummaryReady`. Each event has a primary action (jump to relevant view) and a "dismiss" affordance. Slack/email payloads contain **counts only, never bodies** — same constraint.

## Cross-cutting: Footer / status line

Always visible. Three regions:

- **Left:** daemon status dot + "daemon", sync status dot + "sync · N peers", dream schedule "next dream 03:00"
- **Center:** current filter / scope / view-relevant context
- **Right:** keymap hints for the current view (mini-table, accent letters bold).

## What you don't need to design

- Login / signup / auth — none. Localhost-only single-user.
- Onboarding wizard — none. CLI installer covers init.
- Pricing / billing / marketing — none. This is local infrastructure.
- Mobile / responsive < 960px — desktop only. Designs assume 1280px+ viewport.
- Print styles — not needed.
