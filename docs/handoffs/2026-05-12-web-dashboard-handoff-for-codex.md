# Web dashboard build-to-brief — handoff for Codex

**Date:** 2026-05-12
**Plan:** `docs/plans/2026-05-12-web-dashboard-build-to-brief.md` (v0.5 gate-policy migration)
**State:** working tree dirty, no commits yet — all changes live as uncommitted edits + untracked files on `main` (54 modified files + new router module + new auditSections module + new fonts).

Trey was running out of subscription quota mid-Phase 3.3 fan-out and asked for a clean handoff so Codex can pick up.

---

## 1. Current gate state (verified green at handoff time)

> Historical gate results below are preserved as handoff evidence. Future continuation work should use the v0.5 tiered policy: `pnpm run check:fast` / targeted checks during implementation, `pnpm run check:local` before claiming a phase/milestone complete, and `pnpm run check:full` only for final/pre-merge validation or directly relevant broad UI changes.

- `pnpm run typecheck` — clean
- `pnpm run lint` — clean
- `pnpm exec vitest run` — 15 test files, 39 tests, all pass
- `pnpm run test:e2e -- --reporter=line` — last run (after Trust Artifact landed but before re-verification): **65/65 green**
- `pnpm run test:visual`, `pnpm run test:a11y`, `pnpm run test:perf` — NOT re-run after Phase 3.2; need re-verification once Phase 3 is done
- `bash scripts/check.sh` (Rust release gate) — NOT touched; no Rust changes in this work

The dev server on Trey's machine is at 127.0.0.1:7137 (production `memoryd-web` binary). Vite dev server (for tests) at 127.0.0.1:5173.

---

## 2. What shipped in this session (read this before continuing)

### Phase 1 (all green)

- 1.0 Fullbleed wiring (`Shell.tsx` gained `fullbleed?: boolean`; Reality Check uses it)
- 1.1 `.view` flex class on every view + layout wrapper
- 1.2 `text-align: left` on `.list-item`
- 1.3 Self-hosted Inter Variable + JetBrains Mono Variable (`src/assets/fonts/`, `@font-face` in `tokens.css`)
- 1.4 Sidebar kbd hints moved from `.count` slot into new `.kbd-hint` span (4-col grid)
- 1.5 `rollup-plugin-visualizer` devDep + `ANALYZE=1` wiring in `vite.config.ts`

### Phase 2 per-view sweeps (all green)

- 2.1 Inbox — real `useReviewActionMutation` wiring; `edit` action is silent no-op until Phase 2.6 grows inline edit (used to fire `window.alert` — blocker fixed)
- 2.2 Reality Check — replaced inline `style={{display:'flex',...}}` band-aid with `.view` class; added EmptyState
- 2.3 Recall — encrypted-summary rendering, day-group section labels, brief-verbatim empty state
- 2.4 Dreams — 3-tab structure (Journal/Questions/Cleanup), status pills as nested filter row (sibling of tablist to dodge axe `aria-required-children`)
- 2.5 Peers — `.peer-cards` grid default, table fallback via hash query `?layout=table`
- 2.6 Governance — edit-with-diff modal (side-by-side body + textarea), 403/409 toasts, `PolicyDecisionTraceCard` extended with `policyTrace` field
- 2.7 Settings — OKLCH L/C/H sliders per token, save-as-custom-theme to localStorage, live preview row

### Phase 3.1 — Hash-based router (DONE)

New module: `src/router/{types,parse,useRoute,index}.ts`. Architecture:

- **App-global query params** live in `location.search`: `theme`, `density`, `reducedMotion`, `fontSize`. These survive cross-route navigation and are read in `main.tsx` + `ThemeProvider.tsx`.
- **Route-local query params** live inside the hash: `#/peers?layout=table`, `#/dreams?dreamTab=questions&dreamState=queued`. Read via `hashParams(window.location.hash)` or the `useHashParam(key)` hook.
- `parseHash(hash)` returns a discriminated `Route` (`{kind: 'inbox'}` | `{kind: 'audit', memoryId: string}` etc.). Unknown hashes fall back to inbox.
- `hashFor(route)` is the inverse for anchor `href`s.
- `useRoute()` subscribes to `hashchange` and exposes `{route, navigate}`.
- `main.tsx` seeds `data-theme`/`data-density`/`data-reduced-motion` synchronously before `createRoot().render()` to eliminate flash-of-wrong-theme.
- Legacy `?view=X&dreamTab=Y&theme=Z` URLs auto-redirect via `applyLegacyViewRedirect()` in `main.tsx`. It splits incoming params: route-local ones (layout, variant, recallState, dreamTab, dreamState, settingsTab, tweaks) move into the hash; app-global stays in search.

Source migrations completed:

