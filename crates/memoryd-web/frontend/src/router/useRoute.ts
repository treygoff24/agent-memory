import { useCallback, useEffect, useState } from 'react';

import type { Route } from './types';

import { hashFor, hashParams, parseHash } from './parse';

/**
 * Hash-router hook. Subscribes to `hashchange`, returns the current Route plus
 * a `navigate` that updates `location.hash` (which triggers re-render via the
 * event listener). Query params are preserved across navigation — they're
 * orthogonal state, not part of the route.
 */
export function useRoute(): { route: Route; navigate: (route: Route) => void } {
    const [route, setRoute] = useState<Route>(() => parseHash(window.location.hash));
    useEffect(() => {
        const onHashChange = () => setRoute(parseHash(window.location.hash));
        window.addEventListener('hashchange', onHashChange);
        return () => window.removeEventListener('hashchange', onHashChange);
    }, []);
    const navigate = useCallback((next: Route) => {
        // Update hash via assignment so the browser preserves the query string.
        const target = hashFor(next);
        if (window.location.hash !== target) {
            window.location.hash = target;
        }
    }, []);
    return { route, navigate };
}

/**
 * Read a single route-scoped query param from the hash. Subscribes to
 * `hashchange` so callers re-render when the param changes via anchor
 * navigation. Returns `null` when the param is absent.
 */
export function useHashParam(key: string): string | null {
    const [value, setValue] = useState<string | null>(() => hashParams(window.location.hash).get(key));
    useEffect(() => {
        const onHashChange = () => setValue(hashParams(window.location.hash).get(key));
        window.addEventListener('hashchange', onHashChange);
        return () => window.removeEventListener('hashchange', onHashChange);
    }, [key]);
    return value;
}
