import { StatusDot } from '../../ui';
import type { PeerViewItem } from '../Peers';

function trustTone(trust: PeerViewItem['trust']): 'ok' | 'warn' | 'bad' {
    if (trust === 'revoked') return 'bad';
    if (trust === 'limited') return 'warn';
    return 'ok';
}

function syncLabel(sync: PeerViewItem['sync']): string {
    if (sync === 'in-sync') return 'in sync';
    if (sync === 'behind') return 'behind';
    if (sync === 'fenced') return 'fenced';
    return 'revoked';
}

function reachabilityDot(peer: PeerViewItem): 'ok' | 'warn' | 'idle' {
    if (!peer.reachable) return 'idle';
    if (peer.sync === 'behind') return 'warn';
    return 'ok';
}

function reachabilityLabel(peer: PeerViewItem): string {
    if (!peer.reachable) return 'offline';
    if (peer.lastHandshake.endsWith('d') && parseInt(peer.lastHandshake) > 1) return 'stale';
    return 'online';
}

interface PeerCardProps {
    peer: PeerViewItem;
    selected: boolean;
    onSelect: (id: string) => void;
}

export function PeerCard({ peer, selected, onSelect }: PeerCardProps) {
    const dotKind = reachabilityDot(peer);
    const reach = reachabilityLabel(peer);

    return (
        <button
            aria-pressed={selected}
            className={`peer-card ${selected ? 'selected' : ''}`}
            data-testid="peer-card"
            onClick={() => onSelect(peer.id)}
            type="button"
        >
            {/* Header row */}
            <div className="peer-card-header">
                <StatusDot kind={dotKind} />
                <span className="peer-card-device mono">{peer.device}</span>
                <span className={`badge ${trustTone(peer.trust)}`}>{peer.trust}</span>
                <span className="peer-card-reach">{reach}</span>
            </div>

            {/* Body rows */}
            <div className="peer-card-body">
                <div className="peer-card-row">
                    <span className="peer-card-key">harness</span>
                    <span className="peer-card-val">{peer.label}</span>
                </div>
                <div className="peer-card-row">
                    <span className="peer-card-key">last heartbeat</span>
                    <span className="peer-card-val mono">{peer.lastHandshake}</span>
                </div>
                <div className="peer-card-row">
                    <span className="peer-card-key">sessions</span>
                    <span className="peer-card-val mono">{peer.sessionsOpen > 0 ? peer.sessionsOpen : '—'}</span>
                </div>
                <div className="peer-card-row">
                    <span className="peer-card-key">claim locks</span>
                    <span className="peer-card-val mono">
                        {peer.locksHeld}h / {peer.locksPending}p
                    </span>
                </div>
                <div className="peer-card-row">
                    <span className="peer-card-key">updates 24h</span>
                    <span className="peer-card-val mono">
                        ↓{peer.eventsIn24h} ↑{peer.eventsOut24h}
                    </span>
                </div>
                <div className="peer-card-row">
                    <span className="peer-card-key">sync</span>
                    <span className="peer-card-val">{syncLabel(peer.sync)}</span>
                </div>
            </div>

            {/* Pubkey footer */}
            <div className="peer-card-footer">
                <span className="peer-card-pubkey mono">{peer.devicePubkeyShort}</span>
            </div>
        </button>
    );
}
