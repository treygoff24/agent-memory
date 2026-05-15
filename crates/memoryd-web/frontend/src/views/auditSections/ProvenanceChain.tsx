import type { ProvenanceEvent } from '../../api';

interface Props {
    events: ProvenanceEvent[];
}

/**
 * Trust Artifact §5 — vertical chain of provenance entries with downward
 * arrows between them. Events are rendered in daemon-supplied order (oldest →
 * newest, top → bottom). Empty chain is rare in practice but rendered as an
 * informative empty state rather than hidden.
 */
export function ProvenanceChain({ events }: Props) {
    return (
        <section
            className="audit-section audit-provenance"
            aria-labelledby="audit-provenance-heading"
        >
            <h3
                id="audit-provenance-heading"
                className="audit-section-heading"
            >
                Provenance chain
            </h3>
            {events.length === 0 ? (
                <p className="muted">No provenance events recorded.</p>
            ) : (
                <ol className="audit-provenance-chain">
                    {events.map((event, index) => (
                        <li
                            className="audit-provenance-step"
                            key={`${event.timestamp}-${index}`}
                        >
                            <div className="audit-provenance-row">
                                <span className={`audit-provenance-kind badge badge-${event.kind.toLowerCase()}`}>
                                    {event.kind}
                                </span>
                                <span className="audit-provenance-summary">{event.summary}</span>
                            </div>
                            <div className="audit-provenance-meta muted">
                                <span className="mono">{event.timestamp}</span>
                                <span> · {event.device}</span>
                            </div>
                            {event.evidence ? (
                                <div className="audit-provenance-evidence mono">{event.evidence}</div>
                            ) : null}
                            {index < events.length - 1 ? (
                                <div
                                    className="audit-provenance-arrow"
                                    aria-hidden="true"
                                >
                                    ↓
                                </div>
                            ) : null}
                        </li>
                    ))}
                </ol>
            )}
        </section>
    );
}
