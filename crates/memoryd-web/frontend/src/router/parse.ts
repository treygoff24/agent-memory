import type { Route } from './types';

const DEFAULT_ROUTE: Route = { kind: 'inbox' };

/**
 * Parse a `location.hash` string into a Route. Unknown or malformed hashes
 * default to inbox. The hash format is `#/<view>` or `#/<view>/<id>` with an
 * optional `?key=value&...` route-scoped query suffix (e.g. `#/peers?layout=table`).
 * Route-scoped query state is read separately via `hashParams()`.
 */
export function parseHash(hash: string): Route {
    // Strip route-scoped query suffix before route parsing.
    const [pathPart] = hash.split('?', 2) as [string, string | undefined];
    const trimmed = pathPart.replace(/^#\/?/, '');
    if (trimmed.length === 0) return DEFAULT_ROUTE;
    const [head, tail] = trimmed.split('/', 2) as [string, string | undefined];
    switch (head) {
        case 'inbox':
            return { kind: 'inbox' };
        case 'reality':
            return { kind: 'reality' };
        case 'recall':
            return { kind: 'recall' };
        case 'dreams':
            return { kind: 'dreams' };
        case 'peers':
            return { kind: 'peers' };
        case 'governance':
            return { kind: 'governance' };
        case 'entities':
            return tail ? { kind: 'entities', entityId: tail } : { kind: 'entities' };
        case 'settings':
            return { kind: 'settings' };
        case 'audit':
            // Without a memory_id, an audit hash is malformed; fall back to
            // inbox rather than rendering an empty artifact.
            return tail ? { kind: 'audit', memoryId: tail } : DEFAULT_ROUTE;
        default:
            return DEFAULT_ROUTE;
    }
}

/** Inverse: serialize a Route to the `#/...` form used by anchor hrefs. */
export function hashFor(route: Route): string {
    switch (route.kind) {
        case 'entities':
            return route.entityId ? `#/entities/${encodeURIComponent(route.entityId)}` : '#/entities';
        case 'audit':
            return `#/audit/${encodeURIComponent(route.memoryId)}`;
        default:
            return `#/${route.kind}`;
    }
}

/**
 * Read route-scoped query params from a hash string. The hash format may carry
 * a `?key=value&...` suffix after the route segment (`#/peers?layout=table`);
 * those params are route-local state and live independently of the page-global
 * `location.search` (which holds app-global state like `?theme=`, `?density=`).
 */
export function hashParams(hash: string): URLSearchParams {
    const idx = hash.indexOf('?');
    if (idx === -1) return new URLSearchParams();
    return new URLSearchParams(hash.slice(idx + 1));
}

/**
 * Backwards-compat for the pre-router `?view=...` URLs. If the hash is empty
 * and `?view=` is set, return the implied Route and signal that the caller
 * should rewrite the URL to the hash form. Returns `null` when no migration
 * applies.
 */
export function legacyViewRedirect(search: string): Route | null {
    const params = new URLSearchParams(search);
    const legacy = params.get('view');
    if (!legacy) return null;
    switch (legacy) {
        case 'inbox':
        case 'reality':
        case 'recall':
        case 'dreams':
        case 'peers':
        case 'governance':
        case 'entities':
        case 'settings':
            return { kind: legacy };
        default:
            return null;
    }
}
