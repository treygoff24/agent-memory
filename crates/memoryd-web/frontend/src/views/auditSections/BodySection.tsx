import type { AuditMemoryResponse } from '../../api';

interface Props {
    audit: AuditMemoryResponse;
}

/**
 * Trust Artifact §2 — full memory body. Markdown isn't rendered for v1; we
 * preserve whitespace + show inline-code via the existing `mono` class so
 * the canonical text matches what's on disk. Encrypted memories show the
 * redaction notice from the daemon (body is empty) plus the reveal-externally
 * instructions; an explicit `memoryd memory reveal <id> --reason …` is the
 * only path that yields plaintext, intentionally not surfaced here.
 */
export function BodySection({ audit }: Props) {
    const encrypted = audit.privacy_scan.storage_action === 'encrypt_at_rest';

    if (encrypted) {
        return (
            <section
                className="audit-section audit-body audit-body-encrypted"
                aria-labelledby="audit-body-heading"
            >
                <h3
                    id="audit-body-heading"
                    className="audit-section-heading"
                >
                    Body
                </h3>
                <p className="muted">
                    Body is stored encrypted at rest. The web dashboard never reveals plaintext for encrypted memories —
                    that path is intentionally CLI-only.
                </p>
                <p className="mono">memoryd memory reveal {audit.memory_id} --reason &quot;&lt;why&gt;&quot;</p>
            </section>
        );
    }

    return (
        <section
            className="audit-section audit-body"
            aria-labelledby="audit-body-heading"
        >
            <h3
                id="audit-body-heading"
                className="audit-section-heading"
            >
                Body
            </h3>
            <pre className="audit-body-text">{audit.body || '(empty body)'}</pre>
        </section>
    );
}
