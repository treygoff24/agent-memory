# Handoff: Memorum Dashboard

## Overview

Memorum is a local-first persistent-memory daemon for AI agents. This handoff covers the **operator-facing dashboard** — the UI a single human uses to review, recall, govern, and reality-check the memories the daemon has captured. It is a power-user surface: dense, keyboard-driven, terminal-influenced. Think `htop` × `mutt` × structured ledger viewer, not a SaaS app.

The dashboard contains seven primary views (Inbox, Reality Check, Recall, Dreams, Peers, Governance, Entities) plus shell chrome, command palette, notification bell, toasts, and a developer-facing Tweaks panel. All seven are built and wired against realistic mock data in `src/data.js`.

## About the Design Files

The files in this bundle are **design references created in HTML/JSX** — high-fidelity prototypes showing the intended look, layout, content, and interaction model. They are **not production code**. The CSS is hand-written against design tokens in `styles/tokens.css`; the components are inline-Babel React 18 wired up against mock data; there is no build, no router, no real backend.

Your task is to **recreate these designs in the target codebase's environment** (likely React + a real bundler, or whatever the existing app uses) using its established patterns, libraries, and component primitives. If no environment exists yet, choose what fits the rest of the system and implement against that.

Do **not** copy the inline-Babel `<script type="text/babel">` setup, the global `Object.assign(window, …)` exports, or the `useStateXyz` hook aliases — those are artifacts of running React without a build step. Lift the **markup, layout, design tokens, copy, and interaction patterns**; rebuild the wiring idiomatically.

## Fidelity

**High-fidelity.** Pixel-perfect mockups with final colors, typography, spacing, glyphs, and interaction states. Recreate the UI faithfully:

- Colors are exact OKLCH values defined in `styles/tokens.css` — six themes, all 23 tokens each.
- Typography uses an **explicit two-family system**: a sans for prose UI, a mono for numerics/IDs/timestamps/glyphs. Substitute with whatever the codebase already loads, but preserve the role split — never let a mono token bleed into a body-copy slot or vice versa.
- Density, hairline borders, single-shadow discipline, and 70ch-capped inspector body are not stylistic flourishes — they're load-bearing. Hold them.

## Design Language (TTY-DNA)

The dashboard's signature is **terminal genealogy without retro pastiche**. Concretely:

- **Hairline borders** (`--border-soft` ≈ 1px, `--border` ≈ 1px stronger) — never `2px+` outlines, never colored borders for emphasis. Emphasis comes from background swap or accent inset, not border thickness.
- **One shadow only** (`--shadow-modal`) — applied to modals/popovers, never to cards, buttons, or list rows. Elevation is communicated by background tint, not blur.
- **Accent (warm amber `oklch(0.80 0.13 72)`) is rationed.** It appears on:
  - Active sidebar item (2px inset-left)
  - Selected list row (2px inset-left, surface bg)
  - Primary CTA only (Confirm in Reality Check; primary action in inspector)
  - Brand sigil `◆`
  - Active filter pill's count badge (count only, not the pill chrome)
  - Reality Check progress gauge fill
  - Single-keystroke key-cap text in the action chooser
  - Score-bar fills

  That list is closed. Adding accent anywhere else dilutes it.

- **Tabular numerics everywhere** (`font-variant-numeric: tabular-nums`) — IDs, timestamps, scores, counts, byte counts.
- **Mono font for**: memory IDs (`mem_20260507_*`), SHA fragments, ISO/relative timestamps, namespace paths, command keystrokes, score values, all small-caps section labels (`SECTION-LABEL` style at ~10.5px, 0.10em letter-spacing).
- **Glyphs are typographic, not iconographic** in memory rows: `●` review · `▸` current · `⚠` conflict · `▣` due · `◇` dream · `○` empty. SVG icons (Phosphor-style stroke 1.5) are reserved for shell chrome (sidebar nav, top bar). Never mix the two registers.
- **Empty states are honest.** "Inbox is clear." not "You're all caught up! 🎉". No emoji anywhere.

## Themes (6)

Defined in `styles/tokens.css` as `[data-theme="…"]` blocks on `<html>`:

