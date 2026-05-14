# Web Dashboard — Build to Brief

**Plan owner:** Claude (frontend execution)
**Author date:** 2026-05-12
**Source of truth:** `docs/design/claude-design-brief/` (00–04)
**Implementation target:** `crates/memoryd-web/frontend/`
**Plan revision:** v0.5 (gate-policy migration)

**Revision goal v0.5 (2026-05-14):** Workflow-only migration. The substantive dashboard plan remains in flight, but future work now uses the repo's tiered gate policy: targeted checks and `pnpm run check:fast` during implementation; `pnpm run check:local` before claiming a phase/milestone complete; `pnpm run check:full`, production build, full Playwright matrix, and Rust embed checks only for final/pre-merge validation or changes that directly require them. Do not rerun the full frontend/Rust gates after every micro-task.

**Previous plan revision:** v0.4 (post-review-3 sanity check)

**Revision goal v0.4 (2026-05-12):** Close one residual gap surfaced by the pass-3 sanity check — §3.1 asserted "no race" on init but didn't specify the implementation mechanism. v0.4 pins the mechanism: theme + density + reduced-motion seeding happens in `main.tsx` **synchronously before `createRoot().render(<App />)`**, by reading `location.hash` and `location.search`, then writing `data-theme` / `data-density` / `data-reduced-motion` attributes on `document.documentElement` *before* the first React commit. This eliminates flash-of-wrong-theme on initial load. The hash-router state then hydrates inside `App.tsx` via its own `useRoute()` hook — but the visual root attributes are already correct when React first paints.

**Plan revision:** v0.3 (post-review-2)

**Revision goal v0.3 (2026-05-12):** Fold plan-reviewer pass 2 findings.
- §3.1 router migration scope expanded: the grep target is now `view=\|tweaks=1\|layout=\|variant=\|dreamState=\|recallState=\|inspectorKind=\|theme=`. URL convention specified: **hash drives view routing; existing query params survive as state selectors** (so `/#/recall?recallState=proposed` and `/#/?theme=warm-dark` both work). Init order in `App.tsx`: parse hash for route first, then read query params for state — same tick, no race.
- Historical v0.3 note superseded by v0.5: `test:perf` remains part of `pnpm run check:full`; use targeted perf checks only when the touched surface requires them before final validation.
- Phase 4 gate text fixed: removed the contradicting "impeccable detector reports 0 findings" line. §4.6 stays as optional manual run.
- Historical v0.3 note superseded by v0.5: Phase 3 keeps targeted router/Trust Artifact/Entity Graph checks, but `pnpm run check:full` is reserved for final/pre-merge or explicit high-confidence validation.
- Per-view DoD in Phase 2 now requires "EmptyState renders the brief's exact copy verbatim" — brief at `docs/design/claude-design-brief/02-dashboard-views.md` is the source.

**Revision goal v0.2 (2026-05-12):** Fold plan-reviewer pass 1 findings.
- Added Phase 1 task 1.0 to wire `fullbleed` from route → `Shell` → `.app` class (was a missing implementation, not a verification task).
- Clarified that F-3 fix touches **two** layers: the view-level wrapper AND any intermediate layout wrapper (e.g., `TwoPaneLayout`'s bare div between `.main` and `.panes-2`).
- Rewrote F-4 diagnosis: the centering comes from the browser button UA stylesheet, not from any `font:` shorthand reset. The fix (`.list-item { text-align: left }`) is unchanged.
- Pinned both tooling deps that the plan used as gates: `rollup-plugin-visualizer` (devDep, used in Phase 4 bundle check) and dropped `impeccable` as a hard gate (kept as optional manual run).
- Phase 0 gate now specifies how snapshots are produced (against the running `memoryd-web` binary at `127.0.0.1:7137`, not a Vite dev server) and what they prove (purely "before" reference, no diff assertion).
- Historical v0.2 note superseded by v0.5: between-phase validation now uses `pnpm run check:local` plus targeted visual/a11y/e2e checks instead of the full matrix every time.
- Added explicit warning about `build.rs` re-running `pnpm build` on every cargo compile after font addition.
- Added explicit Phase 3.1 sub-task to migrate `?view=` and `?tweaks=1` query-param consumers in `tests/e2e/` and `tests/visual/` to hash-router URLs in the SAME commit as the router lands (no "lazy update" debt).
- Added explicit note: `@font-face { font-family: 'Inter Variable' }` must match the `--font-sans` value exactly; bumped the relevant subtask.
- Moved Peers view's card-vs-table decision out of §2.2 (where it was the plan author's call) into §9 open questions (where it's Trey's call).
- Stopped using "better than the brief" framing in §2.2 — replaced with "open question, defaulting to <X> pending Trey's call."
- Added per-view Definition-of-Done in Phase 2 so "verify" has falsifiable criteria.
- Fixed Phase 0 gate to only assert what Phase 0 produces; moved the visual-diff statement to Phase 1's gate.

---

## 1. Why this plan exists

The dashboard at `http://127.0.0.1:7137` is **structurally present but visually broken**. Trey's 2026-05-12 screenshot shows:

- Brand sigil, sidebar nav, top bar, filter pills, status indicators, inspector cards all render — every primitive in the brief is wired and the component tree resolves.
- BUT list rows render with centered titles floating in a black void, the inspector floats in the right margin, panes collapse to natural content height, and there's zero "console" feel.

Audit (§2) shows the failure is **not missing CSS** — the 420-rule `styles/app.css` is comprehensive and matches the brief almost 1:1. The failure is **CSS↔component contract violations**:

