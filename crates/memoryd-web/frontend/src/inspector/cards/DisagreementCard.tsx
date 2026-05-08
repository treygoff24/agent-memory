import type { InspectorItem } from '../types';
import { CardFrame } from './CardFrame';

export function DisagreementCard({ item }: { item: InspectorItem }) {
    return (
        <CardFrame title="Disagreement">
            <p className="body-text">
                {item.kind === 'inbox-conflict'
                    ? 'Two devices wrote incompatible versions after the last common ancestor.'
                    : 'No active disagreement is attached to this item.'}
            </p>
        </CardFrame>
    );
}
