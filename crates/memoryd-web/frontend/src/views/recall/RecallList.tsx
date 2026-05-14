import { useVirtualizer } from '@tanstack/react-virtual';
import { useRef, type CSSProperties, type ReactNode } from 'react';

import type { RecallLedgerEvent } from '../Recall';

interface RecallListProps {
    events: RecallLedgerEvent[];
    selectedId: string;
    onSelect: (id: string) => void;
    heavy?: boolean;
}

/** Returns "Today", "Yesterday", or "MMM D, YYYY" relative to the current date. */
function formatDayLabel(isoDate: string): string {
    const today = new Date();
    const todayStr = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`;
    const yesterday = new Date(today);
    yesterday.setDate(today.getDate() - 1);
    const yesterdayStr = `${yesterday.getFullYear()}-${String(yesterday.getMonth() + 1).padStart(2, '0')}-${String(yesterday.getDate()).padStart(2, '0')}`;

    if (isoDate === todayStr) return 'Today';
    if (isoDate === yesterdayStr) return 'Yesterday';

    const d = new Date(`${isoDate}T00:00:00Z`);
    return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric', timeZone: 'UTC' });
}

/** Extracts the UTC date portion (YYYY-MM-DD) from an ISO timestamp. */
function utcDateOf(isoTime: string): string {
    return isoTime.slice(0, 10);
}

function RecallRow({
    event,
    selected,
    onSelect,
    style,
}: {
    event: RecallLedgerEvent;
    selected: boolean;
    onSelect: (id: string) => void;
    style?: CSSProperties;
}) {
    const summaryNode = event.isEncrypted ? (
        <span style={{ color: 'var(--fg-3)' }}>{`[encrypted memory · id ${event.memory_id}]`}</span>
    ) : (
        event.memory
    );

    return (
        <button
            className={`rl-row ${selected ? 'selected' : ''}`}
            onClick={() => onSelect(event.id)}
            style={style}
            type="button"
        >
            <span className="rl-time">{event.time}</span>
            <span className="rl-seq">#{event.seq}</span>
            <span className="rl-dev">{event.device}</span>
            <span className="rl-agent">{event.agent}</span>
            <span className="rl-summary">{summaryNode}</span>
            <span className="rl-ns">{event.namespace}</span>
            <span className="rl-lat">{event.latencyMs}ms</span>
            <span className="rl-score">{event.score.toFixed(2)}</span>
        </button>
    );
}

export function RecallList({ events, selectedId, onSelect, heavy = false }: RecallListProps) {
    const parentRef = useRef<HTMLDivElement>(null);
    const rowVirtualizer = useVirtualizer({
        count: events.length,
        getScrollElement: () => parentRef.current,
        estimateSize: () => 34,
        overscan: 12,
        initialRect: { width: 900, height: 620 },
    });

    if (!heavy) {
        const rows: ReactNode[] = [];
        let lastDate = '';
        for (const event of events) {
            const dateKey = utcDateOf(event.isoTime);
            if (dateKey !== lastDate) {
                lastDate = dateKey;
                // Use rows.length in the key so the same date can appear as multiple
                // non-adjacent group headers (data is not guaranteed to be date-sorted).
                rows.push(
                    <div
                        key={`day-${dateKey}-${rows.length}`}
                        className="list-section"
                    >
                        {formatDayLabel(dateKey)}
                    </div>,
                );
            }
            rows.push(
                <RecallRow
                    key={event.id}
                    event={event}
                    selected={selectedId === event.id}
                    onSelect={onSelect}
                />,
            );
        }

        return (
            <div className="pane-scroll">
                <div
                    className="rl-list"
                    data-testid="recall-virtual-list"
                >
                    {rows}
                </div>
            </div>
        );
    }

    return (
        <div
            className="pane-scroll"
            ref={parentRef}
            tabIndex={0}
            aria-label="Recall event ledger"
        >
            <div
                data-testid="recall-virtual-list"
                style={{ height: rowVirtualizer.getTotalSize(), position: 'relative' }}
            >
                {rowVirtualizer.getVirtualItems().map((virtualRow) => {
                    const event = events[virtualRow.index];
                    if (!event) return null;
                    return (
                        <RecallRow
                            key={event.id}
                            event={event}
                            selected={selectedId === event.id}
                            onSelect={onSelect}
                            style={{
                                position: 'absolute',
                                transform: `translateY(${virtualRow.start}px)`,
                                width: '100%',
                            }}
                        />
                    );
                })}
            </div>
            <div className="rl-virt-hint">
                <span className="mono">{events.length.toLocaleString()}</span> visible · scrolling backed by
                virtualization · older buckets summarized in scrubber
            </div>
        </div>
    );
}
