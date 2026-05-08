import type { InspectorItem } from '../types';

import { CardFrame } from './CardFrame';

export function CoOccurringCard({ item }: { item: InspectorItem }) {
    return (
        <CardFrame
            title="Co-occurring"
            meta={item.kind === 'entity-detail' ? 'top terms' : undefined}
        >
            <div className="meta">project · memory · session</div>
        </CardFrame>
    );
}