1. Every view's outermost JSX wrapper is a bare `<div data-testid="…">` with no className. The shipped CSS uses a flex-grow chain (`.app > .main { display: flex; flex-direction: column }` → view → `.panes-2 { flex: 1 }`). Without a `display: flex` view wrapper, `flex: 1` collapses and panes go to natural height. **This single issue is responsible for most of the screenshot's "floating in void" feel** across 7 views. *Note: this affects TWO layers — the per-view outer wrapper (e.g., `Recall.tsx`'s outer `<div>`) AND the intermediate layout wrapper (e.g., `TwoPane.tsx`'s outer `<div>` between `.main` and `.panes-2`). Both need fixing.*
2. `.list-item` is rendered as a `<button>` with `display: grid`. The CSS global `button { font: inherit; … }` reset doesn't touch `text-align`, so the browser button UA stylesheet's `text-align: center` survives and applies to text inside the grid cells. The `.glyph` cell has an explicit `text-align: center` rule (intentional); the `.body` cell has no explicit rule and inherits the centered alignment. Fix is `.list-item { text-align: left }` — safe because `.list-item .glyph { text-align: center }` is more specific and wins on the glyph cell.
3. Fonts referenced in tokens (`Inter Variable`, `JetBrains Mono Variable`) are **not bundled or self-hosted anywhere in the repo** — they fall back to system-ui, which is the wrong typography per the brief (and a privacy/offline regression the brief explicitly forbids).
4. Sidebar nav uses one `◆` glyph for every nav item (8 identical diamonds). Brief §3.1 mandates Phosphor with a per-role glyph→icon mapping.
5. No client-side router. View 7 (Trust Artifact, `/audit/:memory_id`) and View 8 (Entity Graph, `/entities/:entity_id`) are unreachable as URLs.

The work splits into **structural fixes** (small, high-leverage), **per-view verification** (medium effort, repetitive), and **missing surfaces** (Trust Artifact + Entity Graph + router).

## 2. Gap analysis vs. brief

Numbered against the brief's view inventory and design system.

### 2.1 Foundation gaps

| # | Brief reference | Current state | Gap |
| - | - | - | - |
| F-1 | Brief 01 §2 — Inter Variable + JetBrains Mono Variable, self-hosted | `--font-sans: 'Inter', system-ui, …` and `--font-mono: 'JetBrains Mono', …` in `tokens.css`. No `*.woff2`, `*.ttf`, or `@font-face` declarations anywhere. No `public/` or `src/assets/fonts/` directory. | Self-host both variable fonts; declare `@font-face` with `font-display: swap`; verify bundle contains them post-build. |
| F-2 | Brief 01 §3 — Phosphor Icons, Regular, 16/20px, role-token color | No `@phosphor-icons/react` in `package.json`. Sidebar nav uses one `◆` per item; TopBar uses literal `:` and `●` text; FilterPills uses ASCII keyboard hints. List glyphs use Unicode (`●`, `▸`, `⚠`, `◇`, `○`) — acceptable but inconsistent with brief. | Install `@phosphor-icons/react`, define the 7-glyph mapping table in one module, replace nav/top-bar/footer/empty-state icons. Decide whether to keep Unicode glyphs in `.list-item .glyph` (they survive at 13px in mono and feel TUI-aligned) or switch — **recommendation: keep Unicode for list-glyphs to preserve TUI family resemblance, switch shell chrome to Phosphor**. |
| F-3 | Brief 02 — all views must inherit shell flex chain | 7 view files (`Dreams`, `Entities`, `Governance`, `Peers`, `RealityCheck`, `Recall`, `Inbox` via `TwoPaneLayout` and 3 sibling layouts) wrap their content in bare `<div data-testid="…">`. | Add a `.view` className that supplies `display: flex; flex-direction: column; height: 100%; min-height: 0` and apply it to every view's outer wrapper alongside the existing `data-testid`. |
| F-4 | Brief 01 §4 — `.list-item` is a real interactive element with proper alignment | `<button className="list-item">` renders with browser-default `text-align: center` carried into grid cells. | Add `text-align: left` to `.list-item` (one-line CSS fix). Verify no regression in `.rl-row` / `.pr-row` / `.ent-row` etc. (already left-aligned explicitly). |
| F-5 | Brief 02 — `:` opens command palette, `?` opens help, `g <letter>` jumps to view | `App.tsx` handles `:` and `?` but not `g <letter>` chord. Sidebar has letter shortcuts (`i`/`r`/`l`/`d`/`p`/`g`/`e`/`s`) shown as static text in `.count` slot — confusing because `.count` is for numeric counts per brief, and these are key hints. | Implement `g`-prefix chord. Move letter hints out of `.count` slot into a dedicated `.kbd-hint` slot styled like the top-bar `kbd`. Replace `count` with the real review-queue count from `/api/review`. |
| F-6 | Brief 02 §7 + §8 — Trust Artifact at `/audit/:memory_id`, Entity Graph at `/entities/:entity_id` | No router. `App.tsx` uses `useState<ViewId>` for view selection. URL is read once on init (`?view=…`, `?tweaks=1`) but never updated. | Add minimal hash-based router (`location.hash` with `hashchange` listener). Define routes: `#/inbox`, `#/reality`, `#/recall`, `#/dreams`, `#/peers`, `#/governance`, `#/entities`, `#/entities/:id`, `#/settings`, `#/audit/:memory_id`. No `react-router-dom` dependency — implement in `<200 LOC` to keep bundle small. |

### 2.2 Per-view gaps

