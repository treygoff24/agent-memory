export const themes = ['warm-dark', 'warm-light', 'cool-dark', 'cool-light', 'monochrome', 'high-contrast'] as const;
export type Theme = (typeof themes)[number];
export const densities = ['comfortable', 'compact'] as const;
export type Density = (typeof densities)[number];
export const reducedMotionModes = ['os', 'on', 'off'] as const;
export type ReducedMotion = (typeof reducedMotionModes)[number];
export interface ThemePreferences {
    theme: Theme;
    density: Density;
    reducedMotion: ReducedMotion;
    fontSize: number;
}
export const defaultThemePreferences: ThemePreferences = {
    theme: 'warm-dark',
    density: 'comfortable',
    reducedMotion: 'os',
    fontSize: 14,
};
