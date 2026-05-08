import type { InspectorItem } from '../types';
import { CardFrame } from './CardFrame';

export function EvidenceSummaryCard({ item }: { item: InspectorItem }) {
    return (
        <CardFrame title="Evidence summary">
            <p className="body-text">
                {item.evidence?.length
                    ? `${item.evidence.length} supporting memories are attached to this item.`
                    : 'No evidence summary has been attached yet.'}
            </p>
        </CardFrame>
    );
}
