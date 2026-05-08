import type { InspectorItem } from '../types';
import { CardFrame } from './CardFrame';

export function EntityCard({ item }: { item: InspectorItem }) {
    return (
        <CardFrame title="Entity">
            <dl className="kv">
                <dt>name</dt>
                <dd>{item.title}</dd>
                <dt>namespace</dt>
                <dd>{item.namespace}</dd>
            </dl>
        </CardFrame>
    );
}
