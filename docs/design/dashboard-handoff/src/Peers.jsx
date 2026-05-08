// Peers: trust ledger table + per-peer inspector with Sessions / Claim Locks cards
const { useState: useStateP } = React;

function PeerRow({ p, selected, onSelect }) {
    const trustBadge = {
        trusted: { cls: 'info', label: 'trusted' },
        limited: { cls: 'warn', label: 'limited' },
        revoked: { cls: 'bad', label: 'revoked' },
    }[p.trust] || { cls: '', label: p.trust };
    const syncBadge = {
        'in-sync': { cls: 'ok', label: 'in-sync' },
        behind: { cls: 'warn', label: 'behind' },
        fenced: { cls: 'info', label: 'fenced' },
        revoked: { cls: 'bad', label: 'revoked' },
    }[p.sync_status] || { cls: '', label: p.sync_status };
    return (
        <div
            className={'pr-row' + (selected ? ' selected' : '')}
            tabIndex={0}
            onClick={() => onSelect(p.id)}
        >
            <span className="pr-reach">
                <span className={'status-dot ' + (p.reachable ? 'ok' : 'idle')} />
            </span>
            <span className="pr-device mono">{p.device}</span>
            <span className="pr-label">{p.label}</span>
            <span className={'badge ' + trustBadge.cls}>{trustBadge.label}</span>
            <span className={'badge ' + syncBadge.cls}>{syncBadge.label}</span>
            <span className="pr-key mono">{p.device_pubkey_short}</span>
            <span className="pr-hand mono">{p.last_handshake}</span>
            <span className="pr-locks mono">
                {p.claim_locks_held}/{p.claim_locks_pending}
            </span>
            <span className="pr-tx mono">
                ↓{p.events_in_24h} ↑{p.events_out_24h}
            </span>
        </div>
    );
}

function PeersView({ peers, selectedId, onSelect }) {
    const sel = peers.find((p) => p.id === selectedId) || peers[0];
    return (
        <>
            <div className="view-header">
                <span className="view-title">Peers</span>
                <span className="view-subtitle">
                    · {peers.length} known · {peers.filter((p) => p.reachable).length} reachable ·{' '}
                    {peers.filter((p) => p.trust === 'trusted').length} trusted
                </span>
                <span className="spacer" />
                <button className="btn">+ pair new device</button>
            </div>

            <div className="panes-2">
                <div className="pane left">
                    <div className="pr-table-head">
                        <span>·</span>
                        <span>device</span>
                        <span>label</span>
                        <span>trust</span>
                        <span>sync</span>
                        <span>pubkey</span>
                        <span>last handshake</span>
                        <span>locks h/p</span>
                        <span>events 24h</span>
                    </div>
                    <div className="pane-scroll">
                        {peers.map((p) => (
                            <PeerRow
                                key={p.id}
                                p={p}
                                selected={sel && sel.id === p.id}
                                onSelect={onSelect}
                            />
                        ))}
                    </div>
                </div>
                <div className="pane">
                    <div className="pane-scroll">
                        <PeerInspector
                            p={sel}
                            layout="narrow"
                        />
                    </div>
                </div>
            </div>
        </>
    );
}

