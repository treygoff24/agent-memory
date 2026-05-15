# Web Dashboard — Phase 0 Baseline

**Captured:** 2026-05-12, against `crates/memoryd-web/frontend` at git HEAD `37f94c2`.
**Plan:** `docs/plans/2026-05-12-web-dashboard-build-to-brief.md` (v0.4).

## Build identity

- `memoryd serve` pid 3231, socket `~/.memorum-dev/memoryd.sock`
- `memoryd-web` pid 4111, port 7137 (`GET /` → 200)
- Daemon was seeded via `bash scripts/seed-dev-substrate.sh` against `~/memorum-dev`

## Test gate at baseline (all green)

| Suite                          | Cases   | Result       |
| ------------------------------ | ------- | ------------ |
| `pnpm run lint`                | —       | clean        |
| `pnpm run typecheck`           | —       | clean        |
| `pnpm run test --run` (vitest) | 39      | 39 pass      |
| `pnpm run test:visual`         | 198     | 198 pass     |
| `pnpm run test:a11y` (axe)     | 48      | 48 pass      |
| `pnpm run test:e2e`            | 65      | 65 pass      |
| `pnpm run test:perf`           | 1       | 1 pass       |
| **Total**                      | **351** | **351 pass** |

## Visual baseline reference

The authoritative "before" screenshot is the 1440-viewport capture Trey sent on 2026-05-12,
showing list rows with centered titles floating in a black void, inspector floating in the right
margin, and panes collapsing to natural content height. This file documents the structural state
underlying that visual — see §2 of the plan for the gap inventory.

`docs/dev/web-dashboard-screenshots/` will collect per-phase screenshots as we go. Phase 0 keeps
Trey's original as the canonical "before."

## Lighthouse baseline (skipped)

We don't have the Lighthouse CLI installed in this workspace and the metrics it would produce
(FCP/LCP/CLS) aren't load-bearing for the visual problems we're fixing — those are layout-chain
contract violations, not perf. The perf test suite already enforces a 60fps mean-frame budget on
the Recall view's heavy ledger and a gzipped-bundle budget (80 KB CSS today) — those are the
numbers we'll watch. Under the v0.5 tiered gate policy, bundle visualizer output is a final/Phase-4 check rather than an every-microtask inner-loop check.

## What the gate does NOT catch

The visual test suite asserts DOM presence (`getByTestId(...).toBeAttached()`,
`toHaveAttribute('data-theme', ...)`) but does not take pixel screenshots. **All 198 cases pass
against the visually broken state in Trey's screenshot.** That's the load-bearing observation
for Phase 1: the bugs we're fixing don't have automated regression coverage today. Phase 1's
between-phase gate is the same suite, so it will keep passing throughout. Manual visual review
against the brief is the real verification until we extend the visual suite with pixel diffs
(deferred — out of scope for v1).

## State of known anti-patterns at baseline

From the reviewer agent's orientation read of `src/styles/app.css`:

- `.toast-stack` is defined twice with conflicting positioning (slop accumulation, second
  definition wins). Pre-existing; tagged for a fixup pass during Phase 4.6.
- `pulse-bad` infinite animation has no `prefers-reduced-motion` guard at CSS level (relies on
  `data-reduced-motion="on"` attribute only — OS-level preference is ignored). Tagged as Phase
  4.5 work (X-5 from §2.3 of the plan).
- `.view` class is not defined anywhere in `app.css` (confirms F-3 from the plan §2.1 — the
  flex-chain join is genuinely missing, not a misdiagnosis).

## What Phase 0 proves

That the gate machinery works end-to-end against the current daemon-seeded data, that lint /
typecheck / vitest / Playwright (visual + a11y + e2e + perf) all clear on the existing
codebase, and that the visual bugs we're about to fix live below the level the existing tests
catch. Future Phase 1 follow-up should use `pnpm run check:fast` and targeted checks during
structural fixes, then `pnpm run check:local` plus relevant visual/a11y checks at the phase
boundary.

---

## Phase 4.6 — AI-slop verdict (2026-05-14)

Manual walkthrough against brief §10 anti-patterns, plus automated grep over the live CSS and
component sources. **Verdict: clean across all six axes.**

| Anti-pattern              | Method                                            | Finding   |
| ------------------------- | ------------------------------------------------- | --------- |
| Glassmorphism             | `grep "backdrop-filter\|blur("` over `src/**/*.css` | 0 hits    |
| Gradient buttons          | `grep "linear-gradient\|radial-gradient"`         | 0 hits    |
| Emoji in UI strings       | `grep "[◇●▸⚠○▣◈◆]"` post-Phosphor swap           | Only the brand sigil `◆` in `TopBar.tsx` and `FocusStrip.tsx` (the explicit `§5 invariant 6` exception); plus `icons.ts` doc comments. |
| AI-sparkle / glow / aurora| `grep "sparkle\|glow\|aurora\|shimmer"`           | 0 hits    |
| Drop-shadows for hierarchy| Audited 18 `box-shadow` uses in `app.css`         | All functional: inset accent strips on active nav items, modal/tooltip elevation (legitimate overlay lift), pulse-bad animated outline ring on status dots. **Zero "drop-shadow for visual hierarchy in normal flow."** |
| Card-grid as default      | `grep "cards-grid\|card-grid"` in `src/**/*.tsx`  | 0 hits in components; brief permits Settings (`.cards-grid`) and `.theme-grid` which are explicit grid surfaces, not default-content containers. |

### Stale-state notes from Phase 0 status block

The two issues tagged in the Phase 0 anti-patterns section were addressed:

- `.toast-stack` duplicate definition (Phase 4.6 tag) — manually verified clean after Phase 1
  CSS tuning; the two definitions converged onto one canonical block during the per-view sweep.
- `pulse-bad` animation OS-level reduced-motion gap — closed in Phase 4.5 by adding the
  `@media (prefers-reduced-motion: reduce)` block in `tokens.css`. OS preference now wins
  unless explicitly overridden via the dashboard-level toggle.

### Method

`impeccable@2 --json` is not run as a gate per plan v0.3 (the rule set isn't pinned with this
repo); the manual walkthrough plus targeted greps above are the canonical method.
