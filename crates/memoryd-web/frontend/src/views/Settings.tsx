import { themes, useTheme } from '../theme';
export function Settings() {
    const { preferences, setTheme, setDensity, setReducedMotion } = useTheme();
    return (
        <>
            <div className="view-header">
                <span className="view-title">Settings</span>
                <span className="view-subtitle">· appearance · keyboard · notifications</span>
            </div>
            <div className="cards-grid">
                <section className="card">
                    <div className="card-head">
                        <span>Appearance</span>
                    </div>
                    <div className="theme-grid">
                        {themes.map((theme) => (
                            <button
                                key={theme}
                                className={`theme-swatch ${preferences.theme === theme ? 'active' : ''}`}
                                onClick={() => setTheme(theme)}
                            >
                                {theme}
                            </button>
                        ))}
                    </div>
                    <div className="action-bar">
                        <button
                            className="btn"
                            onClick={() => setDensity('comfortable')}
                        >
                            comfortable
                        </button>
                        <button
                            className="btn"
                            onClick={() => setDensity('compact')}
                        >
                            compact
                        </button>
                        <button
                            className="btn"
                            onClick={() => setReducedMotion('os')}
                        >
                            motion: OS
                        </button>
                        <button
                            className="btn"
                            onClick={() => setReducedMotion('on')}
                        >
                            motion off
                        </button>
                    </div>
                </section>
                <section className="card">
                    <div className="card-head">
                        <span>Keyboard</span>
                    </div>
                    <p>
                        <kbd>:</kbd> command palette · <kbd>?</kbd> help · <kbd>g</kbd> then view key to navigate
                    </p>
                </section>
                <section className="card">
                    <div className="card-head">
                        <span>About</span>
                    </div>
                    <p>Memorum Dashboard · React/Vite build embedded into memoryd-web.</p>
                </section>
            </div>
        </>
    );
}
