import { globalKeymap } from '../keyboard/Keymap';
export function HelpOverlay({ open, onClose }: { open: boolean; onClose(): void }) {
    if (!open) return null;
    return (
        <div
            className="modal-veil"
            onClick={onClose}
        >
            <section
                className="modal"
                onClick={(event) => event.stopPropagation()}
            >
                <h2>Keyboard help</h2>
                {globalKeymap.map((item) => (
                    <p key={item.key}>
                        <kbd>{item.key}</kbd> {item.label}
                    </p>
                ))}
            </section>
        </div>
    );
}
