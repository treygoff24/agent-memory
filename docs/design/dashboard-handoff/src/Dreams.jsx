// Dreams: inbox-style list (proposals, runs) + inspector with EVIDENCE card
const { useState: useStateD, useMemo: useMemoD } = React;

function DreamRow({ item, selected, onSelect }) {
    const stClass = 'dream-status ' + (item.status || '');
    return (
        <div
            className={'list-item' + (selected ? ' selected' : '')}
            tabIndex={0}
            onClick={() => onSelect(item.id)}
        >
            <span className={'glyph ' + (item.kind === 'dream_run' ? 'run' : 'dream')}>{item.glyph}</span>
            <div className="li-main">
                <div className="li-title">{item.title}</div>
                <div className="li-sub">
                    <span className="ns">{item.namespace}</span>
                    {item.sub.map((s, i) => (
                        <React.Fragment key={i}>
                            <span className="dot">·</span>
                            <span>{s}</span>
                        </React.Fragment>
                    ))}
                </div>
            </div>
            <span className={stClass}>{item.status}</span>
            {item.confidence != null && <span className="li-meta mono">{item.confidence.toFixed(2)}</span>}
            <span className="li-meta">{item.meta}</span>
        </div>
    );
}

function DreamsView({ items, selectedId, onSelect, onAct }) {
    const [filter, setFilter] = useStateD('all');
    const visible = useMemoD(
        () => (filter === 'all' ? items : items.filter((i) => i.status === filter)),
        [items, filter],
    );
    const sel = visible.find((i) => i.id === selectedId) || visible[0];

    const counts = {
        all: items.length,
        proposed: items.filter((i) => i.status === 'proposed').length,
        queued: items.filter((i) => i.status === 'queued').length,
        accepted: items.filter((i) => i.status === 'accepted').length,
        dismissed: items.filter((i) => i.status === 'dismissed').length,
        running: items.filter((i) => i.status === 'running').length,
    };

    return (
        <>
            <div className="view-header">
                <span className="view-title">Dreams</span>
                <span className="view-subtitle">· {items.length} · last run 03:04 today</span>
                <span className="spacer" />
                <div className="filter-pills">
                    {[
                        ['all', 'all', counts.all],
                        ['proposed', 'proposed', counts.proposed],
                        ['queued', 'queued', counts.queued],
                        ['accepted', 'accepted', counts.accepted],
                        ['dismissed', 'dismissed', counts.dismissed],
                        ['running', 'running', counts.running],
                    ].map(([id, label, n], i) => (
                        <button
                            key={id}
                            className={'pill' + (filter === id ? ' active' : '')}
                            onClick={() => setFilter(id)}
                        >
                            <span>{label}</span>
                            <span className={'count' + (filter === id ? ' accent' : '')}>{n}</span>
                            <span className="kbd-hint">{i + 1}</span>
                        </button>
                    ))}
                </div>
            </div>

            <div className="panes-2">
                <div className="pane left">
                    <div className="pane-scroll">
                        <div className="list">
                            {visible.map((it) => (
                                <DreamRow
                                    key={it.id}
                                    item={it}
                                    selected={sel && sel.id === it.id}
                                    onSelect={onSelect}
                                />
                            ))}
                        </div>
                    </div>
                </div>
                <div className="pane">
                    <div className="pane-scroll">
                        <DreamInspectorEx
                            item={sel}
                            layout="narrow"
                            onAct={onAct}
                        />
                    </div>
                </div>
            </div>
        </>
    );
}