| View | Brief | Implemented files | Gap |
| - | - | - | - |
| 1. Inbox | 02 §View 1 | `views/Inbox.tsx`, `inboxView/{InboxHeader,InboxList,FilterPills,adapter}.tsx`, 4 layouts | F-3 (bare div) + F-4 (text-align). 4 layouts is excessive — `ThreePane`, `Drawer`, `ModalSheet` aren't in the brief; the brief mandates 40/60 two-pane. **Decision:** keep all 4 layouts but make `two-pane` the only one selectable from UI; URL-toggle others stay as escape hatch for design experimentation. Verify each layout passes F-3. |
| 2. Reality Check | 02 §View 2 | `views/RealityCheck.tsx`, `realityMode/{QuestionStage,AnswerCards,CorrectEditor,SessionSidebar,FocusStrip,CompletionCard,ScoreBreakdown}.tsx` | Brief requires `.app.fullbleed` toggle to dissolve chrome — verify the view sets `data-fullbleed="on"` or equivalent and `App.tsx` applies the class. Verify all 5 states (question/encrypted/refused/correct/complete). Check answer cards' `Confirm/Correct/Forget/Skip` 4-card stack matches brief (current naming is `confirm/correct/forget/skip` per `AnswerCards.tsx` — need to read to confirm). |
| 3. Recall | 02 §View 3 | `views/Recall.tsx`, `recall/{RecallList,TimelineStrip}.tsx` | F-3 (bare div). Brief says vertical timeline grouped by day + sparkline; current implementation is a tabular grid with 8-column row + a separate `TimelineStrip` sparkline. **Open question §9.5: keep tabular or rebuild as vertical timeline?** Defaulting to tabular pending Trey's call (tabular fits 50–300 recall hits/day better at 1440 viewport; brief-mandated timeline is closer to the TUI feel). Either way, add the day-group section labels and verify `summary: null` (encrypted) renders as `[encrypted memory · id mem_…]`. |
| 4. Dreams | 02 §View 4 | `views/Dreams.tsx`, `dreams/DreamList.tsx` | F-3 (bare div). Brief mandates 3 sub-tabs (Journal/Questions/Cleanup). Current implementation flattens to status filters (`proposed`/`queued`/`accepted`/etc.) — **gap**. Need to add the 3-tab structure with status filtering nested per tab. Header summary line ("Last dream: 14:00 today · scope: all · promoted 3 / queued 1 / dropped 0") is missing from current view. |
| 5. Peers | 02 §View 5 | `views/Peers.tsx`, `peersView/TrustLedger.tsx` | F-3 (bare div). **Open question §9.6: brief mandates card-per-peer, current is tabular.** Card-per-peer at N=1–3 (the realistic peer count) shows more info per peer; tabular is denser at higher N but Memorum users won't have higher N. Defaulting to card-per-peer pending Trey's call. Either way: peer detail (inspector or expanded card) exposes device label, online/offline/stale dot, last heartbeat, active session, claim locks, recent peer-updates. Coordination-level indicator strip (observe/soft/strict + description) sits at top of view. |
| 6. Governance | 02 §View 6 | `views/Governance.tsx`, `governanceView/ReviewQueue.tsx` | F-3 (bare div). Brief mandates batch-action bar with select-all + approve-selected + reject-selected; CSS has `.batch-bar` and `.gov-check` already — verify wiring. Inspector additions: policy decision trace (`.trace`/`.trace-row` CSS exists), privacy scan, full provenance chain. Edit affordance opens body in modal with diff preview before commit. Verify all 4 are wired. |
| 7. Trust Artifact | 02 §View 7 | None | **MISSING.** Need full single-column scroll view with 9 sections (header / body / confidence / recall / provenance chain / policy decisions / privacy scan / supersession history / sync state). Linked-to from any memory-id click. Walk-graph button → `#/audit/:id/walk`. |
| 8. Entity Graph | 02 §View 8 | `views/Entities.tsx`, `entitiesView/EntityTable.tsx` | Partial: table exists, **graph rendering missing**. Need SVG graph with left-rail controls (namespace/depth/focus/density/color-by). Decision: implement with hand-rolled force layout in `<300 LOC` rather than pulling `d3-force` (would add ~80KB to bundle). Or accept the bundle cost for correctness — **brief says "your choice"; recommend d3-force for v1 because layout quality is the whole point of the view**. |
| 9. Settings | 02 §View 9 | `views/Settings.tsx`, `settings/{Appearance,ThemeEditor,Keyboard,Notifications,About}Tab.tsx` | F-3 (bare div). Brief mandates theme picker with 6 swatches (CSS has `.theme-grid` + `.theme-swatch`), density toggle, reduced-motion toggle, font-size slider. Theme editor needs OKLCH L/C/H sliders for each of 23 tokens with live preview. Verify wiring. Keyboard tab is read-only table — verify keymap matches `keyboard/Keymap.ts`. |

### 2.3 Cross-cutting gaps

| # | Brief | Current state | Gap |
| - | - | - | - |
| X-1 | 02 cross-cutting — Command palette `:` with categories Navigate/Theme/Action/Help | `palette/CommandPalette.tsx` + `commands.ts` exist. | Verify category tagging, fuzzy match via `fuse.js` (in `package.json`), keyboard nav (`Enter`/`Esc`/`Tab`/`↑↓`). |
| X-2 | 02 cross-cutting — Notification bell dropdown | `App.tsx` renders `.notif` div with `.notif-row` items inline. | Extract to `<NotificationDropdown />` component. Wire to `useNotifications()` (already imported). Add per-row primary action (`route: '/governance'` etc.) that navigates via the new router. |
| X-3 | 02 cross-cutting — Footer status line with daemon/sync/dream + keymap hints | `Footer.tsx` shows `daemon` + `sync · 2 peers` + `:palette` + `?help` static. | Wire to live data: daemon status from `/api/status`, sync peer count from same, dream-next-scheduled from `dreams.next_scheduled_at`. Keymap hints adapt to active view (Inbox shows `j/k nav · enter open · a accept`; Reality Check shows `y confirm · k correct · f forget · s skip`). |
| X-4 | 01 §7 — A11y floor (WCAG AA on every theme; focus rings; `<table>` for tabular; ARIA labels; color-not-alone signals) | `app.css` defines `:focus-visible` outline + `.sr-only`. `aria-label` on TopBar buttons. Some tables use `.rl-table-head` divs with `.th` buttons (not real `<table>`). | Audit each view for: real `<table>` where tabular semantics matter (recall, peers, governance batch list), ARIA labels on all icon-only buttons (Phosphor swap from F-2), color+glyph pairing on status states, focus ring visible on every interactive (verify dark and high-contrast themes). |
| X-5 | 01 §5 — Motion respect `prefers-reduced-motion` | `tokens.css` has `data-reduced-motion="on"` selector. `app.css` has `pulse-bad` infinite animation on `.status-dot.bad`. | Add OS-level `@media (prefers-reduced-motion: reduce)` block that mirrors `data-reduced-motion="on"` behavior. Add explicit toggle in Settings Appearance tab that defaults to "respect OS." Verify the `pulse-bad` opt-out works. |
| X-6 | 00 — Data shapes (real API responses) | `api/{client,queries,mutations,notifications,types}.ts` exist; `data/fixtures.ts` used as fallback. | Verify every view consumes real API data when available, falls back to fixtures only when daemon unreachable. Verify CSRF token plumbing on every mutation (look at `api/mutations.ts`). Verify 403/409 handling matches brief: 403 toast with refresh-and-retry, 409 toast with "this item changed elsewhere." |

