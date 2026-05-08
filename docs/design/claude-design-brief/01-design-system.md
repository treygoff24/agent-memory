# Memorum — Design System

This is the canonical design language for both the TUI and the localhost web dashboard. **Themability is a Day-1 invariant**: every color, font, density, glyph, and motion value is a named token, never a hardcoded literal in a component. Users will customize. The defaults below are warm dark amber — not cyberpunk neon, not cold blue, not corporate grey.

## 1. Color — OKLCH tokens

We use OKLCH (perceptually uniform) so theme variants stay legible across hue rotations. Every component refers to a **role token**, never a literal color.

### 1.1 Default theme: `warm-dark` (matches TUI)

```css
/* Surfaces */
--bg: oklch(0.16 0.006 70); /* page background */
--surface: oklch(0.2 0.007 70); /* raised regions: header, sidebar, card */
--surface-2: oklch(0.24 0.008 70); /* selection, hover, popover */
--border: oklch(0.3 0.01 70); /* hairline separators */
--border-soft: oklch(0.26 0.008 70); /* internal sub-separators */

/* Ink (foreground text) */
--fg: oklch(0.93 0.012 80); /* primary text */
--fg-2: oklch(0.72 0.014 75); /* secondary text, labels */
--fg-3: oklch(0.52 0.012 70); /* tertiary, metadata, captions */
--fg-4: oklch(0.4 0.01 70); /* disabled, placeholders, decoration */

/* Accent — used SPARINGLY. Reserved for selection, brand sigil, primary action. */
--accent: oklch(0.8 0.13 72); /* warm amber */
--accent-soft: oklch(0.32 0.04 72); /* subtle accent surface */

/* Semantic */
--ok: oklch(0.74 0.13 145); /* success, healthy state */
--warn: oklch(0.82 0.14 80); /* warning, due, drift */
--bad: oklch(0.66 0.2 25); /* error, conflict, refusal */
--info: oklch(0.72 0.08 230); /* informational, recall, links — muted not neon */
```

### 1.2 Sibling themes (also Day-1)

- **`warm-light`** — same tokens, light-mode counterparts. Background `oklch(0.98 0.005 80)`, ink `oklch(0.20 0.012 70)`, accent unchanged.
- **`high-contrast`** — pure black/white surfaces, accents bumped to chroma 0.20+, borders solid.
- **`monochrome`** — chroma collapsed to ~0.005 across the board; for users who want zero color cues.
- **`cool-dark`**, **`cool-light`** — hue rotated to ~230 (blue) for users who don't want warm.

### 1.3 Themability rules

- Components reference tokens by **role**, not by literal name. `var(--surface)` not `var(--bg-warm-dark)`.
- Theme switching is live — no reload. CSS custom properties on `<html>`.
- Custom user themes are a TOML file. Same 23-token shape as the TUI's `ColorTokens`.
- A component that needs a color outside the 23 is a smell — escalate to expand the token set, don't reach for a literal.

## 2. Typography

Two variable fonts, both self-hosted (no Google Fonts at runtime — privacy + offline + LAN-isolated install).

### 2.1 Faces

- **Inter Variable** — UI prose, labels, headings, button text, navigation, body copy.
- **JetBrains Mono Variable** — memory IDs, timestamps, file paths, code snippets, commit SHAs, namespaces, anything that should be tabular or unambiguous.

```css
--font-sans: 'Inter Variable', system-ui, -apple-system, sans-serif;
--font-mono: 'JetBrains Mono Variable', ui-monospace, SFMono-Regular, Menlo, monospace;
```

### 2.2 Scale

```css
--text-xs: 11px / 1.4; /* metadata, captions, micro-labels */
--text-sm: 12.5px / 1.5; /* secondary text, list metadata */
--text-base: 14px / 1.55; /* body default */
--text-md: 15px / 1.5; /* inspector body */
--text-lg: 17px / 1.4; /* panel headings */
--text-xl: 22px / 1.35; /* focus-mode headings, Reality Check question */
--text-2xl: 28px / 1.3; /* dashboard route titles */
```

Line length is capped at **70ch** for prose blocks. Numbers are **always tabular** (`font-variant-numeric: tabular-nums`) when displayed in lists or tables.

### 2.3 Weight policy

- Inter: 400 default, 500 for emphasis, 600 for headings. Avoid 700 except for very tight contexts.
- JetBrains Mono: 400 only. Italic 400 reserved for namespace strings.

## 3. Iconography

Use **Phosphor Icons** (Regular weight, 16px or 20px). Phosphor matches the TUI's quiet-glyph spirit better than Lucide or Heroicons. Every icon has a **role-token color** by default; never raw hex.

### 3.1 Glyph→role mapping (preserve from TUI)

| TUI glyph | Phosphor icon  | Role          | Token      |
| --------- | -------------- | ------------- | ---------- |
| `●`       | `circle-fill`  | review needed | `--accent` |
| `▸`       | `play`         | recall event  | `--info`   |
| `⚠`       | `warning`      | conflict      | `--bad`    |
| `▣`       | `square-half`  | due / verify  | `--warn`   |
| `◇`       | `diamond`      | dream output  | `--warn`   |
| `○`       | `circle`       | inert memory  | `--fg-3`   |
| `◆`       | `diamond-fill` | brand sigil   | `--accent` |

