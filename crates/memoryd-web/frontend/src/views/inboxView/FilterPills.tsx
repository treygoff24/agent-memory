import { filterForKind, inboxFilters } from './adapter';
import type { InboxFilterId, InboxViewItem } from './types';

interface FilterPillsProps {
    items: InboxViewItem[];
    active: InboxFilterId;
    onChange: (filter: InboxFilterId) => void;
}

export function FilterPills({ items, active, onChange }: FilterPillsProps) {
    const counts = items.reduce<Record<InboxFilterId, number>>(
        (acc, item) => {
            acc.all += 1;
            acc[filterForKind[item.kind]] += 1;
            return acc;
        },
        { all: 0, review: 0, conflicts: 0, recall: 0, dreams: 0, due: 0 },
    );

    return (
        <div
            className="pills"
            role="tablist"
            aria-label="Inbox filters"
        >
            {inboxFilters.map((filter) => (
                <button
                    key={filter.id}
                    className={`pill ${active === filter.id ? 'active' : ''}`}
                    onClick={() => onChange(filter.id)}
                    role="tab"
                    aria-selected={active === filter.id}
                    aria-controls="inbox-list"
                    type="button"
                >
                    <span>{filter.label}</span>
                    <span className="n">{counts[filter.id]}</span>
                    <span className="pillkey">{filter.key}</span>
                </button>
            ))}
        </div>
    );
}
