import type { PrivacyScanResult } from '../../api';

interface Props {
    scan: PrivacyScanResult;
}

/**
 * Trust Artifact §7 — privacy classifier output for this memory: which labels
 * (email, phone, ssn, person, etc.) fired and which storage action the
 * classifier mandated (plaintext / encrypt_at_rest / refuse). Refusal is only
 * a write-time outcome; if we're rendering audit we're not in that state, but
 * we still show the field for symmetry with the daemon's response shape.
 */
export function PrivacyScanSection({ scan }: Props) {
    return (
        <section
            className="audit-section audit-privacy-scan"
            aria-labelledby="audit-privacy-scan-heading"
        >
            <h3 id="audit-privacy-scan-heading" className="audit-section-heading">
                Privacy scan
            </h3>
            <dl className="audit-stat-grid">
                <div>
                    <dt>Storage action</dt>
                    <dd>
                        <span
                            className={`badge audit-storage-action audit-storage-${scan.storage_action.toLowerCase()}`}
                        >
                            {scan.storage_action}
                        </span>
                    </dd>
                </div>
                <div>
                    <dt>Labels detected</dt>
                    <dd>
                        {scan.labels_detected.length === 0 ? (
                            <span className="muted">none</span>
                        ) : (
                            <ul className="audit-privacy-labels">
                                {scan.labels_detected.map((label, index) => (
                                    <li
                                        className="badge audit-privacy-label"
                                        key={`${label}-${index}`}
                                    >
                                        {label}
                                    </li>
                                ))}
                            </ul>
                        )}
                    </dd>
                </div>
            </dl>
        </section>
    );
}
