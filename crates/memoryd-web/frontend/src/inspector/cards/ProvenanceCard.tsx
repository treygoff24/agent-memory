import type { InspectorItem } from '../types';

import { CardFrame } from './CardFrame';

export function ProvenanceCard({ item }: { item: InspectorItem }) {
    const provenance = item.provenance ?? {};
    return (
        <CardFrame
            title="Provenance"
            meta="2 entries"
        >
            <dl className="kv">
                <dt>written</dt>
                <dd>{provenance.written ?? item.meta ?? 'unknown'}</dd>
                <dt>session</dt>
                <dd className="mono">{provenance.session ?? item.sessionId ?? 'n/a'}</dd>
                <dt>grounding</dt>
                <dd className="mono">{provenance.grounding ?? 'none'}</dd>
                <dt>confidence</dt>
                <dd className="mono">{provenance.confidence ?? item.confidence?.toFixed(2) ?? 'n/a'}</dd>
                <dt>device</dt>
                <dd className="mono">{provenance.device ?? 'local'}</dd>
                <dt>peers seen</dt>
                <dd className="mono">{provenance.peers ?? '0 of 0'}</dd>
            </dl>
        </CardFrame>
    );
}
