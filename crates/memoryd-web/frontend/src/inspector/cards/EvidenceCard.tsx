import type { InspectorItem } from '../types';

import { hashFor } from '../../router';
import { CardFrame } from './CardFrame';

function isCanonicalMemoryId(id: string): boolean {
    return id.startsWith('mem_');
}

export function EvidenceCard({ item }: { item: InspectorItem }) {
    const evidence = item.evidence ?? [];
    return (
        <CardFrame
            title="Evidence"
            meta={`${evidence.length} memories`}
        >
            {evidence.length === 0 ? (
                <div className="meta">No explicit evidence attached.</div>
            ) : (
                <div className="rows">
                    {evidence.map((entry) => (
                        <div
                            className="row"
                            key={entry.id}
                        >
                            {isCanonicalMemoryId(entry.id) ? (
                                <a
                                    className="mono memory-id-link"
                                    href={hashFor({ kind: 'audit', memoryId: entry.id })}
                                >
                                    {entry.id}
                                </a>
                            ) : (
                                <span className="mono">{entry.id}</span>
                            )}
                            <span>{entry.title}</span>
                            <span className="mono">{entry.score?.toFixed(2) ?? 'n/a'}</span>
                        </div>
                    ))}
                </div>
            )}
        </CardFrame>
    );
}
