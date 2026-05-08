import type { InspectorItem } from '../types';
import { CardFrame } from './CardFrame';

export function SessionsCard({ item }: { item: InspectorItem }) {
    return (
        <CardFrame title="Sessions">
            <dl className="kv">
                <dt>current</dt>
                <dd className="mono">{item.sessionId ?? item.provenance?.session ?? 'n/a'}</dd>
                <dt>recalls</dt>
                <dd className="mono">{item.recallCountTotal ?? item.recalls?.length ?? 0}</dd>
            </dl>
        </CardFrame>
    );
}
