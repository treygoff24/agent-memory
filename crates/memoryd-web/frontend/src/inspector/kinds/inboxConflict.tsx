import type { InboxConflictItem, InspectorKindProps } from '../types';

import { DisagreementCard, ProvenanceCard } from '../cards';
import { InspectorHeader, InspectorShell } from './common';

export function InboxConflictInspector({ item, layout, onAction }: InspectorKindProps<InboxConflictItem>) {
    const local = item.diff?.local;
    const remote = item.diff?.remote;
    return (
        <InspectorShell>
            <InspectorHeader
                item={item}
                badge="merge conflict"
            />
            <div className={`insp-grid ${layout === 'narrow' ? 'narrow' : ''}`}>
                <div>
                    <div className="section-label">Sides</div>
                    <div className="diff">
                        <div>
                            <div className="side-head local">local · {local?.device ?? 'local'}</div>
                            <div className="body">{local?.body ?? item.body ?? 'Local side unavailable.'}</div>
                        </div>
                        <div>
                            <div className="side-head remote">remote · {remote?.device ?? 'remote'}</div>
                            <div className="body">{remote?.body ?? 'Remote side unavailable.'}</div>
                        </div>
                    </div>
                    <div className="action-bar">
                        <button
                            className="btn"
                            onClick={() => onAction?.('keep-local', item)}
                        >
                            <span className="key">1</span>Keep local
                        </button>
                        <button
                            className="btn"
                            onClick={() => onAction?.('keep-remote', item)}
                        >
                            <span className="key">2</span>Keep remote
                        </button>
                        <button
                            className="btn primary"
                            onClick={() => onAction?.('custom-merge', item)}
                        >
                            <span className="key">m</span>Custom merge…
                        </button>
                    </div>
                </div>
                <div className="sidecar">
                    <DisagreementCard item={item} />
                    <ProvenanceCard item={item} />
                </div>
            </div>
        </InspectorShell>
    );
}
