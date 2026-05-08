import type { InspectorItem } from '../types';

import { CardFrame } from './CardFrame';

export function ConnectionCard({ item }: { item: InspectorItem }) {
    return (
        <CardFrame title="Connection">
            <dl className="kv">
                <dt>device</dt>
                <dd>{item.provenance?.device ?? item.title}</dd>
                <dt>sync</dt>
                <dd>in sync</dd>
            </dl>
        </CardFrame>
    );
}