- `App.tsx` — `useState<ViewId>` → `useRoute()`, with `routeToView()` mapping + `navigateTo` callback that calls `navigate({kind: id})`
- `Sidebar.tsx` — nav items render as `<a href={hashFor(...)}>`
- `Peers.tsx` — `useLayoutParam` → `useHashParam('layout')`; layout toggle anchor uses `href="#/peers?layout=table"` and `href="#/peers"` (was `href="?"` — blocker fixed)
- `Dreams.tsx`, `Settings.tsx`, `Inbox.tsx`, `Recall.tsx`, `RealityCheck.tsx` — all route-local param reads changed from `URLSearchParams(window.location.search)` to `hashParams(window.location.hash)`

Test surface preserved via legacy redirect — existing `?view=X&dreamTab=Y` URLs in test files keep working unchanged. Only one test URL needed updating: `tests/e2e/settings.spec.ts:13` (the tweaks test had no `?view=settings` to trigger the redirect; it now uses canonical `#/settings?tweaks=1`).

Side benefit: **Phase 4.2 (`g <letter>` chord) is also done** because the chord parser already existed in `useKeymap.ts`; App.tsx now dispatches chord keys via `commands.find((c) => c.shortcut === key)`.

### Phase 3.2 — Trust Artifact (DONE end-to-end, code typechecks + lints; no e2e/visual coverage yet)

New files:

- `src/views/Audit.tsx` — orchestrator. Uses `useAuditQuery(memoryId)`. Walk-graph button is disabled placeholder per plan (defer to v1.1).
- `src/views/auditSections/{HeaderSection,BodySection,ConfidenceSection,RecallSection,ProvenanceChain,PolicyDecisions,PrivacyScanSection,SupersessionHistory,SyncState}.tsx` — 9 sections per brief §View 7
- `src/views/auditSections/index.ts` — barrel

⚠️ **Directory is `auditSections/`, not `audit/`**, because macOS case-insensitive FS collides `./audit` (dir) with `./Audit.tsx` (sibling file) on bare import. If Codex sees `./audit` in any import, that's stale and broken.

Strengthened types in `src/api/types.ts`: `AuditMemoryResponse` no longer has `unknown[]` for the five rich fields. New exported interfaces: `ProvenanceEvent`, `PolicyDecisionEntry`, `PrivacyScanResult`, `SupersessionDirection`, `SupersessionHistoryEntry`, `SyncStateResult`. Field names match the Rust serde-renamed JSON: snake_case throughout (`policy_applied`, `confidence_floor_pass`, `recall_count_30d`, etc.).

CSS added to `src/styles/app.css` (line ~2812 onward) — `.audit-view`, `.audit-section`, `.audit-stat-grid`, `.audit-provenance-chain`, `.audit-policy-list`, `.audit-supersession-cols`, `.audit-sync-devices`, plus utility additions: `.btn.danger`, `.btn:disabled`, `.theme-editor-preview-label`.

### Phase 4.4 (elevated, DONE)

OKLCH L value tuning across all 6 themes for WCAG AA 4.5:1. Documented in `docs/dev/web-dashboard-theme-matrix.md`.

### Critical bug closed during Phase 1: tokens.css orphan

`main.tsx → styles.css` only had `@import "./styles/app.css"` — `tokens.css` was never loaded, so every CSS variable resolved to empty string. `styles.css:1` now has `@import "./styles/tokens.css";` as the first line. This explains why the entire dashboard looked broken in Trey's screenshot at session start.

---

## 3. What's left (priority-ordered)

### Active in_progress when handoff fired

- **Phase 3.3 — Entity Graph** (View 8, d3-force per §9.3 default) — NOT STARTED. Trey was about to fan out a sonnet worker for this. See "How to execute" below.

### Pending

