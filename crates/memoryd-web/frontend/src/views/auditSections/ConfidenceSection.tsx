import type { AuditMemoryResponse } from '../../api';

interface Props {
    audit: AuditMemoryResponse;
}

/**
 * Trust Artifact §3 — confidence score (two-decimal display, matches inspector
 * card formatting) and the daemon-emitted reason text. When the daemon doesn't
 * supply a reason we render a muted placeholder rather than hiding the field;
 * absence is itself information.
 */
export function ConfidenceSection({ audit }: Props) {
    return (
        <section
            className="audit-section audit-confidence"
            aria-labelledby="audit-confidence-heading"
        >
            <h3 id="audit-confidence-heading" className="audit-section-heading">
                Confidence
            </h3>
            <div className="audit-confidence-row">
                <span
                    className="audit-confidence-score mono"
                    aria-label="Current confidence score"
                >
                    {audit.confidence.toFixed(2)}
                </span>
                <span className="audit-confidence-reason">
                    {audit.confidence_reason ?? <span className="muted">no reason recorded</span>}
                </span>
            </div>
        </section>
    );
}
