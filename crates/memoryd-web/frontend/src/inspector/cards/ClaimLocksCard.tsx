import type { InspectorItem } from '../types';
import { CardFrame } from './CardFrame';

export function ClaimLocksCard({ item }: { item: InspectorItem }) {
    return (
        <CardFrame title="Claim locks">
            <dl className="kv">
                <dt>holder</dt>
                <dd>{item.kind === 'peer-detail' ? item.title : 'none'}</dd>
                <dt>pending</dt>
                <dd className="mono">0</dd>
            </dl>
        </CardFrame>
    );
}