| Token           | warm-dark (default)                    |
| --------------- | -------------------------------------- |
| `--bg`          | `oklch(0.16 0.006 70)` — base canvas   |
| `--surface`     | `oklch(0.20 0.007 70)` — sidebar/cards |
| `--surface-2`   | `oklch(0.24 0.008 70)` — hover/popover |
| `--border`      | `oklch(0.30 0.010 70)`                 |
| `--border-soft` | `oklch(0.26 0.008 70)`                 |
| `--fg`          | `oklch(0.93 0.012 80)` — primary text  |
| `--fg-2`        | `oklch(0.72 0.014 75)` — secondary     |
| `--fg-3`        | `oklch(0.52 0.012 70)` — tertiary/meta |
| `--fg-4`        | `oklch(0.40 0.010 70)` — separators    |
| `--accent`      | `oklch(0.80 0.13 72)` — warm amber     |
| `--accent-soft` | `oklch(0.32 0.04 72)`                  |
| `--ok`          | `oklch(0.74 0.13 145)` — green         |
| `--warn`        | `oklch(0.82 0.14 80)` — yellow         |
| `--bad`         | `oklch(0.66 0.20 25)` — red            |
| `--info`        | `oklch(0.72 0.08 230)` — blue          |

The other five themes — `warm-light`, `cool-dark`, `cool-light`, `monochrome`, `high-contrast` — preserve the same token _roles_ with different palettes. All six must remain valid; do not collapse them. See `styles/tokens.css` for the full set.

Spacing scale, radii, and motion tokens are also in `tokens.css`:

```
--radius-sm: 4px
--radius-md: 6px
--radius-lg: 10px
--text-xs: 11px / --text-sm: 12px / --text-md: 13px
--text-base: 14px / --text-lg: 16px / --text-xl: 22px / --text-2xl: 28px
--dur-fast: 90ms / --dur-medium: 180ms / --dur-slow: 320ms
--ease-out: cubic-bezier(0.2, 0.7, 0.2, 1)
```

## Type System

- **Sans (UI body)**: any neutral grotesque the codebase already loads. The mocks declare a fallback stack; substitute with system UI sans (e.g. Inter, IBM Plex Sans, native UI) and keep weights `400 / 500`. No `600+` weights are used in body — emphasis is by color, not weight.
- **Mono (numerics/IDs/glyphs)**: monospace with tabular figures and reasonable Unicode glyph coverage (the `●▸⚠▣◇○✓✗`-class glyphs need to render). JetBrains Mono, IBM Plex Mono, Berkeley Mono — all fine. The codebase's mono works.
- **Section labels**: `font-family: mono; font-size: 10.5px; letter-spacing: 0.10em; text-transform: uppercase; color: var(--fg-4);` — appear above every grouped section in inspectors and on the Reality Check card.

## Screens / Views

### 1. Shell

Persistent chrome wrapping every non-Reality-Check view.

- **Top bar** (44px, `--surface` background): brand sigil `◆ memorum` (left, 220px-wide brand block matching sidebar width), global search (1fr), status cluster on right (palette `:` button, notification bell with unread dot in `--accent`, daemon-status pill `● daemon ok` / `● daemon down`).
- **Sidebar** (220px, `--surface`): app name + version `v0.4.2-alpha` at top in mono, then nav items. Each nav item: phosphor-style SVG icon (16px, stroke 1.5), label, optional count chip, single-key shortcut (`i`/`r`/`l`/`d`/`p`/`g`/`e`/`,`) right-aligned in mono. Active item gets `--surface-2` background + 2px `--accent` inset on the left edge. Hover gets `--surface-2` only. Counts in `--fg-3` mono tabular; on the active item, the count flips to `--accent`.
- **Footer** (28px, persistent keymap): `view-name · selected-meta · keystroke hints (right-aligned)`. Hints rotate per-view: Inbox shows `↑↓ navigate · enter inspect · a accept · r reject · e edit · f forget · : palette`. Mono throughout. Keystrokes in `--accent`, descriptions in `--fg-2`.

### 2. Inbox (`view === "inbox"`)

The default view. Three layout variants exposed via Tweaks → **Layout**:

- **two-pane** (default): filter pills strip → list (~40%) | inspector (1fr).
- **three-pane**: filters column (left) + list + inspector.
- **drawer**: list full-width; inspector slides in as right-edge drawer overlay on row click.
- **modal**: list full-width; inspector opens as a centered sheet.

**Filter pills** (top): `all · review · recall · conflict · due · dream · forgotten`. Each pill is a chip with label + count badge. Active pill has `--surface-2` bg + the count colored `--accent` (count _only_, never the chip itself). Clicking a pill sets the filter.

**List rows** (`.list-item`):

