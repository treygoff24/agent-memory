import { ClaimLocksCard, ConnectionCard, TrafficCard } from '../cards';
import type { InspectorKindProps, PeerDetailItem } from '../types';
import { BodySection, InspectorHeader, InspectorShell } from './common';

export function PeerDetailInspector({ item, layout }: InspectorKindProps<PeerDetailItem>) {
    return (
        <InspectorShell>
            <InspectorHeader
                item={item}
                badge="peer detail"
            />
            <div className={`insp-grid ${layout === 'narrow' ? 'narrow' : ''}`}>
                <div>
                    <BodySection
                        item={item}
                        label="Peer"
                    />
                </div>
                <div className="sidecar">
                    <ConnectionCard item={item} />
                    <ClaimLocksCard item={item} />
                    <TrafficCard item={item} />
                </div>
            </div>
        </InspectorShell>
    );
}
