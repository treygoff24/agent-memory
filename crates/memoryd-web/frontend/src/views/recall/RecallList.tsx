import { useVirtualizer } from '@tanstack/react-virtual';
import { useRef, type CSSProperties } from 'react';

import type { RecallLedgerEvent } from './types';

interface RecallListProps {
    events: RecallLedgerEvent[];
    selectedId: string;
    onSelect: (id: string) => void;
    heavy?: boolean;
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
            <span className="rl-summary">{event.memory}</span>
            <span className="rl-ns">{event.namespace}</span>
            <span className="rl-lat">{event.latencyMs === null ? 'unknown' : `${event.latencyMs}ms`}</span>
            <span className="rl-score">{event.score === null ? 'unknown' : event.score.toFixed(2)}</span>
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
        return (
            <div className="pane-scroll">
                <div
                    className="rl-list"
                    data-testid="recall-virtual-list"
                >
                    {events.map((event) => (
                        <RecallRow
                            key={event.id}
                            event={event}
                            selected={selectedId === event.id}
                            onSelect={onSelect}
                        />
                    ))}
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
