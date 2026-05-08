import type { InspectorItem } from '../types';

import { CardFrame } from './CardFrame';

export function DreamPassCard({ item }: { item: InspectorItem }) {
    return (
        <CardFrame title="Dream pass">
            <dl className="kv">
                <dt>run</dt>
                <dd>latest scheduled pass</dd>
                <dt>confidence</dt>
                <dd className="mono">{item.confidence?.toFixed(2) ?? 'n/a'}</dd>
                <dt>scope</dt>
                <dd>{item.namespace}</dd>
            </dl>
        </CardFrame>
    );
}
