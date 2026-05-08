import { FilterPills } from './FilterPills';
import type { InboxFilterId, InboxViewItem } from './types';

interface InboxHeaderProps {
    items: InboxViewItem[];
    visibleCount: number;
    activeFilter: InboxFilterId;
    onFilterChange: (filter: InboxFilterId) => void;
    label?: string;
}

export function InboxHeader({ items, visibleCount, activeFilter, onFilterChange, label }: InboxHeaderProps) {
    return (
        <div className="view-header">
            <span className="view-title">Inbox</span>
            <span className="view-subtitle">· {label ? `${label} · ` : ''}{visibleCount} items</span>
            <span className="spacer" />
            <FilterPills
                items={items}
                active={activeFilter}
                onChange={onFilterChange}
            />
        </div>
    );
}
