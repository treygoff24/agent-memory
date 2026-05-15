# Web Dashboard — Dogfood Fixups Log

**Parent plan:** `docs/plans/2026-05-12-web-dashboard-build-to-brief.md`
**Started:** 2026-05-14 (Phase 5 dogfood handoff)
**Scope:** Real-time complaints from Trey as he uses the dashboard against
real seeded data. Each fix lives here as a running log — what surfaced,
what changed, where.

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