### 2.4 Out of scope (per brief)

- Login/signup/auth UI — none, localhost-only.
- Onboarding wizard — none, CLI installer covers init.
- Mobile/responsive < 960px — desktop only.
- Print styles — not needed.
- Memory editor textarea (except in Governance edit-with-diff modal, which is explicitly allowed).
- Policy editor UI — brief says deferred to v1.1+, routes return 501.
- Sync dashboard UI — deferred to v1.1+.
- Remote dashboard auth — deferred.

## 3. Phased work plan

Each phase has explicit completion gates, but they are tiered. During implementation, run targeted checks and `pnpm run check:fast`; do **not** run the full pipeline after every small task. Before claiming a phase/milestone complete, run `pnpm run check:local` plus the smallest relevant Playwright/Vitest/Rust embed checks for the touched surface. Reserve `pnpm run check:full`, production builds, full Playwright matrix, and Rust release gates for final validation, CI/pre-merge, or changes that directly require them.

### Phase 0 — Baseline (no code changes)

**Goal:** Capture the current visual state purely as a "before" reference. **No diff assertion is made in this phase** — Playwright visual tests fail by default when there's no prior snapshot, so we're just establishing the starting point.

**How snapshots are produced:** Playwright runs against the **already-running `memoryd-web` binary at `127.0.0.1:7137`** (pid 4111 today), not against a Vite dev server. The Playwright config uses `baseURL: 'http://127.0.0.1:7137'`. No `pnpm run dev` needed — the embedded dist is what we're measuring. If the binary isn't running, start it via `bash scripts/seed-dev-substrate.sh --reset` first.

**Tasks:**
- 0.1 — Manually screenshot `/#/` (or `/` with no router yet) at viewport 1440×900 in Chrome. Stash in `docs/dev/web-dashboard-screenshots/phase-0-pre-fix-inbox.png`. One image; the original screenshot Trey sent is the "official" baseline reference.
- 0.2 — Capture Playwright snapshots for every existing route by running `pnpm run test:visual -- --update-snapshots`. Save under `crates/memoryd-web/frontend/tests/visual/__snapshots__/`. If the existing visual test suite doesn't cover a route, skip that route here — we'll get coverage as views are touched in Phase 2.
- 0.3 — Note current Lighthouse FCP/LCP/CLS + a11y score for `#/inbox` in `docs/dev/web-dashboard-baseline.md`. Short note, ~200 words.

**Gate:** `docs/dev/web-dashboard-baseline.md` exists with the screenshot path + Lighthouse numbers. Snapshot directory exists with at least the inbox/recall/dreams/peers/governance/entities/settings snapshots that the existing test files cover.

### Phase 1 — Structural fixes (high leverage)

**Goal:** Fix the cascade of contract violations that account for ~80% of the visual broken-ness. Single-commit-per-fix so we can bisect.

**Tasks:**
- 1.0 — **Fullbleed wiring (was a missing-implementation, not a verification).** Add `fullbleed` prop to `Shell`. When `true`, render the `<div className="app fullbleed">` so the existing `.app.fullbleed` CSS at `app.css:87-100` actually applies. Wire `App.tsx` to set `fullbleed={view === 'reality'}` (and any future route that needs it). Single commit.
- 1.1 — **F-3 fix.** Add `.view` class to `app.css` (`display: flex; flex-direction: column; flex: 1; min-height: 0;`). Apply to every view's outer wrapper: `Dreams.tsx`, `Entities.tsx`, `Governance.tsx`, `Peers.tsx`, `RealityCheck.tsx`, `Recall.tsx`, plus the 4 inbox layout intermediate wrappers (`TwoPane.tsx`, `ThreePane.tsx`, `Drawer.tsx`, `ModalSheet.tsx` — their bare `<div data-testid="…">` sits between `.main` and `.panes-N`, so the className must be added on the layout-level div, not just the view-level one). Inbox.tsx itself wraps in `<>` (fragment), so no change there — children inherit `.main`'s flex column directly until they hit the layout wrapper. Keep `data-testid` on the same div for Playwright. Single commit.
- 1.2 — **F-4 fix.** Add `text-align: left` to `.list-item` rule in `app.css`. `.list-item .glyph` already has `text-align: center` and is more specific, so the glyph cell stays centered. Verify `.rl-row`, `.pr-row`, `.ent-row`, `.gov-row` aren't affected (they're divs, not buttons — no UA `text-align` to override). Single commit.
- 1.3 — **F-1 fix (self-host fonts).** Download Inter Variable + JetBrains Mono Variable woff2 from rsms/inter and JetBrains/JetBrainsMono releases (~150KB Inter + ~200KB JBM). Place in `crates/memoryd-web/frontend/src/assets/fonts/`. Add `@font-face` declarations to `tokens.css` *above* the `:root` blocks: `@font-face { font-family: 'Inter Variable'; src: url('../assets/fonts/InterVariable.woff2') format('woff2-variations'); font-weight: 100 900; font-style: normal; font-display: swap; }` and same for `JetBrains Mono Variable`. **CRITICAL: the `font-family` value inside `@font-face` must match exactly what `--font-sans` and `--font-mono` reference.** Current `tokens.css:173-174` has `'Inter'` and `'JetBrains Mono'` (without `Variable`). Update those two values to `'Inter Variable'` and `'JetBrains Mono Variable'` in the same commit. Verify post-build by inspecting `dist/assets/` for the woff2 files and `dist/assets/index-*.css` for the `@font-face` rules.
- 1.4 — **F-5 fix (partial: kbd hints).** Add `.nav-item .kbd-hint` styling that mirrors the top-bar `.kbd` look. Move letter shortcuts (`i`/`r`/`l`/etc.) out of the `.count` slot. Reserve `.count` for real numeric counts wired from `/api/status.recall.candidate_attention_count` (inbox), `/api/reality-check.items.length` (reality check), `/api/recall-hits` last-24h count (recall). Defer the full `g <letter>` chord — that's Phase 4.
- 1.5 — **Tooling deps.** Add `rollup-plugin-visualizer` as devDependency. Wire it into `vite.config.ts` behind `process.env.ANALYZE === '1'`. Used in Phase 4 bundle check. (Dropped `impeccable` as a hard gate — kept as optional manual run in Phase 4.6.)

