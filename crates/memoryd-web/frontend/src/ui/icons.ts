/**
 * Phosphor icon registry. Single source of truth for the entire dashboard's
 * iconography — every other module imports from here, never from
 * `@phosphor-icons/react` directly. This keeps tree-shaking honest (one
 * per-icon import here covers every call site) and makes a future
 * icon-system change a single-file edit.
 *
 * Brief §3.1 (TUI glyph → Phosphor → role → token):
 *   ● → Circle weight=fill → review needed → --accent
 *   ▸ → Play               → recall event  → --info
 *   ⚠ → Warning            → conflict      → --bad
 *   ▣ → SquareHalf         → due / verify  → --warn
 *   ◇ → Diamond            → dream output  → --warn
 *   ○ → Circle             → inert memory  → --fg-3
 *   ◆ → Unicode (not a Phosphor entry — brand sigil is rendered as the
 *        literal `◆` character per plan §5 invariant 6).
 *
 * Phosphor 2.x uses a `weight` prop for fill/regular/thin variants rather
 * than separate `<CircleFill />` components, so the `weight` field below
 * carries the brief's intent into the runtime component.
 */

import {
    Bell,
    CheckCircle,
    Circle,
    Clock,
    Diamond,
    Eye,
    Gear,
    Graph,
    type Icon,
    type IconProps,
    Play,
    Scales,
    SquareHalf,
    Terminal,
    Tray,
    Users,
    Warning,
} from '@phosphor-icons/react';

export type GlyphKind = 'review' | 'recall' | 'conflict' | 'due' | 'dream' | 'inert';

export interface GlyphEntry {
    component: Icon;
    weight: NonNullable<IconProps['weight']>;
    defaultColor: string;
}

export const glyphIcons: Record<GlyphKind, GlyphEntry> = {
    review: { component: Circle, weight: 'fill', defaultColor: 'var(--accent)' },
    recall: { component: Play, weight: 'regular', defaultColor: 'var(--info)' },
    conflict: { component: Warning, weight: 'regular', defaultColor: 'var(--bad)' },
    due: { component: SquareHalf, weight: 'regular', defaultColor: 'var(--warn)' },
    dream: { component: Diamond, weight: 'regular', defaultColor: 'var(--warn)' },
    inert: { component: Circle, weight: 'regular', defaultColor: 'var(--fg-3)' },
};

/**
 * Named Phosphor re-exports for sites that need an icon at a non-default
 * weight, color, or context (e.g. DreamList's filled-vs-regular Diamond
 * variant, Inspector's empty-state ico). Importing from here keeps the
 * one-place-to-touch-icons invariant.
 */
export { CheckCircle, Circle, Diamond, Play };

export type NavIconKind = 'inbox' | 'reality' | 'recall' | 'dreams' | 'peers' | 'governance' | 'entities' | 'settings';

export const navIcons: Record<NavIconKind, Icon> = {
    inbox: Tray,
    reality: Eye,
    recall: Clock,
    dreams: Diamond,
    peers: Users,
    governance: Scales,
    entities: Graph,
    settings: Gear,
};

export const chromeIcons = {
    palette: Terminal,
    bell: Bell,
    check: CheckCircle,
} as const;

export type { Icon, IconProps };