- **Phase 3.4** — Cross-link memory IDs everywhere (wrap any memory_id render in `<a href={hashFor({kind: 'audit', memoryId: id})}>`). Sites: inspector cards, recall ledger, supersession history, governance pending items, dream evidence lists. Grep `memory_id` / `memoryId` in `src/inspector/` + `src/views/` to enumerate.
- **Phase 3 gate** — `pnpm run check:local` plus targeted router/Trust Artifact/Entity Graph Playwright/Vitest checks. Run visual/a11y/e2e/perf suites only where the touched surface requires them. If Phase 3 becomes the final/pre-merge milestone, run `pnpm run check:full` once.
- **Phase 4.1** — Phosphor icon swap. `pnpm add @phosphor-icons/react`. New file `src/ui/icons.ts` mapping glyph names → icon components. Replace nav-item icons (per-view: `Inbox`, `Eye`/`CheckCircle`, `Clock`, `Sparkle`/`Diamond`, `Users`, `Scales`, `Graph`, `Gear`), top-bar (`Terminal` for palette, `Bell` for notifications), empty-state icons. **Keep `.list-item .glyph` as Unicode** for TUI family resemblance (per plan).
- **Phase 4.3** — A11y pass. `pnpm run test:a11y` and fix violations. Specific things to verify per the plan: icon-only buttons have `aria-label`; status colors always paired with glyph or label; tabular regions use real `<table>` semantics; skip-to-main-content link present; tab order matches visual order.
- **Phase 4.5** — Reduced-motion verification + X-5 fix. Toggle OS-level reduced motion and dashboard-level toggle independently. Verify modal scale animation, route fade, `pulse-bad` status dot, RC progress gauge all respect the setting.
- **Phase 4.6** — AI-slop manual verdict. Document in `docs/dev/web-dashboard-baseline.md` as a Phase 4 addendum. Optional: `npx impeccable@2 --json crates/memoryd-web/frontend/src` for informational scan.
- **Phase 4 gate** — `pnpm run check:full` once as final frontend validation + manual theme matrix walkthrough (6 themes × 9 views = 54 spot-checks, document in `docs/dev/web-dashboard-theme-matrix.md`).
- **Phase 5** — Dogfood. Tag the build, rebuild memoryd-web, restart daemon. Each Trey-found defect lives in `docs/plans/2026-05-12-web-dashboard-build-to-brief-fixups.md` as a running log.

### Reviewer findings still open from Phase 2.1-2.5 review pass

Two non-blocker mediums noted:

- RC empty state could add a "Run anyway" button + "Next due" body fragment (brief allows the explicit CTA)
- RC `hasData = query.data && !query.isLoading` is wrong under stale-while-revalidate — should be `hasData = !!query.data` and let the loading banner co-exist with stale data.

---

## 4. Known landmines for Codex

1. **`auditSections/` not `audit/`** — macOS case-insensitive FS gotcha. Don't rename back without renaming `Audit.tsx` first.
2. **Don't run `cargo test --workspace` or `pnpm run check:full` inside any task worktree by default** — use targeted checks/`check:fast` while iterating, `check:local` at milestones, and full validation only at final/pre-merge boundaries.
3. **Don't bump `bench/baseline.*.json`** — explicit human commits only.
4. **Don't modify Stream A modules** unless Trey explicitly redirects. This dashboard work is entirely in `crates/memoryd-web/frontend/`.
5. **No commits without Trey's ask.** Working tree is dirty; that's intentional. All work is reviewable as `git diff`.
6. **Visual test snapshots may need re-baselining** after Phase 3.2/3.3 changes. If `test:visual` flags new diffs, eyeball them before regenerating — fixing the snapshot to match broken CSS is the failure mode.
7. **`AuditMemoryResponse` field names are snake_case** (Rust → JSON via serde). `policy_applied`, `confidence_floor_pass`, `last_recalled`, `recall_count_30d`, `merge_status` etc. — not camelCase.
8. **The dev server reuse pattern**: `playwright.config.ts` has `reuseExistingServer: !process.env.CI`. If a `pnpm run dev` is running on 5173, playwright will use it; if not, it'll spawn one and tear it down. Don't kill the wrong Vite process.

---

## 5. How to execute Phase 3.3 (Entity Graph)

The plan §3.3 says:

> Install `d3-force` (~30KB minified). Build `views/Entities.tsx` graph mode: SVG with force-directed layout, controls in left rail. Wire to `GET /api/entity-graph`. Keep the existing `EntityTable.tsx` as a toggleable "list view" mode (URL: `?mode=table` vs default graph). Click entity node → `#/entities/:id`.

Concrete steps:

1. `pnpm add d3-force` and `pnpm add -D @types/d3-force` from `crates/memoryd-web/frontend/`. Confirm bundle still <400KB gzipped via `ANALYZE=1 pnpm run build`.
2. Existing `src/views/Entities.tsx` already renders a table view. Refactor: read `mode` via `useHashParam('mode')` (default 'graph'). When mode is 'table', keep current table renderer; when 'graph', render new force-directed SVG.
3. New file `src/views/entitiesView/EntityGraph.tsx`:
   - Read `GET /api/entity-graph?namespace=&depth=&focus=` via a new `useEntityGraphQuery` (add to `src/api/queries.ts`)
   - Type the response: `{ nodes: Array<{id, label, namespace, sensitivity?, recall_count?}>, edges: Array<{source, target, kind}> }` — verify the Rust shape in `crates/memoryd-web/src/routes/` first
   - Use `forceSimulation` with `forceManyBody`, `forceLink`, `forceCenter`. Run `tick()` until alpha decays or render a fixed number of ticks (~120) for determinism, then static-render SVG. This avoids reduced-motion concerns and snapshot flakiness.
   - Nodes: `<circle>` with `<text>` labels. Color by sensitivity (use existing semantic tokens `--ok`/`--warn`/`--bad`/`--fg-3`).
   - On node click: `navigate({kind: 'entities', entityId: node.id})`.
