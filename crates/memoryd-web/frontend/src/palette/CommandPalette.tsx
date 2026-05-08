import Fuse from 'fuse.js';
import { useMemo, useState } from 'react';

import { commands, type Command } from './commands';
export function CommandPalette({
    open,
    onClose,
    onRun,
}: {
    open: boolean;
    onClose(): void;
    onRun(command: Command): void;
}) {
    const [query, setQuery] = useState('');
    const fuse = useMemo(() => new Fuse(commands, { keys: ['label', 'category'], threshold: 0.35 }), []);
    const visible = query ? fuse.search(query).map((result) => result.item) : commands;
    if (!open) return null;
    return (
        <div
            className="modal-veil"
            onClick={onClose}
        >
            <section
                className="modal palette"
                onClick={(event) => event.stopPropagation()}
            >
                <div className="palette-input">
                    <span className="prompt">:</span>
                    <input
                        autoFocus
                        value={query}
                        onChange={(event) => setQuery(event.target.value)}
                        placeholder="Type a command…"
                    />
                    <kbd>esc</kbd>
                </div>
                <div className="palette-list">
                    {visible.map((command) => (
                        <button
                            key={command.id}
                            className="palette-row"
                            onClick={() => onRun(command)}
                        >
                            <span className="cat">{command.category[0]}</span>
                            <span className="cmd-name">
                                {command.label}
                                <span className="scope">{command.category}</span>
                            </span>
                        </button>
                    ))}
                </div>
            </section>
        </div>
    );
}
