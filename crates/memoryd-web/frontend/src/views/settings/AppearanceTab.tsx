import { themes, type Density, type ReducedMotion, type Theme, type ThemePreferences } from '../../theme';

interface AppearanceTabProps {
  preferences: ThemePreferences;
  setTheme(theme: Theme): void;
  setDensity(density: Density): void;
  setReducedMotion(reducedMotion: ReducedMotion): void;
  setFontSize(fontSize: number): void;
}

export function AppearanceTab({
  preferences,
  setTheme,
  setDensity,
  setReducedMotion,
  setFontSize,
}: AppearanceTabProps) {
  return (
    <section className="card settings-card" aria-labelledby="appearance-heading">
      <div className="card-head">
        <span id="appearance-heading">Appearance</span>
      </div>
      <p className="muted">Choose a dashboard theme, density, and motion policy.</p>
      <div className="theme-grid" aria-label="Theme presets">
        {themes.map((theme) => (
          <button
            key={theme}
            type="button"
            className={`theme-swatch ${preferences.theme === theme ? 'active' : ''}`}
            aria-pressed={preferences.theme === theme}
            onClick={() => setTheme(theme)}
          >
            {theme}
          </button>
        ))}
      </div>
      <div className="action-bar" aria-label="Display density">
        <button
          type="button"
          className={`btn ${preferences.density === 'comfortable' ? 'primary' : ''}`}
          aria-pressed={preferences.density === 'comfortable'}
          onClick={() => setDensity('comfortable')}
        >
          comfortable
        </button>
        <button
          type="button"
          className={`btn ${preferences.density === 'compact' ? 'primary' : ''}`}
          aria-pressed={preferences.density === 'compact'}
          onClick={() => setDensity('compact')}
        >
          compact
        </button>
      </div>
      <div className="action-bar" aria-label="Reduced motion">
        <button
          type="button"
          className={`btn ${preferences.reducedMotion === 'os' ? 'primary' : ''}`}
          aria-pressed={preferences.reducedMotion === 'os'}
          onClick={() => setReducedMotion('os')}
        >
          motion: OS
        </button>
        <button
          type="button"
          className={`btn ${preferences.reducedMotion === 'on' ? 'primary' : ''}`}
          aria-pressed={preferences.reducedMotion === 'on'}
          onClick={() => setReducedMotion('on')}
        >
          motion off
        </button>
        <button
          type="button"
          className={`btn ${preferences.reducedMotion === 'off' ? 'primary' : ''}`}
          aria-pressed={preferences.reducedMotion === 'off'}
          onClick={() => setReducedMotion('off')}
        >
          motion on
        </button>
      </div>
      <label className="settings-field settings-range">
        <span>Base font size</span>
        <input
          aria-label="Base font size"
          type="range"
          min="12"
          max="18"
          step="1"
          value={preferences.fontSize}
          onChange={(event) => setFontSize(Number(event.target.value))}
        />
        <span className="mono">{preferences.fontSize}px</span>
      </label>
    </section>
  );
}
