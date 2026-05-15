import type { AuditMemoryResponse } from '../../api';

interface Props {
    audit: AuditMemoryResponse;
}

function relativeTime(iso: string | null | undefined): string {
    if (!iso) return 'never';
    const then = new Date(iso).getTime();
    if (Number.isNaN(then)) return iso;
    const diffMs = Date.now() - then;
    const minutes = Math.round(diffMs / 60_000);
    if (minutes < 1) return 'just now';
    if (minutes < 60) return `${minutes}m ago`;
    const hours = Math.round(minutes / 60);
    if (hours < 24) return `${hours}h ago`;
    const days = Math.round(hours / 24);
    return `${days}d ago`;
}

/**
 * Trust Artifact §4 — recall stats. Daemon supplies total + 30-day counts and
 * the last-recalled timestamp; we don't have a recall histogram in
 * `AuditMemoryResponse` yet, so the "mini-timeline" is degraded to a single
 * row showing the most recent hit. When the recall-histogram surface lands on
 * the daemon side we'll grow a `<TimelineStrip>`-style chart here.
 */
export function RecallSection({ audit }: Props) {
    return (
        <section
            className="audit-section audit-recall"
            aria-labelledby="audit-recall-heading"
        >
            <h3
                id="audit-recall-heading"
                className="audit-section-heading"
            >
                Recall
            </h3>
            <dl className="audit-stat-grid">
                <div>
                    <dt>Total</dt>
                    <dd className="mono">{audit.recall_count_total}</dd>
                </div>
                <div>
                    <dt>Last 30d</dt>
                    <dd className="mono">{audit.recall_count_30d}</dd>
                </div>
                <div>
                    <dt>Last recalled</dt>
                    <dd>
                        <span className="mono">{relativeTime(audit.last_recalled)}</span>
                        {audit.last_recalled ? (
                            <span className="muted audit-recall-iso"> · {audit.last_recalled}</span>
                        ) : null}
                    </dd>
                </div>
            </dl>
        </section>
    );
}
