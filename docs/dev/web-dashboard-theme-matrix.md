# Web Dashboard — Theme Contrast Matrix (Phase 4.4)

**Captured:** 2026-05-12, after token tuning landed alongside Phase 1.
**Gate:** `pnpm run test:a11y` (axe-playwright with `color-contrast: enabled`) = 48/48 pass.

## Why Phase 4.4 ran inline with Phase 1

Phase 1.3 self-hosted the variable fonts and discovered, as a side effect, that
`tokens.css` was never imported into the bundle — `src/main.tsx → styles.css`
only had `@import "./styles/app.css"`, so every `var(--bg)`, `var(--fg)`,
`var(--accent)` etc. resolved to undefined/empty at runtime. The "warm-dark"
feel in Trey's 2026-05-12 screenshot was browser defaults plus a handful of
hardcoded colors in `app.css`, not the design tokens. Closing the import
exposed ~50 latent contrast violations that axe could now see. Per the plan's
§5 invariant 1 ("Every theme passes WCAG AA on every view"), Phase 1 couldn't
land with the a11y gate red — so Phase 4.4 (theme matrix tuning) moved up.

## What changed in `tokens.css`

For each of the 6 themes, `fg-3` and `fg-4` lightness were the dominant
levers — these are the "muted text on surface" tokens that axe flagged
repeatedly. Border tokens also moved to satisfy adjacent visual hierarchy.

| Theme                 | `--fg-3` before | `--fg-3` after | `--fg-4` before | `--fg-4` after |
| --------------------- | --------------- | -------------- | --------------- | -------------- |
| `warm-dark` (default) | 0.52            | **0.74**       | 0.40            | **0.62**       |
| `warm-light`          | 0.55            | **0.42**       | 0.70            | **0.50**       |
| `cool-dark`           | 0.52            | **0.74**       | 0.40            | **0.62**       |
| `cool-light`          | 0.55            | **0.42**       | 0.70            | **0.50**       |
| `monochrome`          | 0.52            | **0.74**       | 0.40            | **0.62**       |
| `high-contrast`       | 0.78            | **0.85**       | 0.60            | **0.72**       |

`fg-2` also dropped/rose by ~0.05 in light/dark themes to clear `.btn`-on-surface
contrast (the failure mode was the un-pressed density/motion toggles in Settings
where the gray-on-light gray pairing landed at 3.97:1 / 4.07:1 — needed ~4.5).

`border` and `border-soft` were strengthened in dark themes so borders
visually register against the surface they sit on — they're not contrast-graded
by axe (decorative), but the brief calls for visible structure and the original
values were too close to surface.

Semantic colors (`--ok`, `--warn`, `--bad`, `--info`) gained L in dark themes
and lost L in light themes, again to keep them WCAG AA when used as text or
contrasted glyphs.

`--accent-soft` was reverted to the original chroma so the button-primary
pairing (text on `--accent-soft`) stays readable.

## Component fix that wasn't theme-driven

`Governance.tsx`'s inspector-pane `.pane-scroll` failed
`scrollable-region-focusable` (Safari keyboard a11y, WCAG 2.1.1) on all 6
themes because the inspector content is read-only at this density — no
focusable children inside the scroll region. Added `tabIndex={0}` to the
governance inspector's `.pane-scroll` only. Other views' inspectors don't
hit this today (they either don't overflow or contain focusable content);
each per-view sweep in Phase 2 should verify and add `tabIndex={0}` where
warranted.

## Manual matrix walkthrough — deferred to Phase 4 proper

The plan's original Phase 4.4 also called for a 6-themes × 9-views manual
walkthrough (54 spot-checks). axe got us to mechanical WCAG AA compliance; the
manual pass — looking for _aesthetic_ issues per theme (does monochrome look
right, does high-contrast feel jarring, does cool-light feel cold rather than
clinical) — will happen during Phase 5 dogfood with Trey. This doc gets a
"Phase 4 manual addendum" section appended after that.

## Phase 4 mechanical re-check (2026-05-14)

Re-ran `pnpm run test:a11y` after Phase 4 changes (Phosphor swap in 4.1,
g-chord indicator in 4.2, skip-to-main-content + main `tabIndex` in 4.3):
**48/48 pass**, zero axe-core violations across 6 themes × 8 surface views
with `color-contrast: enabled`. No theme tokens needed adjustment — the
icon swaps inherited the role-token colors (`var(--accent)`, `var(--info)`,
`var(--warn)`, `var(--bad)`, `var(--fg-3)`) that already pass contrast.
Manual 54-spot-check walkthrough still parked for Phase 5.
