import { useEffect, useRef, useState } from 'react';

/**
 * All 15 OKLCH colorable tokens from tokens.css.
 * --shadow-modal is a composite box-shadow, not a single OKLCH color, so it is excluded.
 */
const COLOR_TOKENS = [
    // Surfaces
    { id: '--bg', label: 'Background' },
    { id: '--surface', label: 'Surface' },
    { id: '--surface-2', label: 'Surface 2' },
    { id: '--border', label: 'Border' },
    { id: '--border-soft', label: 'Border soft' },
    // Ink
    { id: '--fg', label: 'Foreground' },
    { id: '--fg-2', label: 'Foreground 2' },
    { id: '--fg-3', label: 'Foreground 3' },
    { id: '--fg-4', label: 'Foreground 4' },
    // Accent
    { id: '--accent', label: 'Accent' },
    { id: '--accent-soft', label: 'Accent soft' },
    // Semantic
    { id: '--ok', label: 'OK' },
    { id: '--warn', label: 'Warn' },
    { id: '--bad', label: 'Bad' },
    { id: '--info', label: 'Info' },
] as const;

type TokenId = (typeof COLOR_TOKENS)[number]['id'];

/** Parse an oklch(...) string into [L, C, H] components, clamped to valid ranges. */
function parseOklch(value: string): [number, number, number] {
    const match = value.trim().match(/oklch\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)/);
    if (!match) return [0.5, 0.05, 60];
    return [
        Math.max(0, Math.min(1, parseFloat(match[1]))),
        Math.max(0, Math.min(0.4, parseFloat(match[2]))),
        Math.max(0, Math.min(360, parseFloat(match[3]))),
    ];
}

/** Read the current computed value of a CSS custom property from <html>. */
function readComputedToken(id: TokenId): string {
    if (typeof getComputedStyle === 'undefined') return 'oklch(0.5 0.05 60)';
    return getComputedStyle(document.documentElement).getPropertyValue(id).trim();
}

function buildInitialLch(): Record<TokenId, [number, number, number]> {
    const result = {} as Record<TokenId, [number, number, number]>;
    for (const token of COLOR_TOKENS) {
        result[token.id] = parseOklch(readComputedToken(token.id));
    }
    return result;
}

const STORAGE_KEY_PREFIX = 'memorum.custom-theme.';

function saveCustomTheme(name: string, lch: Record<TokenId, [number, number, number]>): void {
    const payload: Record<string, [number, number, number]> = {};
    for (const token of COLOR_TOKENS) {
        payload[token.id] = lch[token.id];
    }
    localStorage.setItem(STORAGE_KEY_PREFIX + name, JSON.stringify(payload));
}

