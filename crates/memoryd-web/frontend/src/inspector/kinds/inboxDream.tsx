import { DreamPassCard, EvidenceCard } from '../cards';
import type { InboxDreamItem, InspectorKindProps } from '../types';
import { BodySection, InspectorHeader, InspectorShell } from './common';

export function InboxDreamInspector({ item, layout, onAction }: InspectorKindProps<InboxDreamItem>) {
    return (
        <InspectorShell>
            <InspectorHeader
                item={item}
                badge="dream · low confidence"
            />
            <div className={`insp-grid ${layout === 'narrow' ? 'narrow' : ''}`}>
                <div>
                    <BodySection
                        item={item}
                        label="Pattern"
                    />
                    <EvidenceCard item={item} />
                    <div className="action-bar">
                        <button
                            className="btn primary"
                            onClick={() => onAction?.('promote', item)}
                        >
                            <span className="key">p</span>Promote
                        </button>
                        <button
                            className="btn"
                            onClick={() => onAction?.('queue-question', item)}
                        >
                            <span className="key">q</span>Queue question
                        </button>
                        <button
                            className="btn danger"
                            onClick={() => onAction?.('dismiss', item)}
                        >
                            <span className="key">x</span>Dismiss
                        </button>
                    </div>
                </div>
                <div className="sidecar">
                    <DreamPassCard item={item} />
                </div>
            </div>
        </InspectorShell>
    );
}
