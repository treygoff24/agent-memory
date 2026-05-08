import type { InspectorItem } from '../types';

import { CardFrame } from './CardFrame';

export function TrafficCard({ item }: { item: InspectorItem }) {
    return (
        <CardFrame title="Traffic">
            <dl className="kv">
                <dt>events 24h</dt>
                <dd className="mono">{item.kind === 'peer-detail' ? '128' : '0'}</dd>
                <dt>last seen</dt>
                <dd>{item.meta ?? 'recently'}</dd>
            </dl>
        </CardFrame>
    );
}
