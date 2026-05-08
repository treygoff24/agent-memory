import type { EntityDetailItem, InspectorKindProps } from '../types';

import { CoOccurringCard, EntityCard, RecentMemoriesCard } from '../cards';
import { BodySection, InspectorHeader, InspectorShell } from './common';

export function EntityDetailInspector({ item, layout }: InspectorKindProps<EntityDetailItem>) {
    return (
        <InspectorShell>
            <InspectorHeader
                item={item}
                badge="entity"
            />
            <div className={`insp-grid ${layout === 'narrow' ? 'narrow' : ''}`}>
                <div>
                    <BodySection
                        item={item}
                        label="Entity detail"
                    />
                    <EntityCard item={item} />
                </div>
                <div className="sidecar">
                    <CoOccurringCard item={item} />
                    <RecentMemoriesCard item={item} />
                </div>
            </div>
        </InspectorShell>
    );
}
