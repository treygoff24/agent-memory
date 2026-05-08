import { peers } from '../data/fixtures';
import { Badge } from '../ui';
export function Peers() {
    return (
        <>
            <div className="view-header">
                <span className="view-title">Peers</span>
                <span className="view-subtitle">· trust ledger</span>
                <span className="spacer" />
                <button className="btn primary">+ pair new device</button>
            </div>
            <div className="ent-table">
                <div className="ent-thead">
                    <span>device</span>
                    <span>trust</span>
                    <span>sync</span>
                    <span>locks h/p</span>
                    <span>events 24h</span>
                </div>
                {peers.map((peer) => (
                    <div
                        className="ent-row"
                        key={peer.id}
                    >
                        <span>{peer.label}</span>
                        <Badge tone={peer.trust === 'revoked' ? 'bad' : 'ok'}>{peer.trust}</Badge>
                        <Badge tone={peer.sync === 'behind' ? 'warn' : peer.sync === 'revoked' ? 'bad' : 'ok'}>
                            {peer.sync}
                        </Badge>
                        <span className="mono">
                            {peer.locksHeld}/{peer.locksPending}
                        </span>
                        <span className="mono">{peer.events24h}</span>
                    </div>
                ))}
            </div>
        </>
    );
}
