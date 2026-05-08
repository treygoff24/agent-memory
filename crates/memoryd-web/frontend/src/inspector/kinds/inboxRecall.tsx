import type { InboxRecallItem, InspectorKindProps } from '../types';

import { RecentMemoriesCard, SessionsCard } from '../cards';
import { BodySection, InspectorHeader, InspectorShell } from './common';

export function InboxRecallInspector({ item, layout }: InspectorKindProps<InboxRecallItem>) {
    return (
        <InspectorShell>
            <InspectorHeader
                item={item}
                badge="recall event"
            />
            <div className={`insp-grid ${layout === 'narrow' ? 'narrow' : ''}`}>
                <div>
                    <BodySection
                        item={item}
                        label="Surrounding context"
                    />
                    <RecentMemoriesCard item={item} />
                </div>
                <div className="sidecar">
                    <SessionsCard item={item} />
                </div>
            </div>
        </InspectorShell>
    );
}
