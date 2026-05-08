import { useVirtualizer } from '@tanstack/react-virtual';
import { useRef } from 'react';

import { recallEvents } from '../data/fixtures';
export function Recall() {
    const parentRef = useRef<HTMLDivElement>(null);
    const rowVirtualizer = useVirtualizer({
        count: recallEvents.length,
        getScrollElement: () => parentRef.current,
        estimateSize: () => 34,
    });
    return (
        <>
            <div className="view-header">
                <span className="view-title">Recall</span>
                <span className="view-subtitle">· {recallEvents.length} events</span>
            </div>
            <div
                className="pane-scroll"
                ref={parentRef}
                tabIndex={0}
                aria-label="Recall event ledger"
            >
                <div style={{ height: rowVirtualizer.getTotalSize(), position: 'relative' }}>
                    {rowVirtualizer.getVirtualItems().map((virtualRow) => {
                        const event = recallEvents[virtualRow.index];
                        return (
                            <div
                                key={event.id}
                                className="recall-row"
                                style={{
                                    position: 'absolute',
                                    transform: `translateY(${virtualRow.start}px)`,
                                    width: '100%',
                                }}
                            >
                                <span className="mono">{event.time}</span>
                                <span>{event.agent}</span>
                                <span>{event.memory}</span>
                                <span className="mono">{event.score.toFixed(2)}</span>
                            </div>
                        );
                    })}
                </div>
            </div>
        </>
    );
}
