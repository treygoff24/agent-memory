import type { InboxViewItem } from './types';

import { EmptyState } from '../../ui';
import { glyphIcons } from '../../ui/icons';

// `InboxViewItem.glyphKind` is a structural subtype of `GlyphKind`, so it
// indexes `glyphIcons` directly — no identity-map indirection.

interface InboxListProps {
    items: InboxViewItem[];
    selectedId: string;
    focusedId: string;
    onFocus: (id: string) => void;
    onSelect: (id: string) => void;
    onRunAnyway?: (() => void) | undefined;
}

export function InboxList({ items, selectedId, focusedId, onFocus, onSelect, onRunAnyway }: InboxListProps) {
    if (items.length === 0) {
        // Brief §View 1 verbatim title + body. `meta` surfaces "Next due" — daemon
        // doesn't expose `next_due_at` yet (TODO(stream-g): wire when available),
        // so a literal em-dash placeholder reads honestly. `actions` exposes
        // "Run anyway" so a user can force a refetch even when the queue is
        // nominally clear; this matches the RC reviewer's empty-state guidance.
        return (
            <EmptyState
                title="Inbox is clear."
                body="All review items processed."
                meta={
                    <>
                        Next due <span className="mono">—</span>
                    </>
                }
                actions={
                    onRunAnyway ? (
                        <button
                            type="button"
                            className="btn empty-action"
                            onClick={onRunAnyway}
                        >
                            Run anyway
                        </button>
                    ) : null
                }
            />
        );
    }

    return (
        <div
            id="inbox-list"
            className="list"
            role="listbox"
            aria-label="Inbox items"
            aria-activedescendant={focusedId ? `inbox-row-${focusedId}` : undefined}
        >
            {items.map((item) => {
                const selected = selectedId === item.id;
                const focused = focusedId === item.id;
                const glyphEntry = glyphIcons[item.glyphKind];
                const GlyphIcon = glyphEntry.component;
                return (
                    <button
                        key={item.id}
                        id={`inbox-row-${item.id}`}
                        className={`list-item ${selected ? 'selected' : ''} ${focused && !selected ? 'focused' : ''}`}
                        onFocus={() => onFocus(item.id)}
                        onMouseEnter={() => onFocus(item.id)}
                        onClick={() => onSelect(item.id)}
                        role="option"
                        aria-selected={selected}
                        aria-current={focused ? 'true' : undefined}
                        type="button"
                    >
                        <span
                            className={`glyph ${item.kind}`}
                            aria-hidden="true"
                        >
                            <GlyphIcon
                                size={14}
                                weight={glyphEntry.weight}
                                color={glyphEntry.defaultColor}
                            />
                        </span>
                        <span className="body">
                            <span className="title-line">{item.title}</span>
                            <span className="sub">
                                <span className="scope">{item.namespace}</span>
                                {item.sub.map((part) => (
                                    <span key={part}>
                                        <span className="sep">·</span> {part}
                                    </span>
                                ))}
                            </span>
                        </span>
                        <span className="meta">{item.kind}</span>
                    </button>
                );
            })}
        </div>
    );
}
