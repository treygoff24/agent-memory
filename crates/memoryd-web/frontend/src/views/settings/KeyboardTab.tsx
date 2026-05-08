import { globalKeymap } from '../../keyboard/Keymap';

export function KeyboardTab() {
    return (
        <section
            className="card settings-card"
            aria-labelledby="keyboard-heading"
        >
            <div className="card-head">
                <span id="keyboard-heading">Keyboard</span>
            </div>
            <p className="muted">Global shortcuts work outside text inputs and editable regions.</p>
            <div
                className="settings-table"
                role="table"
                aria-label="Keyboard shortcuts"
            >
                {globalKeymap.map((command) => (
                    <div
                        className="settings-table-row"
                        role="row"
                        key={`${command.key}-${command.label}`}
                    >
                        <span role="cell">
                            <kbd>{command.key}</kbd>
                        </span>
                        <span role="cell">{command.label}</span>
                    </div>
                ))}
            </div>
        </section>
    );
}