- 22px glyph column · 1fr title/sub · auto meta column.
- Glyph: `●` review (accent), `▸` recall (info), `⚠` conflict (bad), `▣` due (warn), `◇` dream (warn), `○` memory (fg-3). 11px mono, top-aligned with first text line.
- Title line: 14px sans, primary fg, ellipsis if overflow.
- Sub line: 12px mono, namespace italic in `--fg-2`, separated by `·` dots in `--fg-4`. Format: `personal/work · 4h ago · score 0.78 · src claude-code-session`.
- Meta column: relative time / score / status badge depending on row kind. Mono, tabular.
- Selected row: `--surface` bg + 2px `--accent` inset-left.
- Hover: `--surface` bg only.

**Inspector** — kind-specific. Five variants (matching the five glyph kinds):

- **Review** (`●`): provenance block (source, captured, agent, session), policy decisions inline (allowed / refused / deferred chips), privacy class with explanatory line, 30-day recall sparkline (60×24px, accent fill, mono axis labels at endpoints), action bar at bottom: `Accept (a)` primary + `Reject (r)` + `Edit (e)` + `Forget (f)` secondaries. All keystrokes colored `--accent` in tiny mono caps.
- **Recall** (`▸`): context block ("recalled in claude-code session by request 'how do I configure XYZ'"), event metadata, latency, scoring breakdown.
- **Conflict** (`⚠`): two-column diff. Left = peer A's value with provenance underneath; right = peer B's value with provenance. Three resolution affordances at bottom: `Take A (1)` · `Take B (2)` · `Merge / write new (3)`.
- **Due** (`▣`): five-component reality-check score with horizontal score bars (label · track · value, all aligned). Action bar with `Run Reality Check (enter)`.
- **Dream** (`◇`): proposed memory body, evidence list (each row: source memory ID, excerpt, score), confidence value, `Promote (p)` · `Dismiss (x)` actions.

Inspector body is capped at **70ch** for readability — never let it stretch full width.

### 3. Reality Check (`view === "reality"`) — focus mode

The most-polished view. Activated by clicking sidebar's "Reality Check" or pressing `r`. **Dissolves the standard chrome**: hides top bar and sidebar entirely, replaces them with a single thin status strip.

Layout: 36px top strip (full-width) + main stage (1fr | 260px session rail).

**Top strip** (`.rc-strip`, mono, 12px):

```
◆ memorum  ·  reality check  ·  personal/family  ─────gauge fill─────  3 of 12  esc · pause
```

- Brand sigil `◆` in `--accent`, wordmark `memorum` in `--fg`.
- Separator dots in `--fg-4`.
- "reality check" label in `--fg-2` with 0.04em letter-spacing.
- Namespace scope in `--fg-3` italic.
- **Gauge**: 2px tall, `flex: 1` so it stretches the full strip width between scope and progress text. Track `--border-soft`, fill `--accent`, smooth `--dur-medium ease-out` width transition. Border-radius 999px.
- Progress text in `--fg-2` mono tabular (`3 of 12`).
- `esc · pause` at far right in `--fg-3` (hover `--fg`), clickable to exit.

**Stage** (`.rc-stage`): 56px top padding, 64px side, 36px gap.

- **Question column** (`.rc-card`, `max-width: 64ch`, `margin: 0 auto`) — centered in the 1fr column for ritual focus, not corner-pinned.
  - Scope line above question: `personal/family · written 4 months ago · last verified 92d` in 12px mono, namespace italic `--fg-2`, separator dots `--fg-4`.
  - Question heading: 22px, weight 500, line-height 1.35, letter-spacing -0.005em, `text-wrap: pretty`, max-width 28ch.
  - "What memorum thinks" pull-quote: 2px left border in `--border`, 16px padding, contains a small-caps `WHAT MEMORUM THINKS` label, the body answer at 13px line-height 1.6, and a `Source: …` line in mono `--fg-3`.
  - **Action stack** (`.rc-actions`, 480px max-width, 6px gap):
    - Each action is a row: 28px keycap · 1fr label · meta (right). Border-soft, surface bg, `--radius-sm`, 14px line-height. Hover bumps to `--surface-2`. Primary action (`Confirm — still true`) gets `border-color: oklch(from --accent l c h / 0.5)`, `--accent-soft` background, `--fg` label.
    - Order: `y` Confirm · `k` Correct · `f` Forget · `s` Skip.
    - Keycaps: tiny `--bg` chip with `--border` outline, `--accent` mono text.
    - Description in 11px mono `--fg-3`.
  - **Score breakdown** (`<details>` element, collapsed by default): summary line uses `+` / `−` glyph, label `SCORE BREAKDOWN`, total score right-aligned. When open, renders five score-bar rows:
    ```
    days_since_observed_norm    [████████░░] 0.91
    recall_frequency_norm       [████░░░░░░] 0.45
    cross_source_corroboration  [██░░░░░░░░] 0.20
    confidence_decay            [██████░░░░] 0.62
    sensitivity_weight          [██████████] 1.00
    ```
