import type { PolicyDecisionEntry } from '../../api';

interface Props {
    decisions: PolicyDecisionEntry[];
}

const FIELDS: Array<{ key: keyof PolicyDecisionEntry; label: string }> = [
    { key: 'policy_source', label: 'Source' },
    { key: 'confidence_floor_pass', label: 'Confidence floor' },
    { key: 'grounding_satisfied', label: 'Grounding' },
    { key: 'contradiction_result', label: 'Contradiction' },
    { key: 'tombstone_enforced', label: 'Tombstone' },
    { key: 'sensitivity_gate_result', label: 'Sensitivity gate' },
];

/**
 * Trust Artifact §6 — list of governance decisions that ran against this
 * memory. Each entry shows the policy id with the six per-gate verdicts
 * underneath; we use the same row-key/value pattern as the inspector
 * `PolicyDecisionTraceCard` so memorized scanning works across both surfaces.
 */
export function PolicyDecisions({ decisions }: Props) {
    return (
        <section
            className="audit-section audit-policy-decisions"
            aria-labelledby="audit-policy-decisions-heading"
        >
            <h3 id="audit-policy-decisions-heading" className="audit-section-heading">
                Policy decisions
            </h3>
            {decisions.length === 0 ? (
                <p className="muted">No governance decisions recorded.</p>
            ) : (
                <ul className="audit-policy-list">
                    {decisions.map((decision, index) => (
                        <li
                            className="audit-policy-entry"
                            key={`${decision.policy_applied}-${index}`}
                        >
                            <div className="audit-policy-name mono">{decision.policy_applied}</div>
                            <dl className="audit-policy-grid">
                                {FIELDS.map((field) => (
                                    <div
                                        className="audit-policy-row"
                                        key={field.key}
                                    >
                                        <dt>{field.label}</dt>
                                        <dd className="mono">{decision[field.key]}</dd>
                                    </div>
                                ))}
                            </dl>
                        </li>
                    ))}
                </ul>
            )}
        </section>
    );
}
