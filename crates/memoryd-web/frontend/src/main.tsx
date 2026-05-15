import React from 'react';
import ReactDOM from 'react-dom/client';

import { App } from './App';
import { hashFor, legacyViewRedirect } from './router';
import './styles.css';

const VALID_THEMES = ['warm-dark', 'warm-light', 'cool-dark', 'cool-light', 'monochrome', 'high-contrast'] as const;
const VALID_DENSITY = ['comfortable', 'compact'] as const;
const VALID_MOTION = ['os', 'on', 'off'] as const;

/**
 * Seed visual root attributes BEFORE React's first paint to eliminate the
 * flash-of-wrong-theme that any `useEffect`-based seeding would cause.
 *
 * Precedence per plan §3.1:
 *   1. `?theme=` / `?density=` / `?reducedMotion=` query params (test surface)
 *   2. localStorage (user-saved preferences)
 *   3. `data-theme` already set on the served `index.html` (warm-dark default)
 */
function seedRootAttributes(): void {
    const root = document.documentElement;
    const params = new URLSearchParams(window.location.search);

    const stored = (key: string): string | null => {
        try {
            return window.localStorage.getItem(key);
        } catch {
            return null;
        }
    };

    const theme = params.get('theme') ?? stored('memorum.theme') ?? root.getAttribute('data-theme');
    if (theme && (VALID_THEMES as readonly string[]).includes(theme)) {
        root.setAttribute('data-theme', theme);
    }

    const density = params.get('density') ?? stored('memorum.density');
    if (density && (VALID_DENSITY as readonly string[]).includes(density)) {
        root.setAttribute('data-density', density);
    }

    const motion = params.get('reducedMotion') ?? stored('memorum.reducedMotion');
    if (motion && (VALID_MOTION as readonly string[]).includes(motion)) {
        if (motion === 'on') root.setAttribute('data-reduced-motion', 'on');
        else root.removeAttribute('data-reduced-motion');
    }
}

/**
 * One-time migration for the pre-router `?view=...` URL shape. If the hash is
 * empty AND a legacy `?view=` is present, rewrite to the hash form. Route-local
 * state selectors (layout, variant, recallState, dreamTab, dreamState,
 * settingsTab, tweaks) move into the hash query alongside the route — they're
 * route-scoped and should clear on navigation. App-global preferences (theme,
 * density, reducedMotion, fontSize) stay in `location.search` since they
 * survive cross-route navigation.
 */
const ROUTE_LOCAL_PARAMS = new Set([
    'layout',
    'variant',
    'recallState',
    'dreamTab',
    'dreamState',
    'settingsTab',
    'tweaks',
    'mode',
]);

function applyLegacyViewRedirect(): void {
    if (window.location.hash) return;
    const target = legacyViewRedirect(window.location.search);
    if (!target) return;
    const incoming = new URLSearchParams(window.location.search);
    incoming.delete('view');
    const remainingSearch = new URLSearchParams();
    const routeLocal = new URLSearchParams();
    incoming.forEach((value, key) => {
        if (ROUTE_LOCAL_PARAMS.has(key)) routeLocal.append(key, value);
        else remainingSearch.append(key, value);
    });
    const searchStr = remainingSearch.toString();
    const hashStr = `${hashFor(target)}${routeLocal.toString() ? `?${routeLocal.toString()}` : ''}`;
    const newUrl = `${window.location.pathname}${searchStr ? `?${searchStr}` : ''}${hashStr}`;
    window.history.replaceState(null, '', newUrl);
}

seedRootAttributes();
applyLegacyViewRedirect();

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
    <React.StrictMode>
        <App />
    </React.StrictMode>,
);