- **Session rail** (`.rc-side`, 260px right column): `SESSION` label, then a list of all 12 items with status mark + title. Done items: `✓` in `--ok`, dim text. Current: `▸` in `--accent`, full-bright text. Upcoming: `·` in `--fg-4`, dim text. If >8 items, collapse the tail into `+ N more`.

**State variants** (toggle in Tweaks → State → Reality Check variant):

- `default` — score collapsed.
- `score-open` — breakdown expanded.
- `encrypted` — pull-quote body replaced with `⌬ encrypted memory · score 0.78` + small "reveal externally to confirm/correct" help line in mono. **Confirm action is disabled** with explanatory tooltip; description text changes to `requires external reveal`.
- `refused` — action stack replaced with a `--bad`-tinted card: `REFUSED` label, body explaining why ("a tombstone in the personal/family namespace blocks mutations on entities tagged minor"), `policy_id` + `trace_id` in mono, single `Next item (n)` action.
- `complete` — entire question card swaps to a completion card: large `✓` in `--ok`, "Reality Check complete." heading, three big stat columns (`11 confirmed · 1 forgotten · 0 deferred` — numbers 28px mono tabular, labels 10.5px mono small-caps `--fg-4`), `Next session due in 7 days · session_id rc_20260507_001` meta line above a `--border-soft` divider, single `↵ Dismiss` action. Strip gauge fills 100%, all rail items show `✓`.

### 4. Recall (`view === "recall"`)

Time-ordered ledger of recall events.

- **Strip header**: title + meta line with last-30-day sparkline-style histogram of recall events. Filters row beneath: agent · namespace · device · session.
- **Table** (sticky head): `time · seq · device · agent · summary · namespace · latency · score`. All numeric columns mono tabular, fixed-width. Rows clickable to load inspector. `dataVolume === "heavy"` shows `+ 8,234 more events (virtualized)` hint at bottom.
- **Inspector**: full event detail — request, scoring breakdown, returned memories with their scores, latency stages.

### 5. Dreams (`view === "dreams"`)

Pending dream-pass proposals (memories the daemon synthesized from existing ones during a dream run).

- List rows use `.list-item` with `.li-main` (title + sub) + `dream-status` chip + confidence `0.62` mono chip + relative time meta. Status pills: `proposed` / `promoted` / `dismissed` / `running` (running uses `--info`).
- Inspector: proposed memory body, evidence (list of source memory IDs with excerpts and per-source scores), `Promote (p)` / `Dismiss (x)` action bar.

### 6. Peers (`view === "peers"`)

Trust-ledger table of paired peer devices.

- **Table** (left pane): `device · last sync · trust score · status · drift` columns. Rows clickable.
- **Inspector**: per-peer detail — sessions list, claim locks, trust history, sync log.

### 7. Governance (`view === "governance"`)

Policy decision log + batch approve/refuse surface for queued mutations.

- List rows: glyph (severity dot in `--bad`/`--warn`/`--ok`) + `.li-main` (title + namespace + sub) + decision badge + meta. Severities map: block→bad, warn→warn, allow→ok.
- Batch action bar (top): `Selected: 3` · `Approve all (a)` · `Refuse all (r)` · `Defer all (d)`.
- Inspector: **Policy Decision Trace** card showing full evaluation chain (rule → match → outcome → reason).

### 8. Entities (`view === "entities"`)

Sortable table of entities the daemon has identified across memories.

- Columns: `entity · type · mention count · first seen · last seen · namespaces`.
- Inspector: entity profile with all mentions, linked memories, related entities.

### 9. Settings (`view === "settings"`)

Currently a stub — settings inherits the Tweaks panel. In production, this becomes the permanent settings surface (theme, density, layout default, motion preference, data sources, peer pairing, etc.).

## Surface States

All wired through `t.stateOverlay` (Tweaks → State → Surface state):