**Build-loop warning:** After 1.3 lands, every `cargo build -p memoryd-web` re-runs `pnpm build` (per `build.rs`) which re-embeds ~350KB of fonts. Plan iterations on frontend code via `pnpm run dev` against `http://127.0.0.1:5173` (Vite's default), and only do the cargo build at end-of-phase to verify rust-embed.

**Gate after Phase 1:**
- Visual snapshots regenerated; diff against Phase 0 snapshots shows expected changes (left-aligned list rows, real flex layouts, Inter font computed on `body`).
- Local confidence: `pnpm run check:local` from `crates/memoryd-web/frontend` is green.
- Targeted surface checks: run `pnpm run test:visual` and `pnpm run test:a11y` because Phase 1 changes layout, fonts, and contrast. Run `pnpm run test:e2e:gentle` for the views touched by the structural fixes instead of the full e2e suite unless flow behavior changed broadly.
- Rust embed verification is required because Phase 1 touches fonts/assets and `vite.config.ts`: from repo root run `cargo build -p memoryd-web --release` once at phase end and confirm the embedded build includes the new font assets. Do not run this cargo build after each micro-fix.
- Manual: open `http://127.0.0.1:7137`, inspect `<body>` computed font-family, confirm `Inter Variable` resolves (not a fallback).

### Phase 2 — Per-view sweep

**Goal:** Walk each view, fix the per-view-specific gaps from §2.2. One commit per view. Trey reviews after each.

**Order (by impact and dependency):**
- 2.1 — **Inbox** (highest traffic). Fix F-3 already done in 1.1 — verify it actually renders correctly. Verify `inspectorItemFromInbox` returns the right kind for each row. Verify CSRF on `approve/reject/edit/forget` mutations (`api/mutations.ts`). Real-data swap: today's seed has 8 candidate items — verify they all show up with correct namespaces, sensitivity badges, confidence values, and that the inspector renders InboxReviewInspector for each.
- 2.2 — **Reality Check.** Verify `.app.fullbleed` toggle works (probably needs `App.tsx` to pass a `fullbleed` prop to `Shell` based on active view). Verify all 5 RC states. Confirm answer-card 4-stack order: Confirm primary, Correct, Forget, Skip. Wire to `/api/reality-check` + `/api/reality-check/respond`.
- 2.3 — **Recall.** Day-group section labels added to RecallList. Verify `summary: null` (encrypted) renders as `[encrypted memory · id mem_…]` not blank. Test virtualization at heavy mode (9000 events). Verify sparkline TimelineStrip click filters the table.
- 2.4 — **Dreams.** Add 3-tab structure (Journal/Questions/Cleanup) above the existing status pills (status pills become a within-tab sub-filter). Add header summary line. Verify Pass 1/2/3 breakdown in inspector (CSS `.dream-stages` exists).
- 2.5 — **Peers.** Add coordination-level indicator strip (observe/soft/strict with description) at top of view. Verify peer-detail inspector renders all required fields. Open question §9.6 decides between brief-mandated card-per-peer vs. current `pr-table` tabular layout — DO NOT execute 2.5 until that's answered. If card-per-peer: build a new `.peer-card` primitive sized for 1–3 peers; if tabular stays: confirm `pr-table` works at 1, 3, and 8 peers.
- 2.6 — **Governance.** Verify batch-action bar checkboxes + approve/reject-selected mutations. Verify policy decision trace renders in inspector. Verify edit-with-diff-modal works end-to-end (open, edit body, see diff, commit → `memory_supersede`). Test refusal toast for 403/409.
- 2.7 — **Settings.** Verify all 5 tabs render and each control mutates the right thing: theme picker → `data-theme` attribute on `<html>`, density toggle → `data-density`, reduced-motion → `data-reduced-motion`, font-size slider → `--text-base` override on `<html>`, theme editor OKLCH sliders → live preview. Save-as-custom-theme writes to localStorage (no daemon roundtrip for v1).

**Per-view Definition-of-Done** (each view passes ALL of these before its commit lands):
- The view's outer wrapper has `.view` className applied.
- The view fetches via the appropriate React Query hook (no fixtures used when daemon is up; fixtures only when daemon is unreachable AND we're in a test).
- Loading state renders a `.loading` banner or `[ItemKind] loading…` placeholder, not blank.
- Error state renders the `QueryErrorBanner` with retry affordance.
- Empty state renders `EmptyState` with the brief-specified copy **verbatim** — open `docs/design/claude-design-brief/02-dashboard-views.md`, copy the exact strings, no paraphrasing.
- Real seeded data renders without overflow, ellipsis-breakage, or color-only signals.
- Keyboard navigation works (per-view keys match `keyboard/Keymap.ts`).
- Visual snapshot regenerated and committed.
- A11y check passes (no axe violations in the new view's DOM).

**Gate after Phase 2:**
- All 7 sidebar views meet the DoD above.
- Real seeded data flows through every view; no view falls back to fixtures while the daemon is up.
- `pnpm run check:local` from `crates/memoryd-web/frontend` is green.
- Because Phase 2 sweeps all sidebar views, also run the relevant UI surface gates once at phase end: `pnpm run test:visual`, `pnpm run test:a11y`, and targeted/grepped `pnpm run test:e2e:gentle` flows for views whose behavior changed. Save the full e2e matrix for final validation unless the phase changed broad navigation or mutation flows.

### Phase 3 — Missing surfaces

**Goal:** Build Trust Artifact (View 7) and complete Entity Graph (View 8). Add the router that makes them reachable.

- 3.1 — **Router.** Implement minimal hash-based router. Add `useRoute()` hook in `src/router/`. Routes: `#/inbox` (default), `#/reality`, `#/recall`, `#/dreams`, `#/peers`, `#/governance`, `#/entities`, `#/entities/:id`, `#/settings`, `#/audit/:memory_id`. Update `App.tsx` to read view from route. Update `Sidebar` to render `<a href="#/inbox">`-style nav (cmd-click opens in new tab). Wire CommandPalette `Navigate` category to push routes. Update `Footer` keymap hints to react to route.

  **URL convention** (this resolves the pass-2 blocker):
  - **Hash = view routing only.** `location.hash` carries `#/<view>` or `#/<view>/:id`. Read on `hashchange` + on first mount.
  - **Query params = state selectors, preserved as-is.** `location.search` continues to carry `?view=`, `?tweaks=1`, `?layout=`, `?variant=`, `?dreamState=`, `?recallState=`, `?inspectorKind=`, `?theme=`, etc. View components parse query params exactly as they do today.
  - **Init mechanism (pinned to close the v0.3 race gap)**: visual root attributes (`data-theme`, `data-density`, `data-reduced-motion`) are seeded **synchronously in `main.tsx` before `createRoot().render(<App />)`**, by reading `location.hash` + `location.search` and writing the attributes on `document.documentElement`. React's first commit therefore paints against the correct attributes — no flash, no `useEffect` round-trip. The hash-router `useRoute()` hook then hydrates inside `App.tsx` for live updates on `hashchange`. The localStorage-persisted theme (if any) is read in the same pre-render block, with this precedence order: explicit `?theme=` query param → localStorage → `data-theme` default in `index.html` (`warm-dark`).
  - **Backwards-compat for `?view=`**: if hash is empty AND `?view=` is present, treat as a one-time redirect (write the hash, strip the param). Otherwise hash wins.

  **In the SAME commit:** migrate every test file that consumes the listed query params. Run `grep -rn 'view=\|tweaks=1\|layout=\|variant=\|dreamState=\|recallState=\|inspectorKind=\|theme=' crates/memoryd-web/frontend/tests/` first to enumerate callsites. For each: rewrite `?view=recall` to `/#/recall`; keep `?recallState=proposed` etc. as query suffix. Example: `/?view=recall&recallState=proposed` → `/#/recall?recallState=proposed`. No "lazy update" debt.
- 3.2 — **Trust Artifact view.** Create `views/Audit.tsx` + `views/audit/{HeaderSection,BodySection,ConfidenceSection,RecallSection,ProvenanceChain,PolicyDecisions,PrivacyScanSection,SupersessionHistory,SyncState}.tsx`. Single-column scroll layout per brief. Use existing inspector cards where overlap exists (`ProvenanceCard`, `PolicyDecisionTraceCard`, `PrivacyScanCard`). Wire to `GET /api/audit/:memory_id`. Walk-provenance-graph button → `#/audit/:id/walk` (defer the walk sub-route to v1.1; ship a "coming soon" placeholder).
- 3.3 — **Entity Graph.** Install `d3-force` (~30KB minified). Build `views/Entities.tsx` graph mode: SVG with force-directed layout, controls in left rail. Wire to `GET /api/entity-graph`. Keep the existing `EntityTable.tsx` as a toggleable "list view" mode (URL: `?mode=table` vs default graph). Click entity node → `#/entities/:id`.
- 3.4 — **Cross-link memory IDs.** Anywhere a memory ID renders (inspector, recall ledger, etc.), wrap in `<a href="#/audit/:id">` so the trust artifact is reachable from everywhere.

**Gate after Phase 3:**
- `#/audit/mem_…` for a real seeded memory renders all 9 sections.
- `#/entities` shows a graph; clicking a node navigates to that entity's detail.
- All 9 views in the brief are now reachable from the sidebar or the command palette.
- `pnpm run check:local` from `crates/memoryd-web/frontend` is green.
- Targeted Phase 3 checks: run router/Trust Artifact/Entity Graph Vitest or Playwright tests directly; run `pnpm run test:e2e:gentle` or grepped `pnpm run test:e2e -- --grep ...` for navigation and Trust Artifact flows; run `pnpm run test:visual` for the new surfaces; run `pnpm run test:a11y` for the new DOM. Do not run the full pipeline after each of 3.1/3.2/3.3/3.4.
- `ANALYZE=1 pnpm run build` shows bundle size still under 400KB gzipped after Phosphor + d3-force land. If Phase 3 is the final/pre-merge milestone, run `pnpm run check:full` once.

### Phase 4 — Quality & polish

**Goal:** Ship the brief's invariants (a11y, motion, keyboard, themes) and pass the AI-slop test.

- 4.1 — **Phosphor icon swap (F-2).** Install `@phosphor-icons/react`. Define the glyph→icon mapping module (`src/ui/icons.ts`) per brief §3.1. Replace nav-item icons (one icon per view: `Inbox`, `Eye`/`CheckCircle`, `Clock`, `Sparkle`/`Diamond`, `Users`, `Scales`, `Graph`, `Gear`), top-bar buttons (`Terminal` for palette, `Bell` for notifications), empty-state icons. Keep `.list-item .glyph` as Unicode for TUI family resemblance unless Trey wants otherwise.
- 4.2 — **`g <letter>` chord.** Implement chord parser in `keyboard/useKeymap.ts`. `g` enters chord state for 1000ms, second keypress routes. Show "g …" indicator in footer while chord active.
- 4.3 — **A11y pass.** `pnpm run test:a11y` (axe-playwright). Fix all violations. Specific things to verify: icon-only buttons have `aria-label`; status colors always paired with glyph or label; tabular regions use real `<table>` semantics where they communicate row/column relationships (recall, peers, governance batch); skip-to-main-content link present; tab order matches visual order.
- 4.4 — **Theme matrix.** Switch through all 6 themes (warm-dark, warm-light, high-contrast, monochrome, cool-dark, cool-light) on every view. Spot-check WCAG AA contrast (4.5:1 body, 3:1 large) using browser devtools or `axe-core`. Fix any theme-specific contrast failure by adjusting that theme's token, not the component.
- 4.5 — **Reduced-motion verification.** Toggle OS-level reduced motion (System Preferences) and dashboard-level reduced-motion toggle independently. Verify modal scale animation, route fade, `pulse-bad` status dot, RC progress gauge all respect the setting.
- 4.6 — **AI-slop verdict (manual only; no automated gate).** Optional: run `npx impeccable@2 --json crates/memoryd-web/frontend/src` for a quick automated scan; treat as informational, not a gate (the tool is unpinned and its rule set isn't versioned with this repo). Required: manual walkthrough against brief §10 anti-patterns — no glassmorphism, no gradient buttons, no emoji in UI strings, no AI-sparkle styling, no drop shadows for hierarchy, no card-grid as default container. Document the verdict in `docs/dev/web-dashboard-baseline.md` as a Phase 4 addendum.

**Gate after Phase 4:**
- `pnpm run check:full` from `crates/memoryd-web/frontend` is green once, as final/frontend pre-merge validation.
- Manual theme matrix walkthrough: all 6 themes × all 9 views = 54 spot-checks, document any deviation in `docs/dev/web-dashboard-theme-matrix.md`.
- AI-slop manual verdict documented in `docs/dev/web-dashboard-baseline.md`. (Impeccable scan is optional; no gate assertion on its output.)
- Bundle stays under 400KB gzipped per `ANALYZE=1 pnpm run build` + visualizer output.

### Phase 5 — Real-data dogfood

**Goal:** Ship to Trey for real-time dogfood (the original ask). This phase is where his complaints come in and we fix them.

- 5.1 — Tag the build (e.g. `dogfood-web-2026-05-MM-vN`). Rebuild memoryd-web. Restart daemon if needed.
- 5.2 — Trey browses; surfaces complaints; we fix them in real time. Each fix lives in `docs/plans/2026-05-12-web-dashboard-build-to-brief-fixups.md` as a running log.

## 4. Tests, gates, and verification

### 4.1 Test pyramid

| Layer | Tool | What it verifies |
| - | - | - |
| Unit | Vitest (`pnpm run test`) | Pure-function logic in adapters, fixture transforms, router parsing. |
| Component | Vitest + React Testing Library | Each view renders given a fixture, handles keyboard interaction, fires expected mutations. |
| Visual | Playwright (`pnpm run test:visual`) | Per-route snapshot diff. Catches CSS regressions automatically. |
| E2E | Playwright (`pnpm run test:e2e`) | Full flows: open palette, navigate via chord, approve in inbox, complete a Reality Check session, walk to Trust Artifact. |
| A11y | axe-playwright (`pnpm run test:a11y`) | WCAG AA + ARIA + focus + color-only signals. |
| Perf | Playwright + Lighthouse (`pnpm run test:perf`) | LCP / CLS / bundle size budget. Bundle ≤ 400KB gzipped target. |

### 4.2 Tiered gate policy

Use the repo's tiered gates rather than the full pipeline at every phase boundary:

```bash
# from crates/memoryd-web/frontend
pnpm run check:fast    # inner loop: typecheck + capped Vitest
pnpm run check:local   # milestone confidence: lint + typecheck + capped Vitest
pnpm run check:full    # final/pre-merge: lint + typecheck + Vitest + visual + a11y + perf + e2e
```

During implementation, prefer `pnpm run check:fast`, `pnpm run test:gentle`, targeted Vitest tests, and grepped/capped Playwright (`pnpm run test:e2e:gentle` or `pnpm run test:e2e -- --grep ...`). Before marking a phase/milestone complete, run `pnpm run check:local` plus the targeted visual/a11y/e2e/perf checks that match the touched surface. Reserve `pnpm run check:full` for final validation, CI/pre-merge, or broad changes that directly require the full Playwright matrix.

Run `cargo build -p memoryd-web --release` or `cargo test -p memoryd-web --test frontend_smoke` only when verifying the Rust-embedded production dist (for example after changes to `package.json`, `vite.config.ts`, assets, or final/pre-merge validation). The Rust binary embeds the dist at build time; stale embeds are real, but cargo rebuilds are not an inner-loop frontend check.

### 4.3 Visual baselines

Playwright visual snapshots live under `crates/memoryd-web/frontend/tests/visual/`. After each phase, regenerate the snapshots and Trey eyeballs the changeset. Don't trust pixel-perfect equality across phases — diff against the previous phase's snapshots, not the Phase 0 baseline, except in Phase 0→1 where the diff is intentionally large.

### 4.4 Manual screenshot diff

After each Phase, take one viewport-1440 screenshot of `#/inbox` and stash in `docs/dev/web-dashboard-screenshots/phase-N-inbox.png`. Provides a quick visual changelog for code review.

## 5. Invariants (will fail review if violated)

These are spec-mandated; treat them like the Stream A invariants:

1. **Every theme passes WCAG AA on every view.** Body text 4.5:1, large text and UI components 3:1. Verify via axe-playwright.
2. **`prefers-reduced-motion` is always respected.** Both OS-level and the in-dashboard toggle. Animations that survive must be honest about why.
3. **Fonts are self-hosted.** No runtime requests to `fonts.googleapis.com` or `fonts.gstatic.com`. Verify by running the daemon offline and confirming Inter/JBM render.
4. **CSRF on every mutation.** Every `POST`/`PUT`/`DELETE` reads `<meta name="csrf-token">` and sends `X-Memorum-CSRF`. 403 handling shows a toast with refresh-and-retry.
5. **No card-grid as default layout.** Lists, tables, and panes carry the dashboard. Card-grid only appears in Settings (`.cards-grid`) and Peers' deferred card-view-mode (out of scope here).
6. **No emoji in UI strings.** Phosphor + Unicode line-art glyphs only. (Brief §10.)
7. **Localhost-only stays localhost-only.** No new env-aware behavior. No "if remote" branches. The dashboard never asks who you are.
8. **Every memory-id is a link.** Wherever a memory ID is rendered as text, it's wrapped in `<a href="#/audit/:id">`. Discoverability invariant.

## 6. Risks and mitigations

| Risk | Mitigation |
| - | - |
| Bundle size blows past 400KB after adding Phosphor + d3-force + fonts | Tree-shake Phosphor (import per-icon, not whole pack). Verify with `rollup-plugin-visualizer` (added as devDep in Phase 1.5, run via `ANALYZE=1 pnpm run build`). If d3-force is too heavy, fall back to hand-rolled force layout (~200 LOC, no dep). |
| Self-hosted fonts cause FOIT/FOUT on first paint | `font-display: swap` + preload critical weights. Acceptable for v1; revisit if Lighthouse CLS drops below 0.1. |
| Adding the router breaks existing `?view=…` and `?tweaks=1` URL params used in tests | Phase 3.1 explicitly migrates the test fixtures in the SAME commit as the router. No lazy update. Grep callsites first; rewrite atomically. |
| Playwright visual snapshots are noisy across font rendering changes | Take snapshots at viewport 1440×900, regenerate baseline once per phase, accept small (≤1%) pixel diffs. |
| Inspector kind components have drifted from the brief's section contracts | Audit `inspector/kinds/*.tsx` against brief §View 1 inspector spec in Phase 2.1 specifically. Treat any deviation as a contract bug, not a polish question. |
| `cargo build -p memoryd-web` doesn't pick up new dist after a frontend-only change | `build.rs` already exists; verify it's a cargo rerun-if-changed on `frontend/dist`. Add a step to Phase 1's gate that confirms the embed contains a known new string from the dist. |
| Trey's screenshot was taken on the old running daemon (1h21m uptime, pid 3231), not a fresh rebuild | After every phase, restart memoryd via `bash scripts/seed-dev-substrate.sh --reset` if dogfooding the visuals. The daemon binary embeds the dist; an old daemon serves an old dist. |
| `@font-face` name doesn't match `--font-sans` reference | Phase 1.3 explicitly bumps `tokens.css:173-174` from `'Inter'` → `'Inter Variable'` and `'JetBrains Mono'` → `'JetBrains Mono Variable'` in the same commit. Silent system-ui fallback is exactly the bug we're fixing — a mismatch reintroduces it. |
| `build.rs` re-runs `pnpm build` on every cargo compile, with new fonts it gets slower | Use `pnpm run dev` (Vite, hot reload, no cargo) for frontend iteration. `cargo build` is end-of-phase verification only. |

## 7. Deferred (v1.1+)

These are listed for completeness but explicitly not in this plan's scope:

- Policy editor UI (brief §What's out of scope).
- Sync dashboard UI (brief same).
- Remote dashboard auth (brief same).
- Walk-provenance-graph sub-route (`#/audit/:id/walk`) — ship a placeholder in Phase 3.2.
- Custom theme persistence to disk (TOML file per brief §1.3) — v1 stores in localStorage only.
- Notification provider integrations beyond passive (Slack webhook, email, OS notification) — UI toggles exist per brief, but backend wiring beyond passive is out of scope.

## 8. Estimated effort

Rough sizing, Claude-execution:

| Phase | Wall-clock estimate |
| - | - |
| 0 — Baseline | 30 min |
| 1 — Structural fixes | 2–3 hours (mostly font sourcing + verify) |
| 2 — Per-view sweep | 5–7 hours (7 views, ~45 min each on average; Settings + Dreams + Governance are the heavy ones) |
| 3 — Missing surfaces | 4–6 hours (router 1h, Trust Artifact 2h, Entity Graph 2h, cross-link 30min) |
| 4 — Quality & polish | 3–4 hours |
| 5 — Real-data dogfood | open-ended (Trey-driven) |
| **Total before Phase 5** | **~15–20 hours of focused work** |

If Trey wants ship-it-fast, Phase 0→1→2.1 (just Inbox) is ~3 hours and gets the dashboard from "embarrassing" to "real." Everything after is incrementally better.

## 9. Open questions for Trey

Before I start Phase 1 I'd like answers on these. Items §9.5 and §9.6 actually BLOCK their respective Phase 2 tasks (2.3 and 2.5); the rest just shape execution:

1. **Phosphor vs. Unicode for list-item glyphs.** Recommend: Phosphor for shell chrome only; Unicode line-art (`●▸⚠◇○`) for `.list-item .glyph` to preserve TUI family resemblance. OK to keep both?
2. **4 inbox layouts.** Recommend: keep all 4 in code (URL-toggleable), make `two-pane` the only one selectable from UI. Or delete the other 3? Brief only mandates two-pane.
3. **Entity Graph: d3-force or hand-rolled?** Recommend d3-force for layout quality. ~30KB bundle cost.
4. **Memory-id-as-link.** Recommend yes everywhere. Any place you'd want a memory ID rendered without being clickable?
5. **Recall view: tabular grid (current) or brief-mandated vertical timeline?** Recommend tabular — denser for 50–300 events/day at 1440 viewport, virtualizable. Brief's vertical timeline matches TUI feel more closely. Your call. **Blocks Phase 2.3.**
6. **Peers view: card-per-peer (brief) or tabular grid (current)?** Recommend card-per-peer — N is small (1–3 devices typical), card density carries more info per peer than a row. Brief explicitly mandates cards. Your call. **Blocks Phase 2.5.**
7. **Phase 5 dogfood loop venue.** Want me to write the fixup-log file as we go, or just talk through each complaint inline and let me edit?

---

**Plan ends.** Next: pass to `@agent-plan-reviewer` for adversarial review round 1, fold findings into v0.2, second pass, then declare ready-to-build.
