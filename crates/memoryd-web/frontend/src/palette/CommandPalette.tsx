import Fuse from 'fuse.js';
import { useEffect, useMemo, useState } from 'react';

import type { ViewId } from '../views';

import { commands, type Command } from './commands';
export function CommandPalette({
    open,
    onClose,
    onRun,
    activeView,
}: {
    open: boolean;
    onClose(): void;
    onRun(command: Command): void;
    activeView?: ViewId;
}) {
    const [query, setQuery] = useState('');
    const [selectedIndex, setSelectedIndex] = useState(0);
    const availableCommands = useMemo(
        () => commands.filter((command) => !command.scope || command.scope === activeView),
        [activeView],
    );
    const fuse = useMemo(
        () =>
            new Fuse(availableCommands, {
                keys: ['label', 'category', 'shortcut'],
                threshold: 0.35,
            }),
        [availableCommands],
    );
    const visible = query ? fuse.search(query).map((result) => result.item) : availableCommands;

    useEffect(() => setSelectedIndex(0), [query, open]);

    function run(command: Command | undefined) {
        if (!command) return;
        onRun(command);
    }

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
                        onKeyDown={(event) => {
                            if (event.key === 'Escape') {
                                event.preventDefault();
                                onClose();
                            }
                            if (event.key === 'Enter') {
                                event.preventDefault();
                                run(visible[selectedIndex] ?? visible[0]);
                            }
                            if (event.key === 'ArrowDown') {
                                event.preventDefault();
                                setSelectedIndex((index) => Math.min(index + 1, visible.length - 1));
                            }
                            if (event.key === 'ArrowUp') {
                                event.preventDefault();
                                setSelectedIndex((index) => Math.max(index - 1, 0));
                            }
                        }}
                        placeholder="Type a command…"
                    />
                    <kbd>esc</kbd>
                </div>
                <div className="palette-list">
                    {visible.map((command, index) => (
                        <button
                            key={command.id}
                            type="button"
                            className={`palette-row ${index === selectedIndex ? 'selected' : ''}`}
                            onMouseEnter={() => setSelectedIndex(index)}
                            onClick={() => run(command)}
                        >
                            <span className="cat">{command.category[0]}</span>
                            <span className="cmd-name">
                                {command.label}
                                <span className="scope">{command.category}</span>
                            </span>
                            {command.shortcut && <kbd>{command.shortcut}</kbd>}
                        </button>
                    ))}
                    {visible.length === 0 && <div className="palette-empty">No commands match.</div>}
                </div>
            </section>
        </div>
    );
}
