import type { InspectorItem } from '../types';

import { CardFrame } from './CardFrame';

export function TrafficCard({ item }: { item: InspectorItem }) {
    // For peer-detail items, recallCountTotal is overloaded to mean "events 24h"
    // (see Peers.tsx inspectorItemFromPeer). When the daemon doesn't supply the
    // counter, render an em-dash rather than invent a value.
    const events24h = item.recallCountTotal ?? '—';
    return (
        <CardFrame title="Traffic">
            <dl className="kv">
                <dt>events 24h</dt>
                <dd className="mono">{events24h}</dd>
                <dt>last seen</dt>
                <dd>{item.meta ?? 'recently'}</dd>
            </dl>
        </CardFrame>
    );
}
