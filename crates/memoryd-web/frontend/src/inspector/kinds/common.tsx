import type { ReactNode } from 'react';

import type { InspectorItem } from '../types';

import { hashFor } from '../../router';

function isCanonicalMemoryId(id: string | undefined): id is string {
    return id?.startsWith('mem_') ?? false;
}

function auditMemoryIdForItem(item: InspectorItem): string | null {
    const candidate = item.memoryId ?? item.id;
    switch (item.kind) {
        case 'inbox-review':
        case 'inbox-recall':
        case 'inbox-conflict':
        case 'inbox-due':
        case 'inbox-dream':
        case 'recall-event':
        case 'dream-output':
        case 'governance-decision':
            return isCanonicalMemoryId(candidate) ? candidate : null;
        default:
            return null;
    }
}

/**
 * Route the inspector header id to the right hash target per item kind.
 * Only canonical memory IDs route to Trust Artifact; event IDs and synthetic
 * dream IDs stay plain text so they do not create broken audit links.
 */
function idHrefForKind(item: InspectorItem): string | null {
    const memoryId = auditMemoryIdForItem(item);
    if (memoryId && memoryId === item.id) return hashFor({ kind: 'audit', memoryId });
    switch (item.kind) {
        case 'entity-detail':
            return hashFor({ kind: 'entities', entityId: item.id });
        default:
            return null;
    }
}

export function InspectorHeader({ item, badge }: { item: InspectorItem; badge: string }) {
    const idHref = idHrefForKind(item);
    const memoryId = auditMemoryIdForItem(item);
    const memoryHref = memoryId && memoryId !== item.id ? hashFor({ kind: 'audit', memoryId }) : null;
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
            <div className="meta mono">
                id{' '}
                {idHref ? (
                    <a
                        className="memory-id-link"
                        href={idHref}
                    >
                        {item.id}
                    </a>
                ) : (
                    item.id
                )}
            </div>
            {memoryHref && memoryId ? (
                <div className="meta mono">
                    memory{' '}
                    <a
                        className="memory-id-link"
                        href={memoryHref}
                    >
                        {memoryId}
                    </a>
                </div>
            ) : null}
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
