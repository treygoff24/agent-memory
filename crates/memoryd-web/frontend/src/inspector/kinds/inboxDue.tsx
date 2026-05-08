import { PolicyCard, RecentMemoriesCard } from '../cards';
import type { InboxDueItem, InspectorKindProps } from '../types';
import { BodySection, InspectorHeader, InspectorShell } from './common';

export function InboxDueInspector({ item, layout, onAction }: InspectorKindProps<InboxDueItem>) {
    return (
        <InspectorShell>
            <InspectorHeader
                item={item}
                badge="due for verify"
            />
            <div className={`insp-grid ${layout === 'narrow' ? 'narrow' : ''}`}>
                <div>
                    <BodySection
                        item={item}
                        label="Reality-check score"
                    />
                    <div className="action-bar">
                        <button
                            className="btn primary"
                            onClick={() => onAction?.('verify-now', item)}
                        >
                            <span className="key">v</span>Verify now
                        </button>
                        <button
                            className="btn"
                            onClick={() => onAction?.('skip', item)}
                        >
                            <span className="key">s</span>Skip 30d
                        </button>
                    </div>
                </div>
                <div className="sidecar">
                    <RecentMemoriesCard item={item} />
                    <PolicyCard item={item} />
                </div>
            </div>
        </InspectorShell>
    );
}
