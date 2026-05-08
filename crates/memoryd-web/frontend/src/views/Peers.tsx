import { useMemo, useState } from 'react';

import { peers, type PeerItem } from '../data/fixtures';
import { Inspector, type InspectorItem } from '../inspector';
import { TrustLedger } from './peersView';

export type PeerTrust = 'trusted' | 'limited' | 'revoked';
export type PeerSync = 'in-sync' | 'behind' | 'fenced' | 'revoked';

export interface PeerViewItem extends PeerItem {
    device: string;
    trust: PeerTrust;
    sync: PeerSync;
    reachable: boolean;
    devicePubkeyShort: string;
    lastHandshake: string;
    lastHandshakeTs: string;
    sessionsOpen: number;
    eventsIn24h: number;
    eventsOut24h: number;
    fenceReason?: string;
    revocation?: {
        at: string;
        by: string;
        reason: string;
    };
}

export type PeerSortKey = 'device' | 'label' | 'trust' | 'sync' | 'devicePubkeyShort' | 'lastHandshake' | 'locks' | 'events24h';

interface SortState {
    key: PeerSortKey;
    dir: 'asc' | 'desc';
}

function normalizePeer(peer: PeerItem, index: number): PeerViewItem {
    const device = peer.id.replace(/^peer_/, '');
    const pubkeys = ['ed25519:8b3f…a91c', 'ed25519:c104…bb20', 'ed25519:901d…12aa'];
    const row: PeerViewItem = {
        ...peer,
        device,
        trust: peer.trust === 'revoked' ? 'revoked' : 'trusted',
        sync: peer.sync === 'revoked' ? 'revoked' : peer.sync === 'behind' ? 'behind' : 'in-sync',
        reachable: peer.sync !== 'revoked',
        devicePubkeyShort: pubkeys[index] ?? `ed25519:${index.toString(16).padStart(4, '0')}…`,
        lastHandshake: index === 0 ? '2m' : index === 1 ? '17m' : '45d',
        lastHandshakeTs: index === 0 ? '2026-05-08T13:52:00Z' : index === 1 ? '2026-05-08T13:37:00Z' : '2026-03-24T09:00:00Z',
        sessionsOpen: index === 0 ? 2 : index === 1 ? 1 : 0,
        eventsIn24h: peer.events24h,
        eventsOut24h: Math.max(0, Math.round(peer.events24h * 0.72)),
    };
    if (peer.trust === 'revoked') {
        row.revocation = {
            at: '2026-04-21T16:10:00Z',
            by: 'mbp',
            reason: 'retired device no longer participates in claim-lock sync',
        };
    }
    return row;
}

function peerRows(): PeerViewItem[] {
    const base = peers.map(normalizePeer);
    return [
        ...base,
        {
            id: 'peer_phone',
            label: 'Travel phone',
            device: 'phone',
            trust: 'limited',
            sync: 'fenced',
            reachable: true,
            locksHeld: 0,
            locksPending: 2,
            events24h: 19,
            eventsIn24h: 19,
            eventsOut24h: 7,
            devicePubkeyShort: 'ed25519:5bf0…7c44',
            lastHandshake: '41m',
            lastHandshakeTs: '2026-05-08T13:13:00Z',
            sessionsOpen: 0,
            fenceReason: 'limited trust: can read project memories but cannot write personal namespaces',
        },
    ];
}

function valueForSort(item: PeerViewItem, key: PeerSortKey): string | number {
    if (key === 'locks') return item.locksHeld + item.locksPending;
    if (key === 'events24h') return item.events24h;
    return item[key];
}

function sortPeers(items: PeerViewItem[], sort: SortState): PeerViewItem[] {
    return [...items].sort((a, b) => {
        const av = valueForSort(a, sort.key);
        const bv = valueForSort(b, sort.key);
        const result = typeof av === 'number' && typeof bv === 'number' ? av - bv : String(av).localeCompare(String(bv));
        return sort.dir === 'asc' ? result : -result;
    });
}

function inspectorItemFromPeer(peer: PeerViewItem | undefined): InspectorItem | null {
    if (!peer) return null;
    return {
        kind: 'peer-detail',
        id: peer.id,
        title: peer.label,
        namespace: peer.device,
        body: `${peer.label} is ${peer.trust}/${peer.sync}. ${peer.fenceReason ?? 'No namespace fence is active.'}`,
        meta: peer.lastHandshake,
        sessionId: peer.sessionsOpen > 0 ? `${peer.sessionsOpen} open` : 'none',
        recallCountTotal: peer.events24h,
        recallCount30d: peer.eventsIn24h,
        policy: {
            governance: `${peer.trust}/${peer.sync}`,
            privacy: peer.fenceReason ? 'namespace fence active' : 'trusted peer policy',
            tombstone: peer.revocation ? 'revoked' : 'none',
        },
        provenance: {
            written: peer.lastHandshake,
            session: peer.sessionsOpen > 0 ? `${peer.sessionsOpen} active` : 'none',
            confidence: peer.trust,
            device: peer.devicePubkeyShort,
            peers: peer.reachable ? 'reachable' : 'offline',
        },
        summary: `${peer.eventsIn24h} inbound / ${peer.eventsOut24h} outbound events in the last 24h.`,
    };
}

export function Peers() {
    const items = useMemo(peerRows, []);
    const [sort, setSort] = useState<SortState>({ key: 'device', dir: 'asc' });
    const sorted = useMemo(() => sortPeers(items, sort), [items, sort]);
    const [selectedId, setSelectedId] = useState(sorted[0]?.id ?? '');
    const selected = sorted.find((peer) => peer.id === selectedId) ?? sorted[0];

    function updateSort(key: PeerSortKey) {
        setSort((current) => ({ key, dir: current.key === key && current.dir === 'desc' ? 'asc' : 'desc' }));
    }

    return (
        <div data-testid="peers-view">
            <div className="view-header">
                <span className="view-title">Peers</span>
                <span className="view-subtitle">
                    · {items.length} known · {items.filter((peer) => peer.reachable).length} reachable · {items.filter((peer) => peer.trust === 'trusted').length} trusted
                </span>
                <span className="spacer" />
                <button
                    className="btn primary"
                    type="button"
                >
                    + pair new device
                </button>
            </div>
            <div className="panes-2">
                <div className="pane left">
                    <TrustLedger
                        peers={sorted}
                        selectedId={selected?.id ?? ''}
                        sort={sort}
                        onSort={updateSort}
                        onSelect={setSelectedId}
                    />
                </div>
                <div className="pane">
                    <div className="pane-scroll">
                        <Inspector
                            item={inspectorItemFromPeer(selected)}
                            layout="narrow"
                        />
                    </div>
                </div>
            </div>
        </div>
    );
}
