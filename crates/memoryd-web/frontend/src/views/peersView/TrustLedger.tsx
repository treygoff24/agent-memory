import type { PeerSortKey, PeerViewItem } from './types';

interface TrustLedgerProps {
    peers: PeerViewItem[];
    selectedId: string;
    sort: { key: PeerSortKey; dir: 'asc' | 'desc' };
    onSort: (key: PeerSortKey) => void;
    onSelect: (id: string) => void;
}

const columns: Array<{ key: PeerSortKey; label: string }> = [
    { key: 'device', label: 'device' },
    { key: 'label', label: 'label' },
    { key: 'trust', label: 'trust' },
    { key: 'sync', label: 'sync' },
    { key: 'devicePubkeyShort', label: 'pubkey' },
    { key: 'lastHandshake', label: 'last handshake' },
    { key: 'locks', label: 'locks h/p' },
    { key: 'events24h', label: 'events 24h' },
];

function trustTone(trust: PeerViewItem['trust']): string {
    if (trust === 'stale') return 'warn';
    if (trust === 'unknown') return 'info';
    return 'info';
}

function syncTone(sync: PeerViewItem['sync']): string {
    if (sync === 'stale') return 'warn';
    if (sync === 'unknown') return 'info';
    return 'ok';
}

export function TrustLedger({ peers, selectedId, sort, onSort, onSelect }: TrustLedgerProps) {
    return (
        <>
            <div
                className="pr-table-head"
                data-testid="peer-ledger-head"
            >
                <span>·</span>
                {columns.map((column) => (
                    <button
                        key={column.key}
                        aria-label={`Sort by ${column.label}${sort.key === column.key ? ` ${sort.dir}` : ''}`}
                        aria-pressed={sort.key === column.key}
                        className={`th ${sort.key === column.key ? 'active' : ''}`}
                        onClick={() => onSort(column.key)}
                        type="button"
                    >
                        <span>{column.label}</span>
                        {sort.key === column.key ? (
                            <span className="th-arrow mono">{sort.dir === 'asc' ? '↑' : '↓'}</span>
                        ) : null}
                    </button>
                ))}
            </div>
            <div className="pane-scroll">
                {peers.map((peer) => (
                    <button
                        key={peer.id}
                        className={`pr-row ${selectedId === peer.id ? 'selected' : ''}`}
                        data-testid="peer-row"
                        onClick={() => onSelect(peer.id)}
                        type="button"
                    >
                        <span className="pr-reach">
                            <span className={`status-dot ${peer.reachable ? 'ok' : 'idle'}`} />
                        </span>
                        <span className="pr-device mono">{peer.device}</span>
                        <span className="pr-label">{peer.label}</span>
                        <span className={`badge ${trustTone(peer.trust)}`}>{peer.trust}</span>
                        <span className={`badge ${syncTone(peer.sync)}`}>{peer.sync}</span>
                        <span className="pr-key mono">{peer.devicePubkeyShort}</span>
                        <span className="pr-hand mono">{peer.lastHandshake}</span>
                        <span className="pr-locks mono">
                            {peer.locksHeld}/{peer.locksPending ?? '—'}
                        </span>
                        <span className="pr-tx mono">
                            ↓{peer.eventsIn24h ?? '—'} ↑{peer.eventsOut24h ?? '—'}
                        </span>
                    </button>
                ))}
            </div>
        </>
    );
}
