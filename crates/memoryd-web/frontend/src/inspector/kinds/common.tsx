import type { ReactNode } from 'react';

import type { InspectorItem } from '../types';

export function InspectorHeader({ item, badge }: { item: InspectorItem; badge: string }) {
    return (
        <>
            <div className="insp-head">
                <span className="insp-title">{item.title}</span>
                <span className="insp-scope">{item.namespace}</span>
                <span className="insp-badges">
                    <span className="badge">{badge}</span>
                    {item.encrypted ? <span className="badge encrypted">encrypted at rest</span> : null}
                </span>
            </div>
            <div className="meta mono">id {item.id}</div>
        </>
    );
}

export function BodySection({ item, label = 'Body' }: { item: InspectorItem; label?: string }) {
    return (
        <>
            <div className="section-label">{label}</div>
            <p className="body-text">{item.body ?? item.summary ?? 'No body available for this item.'}</p>
        </>
    );
}

export function InspectorShell({ children }: { children: ReactNode }) {
    return (
        <div
            className="inspector"
            role="region"
            aria-label="Inspector"
        >
            {children}
        </div>
    );
}