function DreamInspectorEx({ item, layout, onAct }) {
    if (!item)
        return (
            <div
                className="empty"
                style={{ paddingTop: 120 }}
            >
                <span className="ico">○</span>
                <h3>No dream selected</h3>
                <p>Pick a row to see the evidence chain.</p>
            </div>
        );

    if (item.kind === 'dream_run') {
        return (
            <div className="inspector">
                <div className="insp-head">
                    <span className="insp-title">{item.title}</span>
                    <span className="insp-scope">{item.namespace}</span>
                    <span className="insp-badges">
                        <span className={'badge ' + (item.status === 'running' ? 'info' : '')}>{item.status}</span>
                    </span>
                </div>
                <div className={'insp-grid' + (layout === 'narrow' ? ' narrow' : '')}>
                    <div>
                        <div className="section-label">Run summary</div>
                        <p className="body-text">
                            Dream pass <span className="mono">{item.pass || 'all'}</span>{' '}
                            {item.status === 'running' ? 'is running now' : 'completed'}.
                            {item.duration && ` Duration ${item.duration}.`} The pass synthesizes patterns across recent
                            memories, queues low-confidence findings as questions, and auto-promotes high-confidence
                            cleanups.
                        </p>
                        <div className="section-label">Stages</div>
                        <div className="dream-stages">
                            <div className="stage done">
                                <span className="mark">✓</span> ingest · 412 memories scanned
                            </div>
                            <div className="stage done">
                                <span className="mark">✓</span> cluster · 18 candidate patterns
                            </div>
                            <div className="stage done">
                                <span className="mark">✓</span> score · 7 above 0.60
                            </div>
                            <div className={item.status === 'running' ? 'stage now' : 'stage done'}>
                                <span className="mark">{item.status === 'running' ? '▸' : '✓'}</span> propose ·{' '}
                                {item.status === 'running' ? 'in progress' : '3 promoted, 1 queued'}
                            </div>
                            <div className={item.status === 'running' ? 'stage queued' : 'stage done'}>
                                <span className="mark">·</span> close out
                            </div>
                        </div>
                    </div>
                    <div className="sidecar">
                        <div className="card">
                            <div className="card-head">
                                <span>Run</span>
                            </div>
                            <dl className="kv">
                                <dt>started</dt>
                                <dd>{item.meta}</dd>
                                {item.duration && (
                                    <>
                                        <dt>duration</dt>
                                        <dd className="mono">{item.duration}</dd>
                                    </>
                                )}
                                <dt>scope</dt>
                                <dd>{item.namespace}</dd>
                                <dt>pass</dt>
                                <dd className="mono">{item.pass || 'all'}</dd>
                            </dl>
                        </div>
                    </div>
                </div>
            </div>
        );
    }

    return (
        <div className="inspector">
            <div className="insp-head">
                <span className="insp-title">{item.title}</span>
                <span className="insp-scope">{item.namespace}</span>
                <span className="insp-badges">
                    <span
                        className={
                            'badge ' + (item.status === 'accepted' ? '' : item.status === 'dismissed' ? 'bad' : 'warn')
                        }
                    >
                        {item.status}
                    </span>
                    {item.confidence < 0.8 && <span className="badge">low confidence</span>}
                </span>
            </div>
            <div
                style={{ padding: '4px 0 0', color: 'var(--fg-3)', fontFamily: 'var(--font-mono)', fontSize: '10.5px' }}
            >
                id {item.id} · pass {item.pass}
            </div>

            <div className={'insp-grid' + (layout === 'narrow' ? ' narrow' : '')}>
                <div>
                    <div className="section-label">Pattern</div>
                    <p className="body-text">
                        Dream pass <span className="mono">{item.pass}</span> synthesized this{' '}
                        {item.status === 'queued' ? 'question' : 'proposal'} from {item.evidence?.length || 0} memories.
                        Confidence {item.confidence.toFixed(2)}{' '}
                        {item.confidence >= 0.8
                            ? '≥ 0.80 (above auto-promote threshold)'
                            : '< 0.80 (queued for review)'}
                        .
                    </p>

                    <div className="section-label">
                        Evidence <span className="meta">{item.evidence?.length || 0} memories</span>
                    </div>
                    <div className="evidence-list">
                        {(item.evidence || []).map((e) => (
                            <div
                                key={e.id}
                                className={'ev-row' + (e.superseded ? ' superseded' : '')}
                            >
                                <a
                                    href="#"
                                    className="mono ev-id"
                                >
                                    {e.id}
                                </a>
                                <span className="ev-title">{e.title}</span>
                                <span className="ev-weight mono">{e.weight.toFixed(2)}</span>
                                <span className="ev-bar">
                                    <span
                                        className="ev-fill"
                                        style={{ width: e.weight * 100 + '%' }}
                                    />
                                </span>
                            </div>
                        ))}
                    </div>

                    {item.status !== 'accepted' && item.status !== 'dismissed' && (
                        <div className="action-bar">
                            <button
                                className="btn primary"
                                onClick={() => onAct && onAct('promote')}
                            >
                                <span className="key">p</span>Promote
                            </button>
                            <button
                                className="btn"
                                onClick={() => onAct && onAct('queue-q')}
                            >
                                <span className="key">q</span>Queue question
                            </button>
                            <button
                                className="btn danger"
                                onClick={() => onAct && onAct('dismiss')}
                            >
                                <span className="key">x</span>Dismiss
                            </button>
                        </div>
                    )}
                </div>
                <div className="sidecar">
                    <div className="card">
                        <div className="card-head">
                            <span>Dream pass</span>
                        </div>
                        <dl className="kv">
                            <dt>run at</dt>
                            <dd>{item.run_at}</dd>
                            <dt>pass</dt>
                            <dd className="mono">{item.pass}</dd>
                            <dt>confidence</dt>
                            <dd className="mono">{item.confidence.toFixed(2)}</dd>
                            <dt>status</dt>
                            <dd>{item.status}</dd>
                            <dt>scope</dt>
                            <dd>{item.namespace}</dd>
                        </dl>
                    </div>
                    <div className="card">
                        <div className="card-head">
                            <span>Evidence summary</span>
                        </div>
                        <dl className="kv">
                            <dt>memories</dt>
                            <dd className="mono">{item.evidence?.length || 0}</dd>
                            <dt>weight max</dt>
                            <dd className="mono">
                                {item.evidence ? Math.max(...item.evidence.map((e) => e.weight)).toFixed(2) : '—'}
                            </dd>
                            <dt>weight min</dt>
                            <dd className="mono">
                                {item.evidence ? Math.min(...item.evidence.map((e) => e.weight)).toFixed(2) : '—'}
                            </dd>
                        </dl>
                    </div>
                </div>
            </div>
        </div>
    );
}

Object.assign(window, { DreamsView, DreamInspectorEx });
