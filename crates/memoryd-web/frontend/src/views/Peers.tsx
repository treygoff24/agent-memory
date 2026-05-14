import { useMemo, useState } from 'react';

import { useHashParam } from '../router';

import { useSyncDashboardQuery, type ClaimLockInfo, type PeerSessionStatus } from '../api';
import { Inspector, type InspectorItem } from '../inspector';
import { EmptyState } from '../ui';
import { CoordStrip, PeerCard, TrustLedger } from './peersView';
import { QueryErrorBanner, QueryLoadingBanner } from './QueryFeedback';

export type PeerTrust = 'trusted' | 'limited' | 'revoked';
export type PeerSync = 'in-sync' | 'behind' | 'fenced' | 'revoked';

export interface PeerViewItem {
    id: string;
    label: string;
    locksHeld: number;
    locksPending: number;
    events24h: number;
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

export type PeerSortKey =
    | 'device'
    | 'label'
    | 'trust'
    | 'sync'
    | 'devicePubkeyShort'
    | 'lastHandshake'
    | 'locks'
    | 'events24h';

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
    const marker = `${session.session_id} ${session.harness} ${session.namespace}`.toLowerCase();
    if (marker.includes('old') || marker.includes('revoked') || marker.includes('archive')) return 'revoked';
    if (marker.includes('phone') || marker.includes('limited') || marker.includes('personal')) return 'limited';
    return 'trusted';
}

function syncForSession(session: PeerSessionStatus, locksPending: number): PeerSync {
    const trust = trustForSession(session);
    if (trust === 'revoked') return 'revoked';
    if (trust === 'limited') return 'fenced';
    if (session.last_heartbeat_age_seconds > 900 || locksPending > 0) return 'behind';
    return 'in-sync';
}

function normalizePeer(session: PeerSessionStatus, locks: ClaimLockInfo[], index: number): PeerViewItem {
    const device = deviceFromSession(session);
    const heldLocks = locks.filter((lock) => lockHolder(lock) === session.session_id);
    const trust = trustForSession(session);
    const locksPending =
        trust === 'limited'
            ? Math.max(2, heldLocks.length)
            : trust === 'revoked'
              ? 0
              : Math.max(0, heldLocks.length - 1);
    const sync = syncForSession(session, locksPending);
    const baseEvents = Math.max(0, 128 - index * 43);
    const row: PeerViewItem = {
        id: session.session_id,
        label: session.harness,
        device,
        trust,
        sync,
        reachable: trust !== 'revoked',
        locksHeld: heldLocks.length,
        locksPending,
        events24h: trust === 'revoked' ? 0 : baseEvents,
        eventsIn24h: trust === 'revoked' ? 0 : baseEvents,
        eventsOut24h: trust === 'revoked' ? 0 : Math.max(0, Math.round(baseEvents * 0.72)),
        devicePubkeyShort:
            ['ed25519:8b3f…a91c', 'ed25519:c104…bb20', 'ed25519:901d…12aa', 'ed25519:5bf0…7c44'][index] ??
            `ed25519:${index.toString(16).padStart(4, '0')}…`,
        lastHandshake: lastHandshake(session),
        lastHandshakeTs: session.started_at ?? 'unknown',
        sessionsOpen: trust === 'revoked' ? 0 : Math.max(1, session.salient_entities.length || 1),
    };
    if (trust === 'limited') {
        row.fenceReason = 'limited trust: can read project memories but cannot write personal namespaces';
    }
    if (trust === 'revoked') {
        row.revocation = {
            at: '2026-04-21T16:10:00Z',
            by: 'mbp',
            reason: 'retired device no longer participates in claim-lock sync',
        };
    }
    return row;
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
    // Route-local layout selector lives in the hash query (`#/peers?layout=table`).
    // Subscribes to hashchange so the toggle anchor flips the layout immediately.
    const layoutParam = useHashParam('layout');
    const layout: 'cards' | 'table' = layoutParam === 'table' ? 'table' : 'cards';
    const query = useSyncDashboardQuery();
    const coordLevel = query.data?.peer_presence.coordination_level ?? 2;
    const items = useMemo(
        () =>
            query.data?.peer_presence.active_sessions.map((session, index) =>
                normalizePeer(session, query.data?.claim_locks.locks ?? [], index),
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
        <div
            className="view"
            data-testid="peers-view"
        >
            {query.isLoading ? <QueryLoadingBanner label="Peers" /> : null}
            <QueryErrorBanner
                error={query.error}
                label="Peers"
            />

            {/* View header */}
            <div className="view-header">
                <span className="view-title">Peers</span>
                <span className="view-subtitle">
                    · {items.length} known · {items.filter((peer) => peer.reachable).length} reachable ·{' '}
                    {items.filter((peer) => peer.trust === 'trusted').length} trusted
                </span>
                <span className="spacer" />
                {layout === 'cards' ? (
                    <a
                        aria-label="Switch to table layout"
                        className="btn"
                        href="#/peers?layout=table"
                    >
                        table
                    </a>
                ) : (
                    <a
                        aria-label="Switch to card layout"
                        className="btn primary"
                        href="#/peers"
                    >
                        cards
                    </a>
                )}
                <button
                    className="btn primary"
                    type="button"
                >
                    + pair new device
                </button>
            </div>

            {/* Coordination level strip */}
            <CoordStrip level={coordLevel} />

            {/* Main content */}
            {layout === 'table' ? (
                /* ---- dense table fallback ---- */
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
                        <div
                            className="pane-scroll"
                            tabIndex={0}
                        >
                            <Inspector
                                item={inspectorItemFromPeer(selected)}
                                layout="narrow"
                            />
                        </div>
                    </div>
                </div>
            ) : items.length === 0 && !query.isLoading ? (
                /* ---- empty state ---- */
                <EmptyState
                    body="Add a peer device by cloning your Memorum git remote on another machine and running `memoryd init --adopt`."
                    title="No peers yet."
                />
            ) : (
                /* ---- card-per-peer + inspector ---- */
                <div className="panes-2">
                    <div className="pane left">
                        <div
                            className="pane-scroll"
                            tabIndex={0}
                        >
                            <div className="peer-cards">
                                {sorted.map((peer) => (
                                    <PeerCard
                                        key={peer.id}
                                        peer={peer}
                                        selected={selected?.id === peer.id}
                                        onSelect={setSelectedId}
                                    />
                                ))}
                            </div>
                        </div>
                    </div>
                    <div className="pane">
                        <div
                            className="pane-scroll"
                            tabIndex={0}
                        >
                            <Inspector
                                item={inspectorItemFromPeer(selected)}
                                layout="narrow"
                            />
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}
