import type { SupersessionHistoryEntry } from '../../api';

import { hashFor } from '../../router';

interface Props {
    history: SupersessionHistoryEntry[];
}

/**
 * Trust Artifact §8 — bidirectional supersession history. Daemon emits a
 * single flat list with `direction` discriminator (`supersedes` / `superseded_by`);
 * we split it into two visually distinct columns so the directionality is
 * legible at a glance. Every memory id is a hash link to its own audit view.
 */
export function SupersessionHistory({ history }: Props) {
    const supersedes = history.filter((entry) => entry.direction === 'supersedes');
    const supersededBy = history.filter((entry) => entry.direction === 'superseded_by');

    return (
        <section
            className="audit-section audit-supersession"
            aria-labelledby="audit-supersession-heading"
        >
            <h3
                id="audit-supersession-heading"
                className="audit-section-heading"
            >
                Supersession history
            </h3>
            <div className="audit-supersession-cols">
                <SupersessionColumn
                    label="Supersedes"
                    entries={supersedes}
                    emptyHint="This memory does not supersede any prior memory."
                />
                <SupersessionColumn
                    label="Superseded by"
                    entries={supersededBy}
                    emptyHint="No newer memory has superseded this one."
                />
            </div>
        </section>
    );
}

function SupersessionColumn({
    label,
    entries,
    emptyHint,
}: {
    label: string;
    entries: SupersessionHistoryEntry[];
    emptyHint: string;
}) {
    return (
        <div className="audit-supersession-col">
            <h4 className="audit-supersession-col-heading">{label}</h4>
            {entries.length === 0 ? (
                <p className="muted">{emptyHint}</p>
            ) : (
                <ul className="audit-supersession-list">
                    {entries.map((entry) => (
                        <li
                            className="audit-supersession-row"
                            key={`${entry.direction}-${entry.memory_id}`}
                        >
                            <a
                                className="audit-supersession-id mono"
                                href={hashFor({ kind: 'audit', memoryId: entry.memory_id })}
                            >
                                {entry.memory_id}
                            </a>
                            <span className="audit-supersession-title">{entry.title}</span>
                            {entry.at ? <span className="muted mono"> · {entry.at}</span> : null}
                        </li>
                    ))}
                </ul>
            )}
        </div>
    );
}
