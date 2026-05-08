import type { InspectorItem } from '../types';
import { CardFrame } from './CardFrame';

export function PolicyDecisionTraceCard({ item }: { item: InspectorItem }) {
    return (
        <CardFrame
            title="Policy decision trace"
            meta={item.kind === 'governance-decision' ? 'active' : undefined}
        >
            <dl className="kv">
                <dt>decision</dt>
                <dd>{item.kind === 'governance-decision' ? item.title : 'no manual decision'}</dd>
                <dt>policy</dt>
                <dd>{item.policy?.governance ?? 'default-v1'}</dd>
            </dl>
        </CardFrame>
    );
}
