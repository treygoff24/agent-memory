import { useMemo } from 'react';

import { useRealityCheckQuery, useStatusQuery } from '../api';
import { hashFor } from '../router';
import type { ViewId } from '../views';

type NavViewId = Exclude<ViewId, 'audit'>;

export const navItems: Array<{ id: NavViewId; label: string; key: string }> = [
    { id: 'inbox', label: 'Inbox', key: 'i' },
    { id: 'reality', label: 'Reality Check', key: 'r' },
    { id: 'recall', label: 'Recall', key: 'l' },
    { id: 'dreams', label: 'Dreams', key: 'd' },
    { id: 'peers', label: 'Peers', key: 'p' },
    { id: 'governance', label: 'Governance', key: 'g' },
    { id: 'entities', label: 'Entities', key: 'e' },
    { id: 'settings', label: 'Settings', key: 's' },
];

// Counts next to nav items are *attention* signals, not totals. Recall has no
// natural attention signal at sidebar-glance scale — a "last 24h hits" count
// requires daemon-side bucketing we don't expose yet.
function useSidebarCounts(): Partial<Record<ViewId, number>> {
    const status = useStatusQuery();
    const reality = useRealityCheckQuery();
    return useMemo(() => {
        const out: Partial<Record<ViewId, number>> = {};
        if (typeof status.data?.review.candidate === 'number') out.inbox = status.data.review.candidate;
        if (reality.data) out.reality = reality.data.items.length;
        return out;
    }, [status.data, reality.data]);
}

export function Sidebar({ active, onNav }: { active: ViewId; onNav(id: ViewId): void }) {
    const counts = useSidebarCounts();
    return (
        <nav
            className="sidebar"
            aria-label="Primary"
        >
            <div className="sidebar-section">Workspace</div>
            {navItems.map((item) => {
                const count = counts[item.id];
                const href = hashFor({ kind: item.id } as { kind: NavViewId });
                return (
                    <a
                        key={item.id}
                        href={href}
                        className={`nav-item ${active === item.id ? 'active' : ''}`}
                        // Plain link semantics — cmd-click opens a new tab and
                        // hashchange handles same-tab navigation. The onClick
                        // is a redundant hook for callers that want to react
                        // to nav (e.g., closing a modal).
                        onClick={(event) => {
                            if (event.metaKey || event.ctrlKey || event.shiftKey) return;
                            onNav(item.id);
                        }}
                    >
                        <span className="ico">◆</span>
                        <span className="label">{item.label}</span>
                        {typeof count === 'number' && count > 0 ? <span className="count">{count}</span> : null}
                        <span
                            className="kbd-hint"
                            aria-hidden="true"
                        >
                            {item.key}
                        </span>
                    </a>
                );
            })}
        </nav>
    );
}
