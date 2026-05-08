import type { InspectorItem } from '../types';
import { CardFrame } from './CardFrame';

export function PrivacyScanCard({ item }: { item: InspectorItem }) {
    return (
        <CardFrame
            title="Privacy scan"
            meta={item.encrypted ? 'encrypted' : '0 labels'}
        >
            <div className="meta">
                {item.encrypted
                    ? 'encrypted at rest · plaintext body redacted from broad surfaces'
                    : 'no sensitive labels detected · storage action: plaintext'}
            </div>
        </CardFrame>
    );
}
