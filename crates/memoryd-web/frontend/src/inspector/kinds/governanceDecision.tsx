import { PolicyCard, PolicyDecisionTraceCard, PrivacyScanCard, ProvenanceCard } from '../cards';
import type { GovernanceDecisionItem, InspectorKindProps } from '../types';
import { BodySection, InspectorHeader, InspectorShell } from './common';

export function GovernanceDecisionInspector({ item, layout }: InspectorKindProps<GovernanceDecisionItem>) {
    return (
        <InspectorShell>
            <InspectorHeader
                item={item}
                badge="governance"
            />
            <div className={`insp-grid ${layout === 'narrow' ? 'narrow' : ''}`}>
                <div>
                    <BodySection
                        item={item}
                        label="Decision"
                    />
                    <PolicyDecisionTraceCard item={item} />
                </div>
                <div className="sidecar">
                    <ProvenanceCard item={item} />
                    <PolicyCard item={item} />
                    <PrivacyScanCard item={item} />
                </div>
            </div>
        </InspectorShell>
    );
}
