import { useAuditQuery } from '../api';
import { useRoute } from '../router';
import { EmptyState } from '../ui';
import {
    BodySection,
    ConfidenceSection,
    HeaderSection,
    PolicyDecisions,
    PrivacyScanSection,
    ProvenanceChain,
    RecallSection,
    SupersessionHistory,
    SyncState,
} from './auditSections';
import { QueryErrorBanner, QueryLoadingBanner } from './QueryFeedback';

/**
 * Trust Artifact (Brief §View 7). Single-column scroll layout with nine
 * sections per the brief, plus a top-right "walk provenance graph" affordance
 * that defers to v1.1 (renders a placeholder explaining the future sub-route).
 * Reached via `#/audit/:memory_id`.
 */
export function Audit() {
    const { route } = useRoute();
    const memoryId = route.kind === 'audit' ? route.memoryId : '';
    const query = useAuditQuery(memoryId);

    if (!memoryId) {
        return (
            <div
                className="view"
                data-testid="audit-view"
            >
                <EmptyState
                    title="Trust Artifact requires a memory id."
                    body="Open this view from a memory-id link or via #/audit/:memory_id directly."
                />
            </div>
        );
    }

    return (
        <div
            className="view audit-view"
            data-testid="audit-view"
        >
            <div className="view-header audit-toolbar">
                <span className="view-title">Trust Artifact</span>
                <span className="view-subtitle mono">· {memoryId}</span>
                <span className="spacer" />
                <button
                    type="button"
                    className="btn"
                    title="Walk the provenance graph (planned for v1.1)"
                    aria-label="Walk provenance graph (planned for v1.1)"
                    disabled
                >
                    walk provenance graph
                </button>
            </div>

            {!query.data && query.isLoading ? <QueryLoadingBanner label="Trust artifact" /> : null}
            <QueryErrorBanner
                error={query.error}
                label="Trust artifact"
            />

            {query.data ? (
                <div
                    className="pane-scroll audit-scroll"
                    tabIndex={0}
                >
                    <HeaderSection audit={query.data} />
                    <BodySection audit={query.data} />
                    <ConfidenceSection audit={query.data} />
                    <RecallSection audit={query.data} />
                    <ProvenanceChain events={query.data.provenance_chain} />
                    <PolicyDecisions decisions={query.data.policy_decisions} />
                    <PrivacyScanSection scan={query.data.privacy_scan} />
                    <SupersessionHistory history={query.data.supersession_history} />
                    <SyncState state={query.data.sync_state} />
                </div>
            ) : null}
        </div>
    );
}
