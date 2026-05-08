import {
    defaultThemePreferences,
    densities,
    reducedMotionModes,
    themes,
    type Density,
    type ReducedMotion,
    type Theme,
    type ThemePreferences,
} from './types';

function readToken<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
    if (typeof localStorage.getItem !== 'function') return fallback;
    const value = localStorage.getItem(key);
    return allowed.includes(value as T) ? (value as T) : fallback;
}

export function loadPreferences(): ThemePreferences {
    return {
        theme: readToken<Theme>('memorum.theme', themes, defaultThemePreferences.theme),
        density: readToken<Density>('memorum.density', densities, defaultThemePreferences.density),
        reducedMotion: readToken<ReducedMotion>(
            'memorum.reducedMotion',
            reducedMotionModes,
            defaultThemePreferences.reducedMotion,
        ),
    };
}

export function savePreferences(preferences: ThemePreferences): void {
    if (typeof localStorage.setItem !== 'function') return;
    localStorage.setItem('memorum.theme', preferences.theme);
    localStorage.setItem('memorum.density', preferences.density);
    localStorage.setItem('memorum.reducedMotion', preferences.reducedMotion);
}

export function resolveReducedMotion(setting: ReducedMotion): boolean {
    if (setting === 'on') return true;
    if (setting === 'off') return false;
    return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}
