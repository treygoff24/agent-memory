import type { InspectorItem } from '../types';

import { CardFrame } from './CardFrame';

export function RecentMemoriesCard({ item }: { item: InspectorItem }) {
    return (
        <CardFrame
            title="Recent memories"
            meta={item.recallCount30d ? `30d ${item.recallCount30d}` : undefined}
        >
            <div className="meta">{item.summary ?? item.body ?? 'No recent memory details.'}</div>
        </CardFrame>
    );
}