export function ThemeEditorTab() {
    const [lch, setLch] = useState<Record<TokenId, [number, number, number]>>(() => buildInitialLch());
    const [saveStatus, setSaveStatus] = useState<'idle' | 'saved'>('idle');
    const nameRef = useRef<HTMLInputElement>(null);

    /** Apply a single token override to documentElement immediately (live preview). */
    function applyToken(id: TokenId, next: [number, number, number]): void {
        const [l, c, h] = next;
        document.documentElement.style.setProperty(id, `oklch(${l.toFixed(3)} ${c.toFixed(3)} ${h.toFixed(1)})`);
    }

    function updateChannel(id: TokenId, channel: 0 | 1 | 2, rawValue: number): void {
        setLch((current) => {
            const prev = current[id];
            const next: [number, number, number] = [prev[0], prev[1], prev[2]];
            next[channel] = rawValue;
            applyToken(id, next);
            return { ...current, [id]: next };
        });
        setSaveStatus('idle');
    }

    function handleReset(): void {
        for (const token of COLOR_TOKENS) {
            document.documentElement.style.removeProperty(token.id);
        }
        setLch(buildInitialLch());
        setSaveStatus('idle');
    }

    function handleSave(): void {
        const name = (nameRef.current?.value.trim() ?? '').replace(/[^a-z0-9-_]/gi, '-') || 'custom';
        saveCustomTheme(name, lch);
        setSaveStatus('saved');
    }

    /** Re-read computed values when the active theme changes externally (e.g. appearance tab). */
    useEffect(() => {
        return () => {
            // On unmount, remove any inline overrides so theme selector works cleanly.
            for (const token of COLOR_TOKENS) {
                document.documentElement.style.removeProperty(token.id);
            }
        };
    }, []);

    return (
        <section
            className="card settings-card"
            aria-labelledby="theme-editor-heading"
        >
            <div className="card-head">
                <span id="theme-editor-heading">Theme editor</span>
            </div>
            <p className="muted">
                Tune individual OKLCH color tokens. Changes preview live — the active theme preset is unchanged until
                you &quot;Save as custom theme.&quot;
            </p>

            <div className="theme-editor-token-list">
                {COLOR_TOKENS.map((token) => {
                    const [l, c, h] = lch[token.id];
                    const previewColor = `oklch(${l.toFixed(3)} ${c.toFixed(3)} ${h.toFixed(1)})`;
                    return (
                        <div
                            key={token.id}
                            className="theme-editor-token-row"
                        >
                            <div className="theme-editor-token-header">
                                <span
                                    className="theme-editor-swatch"
                                    style={{ background: previewColor }}
                                    aria-hidden="true"
                                />
                                <span className="theme-editor-token-label">{token.label}</span>
                                <span className="theme-editor-token-id mono">{token.id}</span>
                            </div>
                            <div className="theme-editor-sliders">
                                <label className="theme-editor-slider-row">
                                    <span className="theme-editor-channel-label">L</span>
                                    <input
                                        type="range"
                                        aria-label={`${token.label} lightness`}
                                        min="0"
                                        max="1"
                                        step="0.005"
                                        value={l}
                                        onChange={(e) => updateChannel(token.id, 0, parseFloat(e.target.value))}
                                    />
                                    <span className="theme-editor-channel-value mono">{l.toFixed(2)}</span>
                                </label>
                                <label className="theme-editor-slider-row">
                                    <span className="theme-editor-channel-label">C</span>
                                    <input
                                        type="range"
                                        aria-label={`${token.label} chroma`}
                                        min="0"
                                        max="0.4"
                                        step="0.002"
                                        value={c}
                                        onChange={(e) => updateChannel(token.id, 1, parseFloat(e.target.value))}
                                    />
                                    <span className="theme-editor-channel-value mono">{c.toFixed(3)}</span>
                                </label>
                                <label className="theme-editor-slider-row">
                                    <span className="theme-editor-channel-label">H</span>
                                    <input
                                        type="range"
                                        aria-label={`${token.label} hue`}
                                        min="0"
                                        max="360"
                                        step="1"
                                        value={h}
                                        onChange={(e) => updateChannel(token.id, 2, parseFloat(e.target.value))}
                                    />
                                    <span className="theme-editor-channel-value mono">{Math.round(h)}°</span>
                                </label>
                            </div>
                        </div>
                    );
                })}
            </div>

            <div className="theme-editor-preview">
                <div
                    className="theme-editor-preview-label muted"
                    id="theme-editor-preview-label"
                >
                    Custom theme preview
                </div>
                <div
                    className="theme-editor-preview-row"
                    aria-labelledby="theme-editor-preview-label"
                    style={{
                        background: `oklch(${lch['--surface'][0].toFixed(3)} ${lch['--surface'][1].toFixed(3)} ${lch['--surface'][2].toFixed(1)})`,
                        border: `1px solid oklch(${lch['--border'][0].toFixed(3)} ${lch['--border'][1].toFixed(3)} ${lch['--border'][2].toFixed(1)})`,
                        color: `oklch(${lch['--fg'][0].toFixed(3)} ${lch['--fg'][1].toFixed(3)} ${lch['--fg'][2].toFixed(1)})`,
                    }}
                >
                    <span
                        style={{
                            display: 'inline-block',
                            width: '8px',
                            height: '8px',
                            borderRadius: '50%',
                            background: `oklch(${lch['--accent'][0].toFixed(3)} ${lch['--accent'][1].toFixed(3)} ${lch['--accent'][2].toFixed(1)})`,
                            marginRight: '8px',
                            flexShrink: 0,
                        }}
                        aria-hidden="true"
                    />
                    <span>Sample memory row</span>
                    <span
                        style={{
                            marginLeft: 'auto',
                            color: `oklch(${lch['--bad'][0].toFixed(3)} ${lch['--bad'][1].toFixed(3)} ${lch['--bad'][2].toFixed(1)})`,
                            fontSize: 'var(--text-xs)',
                        }}
                    >
                        conflict
                    </span>
                </div>
            </div>

            <div className="action-bar">
                <label className="settings-field" style={{ flex: 1 }}>
                    <span className="sr-only">Custom theme name</span>
                    <input
                        ref={nameRef}
                        type="text"
                        placeholder="Theme name (e.g. my-dark)"
                        defaultValue="custom"
                        aria-label="Custom theme name"
                        maxLength={40}
                    />
                </label>
                <button
                    type="button"
                    className="btn primary"
                    onClick={handleSave}
                >
                    Save as custom theme
                </button>
                <button
                    type="button"
                    className="btn"
                    onClick={handleReset}
                >
                    Reset
                </button>
                {saveStatus === 'saved' && (
                    <span
                        className="muted"
                        role="status"
                        aria-live="polite"
                    >
                        Saved to localStorage.
                    </span>
                )}
            </div>
        </section>
    );
}
