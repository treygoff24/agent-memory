// Inspector — adapts to item kind
const { useState: useStateI } = React;

function Sparkline({ data, height = 28 }) {
    const max = Math.max(...data);
    return (
        <div
            className="sparkbars"
            style={{ height }}
        >
            {data.map((v, i) => {
                const h = Math.max(1, Math.round((v / max) * height));
                return (
                    <div
                        key={i}
                        className={'b' + (v >= max * 0.85 ? ' tall' : '')}
                        style={{ height: h }}
                        title={`day -${data.length - i}: ${v} recalls`}
                    />
                );
            })}
        </div>
    );
}

function ProvenanceCard({ p }) {
    return (
        <div className="card">
            <div className="card-head">
                <span>Provenance · 2 entries</span>
            </div>
            <dl className="kv">
                <dt>written</dt>
                <dd>{p.written}</dd>
                <dt>session</dt>
                <dd>
                    <a
                        href="#"
                        className="mono"
                    >
                        {p.session}
                    </a>
                </dd>
                <dt>grounding</dt>
                <dd>
                    <a
                        href="#"
                        className="mono"
                    >
                        {p.grounding}
                    </a>
                </dd>
                <dt>confidence</dt>
                <dd className="mono">{p.confidence}</dd>
                <dt>device</dt>
                <dd className="mono">{p.device}</dd>
                <dt>peers seen</dt>
                <dd className="mono">{p.peers}</dd>
            </dl>
        </div>
    );
}

function PolicyCard({ p }) {
    return (
        <div className="card">
            <div className="card-head">
                <span>Policy</span>
            </div>
            <dl className="kv">
                <dt>privacy</dt>
                <dd>{p.privacy}</dd>
                <dt>governance</dt>
                <dd>{p.governance}</dd>
                <dt>tombstone</dt>
                <dd>{p.tombstone}</dd>
            </dl>
        </div>
    );
}

function PrivacyCard() {
    return (
        <div className="card">
            <div className="card-head">
                <span>Privacy scan</span>
                <span className="card-meta">0 labels</span>
            </div>
            <div style={{ color: 'var(--fg-3)', fontFamily: 'var(--font-mono)', fontSize: 'var(--text-xs)' }}>
                no sensitive labels detected · storage action: plaintext
            </div>
        </div>
    );
}