Keep the icon vocabulary **small** (~12 distinct icons across the whole dashboard). Don't decorate with icons; use them only when they add scanability.

## 4. Layout and density

### 4.1 Density modes

- **`comfortable`** (default web) — 8px row gap, 12px section gap, 16px page padding.
- **`compact`** — 4px row gap, 8px section gap, 12px page padding. Toggleable in settings.

### 4.2 Spatial primitives

```css
--space-1: 4px;
--space-2: 8px;
--space-3: 12px;
--space-4: 16px;
--space-5: 24px;
--space-6: 32px;
--space-7: 48px;
--space-8: 64px;
--radius-xs: 3px; /* tags, badges */
--radius-sm: 6px; /* list items, inputs */
--radius-md: 10px; /* cards, terminal frames */
--radius-lg: 16px; /* modals */
```

### 4.3 Borders

Single hairline (1px, `--border-soft`) between sub-sections; full border (1px, `--border`) around independent regions. **Never doubled borders** when two bordered regions touch — use a single shared border (this is the same lesson as TUI `Spacing::Overlap(1)`).

## 5. Motion

**Restraint.** This is a console, not a marketing site.

```css
--ease-out: cubic-bezier(0.16, 1, 0.3, 1);
--ease-in-out: cubic-bezier(0.65, 0, 0.35, 1);
--dur-fast: 120ms; /* hover, focus, button-press */
--dur-medium: 220ms; /* panel transition, modal open */
--dur-slow: 380ms; /* route change */
```

### 5.1 Allowed motion

- Fade-in on data load (≤220ms). No skeleton pulse animations — they are visual noise here. Use a static `--fg-4` placeholder block with text "loading…" if needed.
- Focus ring: instant, no transition.
- Modal open: scale 0.98→1.00 + fade, 220ms, `--ease-out`.
- Route change: fade, 220ms.
- List-item entry: subtle fade, 120ms, **only on first paint** — never on filter changes (would be noisy).

### 5.2 Forbidden motion

- No bouncing.
- No springs except focus-following scrolls.
- No infinite-loop animations except a small pulse on the daemon-down banner (and even that is opt-in via theme).
- **`prefers-reduced-motion: reduce`** disables all of the above; replace with instant transitions.

## 6. Sound

None. Ever. Not even for Reality Check completion. This is a desktop console, not a notification surface.

## 7. Accessibility floor

- WCAG AA (4.5:1 for body text, 3:1 for large text and UI components) on every theme.
- All interactive elements have a visible focus ring (`outline: 2px solid var(--accent); outline-offset: 2px;`).
- Full keyboard navigation. Every action reachable without a mouse.
- Skip-to-main-content link, present but visually hidden until focused.
- Tabular data uses real `<table>` elements with `<th scope=...>`.
- ARIA labels on all icon-only buttons.
- Screen-reader-only `.sr-only` class for context that's visually obvious but missing from DOM.
- Color is **never** the only signal — every state that uses color also uses a glyph or label.

## 8. Component vocabulary

Standard primitives the dashboard composes:

- **Pill** — filter chip with optional count. Active state uses `--surface` background + `--border`.
- **Badge** — inline status (sensitivity, encryption, governance state). Smaller than a pill.
- **List item** — three-column grid (glyph / body / meta), selection state uses inset 2px `--accent` left border.
- **Inspector panel** — right-hand detail view, two-column inside (body + sidecar metadata).
- **Command palette** — `:`-triggered modal, fuzzy match, scoped commands.
- **Toast** — bottom-center, auto-dismiss 4s, manual close. One at a time. Used sparingly.
- **Banner** — top-of-page, persistent, dismissible. Used for daemon-down or sync-conflict states.
- **Empty state** — centered icon + heading + body + (optional) action. No illustrations.
- **Status dot** — 8px circle, semantic color, paired with a label. Always paired.

## 9. Brand

- **Sigil:** `◆` — diamond filled. Used at the start of the wordmark `◆ memorum`.
- **Wordmark:** lowercase `memorum`, Inter 500, letter-spacing 0.04em.
- **No logo image** in v1. Sigil + wordmark is enough. Keep it under 20px tall in chrome.

## 10. Anti-patterns to avoid

- **No glassmorphism.** This is not 2021 macOS. Solid surfaces with hairlines.
- **No gradient buttons.** Solid `--surface-2` with text `--fg`, accent only on the primary CTA.
- **No emoji** in UI strings. Phosphor icons cover any iconography need.
- **No "AI sparkle"** styling. We are agent infrastructure, not a brand selling AI to consumers.
- **No drop shadows for hierarchy** — use borders. One subtle shadow on modals only.
- **No card-grid layouts** as a default. Lists, tables, and panels carry the dashboard.
