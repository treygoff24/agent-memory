import { DreamPassCard, EvidenceSummaryCard } from '../cards';
import type { DreamOutputItem, InspectorKindProps } from '../types';
import { BodySection, InspectorHeader, InspectorShell } from './common';

export function DreamOutputInspector({ item, layout }: InspectorKindProps<DreamOutputItem>) {
    return (
        <InspectorShell>
            <InspectorHeader
                item={item}
                badge="dream output"
            />
            <div className={`insp-grid ${layout === 'narrow' ? 'narrow' : ''}`}>
                <div>
                    <BodySection
                        item={item}
                        label="Dream output"
                    />
                    <EvidenceSummaryCard item={item} />
                </div>
                <div className="sidecar">
                    <DreamPassCard item={item} />
                </div>
            </div>
        </InspectorShell>
    );
}
