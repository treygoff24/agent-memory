import { useMemo, useState } from 'react';

import type { PeerSortKey, PeerSync, PeerTrust, PeerViewItem } from './peersView/types';

import { useSyncDashboardQuery, type ClaimLockInfo, type PeerSessionStatus } from '../api';
import { Inspector, type InspectorItem } from '../inspector';
import { TrustLedger } from './peersView';
import { QueryErrorBanner, QueryLoadingBanner } from './QueryFeedback';

export type { PeerSortKey, PeerViewItem } from './peersView/types';

interface SortState {
    key: PeerSortKey;
    dir: 'asc' | 'desc';
}

function deviceFromSession(session: PeerSessionStatus): string {
    return session.session_id.replace(/^peer_/, '') || session.harness.toLowerCase().replaceAll(' ', '-');
}

function lastHandshake(session: PeerSessionStatus): string {
    const seconds = session.last_heartbeat_age_seconds;
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
    return `${Math.round(seconds / 86_400)}d`;
}

function lockHolder(lock: ClaimLockInfo): string {
    return lock.holder ?? lock.held_by ?? '';
}

function trustForSession(session: PeerSessionStatus): PeerTrust {
    if (session.last_heartbeat_age_seconds > 900) return 'stale';
    if (session.started_at) return 'local active';
    return 'unknown';
}

function syncForSession(session: PeerSessionStatus): PeerSync {
    const trust = trustForSession(session);
    if (trust === 'local active') return 'active';
    if (trust === 'stale') return 'stale';
    return 'unknown';
}

function normalizePeer(session: PeerSessionStatus, locks: ClaimLockInfo[]): PeerViewItem {
    const device = deviceFromSession(session);
    const heldLocks = locks.filter((lock) => lockHolder(lock) === session.session_id);
    const trust = trustForSession(session);
    const sync = syncForSession(session);
    const row: PeerViewItem = {
        id: session.session_id,
        label: session.harness,
        device,
        trust,
        sync,
        reachable: trust === 'local active',
        locksHeld: heldLocks.length,
        locksPending: null,
        events24h: null,
        eventsIn24h: null,
        eventsOut24h: null,
        devicePubkeyShort: 'unknown',
        lastHandshake: lastHandshake(session),
        lastHandshakeTs: session.started_at ?? 'unknown',
        sessionsOpen: trust === 'local active' ? 1 : 0,
    };
    return row;
}

function valueForSort(item: PeerViewItem, key: PeerSortKey): string | number {
    if (key === 'locks') return item.locksHeld + (item.locksPending ?? 0);
    if (key === 'events24h') return item.events24h ?? 0;
    return item[key];
}

function sortPeers(items: PeerViewItem[], sort: SortState): PeerViewItem[] {
    return [...items].sort((a, b) => {
        const av = valueForSort(a, sort.key);
        const bv = valueForSort(b, sort.key);
        const result =
            typeof av === 'number' && typeof bv === 'number' ? av - bv : String(av).localeCompare(String(bv));
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
        body: `${peer.label} is ${peer.trust}/${peer.sync}. Trust is not inferred without daemon policy state.`,
        meta: peer.lastHandshake,
        sessionId: peer.sessionsOpen > 0 ? `${peer.sessionsOpen} open` : 'none',
        // The peer-detail inspector card stack (peerDetail.tsx) renders
        // ConnectionCard / ClaimLocksCard / TrafficCard — no RecentMemoriesCard
        // — so recallCountTotal is safely overloaded here to feed TrafficCard's
        // "events 24h" display. When the daemon doesn't supply per-peer event
        // counters we omit the fields entirely so TrafficCard's em-dash
        // fallback fires. Replace with dedicated peer-traffic fields on
        // InspectorItem if these ever need to flow into a recall card.
        ...(peer.events24h !== null ? { recallCountTotal: peer.events24h } : {}),
        ...(peer.eventsIn24h !== null ? { recallCount30d: peer.eventsIn24h } : {}),
        policy: {
            governance: `${peer.trust}/${peer.sync}`,
            privacy: 'unknown',
            tombstone: 'unknown',
        },
        provenance: {
            written: peer.lastHandshake,
            session: peer.sessionsOpen > 0 ? `${peer.sessionsOpen} active` : 'none',
            confidence: peer.trust,
            device: peer.devicePubkeyShort,
            peers: peer.reachable ? 'reachable' : 'stale/unknown',
        },
        summary: 'Per-peer traffic is unavailable from the daemon; showing heartbeat and lock state only.',
    };
}

export function Peers() {
    const query = useSyncDashboardQuery();
    const items = useMemo(
        () =>
            query.data?.peer_presence.active_sessions.map((session) =>
                normalizePeer(session, query.data?.claim_locks.locks ?? []),
            ) ?? [],
        [query.data],
    );
    const [sort, setSort] = useState<SortState>({ key: 'device', dir: 'asc' });
    const sorted = useMemo(() => sortPeers(items, sort), [items, sort]);
    const [selectedId, setSelectedId] = useState(sorted[0]?.id ?? '');
    const selected = sorted.find((peer) => peer.id === selectedId) ?? sorted[0];

    function updateSort(key: PeerSortKey) {
        setSort((current) => ({
            key,
            dir: current.key === key && current.dir === 'desc' ? 'asc' : 'desc',
        }));
    }

    return (
        <div data-testid="peers-view">
            {query.isLoading ? <QueryLoadingBanner label="Peers" /> : null}
            <QueryErrorBanner
                error={query.error}
                label="Peers"
            />
            <div className="view-header">
                <span className="view-title">Peers</span>
                <span className="view-subtitle">
                    · {items.length} known · {items.filter((peer) => peer.reachable).length} reachable ·{' '}
                    {items.filter((peer) => peer.trust === 'local active').length} local active
                </span>
                <span className="spacer" />
                <button
                    className="btn primary"
                    type="button"
                    disabled
                    aria-disabled="true"
                    aria-describedby="pairing-unavailable-copy"
                    title="Pairing API is not available in alpha."
                >
                    + pair new device
                </button>
                <span
                    id="pairing-unavailable-copy"
                    className="meta"
                >
                    Pairing API is not available in alpha.
                </span>
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