function ReviewInspector({ item, onAct, layout }) {
    const sidecarInline = layout === 'narrow';
    return (
        <div className="inspector">
            <div className="insp-head">
                <span className="insp-title">{item.title}</span>
                <span className="insp-scope">{item.namespace}</span>
                <span className="insp-badges">
                    <span className="badge warn">candidate</span>
                    <span className="badge">{item.sensitivity || 'plaintext'}</span>
                    {item.encrypted && <span className="badge encrypted">encrypted at rest</span>}
                </span>
            </div>
            <div
                style={{ padding: '4px 0 0', color: 'var(--fg-3)', fontFamily: 'var(--font-mono)', fontSize: '10.5px' }}
            >
                id {item.id.slice(0, 28)}…
            </div>
            <div style={{ display: 'none' }}></div>

            <div className={'insp-grid' + (sidecarInline ? ' narrow' : '')}>
                <div>
                    <div className="section-label">Body</div>
                    <p
                        className="body-text"
                        dangerouslySetInnerHTML={{ __html: (item.body || '').replace(/`([^`]+)`/g, '<code>$1</code>') }}
                    />

                    <div className="section-label">
                        Recall{' '}
                        <span className="meta">
                            total {item.recall_count_total ?? 0} · 30d {item.recall_count_30d ?? 0}
                        </span>
                    </div>
                    {!item.recalls || item.recalls.length === 0 ? (
                        <div className="recall-list">
                            <span className="none">No recall events yet.</span>
                        </div>
                    ) : (
                        <div className="recall-list">
                            {item.recalls.map((r, i) => (
                                <React.Fragment key={i}>
                                    <span className="when">{r.when}</span>
                                    <span className="who">{r.who}</span>
                                    <a
                                        href="#"
                                        className="ses"
                                    >
                                        {r.session}
                                    </a>
                                </React.Fragment>
                            ))}
                        </div>
                    )}

                    <div className="section-label">30-day recall</div>
                    {(() => {
                        const sd = item.spark || [
                            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        ];
                        const sum = sd.reduce((a, b) => a + b, 0);
                        return sum === 0 ? <div className="sparkbars-empty" /> : <Sparkline data={sd} />;
                    })()}

                    <div className="action-bar">
                        <button
                            className="btn primary"
                            onClick={() => onAct('approve')}
                        >
                            <span className="key">a</span>Accept
                        </button>
                        <button
                            className="btn"
                            onClick={() => onAct('reject')}
                        >
                            <span className="key">r</span>Reject
                        </button>
                        <button
                            className="btn"
                            onClick={() => onAct('edit')}
                        >
                            <span className="key">e</span>Edit
                        </button>
                        <button
                            className="btn danger"
                            onClick={() => onAct('forget')}
                        >
                            <span className="key">f</span>Forget
                        </button>
                    </div>
                </div>

                <div className="sidecar">
                    <ProvenanceCard
                        p={
                            item.provenance || {
                                written: '2m ago by claude-code',
                                session: 'a8b3f2c',
                                grounding: 'commit 8a3f2c',
                                confidence: item.confidence || '0.84',
                                device: 'mbp',
                                peers: '0 of 2',
                            }
                        }
                    />
                    <PolicyCard
                        p={item.policy || { privacy: 'plaintext', governance: 'auto-approve', tombstone: 'none' }}
                    />
                    <PrivacyCard />
                </div>
            </div>
        </div>
    );
}

function RecallInspector({ item, layout }) {
    return (
        <div className="inspector">
            <div className="insp-head">
                <span className="insp-title">Recall: {item.summary || item.title}</span>
                <span className="insp-scope">{item.namespace}</span>
                <span className="insp-badges">
                    <span className="badge info">recall event</span>
                </span>
            </div>
            <div className={'insp-grid' + (layout === 'narrow' ? ' narrow' : '')}>
                <div>
                    <div className="section-label">Surrounding context</div>
                    <p className="body-text">
                        {item.surroundingContext || 'Agent retrieved this memory while drafting in the listed session.'}
                    </p>
                    <div className="section-label">Memory recalled</div>
                    <div className="card">
                        <div className="card-head">
                            <a
                                href="#"
                                className="mono"
                                style={{ color: 'var(--info)' }}
                            >
                                {item.memory_id || 'mem_…'}
                            </a>
                            <span className="card-meta">open trust artifact →</span>
                        </div>
                        <div
                            className="body-text"
                            style={{ fontSize: 'var(--text-sm)' }}
                        >
                            {item.summary || item.title}
                        </div>
                    </div>
                </div>
                <div className="sidecar">
                    <div className="card">
                        <div className="card-head">
                            <span>Event</span>
                        </div>
                        <dl className="kv">
                            <dt>recalled</dt>
                            <dd>12m ago</dd>
                            <dt>session</dt>
                            <dd>
                                <a
                                    href="#"
                                    className="mono"
                                >
                                    {item.sessionId || 'a8b3f2c'}
                                </a>
                            </dd>
                            <dt>device</dt>
                            <dd className="mono">mbp</dd>
                            <dt>seq</dt>
                            <dd className="mono">1281</dd>
                        </dl>
                    </div>
                </div>
            </div>
        </div>
    );
}

function ConflictInspector({ item, layout, onResolve }) {
    const d = item.diff;
    return (
        <div className="inspector">
            <div className="insp-head">
                <span className="insp-title">{item.title}</span>
                <span className="insp-scope">{item.namespace}</span>
                <span className="insp-badges">
                    <span className="badge bad">merge conflict</span>
                </span>
            </div>
            <div className={'insp-grid' + (layout === 'narrow' ? ' narrow' : '')}>
                <div>
                    <div className="section-label">Sides</div>
                    <div className="diff">
                        <div>
                            <div className="side-head local">local · {d.local.device}</div>
                            <div className="body">{d.local.body}</div>
                            <div className="meta">
                                {d.local.written} · session <a href="#">{d.local.session}</a>
                            </div>
                        </div>
                        <div>
                            <div className="side-head remote">remote · {d.remote.device}</div>
                            <div className="body">{d.remote.body}</div>
                            <div className="meta">
                                {d.remote.written} · session <a href="#">{d.remote.session}</a>
                            </div>
                        </div>
                    </div>

                    <div className="action-bar">
                        <button
                            className="btn"
                            onClick={() => onResolve('local')}
                        >
                            <span className="key">1</span>Keep local
                        </button>
                        <button
                            className="btn"
                            onClick={() => onResolve('remote')}
                        >
                            <span className="key">2</span>Keep remote
                        </button>
                        <button
                            className="btn primary"
                            onClick={() => onResolve('merge')}
                        >
                            <span className="key">m</span>Custom merge…
                        </button>
                    </div>
                </div>
                <div className="sidecar">
                    <div className="card">
                        <div className="card-head">
                            <span>Conflict reason</span>
                        </div>
                        <div style={{ fontSize: 'var(--text-sm)', color: 'var(--fg-2)', lineHeight: 1.5 }}>
                            Both devices wrote to{' '}
                            <span
                                className="mono"
                                style={{ color: 'var(--fg)' }}
                            >
                                prefs/editor
                            </span>{' '}
                            after the last common ancestor. Stream-I claim lock did not arbitrate.
                        </div>
                    </div>
                    <div className="card">
                        <div className="card-head">
                            <span>Last common ancestor</span>
                        </div>
                        <dl className="kv">
                            <dt>at</dt>
                            <dd>2026-04-12</dd>
                            <dt>body</dt>
                            <dd style={{ whiteSpace: 'normal', color: 'var(--fg-2)' }}>Primary editor is Helix.</dd>
                        </dl>
                    </div>
                </div>
            </div>
        </div>
    );
}

function DueInspector({ item, layout }) {
    return (
        <div className="inspector">
            <div className="insp-head">
                <span className="insp-title">{item.title}</span>
                <span className="insp-scope">{item.namespace}</span>
                <span className="insp-badges">
                    <span className="badge warn">due for verify</span>
                </span>
            </div>
            <div className={'insp-grid' + (layout === 'narrow' ? ' narrow' : '')}>
                <div>
                    <div className="section-label">
                        Reality-check score <span className="meta">0.82 · would be next in session</span>
                    </div>
                    <div className="scorebars">
                        <div className="row">
                            <span className="label">days_since_observed_norm</span>
                            <span className="track">
                                <span
                                    className="fill"
                                    style={{ width: '91%' }}
                                />
                            </span>
                            <span className="val">0.91</span>
                        </div>
                        <div className="row">
                            <span className="label">recall_frequency_norm</span>
                            <span className="track">
                                <span
                                    className="fill"
                                    style={{ width: '45%' }}
                                />
                            </span>
                            <span className="val">0.45</span>
                        </div>
                        <div className="row">
                            <span className="label">cross_source_corroboration</span>
                            <span className="track">
                                <span
                                    className="fill"
                                    style={{ width: '20%' }}
                                />
                            </span>
                            <span className="val">0.20</span>
                        </div>
                        <div className="row">
                            <span className="label">confidence_decay</span>
                            <span className="track">
                                <span
                                    className="fill"
                                    style={{ width: '62%' }}
                                />
                            </span>
                            <span className="val">0.62</span>
                        </div>
                        <div className="row">
                            <span className="label">sensitivity_weight</span>
                            <span className="track">
                                <span
                                    className="fill"
                                    style={{ width: '100%' }}
                                />
                            </span>
                            <span className="val">1.00</span>
                        </div>
                    </div>

                    <div className="action-bar">
                        <button className="btn primary">
                            <span className="key">v</span>Verify now
                        </button>
                        <button className="btn">
                            <span className="key">s</span>Skip 30d
                        </button>
                    </div>
                </div>
                <div className="sidecar">
                    <div className="card">
                        <div className="card-head">
                            <span>Memory</span>
                        </div>
                        <dl className="kv">
                            <dt>last verified</dt>
                            <dd>92 days ago</dd>
                            <dt>last recalled</dt>
                            <dd>8d ago</dd>
                            <dt>recall 30d</dt>
                            <dd className="mono">4</dd>
                        </dl>
                    </div>
                </div>
            </div>
        </div>
    );
}

function DreamInspector({ item, layout }) {
    return (
        <div className="inspector">
            <div className="insp-head">
                <span className="insp-title">{item.title}</span>
                <span className="insp-scope">{item.namespace}</span>
                <span className="insp-badges">
                    <span className="badge warn">dream · low confidence</span>
                </span>
            </div>
            <div className={'insp-grid' + (layout === 'narrow' ? ' narrow' : '')}>
                <div>
                    <div className="section-label">Pattern</div>
                    <p className="body-text">
                        Across {item.evidence?.length || 3} memories about programming language preference, the dream
                        pass detected a consistent leaning toward Rust for systems-level work, with explicit
                        dissatisfaction with Go's garbage collector and runtime model. Confidence is below auto-promote
                        threshold ({(item.confidence || 0.62).toFixed(2)} &lt; 0.80).
                    </p>
                    <div className="section-label">
                        Evidence <span className="meta">{item.evidence?.length || 3} memories</span>
                    </div>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                        {(
                            item.evidence || [
                                { id: 'mem_…ts0001', title: 'Rewrote concurrency layer in Rust' },
                                { id: 'mem_…go0007', title: 'Go GC pauses irritating in atlasos' },
                                { id: 'mem_…rs0014', title: "Liked tokio's runtime model" },
                            ]
                        ).map((e) => (
                            <div
                                key={e.id}
                                style={{
                                    display: 'grid',
                                    gridTemplateColumns: 'max-content 1fr auto',
                                    gap: 12,
                                    fontSize: 'var(--text-sm)',
                                    padding: '6px 0',
                                    borderBottom: '1px solid var(--border-soft)',
                                }}
                            >
                                <a
                                    href="#"
                                    className="mono"
                                    style={{ color: 'var(--info)' }}
                                >
                                    {e.id}
                                </a>
                                <span style={{ color: 'var(--fg-2)' }}>{e.title}</span>
                                <span
                                    className="mono"
                                    style={{ color: 'var(--fg-3)', fontVariantNumeric: 'tabular-nums' }}
                                >
                                    0.{60 + (e.title.length % 30)}
                                </span>
                            </div>
                        ))}
                    </div>

                    <div className="action-bar">
                        <button className="btn primary">
                            <span className="key">p</span>Promote
                        </button>
                        <button className="btn">
                            <span className="key">q</span>Queue question
                        </button>
                        <button className="btn danger">
                            <span className="key">x</span>Dismiss
                        </button>
                    </div>
                </div>
                <div className="sidecar">
                    <div className="card">
                        <div className="card-head">
                            <span>Dream pass</span>
                        </div>
                        <dl className="kv">
                            <dt>run at</dt>
                            <dd>03:00 today</dd>
                            <dt>confidence</dt>
                            <dd className="mono">{(item.confidence || 0.62).toFixed(2)}</dd>
                            <dt>scope</dt>
                            <dd>meta/preferences</dd>
                        </dl>
                    </div>
                </div>
            </div>
        </div>
    );
}

function Inspector({ item, layout = 'wide', onAct }) {
    if (!item) {
        return (
            <div
                className="empty"
                style={{ paddingTop: 120 }}
            >
                <span className="ico">○</span>
                <h3>Nothing selected</h3>
                <p>Pick a row to inspect it. Use ↑↓ to navigate, enter to focus.</p>
            </div>
        );
    }
    switch (item.kind) {
        case 'recall':
            return (
                <RecallInspector
                    item={item}
                    layout={layout}
                />
            );
        case 'conflict':
            return (
                <ConflictInspector
                    item={item}
                    layout={layout}
                    onResolve={onAct || (() => {})}
                />
            );
        case 'due':
            return (
                <DueInspector
                    item={item}
                    layout={layout}
                />
            );
        case 'dream':
            return (
                <DreamInspector
                    item={item}
                    layout={layout}
                />
            );
        default:
            return (
                <ReviewInspector
                    item={item}
                    onAct={onAct || (() => {})}
                    layout={layout}
                />
            );
    }
}

Object.assign(window, { Inspector, Sparkline });