- **happy path** (default).
- **empty inbox**: list area replaced by `○ Inbox is clear.` heading + meta line ("All review items processed. Last activity: 2 hours ago. Reality Check next due in 3 days.").
- **daemon-down banner**: top of `.main`, full-width strip in `--bad` tint with label `daemon down`, message `memoryd unreachable on 127.0.0.1:7137. Mutations disabled. Retrying every 5s.`, and a `Retry now` action button. While shown, all mutation actions should visually appear available but no-op (or surface the CSRF toast on click).
- **CSRF / 403 toast**: warning toast `Mutation refused (403) · CSRF token expired. Refresh and retry — your selection is preserved.` with a `Refresh token` action.
- **command palette open**: centered modal at `--shadow-modal` elevation, search field on top, command list below grouped by category (Navigate / Action / Theme / View). Keyboard-driven. Items show keystroke hint in mono `--accent` if they have one.
- **notification bell open**: dropdown anchored to bell button. Each row: glyph · title · meta line · action link. 4 mock notifications.

## Interactions & Behavior

### Keyboard

The dashboard is keyboard-first. Bindings:

- `:` — open command palette (anywhere). Esc closes it.
- `g i / g r / g l / g d / g p / g g / g e` — jump to view (Inbox / Reality / recall Ledger / Dreams / Peers / Governance / Entities). Two-key sequences.
- `↑ ↓` — navigate list rows. `enter` — open selected in inspector (or, in drawer/modal layout, open the overlay).
- In Inbox with a row selected: `a` accept · `r` reject · `e` edit · `f` forget. Each pushes a toast and (in real impl) calls the daemon mutation.
- In Reality Check: `y` confirm · `k` correct · `f` forget · `s` skip · `esc` pause/exit.
- In Governance with rows selected: `a` approve all · `r` refuse all · `d` defer all.
- `?` — opens help (stub in mocks).

When typing in an input/textarea, swallow these.

### Mutations

All mutations flow through a single dispatch in `App.jsx → actOnSelected(action)`. In production this hits the daemon over its local socket; in mocks it pushes a toast. Treat each mutation as optimistic-with-rollback: apply locally, push toast immediately, reconcile on daemon ack.

### Animations

Restrained. Only:

- Sidebar nav active-indicator: instant (no slide).
- Reality Check gauge fill: `width var(--dur-medium) var(--ease-out)`.
- Hover bg swaps: `var(--dur-fast)` ease.
- Modal/popover entrance: `--dur-medium`, opacity + 4px translateY.
- Toast entrance/exit: `--dur-medium`, slide from top-right.

Honor `prefers-reduced-motion` and the `motion: "reduced"` Tweak — disable all transitions when set.

## State Management

Top-level state (currently in `App.jsx`):

- `view: "inbox" | "reality" | "recall" | "dreams" | "peers" | "governance" | "entities" | "settings"` — current route.
- `selectedByView: Record<view, string | null>` — **per-view selection state** so navigating between views doesn't leak selection. Inbox keeps its selected memory while you visit Recall.
- `paletteOpen / bellOpen / drawerOpen / modalOpen: boolean`
- `toasts: Toast[]` — auto-dismiss after 6s.
- `t` from `useTweaks(TWEAK_DEFAULTS)` — theme, density, layout, motion, dataVolume, stateOverlay, rcVariant. Persisted (in mocks) via `__edit_mode_set_keys` postMessage; in production, persist to localStorage or user preferences.

In your codebase: use whatever state primitive is idiomatic (Zustand, Redux, signals, React context). The mock's `useState` calls are a placeholder.

### Data fetching

In production, replace `MEMORUM_DATA` (currently a static export from `src/data.js`) with the real daemon connections:

- Inbox/Recall/Dreams/Governance lists — paginated/streamed from local socket.
- Reality Check session — fetch on view enter, mutate as user advances.
- Notifications — server-pushed.
- Peers — periodic sync poll.
- Entities — query on view enter.

All mock data shapes in `data.js` are designed to mirror what the real daemon would return — preserve the field names and types when wiring.

## Tweaks Panel

`tweaks-panel.jsx` is a **dev-only** floating panel (bottom-right) for exercising the design surface — theme switcher, density toggle, layout variants, motion override, data volume, surface-state overlays, Reality Check variants. It's gated by an `__activate_edit_mode` postMessage from a host shell.

