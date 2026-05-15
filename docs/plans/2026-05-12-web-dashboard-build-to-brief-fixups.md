# Web Dashboard — Dogfood Fixups Log

**Parent plan:** `docs/plans/2026-05-12-web-dashboard-build-to-brief.md`
**Started:** 2026-05-14 (Phase 5 dogfood handoff)
**Scope:** Real-time complaints from Trey as he uses the dashboard and TUI
against real seeded data. Each fix lives here as a running log — what
surfaced, what changed, where. TUI fidelity gaps land here too even though
the parent plan is dashboard-shaped — same dogfood pass, same operator.

## Build tag

- **Phase 4 complete head:** `feature/tiered-gate-dashboard-workflow` (unstaged
  Phase 3+4 working tree; commit when Trey signs off)
- **Bundle at dogfood entry:** 121.6 KB JS gzipped + 9.6 KB CSS gzipped,
  Inter Variable + JetBrains Mono Variable self-hosted (~465 KB raw woff2)
- **Test gate at dogfood entry:** `pnpm run check:full` green
  (40 vitest + 198 visual + 48 a11y + 65 e2e + 1 perf + budgets)

## Known carry-overs (not blocking dogfood)

- **Recall view as real `<table>`** — current `<button className="rl-row">`
  shape works but doesn't announce row/column relationships to screen
  readers. A follow-up refactor converts to `<table role="grid">` with
  per-row keyboard handlers. Touches `RecallList.tsx` and the rl-* CSS
  block. Deferred from Phase 4.3 a11y pass.
- **Entity Graph SVG nodes — keyboard access** — currently mouse-only after
  Phase 3 downgraded `<g role="button" tabindex={0}>` (axe nested-interactive
  violation). The focus-entity dropdown provides one keyboard path.
  Follow-up: HTML `<button>` overlay positioned absolutely over each node's
  coords.
- **Memory-id-as-link in RecallList rows** — same HTML constraint as above
  (anchor inside button). Fixes alongside the Recall-as-table refactor.
- **`isTextInputTarget` duplication** — `useKeymap.ts` exports it, but
  `Inspector.tsx`, `Inbox.tsx`, `RealityCheck.tsx` each ship a local copy
  with minor variations. Tag for cleanup.
- **Theme matrix manual 54-spot-check** — axe verifies mechanical contrast;
  the aesthetic walkthrough (does monochrome look right, does high-contrast
  feel jarring) is intentionally a dogfood activity.

## Fixups

_Append as they come in. Format: short heading + commit ref + 1-sentence
"what + why". No prose verbatim from the chat — the entry is the durable
record._

### 2026-05-14 — Phase 5 starts here

### `memoryd ui` was forwarding a `--panel` flag the TUI never accepted
`a5e2765` — Dropped `--panel` from `memoryd ui`'s clap surface and from
`ui_subprocess_args`. `memoryd-tui` has no multi-panel concept — `panel`
literally doesn't appear in `crates/memoryd-tui/src/**` — so the documented
flag was vestigial and broke every invocation of `memoryd ui` with
`error: unexpected argument '--panel' found`. `cli_contract` tests now
assert clap rejection of `--panel` + that `ui_subprocess_args` no longer
forwards it to the subprocess. Surfaced first thing on Phase 5 entry while
the TUI was being opened against the seeded dev substrate.

### TUI first-impression: chrome was dropped, structure isn't legible at a glance
`ASSESSMENT` — No commit yet; fix candidates below land as their own log
entries.

**Operator verdict on first launch against seeded data:** disorientation,
not curiosity. Sense of "I don't know what I'm looking at, too much text,
I'm out." Wrong impression for the audience — power user wants density,
not chaos, and density only reads as density when the chrome carries its
weight. Right now the chrome is uniformly plain text plus heavier-than-
intended borders, so the structure doesn't pop.

**The design is real and the TUI was implemented against it.** Codex's
fat commit `0fc3d35` (2026-05-08, "Phase 4 — TUI redesign — Cluster C")
landed `docs/design/claude-design-brief/` (00-product-brief through
04-tui-reference.html) alongside a new `memorum-theme` crate and the
shell rewrite (`focus/`, `inbox/`, `inspector/`, `palette/`, `status/`)
that replaced the prior 9-panel tab bar. Theme tokens in
`crates/memorum-theme/src/presets/default_warm_dark.toml` map 1:1 onto
the design's CSS variables and the warm-dark amber palette intent is
intact.

**What got lost in translation** (design intent → live render):

| # | Design intent | Live render | Where |
|---|---|---|---|
| 1 | Border tokens deliberately subtle (`oklch(0.30 0.01 70)`) | Theme bumped to `oklch(0.45 0.020 72)` — ~50% brighter L → all borders shout | `crates/memorum-theme/src/presets/default_warm_dark.toml:9-10` |
| 2 | Two-pane uses a **single shared** border ("no doubled separator") | Each pane has its own `Block::new().borders(Borders::ALL)` rectangle → doubled vertical rule down the middle + top/bottom borders on both | `crates/memoryd-tui/src/{inbox,inspector}/mod.rs` block construction + `render_inbox_shell` in `crates/memoryd-tui/src/app.rs:722-730` |
| 3 | Filter pills as proper chips: rounded border, surface bg on active, count in muted/accent | Flat single-line spans (`all·8  review·8  conflicts·0 …`) styled only by fg color toggle — looks like a sentence, not a control bar | `crates/memoryd-tui/src/app.rs:706-720` (`render_header`) |
| 4 | Shortcut hints right-aligned as `<kbd>` chips (surface bg, 11px, border) | Plain trailing dim text glued onto the same header line as the pills | same `render_header` |
| 5 | Brand sigil `◆` + spacer + "Memorum" reads as a distinct unit before the pills | Brand glues directly into the pill row with no visual separator | same `render_header` |
| 6 | Inspector header: title (semibold) + scope (italic) + small `badge.warn` / `badge.ok` status chips above the KV body | Title + KV body present; no badge row | `crates/memoryd-tui/src/inspector/mod.rs` |

**What's NOT broken:** palette intent (warm-dark amber, *not* cyan-on-
dark), glyph alphabet (`●` review, `◇` recall, `⚠` conflict, `◌` dream,
`▸` due, `○` memory), two-pane composition, status row at bottom with
keymap hints, JetBrains Mono inherited from terminal. The skeleton is
right. The chrome on the skeleton is the gap.

**Fix candidates, highest-leverage first** (each gets its own log entry
when it lands):

1. Border subtlety pass — single token change, biggest single visual
   win. Drop `border` from L=0.45 → L=0.30 in default-warm-dark (and
   re-baseline the other presets so the gap closes everywhere, not just
   on default). ~5 min, theme-only.
2. Single shared border between panes — switch both panes off
   `Borders::ALL` so the vertical rule isn't doubled. ~30 min.
3. Header pill rendering — wrap each filter label as a styled chip
   (rounded border + surface bg on active, count in accent). ~1-2 hr.
4. `kbd` chip rendering for the right-aligned shortcut hints. ~30 min.
5. Brand/pills/hints visual separator — give the brand its own zone on
   the left, push pills to the middle, push kbd hints to the right
   instead of letting them all collapse into one line. ~1 hr (folds
   into #3 + #4 if done together).
6. Inspector header status badges — `status_ok`/`status_warn` colored
   badge row above the KV body. ~1 hr.

A pass through #1 → #5 in order is roughly half a day and almost
certainly closes the "I'm out" reaction; #6 is polish on top.