function PeerInspector({ p, layout }) {
    if (!p)
        return (
            <div className="empty">
                <span className="ico">○</span>
                <h3>No peer selected</h3>
            </div>
        );
    return (
        <div className="inspector">
            <div className="insp-head">
                <span className="insp-title">{p.label}</span>
                <span className="insp-scope">{p.device}</span>
                <span className="insp-badges">
                    <span
                        className={'badge ' + (p.trust === 'trusted' ? 'info' : p.trust === 'revoked' ? 'bad' : 'warn')}
                    >
                        {p.trust}
                    </span>
                    <span
                        className={
                            'badge ' + (p.sync_status === 'in-sync' ? '' : p.sync_status === 'revoked' ? 'bad' : 'warn')
                        }
                    >
                        {p.sync_status}
                    </span>
                </span>
            </div>
            <div
                style={{ padding: '4px 0 0', color: 'var(--fg-3)', fontFamily: 'var(--font-mono)', fontSize: '10.5px' }}
            >
                {p.device_pubkey_short}
            </div>

            <div className={'insp-grid' + (layout === 'narrow' ? ' narrow' : '')}>
                <div>
                    {p.fence_reason && (
                        <>
                            <div className="section-label">Fence active</div>
                            <div
                                className="banner inline"
                                style={{ margin: '0 0 14px' }}
                            >
                                <span className="status-dot info" /> <span>{p.fence_reason}</span>
                            </div>
                        </>
                    )}
                    {p.revocation && (
                        <>
                            <div className="section-label">Revocation</div>
                            <div className="card">
                                <dl className="kv">
                                    <dt>revoked at</dt>
                                    <dd>{p.revocation.at}</dd>
                                    <dt>by</dt>
                                    <dd>{p.revocation.by}</dd>
                                    <dt>reason</dt>
                                    <dd style={{ whiteSpace: 'normal' }}>{p.revocation.reason}</dd>
                                </dl>
                            </div>
                        </>
                    )}

                    <div className="section-label">
                        Sessions <span className="meta">{p.sessions_open} open</span>
                    </div>
                    {p.sessions_open > 0 ? (
                        <div className="card">
                            <div className="card-head">
                                <span>Active sessions on this peer</span>
                            </div>
                            <div className="kv">
                                <dt>
                                    <span className="mono">a8b3f2c</span>
                                </dt>
                                <dd>claude-code · started 14m ago · 3 recalls</dd>
                                {p.sessions_open > 1 && (
                                    <>
                                        <dt>
                                            <span className="mono">d1e9f4a</span>
                                        </dt>
                                        <dd>codex-cli · started 2h ago · 11 recalls</dd>
                                    </>
                                )}
                            </div>
                        </div>
                    ) : (
                        <div className="card">
                            <div
                                style={{
                                    color: 'var(--fg-3)',
                                    fontFamily: 'var(--font-mono)',
                                    fontSize: 'var(--text-xs)',
                                }}
                            >
                                no sessions open on this peer
                            </div>
                        </div>
                    )}

                    <div className="section-label">
                        Claim locks{' '}
                        <span className="meta">
                            {p.claim_locks_held} held · {p.claim_locks_pending} pending
                        </span>
                    </div>
                    {p.claim_locks_held + p.claim_locks_pending > 0 ? (
                        <div className="card">
                            <div className="kv">
                                {p.claim_locks_held > 0 && (
                                    <>
                                        <dt>held</dt>
                                        <dd className="mono">
                                            prefs/editor, me/security/ssh, project:atlasos/migrations, work/clients/acme
                                        </dd>
                                    </>
                                )}
                                {p.claim_locks_pending > 0 && (
                                    <>
                                        <dt>pending</dt>
                                        <dd className="mono">personal/family (waiting on consent)</dd>
                                    </>
                                )}
                            </div>
                        </div>
                    ) : (
                        <div className="card">
                            <div
                                style={{
                                    color: 'var(--fg-3)',
                                    fontFamily: 'var(--font-mono)',
                                    fontSize: 'var(--text-xs)',
                                }}
                            >
                                no locks · this peer is read-write idle
                            </div>
                        </div>
                    )}

                    {p.trust !== 'revoked' && (
                        <div className="action-bar">
                            <button className="btn primary">
                                <span className="key">h</span>Force handshake
                            </button>
                            <button className="btn">
                                <span className="key">p</span>Pause sync
                            </button>
                            <button className="btn">
                                <span className="key">f</span>Fence namespace…
                            </button>
                            <button className="btn danger">
                                <span className="key">x</span>Revoke device
                            </button>
                        </div>
                    )}
                </div>

                <div className="sidecar">
                    <div className="card">
                        <div className="card-head">
                            <span>Connection</span>
                        </div>
                        <dl className="kv">
                            <dt>reachable</dt>
                            <dd>{p.reachable ? 'yes' : 'no'}</dd>
                            <dt>last handshake</dt>
                            <dd>{p.last_handshake}</dd>
                            <dt>handshake_ts</dt>
                            <dd className="mono">{p.last_handshake_ts.slice(0, 19)}Z</dd>
                            <dt>first paired</dt>
                            <dd className="mono">{p.first_paired}</dd>
                        </dl>
                    </div>
                    <div className="card">
                        <div className="card-head">
                            <span>Traffic 24h</span>
                        </div>
                        <dl className="kv">
                            <dt>events in</dt>
                            <dd className="mono">{p.events_in_24h}</dd>
                            <dt>events out</dt>
                            <dd className="mono">{p.events_out_24h}</dd>
                            <dt>bytes</dt>
                            <dd className="mono">{p.bytes_24h}</dd>
                        </dl>
                    </div>
                    <div className="card">
                        <div className="card-head">
                            <span>Disagreement</span>
                        </div>
                        <dl className="kv">
                            <dt>last</dt>
                            <dd>{p.last_disagreement}</dd>
                        </dl>
                    </div>
                </div>
            </div>
        </div>
    );
}

Object.assign(window, { PeersView, PeerInspector });