**Do not ship the Tweaks panel.** Production has no concept of a "data volume" toggle or "show me the CSRF toast" override. Lift the _defaults_ (warm-dark theme, comfortable density, two-pane layout, full motion) and drop the panel.

That said: the **theme selector** belongs in real Settings. Keep all six themes wired through `[data-theme]` on `<html>` so users can pick from Settings → Appearance.

## Design Tokens

See `styles/tokens.css` for the canonical list. Summary:

- **Color**: 23 tokens × 6 themes (warm-dark, warm-light, cool-dark, cool-light, monochrome, high-contrast).
- **Type scale**: `--text-xs (11px) → --text-2xl (28px)` with seven steps.
- **Radii**: `--radius-sm (4px)`, `--radius-md (6px)`, `--radius-lg (10px)`. No fully-rounded chips (`999px` only on the gauge progress bar).
- **Motion**: `--dur-fast (90ms)`, `--dur-medium (180ms)`, `--dur-slow (320ms)`, `--ease-out (0.2, 0.7, 0.2, 1)`.
- **Shadow**: single token `--shadow-modal` — modal/popover only.
- **Density**: an additional `[data-density="compact"]` selector tightens row heights, paddings, and font sizes by ~15%. Both densities are spec'd.

Lift these into the codebase's token system (CSS variables, design-tokens-w3c JSON, Tailwind config, whatever exists).

## Assets

No bitmap or vector assets. Every icon is either:

- **Inline phosphor-style SVG** rendered by `src/icons.jsx` (sidebar nav glyphs, top-bar palette/bell, footer keystroke wrappers). Stroke 1.5, 16×16 viewBox. Lift the SVG paths into your icon system or import phosphor-icons directly.
- **Unicode glyph in a mono font** (memory-row glyphs, score bar caps, status marks). Renders without any asset pipeline.

The brand mark is the single character `◆` in `--accent`, mono font. No logo file.

## Files in this Bundle

Open `Memorum Dashboard.html` in a browser to see all views running against mock data. Components are split per-view:

| File                     | What's in it                                                                                     |
| ------------------------ | ------------------------------------------------------------------------------------------------ |
| `Memorum Dashboard.html` | Entry — mounts `<App>` and loads all the script tags below.                                      |
| `styles/tokens.css`      | All 6 themes' design tokens. Drop into your codebase first.                                      |
| `styles/app.css`         | All component styles. Cherry-pick per-view as you implement.                                     |
| `src/data.js`            | Mock data. Use it as a schema reference for the daemon's API.                                    |
| `src/icons.jsx`          | Phosphor-style icon components.                                                                  |
| `src/Shell.jsx`          | TopBar, Sidebar, Footer.                                                                         |
| `src/Inbox.jsx`          | Inbox list + filter pills + layout variants.                                                     |
| `src/Inspector.jsx`      | All five inspector kinds.                                                                        |
| `src/RealityCheck.jsx`   | Reality Check focus mode + 5 state variants.                                                     |
| `src/Recall.jsx`         | Recall ledger view.                                                                              |
| `src/Dreams.jsx`         | Dreams list + inspector.                                                                         |
| `src/Peers.jsx`          | Peers trust ledger.                                                                              |
| `src/Governance.jsx`     | Governance log + batch action bar.                                                               |
| `src/Entities.jsx`       | Entities table.                                                                                  |
| `src/UI.jsx`             | Shared primitives — toast, banner, command palette, notification bell, modal veil, filter pills. |
| `src/App.jsx`            | Top-level wiring: routing, state, mutation dispatch, tweaks integration.                         |
| `tweaks-panel.jsx`       | Dev tweaks panel. **Don't ship.**                                                                |

## Implementation Order

Suggested sequence:

1. **Tokens first** — port `styles/tokens.css` into the codebase's token system, validate all 6 themes render against a sample card.
2. **Shell** — top bar + sidebar + footer + keyboard router.
3. **Inbox** (two-pane variant) + inspector — the most-used view, gets the most components flowing.
4. **Reality Check** — second-highest leverage; the focus-mode chrome dissolution proves the shell can yield.
5. **Recall + Dreams + Governance + Peers + Entities** — same patterns as Inbox, parallelizable.
6. **Command palette + notifications + toasts**.
7. **Settings** — replaces the Tweaks panel for theme/density/layout/motion.
8. **Surface states** — daemon-down, empty inbox, CSRF toast, etc.

Hold the rationing rules (accent, shadows, borders) at every step. The dashboard's character lives in those constraints.
