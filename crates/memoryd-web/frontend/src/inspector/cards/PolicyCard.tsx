import type { InspectorItem } from '../types';

import { CardFrame } from './CardFrame';

export function PolicyCard({ item }: { item: InspectorItem }) {
    const policy = item.policy ?? {};
    return (
        <CardFrame title="Policy">
            <dl className="kv">
                <dt>privacy</dt>
                <dd>{policy.privacy ?? item.sensitivity ?? 'plaintext'}</dd>
                <dt>governance</dt>
                <dd>{policy.governance ?? 'auto-approve'}</dd>
                <dt>tombstone</dt>
                <dd>{policy.tombstone ?? 'none'}</dd>
            </dl>
        </CardFrame>
    );
}
