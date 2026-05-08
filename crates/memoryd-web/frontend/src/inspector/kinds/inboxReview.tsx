import type { InboxReviewItem, InspectorKindProps } from '../types';

import { PolicyCard, PrivacyScanCard, ProvenanceCard } from '../cards';
import { BodySection, InspectorHeader, InspectorShell } from './common';

export function InboxReviewInspector({ item, layout, onAction }: InspectorKindProps<InboxReviewItem>) {
    return (
        <InspectorShell>
            <InspectorHeader
                item={item}
                badge="candidate"
            />
            <div className={`insp-grid ${layout === 'narrow' ? 'narrow' : ''}`}>
                <div>
                    <BodySection item={item} />
                    <div className="action-bar">
                        <button
                            className="btn primary"
                            onClick={() => onAction?.('approve', item)}
                        >
                            <span className="key">a</span>Accept
                        </button>
                        <button
                            className="btn"
                            onClick={() => onAction?.('reject', item)}
                        >
                            <span className="key">r</span>Reject
                        </button>
                        <button
                            className="btn"
                            onClick={() => onAction?.('edit', item)}
                        >
                            <span className="key">e</span>Edit
                        </button>
                        <button
                            className="btn danger"
                            onClick={() => onAction?.('forget', item)}
                        >
                            <span className="key">f</span>Forget
                        </button>
                    </div>
                </div>
                <div className="sidecar">
                    <ProvenanceCard item={item} />
                    <PolicyCard item={item} />
                    <PrivacyScanCard item={item} />
                </div>
            </div>
        </InspectorShell>
    );
}
