import { useState } from 'react';

const editableTokens = [
    { id: 'accent', label: 'Accent', defaultValue: '#d8a44f' },
    { id: 'surface', label: 'Surface', defaultValue: '#2f2b25' },
    { id: 'danger', label: 'Danger', defaultValue: '#d76b4c' },
] as const;

type EditableTokenId = (typeof editableTokens)[number]['id'];

const defaultTokenValues: Record<EditableTokenId, string> = {
    accent: '#d8a44f',
    surface: '#2f2b25',
    danger: '#d76b4c',
};

export function ThemeEditorTab() {
    const [tokens, setTokens] = useState<Record<EditableTokenId, string>>(() => defaultTokenValues);
    const [saved, setSaved] = useState(false);

    return (
        <section
            className="card settings-card"
            aria-labelledby="theme-editor-heading"
        >
            <div className="card-head">
                <span id="theme-editor-heading">Theme editor</span>
            </div>
            <p className="muted">Tune a custom preview before promoting values into token CSS.</p>
            <div className="settings-form-grid">
                {editableTokens.map((token) => (
                    <label
                        key={token.id}
                        className="settings-field"
                    >
                        <span>{token.label}</span>
                        <input
                            aria-label={`${token.label} token`}
                            type="color"
                            value={tokens[token.id]}
                            onChange={(event) => {
                                setTokens((current) => ({ ...current, [token.id]: event.target.value }));
                                setSaved(false);
                            }}
                        />
                    </label>
                ))}
            </div>
            <div
                className="theme-editor-preview"
                style={{
                    borderColor: tokens.accent,
                    background: `linear-gradient(135deg, ${tokens.surface}, ${tokens.accent}22)`,
                }}
            >
                <span>Custom theme preview</span>
                <span style={{ color: tokens.danger }}>conflict badge</span>
            </div>
            <div className="action-bar">
                <button
                    type="button"
                    className="btn primary"
                    onClick={() => setSaved(true)}
                >
                    Save as custom theme draft
                </button>
                {saved && <span className="muted">Draft saved locally for this session.</span>}
            </div>
        </section>
    );
}
