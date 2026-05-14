/**
 * Hash-based route shapes. The URL convention:
 * - Hash drives view routing: `#/<view>` or `#/<view>/:id`.
 * - App-global query params live in `location.search` (?theme=, ?density=,
 *   ?reducedMotion=) — they survive navigation and apply across the shell.
 * - Route-local query params live inside the hash (`#/peers?layout=table`,
 *   `#/dreams?dreamTab=questions&dreamState=queued`) and are read via
 *   `hashParams()` / `useHashParam()` from `router/`. They clear on route
 *   navigation, which matches their semantic scope.
 *
 * Phase 3.2 fills in the Trust Artifact view; Phase 3.3 splits Entities into
 * graph vs detail. Until then, `audit` renders a placeholder and `entities` +
 * `entities/:id` both render the existing Entities view.
 */
export type Route =
    | { kind: 'inbox' }
    | { kind: 'reality' }
    | { kind: 'recall' }
    | { kind: 'dreams' }
    | { kind: 'peers' }
    | { kind: 'governance' }
    | { kind: 'entities'; entityId?: string }
    | { kind: 'settings' }
    | { kind: 'audit'; memoryId: string };

export type RouteKind = Route['kind'];
