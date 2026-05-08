import type { InspectorKindProps, RecallEventItem } from '../types';

import { RecentMemoriesCard, SessionsCard } from '../cards';
import { BodySection, InspectorHeader, InspectorShell } from './common';

export function RecallEventInspector({ item, layout }: InspectorKindProps<RecallEventItem>) {
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
                        label="Memory recalled"
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