4. Left-rail controls component `EntityGraphControls.tsx`: namespace filter (select), depth (range 1-5), focus entity (text input), density toggle, color-by toggle (sensitivity / recall frequency / confidence).
5. Right rail detail card when an entity id is in the route: entity name + memory list + supersession chain. Reuse `Inspector` if it fits, or a smaller dedicated component.
6. Empty state: `<EmptyState title="No entities mapped for this namespace yet." />`.
7. Tests:
   - Update `tests/views/Entities.test.tsx` for graph default + table toggle
   - Add `tests/visual/entities.spec.ts` graph variant (warm-dark theme only initially — graph rendering is deterministic if simulation is pre-ticked)
   - Add `tests/e2e/entities.spec.ts` flow: click node → URL becomes `#/entities/:id`

---

## 6. How to execute Phase 3.4 (cross-link memory IDs)

Grep + wrap:

```bash
cd crates/memoryd-web/frontend/src
grep -rln "memory_id\|memoryId" --include="*.tsx" inspector/ views/
```

For each render site, replace the bare text node with an anchor:

```tsx
// before
<span className="mono">{item.memory_id}</span>
// after
<a className="mono" href={hashFor({kind: 'audit', memoryId: item.memory_id})}>{item.memory_id}</a>
```

`hashFor` already imported in `Sidebar.tsx`, `Audit.tsx`, `SupersessionHistory.tsx`, `HeaderSection.tsx`. Add the import wherever needed.

Style: add a `.memory-id-link` class to `app.css` so the anchor gets `color: var(--fg-2); text-decoration: none; :hover { text-decoration: underline; }`. Matches the existing `.audit-id` pattern.

---

## 7. Files Codex must not blindly trust

These were touched by sonnet workers during the session and Trey's standing rule is to verify worker output on disk:

- `src/views/Governance.tsx` — worker landed edit-with-diff modal. Look for: blocking OS modals, `console.log`, `alert()`. Should be none.
- `src/views/settings/ThemeEditorTab.tsx` — worker landed OKLCH sliders. There was a dead `savedName` state that I removed; verify nothing else dangling.
- `src/inspector/cards/PolicyDecisionTraceCard.tsx` — worker added `TraceRows` sub-component.
- `src/views/peersView/{PeerCard,CoordStrip}.tsx` — new files from peers worker.

If anything looks AI-sloppy (decorative comments, narrating motion, `// updated to use new API`), kill it.

---

## 8. Suggested Codex execution order

1. Bring up the dev server (`pnpm run dev`) and open `http://127.0.0.1:5173/#/audit/<some-id>` against seeded data. Confirm Trust Artifact renders all 9 sections — this is the single biggest piece of unverified surface from this session.
2. Phase 3.4 (cross-link memory IDs) — small, mechanical, sets up the Trust Artifact entry points for real dogfooding.
3. Phase 3.3 (Entity Graph) — biggest remaining piece. Budget 2-3 hours.
4. Phase 3 gate (`pnpm run check:local` plus targeted visual/a11y/e2e/perf checks; `check:full` only if this is final/pre-merge).
5. Phase 4.1 (Phosphor icons) — cosmetic but plan-required.
6. Phase 4.3 + 4.5 (a11y + reduced-motion) — fix what axe-playwright surfaces.
7. Phase 4.6 (AI-slop verdict) — final manual pass.
8. Phase 4 gate (`pnpm run check:full` once + manual theme matrix).
9. Hand back to Trey for Phase 5 dogfood.

---

## 9. Quick reference

**Run gates** (from `crates/memoryd-web/frontend/`):

```bash
pnpm run check:fast
pnpm run check:local
pnpm run check:full
pnpm run typecheck
pnpm run lint
pnpm exec vitest run
pnpm run test:gentle
pnpm run test:e2e:gentle
pnpm run test:e2e -- --reporter=line
pnpm run test:visual
pnpm run test:a11y
pnpm run test:perf
```

**Bundle analyze**:

```bash
ANALYZE=1 pnpm run build
# open dist/stats.html
```

**Plan file**: `docs/plans/2026-05-12-web-dashboard-build-to-brief.md`
**Brief**: `docs/design/claude-design-brief/02-dashboard-views.md`
**Baseline doc**: `docs/dev/web-dashboard-baseline.md`
**Theme matrix**: `docs/dev/web-dashboard-theme-matrix.md`

Good luck. — Claude
