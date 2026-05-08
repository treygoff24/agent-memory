import type { InboxViewItem } from './types';

import { EmptyState } from '../../ui';

interface InboxListProps {
    items: InboxViewItem[];
    selectedId: string;
    focusedId: string;
    onFocus: (id: string) => void;
    onSelect: (id: string) => void;
}

export function InboxList({ items, selectedId, focusedId, onFocus, onSelect }: InboxListProps) {
    if (items.length === 0) {
        return (
            <EmptyState
                title="Inbox is clear"
                body="No matching review items."
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
                            {item.glyph}
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
