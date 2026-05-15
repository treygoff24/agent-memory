import type { AuditMemoryResponse } from '../../api';

import { hashFor } from '../../router';

interface Props {
    audit: AuditMemoryResponse;
}

/**
 * Trust Artifact §1 — title, namespace, status badge, encryption badge,
 * sensitivity badge, and the three primary actions (open in editor, supersede,
 * forget). The walk-graph button is rendered top-right of the entire view by
 * `Audit.tsx`, not here.
 */
export function HeaderSection({ audit }: Props) {
    const encrypted = audit.privacy_scan.storage_action === 'encrypt_at_rest';
    const sensitivity = (audit.privacy_scan.labels_detected[0] ?? 'public').toLowerCase();

    return (
        <header className="audit-section audit-header">
            <div className="audit-header-titlebar">
                <h2 className="audit-title">{audit.title || '(untitled memory)'}</h2>
                <div
                    className="audit-actions"
                    role="group"
                    aria-label="Memory actions"
                >
                    <button
                        type="button"
                        className="btn"
                        aria-label="Open canonical file in external editor"
                    >
                        open in editor
                    </button>
                    <button
                        type="button"
                        className="btn"
                        aria-label="Supersede this memory"
                    >
                        supersede
                    </button>
                    <button
                        type="button"
                        className="btn danger"
                        aria-label="Forget this memory"
                    >
                        forget
                    </button>
                </div>
            </div>
            <div className="audit-meta-row">
                <a
                    className="audit-id mono"
                    href={hashFor({ kind: 'audit', memoryId: audit.memory_id })}
                    aria-label="Canonical memory id"
                >
                    {audit.memory_id}
                </a>
                <span className="audit-namespace muted">{audit.namespace}</span>
                <span className={`badge audit-status badge-${audit.status.toLowerCase()}`}>{audit.status}</span>
                {encrypted ? <span className="badge badge-encrypted">encrypted</span> : null}
                <span className={`badge audit-sensitivity badge-sensitivity-${sensitivity}`}>{sensitivity}</span>
            </div>
        </header>
    );
}
