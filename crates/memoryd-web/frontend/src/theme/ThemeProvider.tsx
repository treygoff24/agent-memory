import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';

import { loadPreferences, resolveReducedMotion, savePreferences } from './storage';
import { defaultThemePreferences, type Density, type ReducedMotion, type Theme, type ThemePreferences } from './types';

interface ThemeContextValue {
    preferences: ThemePreferences;
    setTheme(theme: Theme): void;
    setDensity(density: Density): void;
    setReducedMotion(reducedMotion: ReducedMotion): void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

function queryPreference<K extends keyof ThemePreferences>(
    key: K,
    allowed: readonly ThemePreferences[K][],
): ThemePreferences[K] | null {
    const params = new URLSearchParams(window.location.search);
    const value = params.get(key);
    return allowed.includes(value as ThemePreferences[K]) ? (value as ThemePreferences[K]) : null;
}

function initialPreferences(): ThemePreferences {
    const stored = loadPreferences();
    return {
        theme:
            queryPreference('theme', [
                'warm-dark',
                'warm-light',
                'cool-dark',
                'cool-light',
                'monochrome',
                'high-contrast',
            ]) ?? stored.theme,
        density: queryPreference('density', ['comfortable', 'compact']) ?? stored.density,
        reducedMotion: queryPreference('reducedMotion', ['os', 'on', 'off']) ?? stored.reducedMotion,
    };
}

export function ThemeProvider({ children }: { children: ReactNode }) {
    const [preferences, setPreferences] = useState<ThemePreferences>(() => {
        if (typeof window === 'undefined') return defaultThemePreferences;
        return initialPreferences();
    });

    useEffect(() => {
        const root = document.documentElement;
        root.dataset.theme = preferences.theme;
        root.dataset.density = preferences.density;
        root.dataset.reducedMotion = resolveReducedMotion(preferences.reducedMotion) ? 'on' : 'off';
        savePreferences(preferences);
    }, [preferences]);

    useEffect(() => {
        if (preferences.reducedMotion !== 'os') return undefined;
        const media = window.matchMedia('(prefers-reduced-motion: reduce)');
        const update = () => {
            document.documentElement.dataset.reducedMotion = media.matches ? 'on' : 'off';
        };
        media.addEventListener('change', update);
        update();
        return () => media.removeEventListener('change', update);
    }, [preferences.reducedMotion]);

    const value = useMemo<ThemeContextValue>(
        () => ({
            preferences,
            setTheme: (theme) => setPreferences((current) => ({ ...current, theme })),
            setDensity: (density) => setPreferences((current) => ({ ...current, density })),
            setReducedMotion: (reducedMotion) => setPreferences((current) => ({ ...current, reducedMotion })),
        }),
        [preferences],
    );

    return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
    const value = useContext(ThemeContext);
    if (!value) throw new Error('useTheme must be used inside ThemeProvider');
    return value;
}
