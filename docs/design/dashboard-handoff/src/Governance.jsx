// Governance: inbox-pattern review queue + Policy Decision Trace inspector card + batch action bar
const { useState: useStateG, useMemo: useMemoG } = React;

function GovRow({ item, selected, onSelect, checked, onCheck }) {
    const sevDot = { block: 'bad', warn: 'warn', info: 'ok' }[item.severity] || 'ok';
    return (
        <div
            className={'gov-row list-item' + (selected ? ' selected' : '')}
            tabIndex={0}
            onClick={() => onSelect(item.id)}
        >
            <span
                className="gov-check"
                onClick={(e) => {
                    e.stopPropagation();
                    onCheck(item.id);
                }}
            >
                <input
                    type="checkbox"
                    checked={!!checked}
                    onChange={() => onCheck(item.id)}
                />
            </span>
            <span className={'glyph ' + (item.severity === 'block' ? 'conflict' : 'due')}>
                <span className={'status-dot ' + sevDot} />
            </span>
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
            <span className={'badge ' + (item.severity === 'block' ? 'bad' : item.severity === 'warn' ? 'warn' : '')}>
                {item.decision}
            </span>
            <span className="li-meta">{item.meta}</span>
        </div>
    );
}

function GovernanceView({ items, selectedId, onSelect, onAct }) {
    const [filter, setFilter] = useStateG('all');
    const [checked, setChecked] = useStateG(new Set());
    const visible = useMemoG(
        () => (filter === 'all' ? items : items.filter((i) => i.severity === filter || i.decision === filter)),
        [items, filter],
    );
    const sel = visible.find((i) => i.id === selectedId) || visible[0];

    const counts = {
        all: items.length,
        block: items.filter((i) => i.severity === 'block').length,
        warn: items.filter((i) => i.severity === 'warn').length,
        info: items.filter((i) => i.severity === 'info').length,
        consent_required: items.filter((i) => i.decision === 'consent_required').length,
        redact_proposed: items.filter((i) => i.decision === 'redact_proposed').length,
    };

    function toggle(id) {
        setChecked((s) => {
            const ns = new Set(s);
            if (ns.has(id)) ns.delete(id);
            else ns.add(id);
            return ns;
        });
    }
    function toggleAll() {
        if (checked.size === visible.length) setChecked(new Set());
        else setChecked(new Set(visible.map((i) => i.id)));
    }
    const allOn = checked.size === visible.length && visible.length > 0;
    const someOn = checked.size > 0;

    return (
        <>
            <div className="view-header">
                <span className="view-title">Governance</span>
                <span className="view-subtitle">
                    · review queue · {items.length} items · {counts.block} blocks
                </span>
                <span className="spacer" />
                <div className="filter-pills">
                    {[
                        ['all', 'all', counts.all],
                        ['block', 'blocks', counts.block],
                        ['warn', 'warnings', counts.warn],
                        ['info', 'info', counts.info],
                        ['consent_required', 'consent', counts.consent_required],
                        ['redact_proposed', 'redactions', counts.redact_proposed],
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

            {someOn && (
                <div className="batch-bar">
                    <span className="batch-count">
                        <span className="mono">{checked.size}</span> selected
                    </span>
                    <span className="sep">·</span>
                    <button
                        className="btn-link"
                        onClick={toggleAll}
                    >
                        {allOn ? 'deselect all' : `select all ${visible.length}`}
                    </button>
                    <span className="spacer" />
                    <button className="btn primary">
                        <span className="key">a</span>Approve selected
                    </button>
                    <button className="btn">
                        <span className="key">r</span>Reject selected
                    </button>
                    <button className="btn">
                        <span className="key">d</span>Defer 7d
                    </button>
                    <button className="btn danger">
                        <span className="key">x</span>Tombstone
                    </button>
                    <button
                        className="btn-link"
                        onClick={() => setChecked(new Set())}
                    >
                        clear
                    </button>
                </div>
            )}

            <div className="panes-2">
                <div className="pane left">
                    <div className="pane-scroll">
                        <div className="list">
                            {visible.map((it) => (
                                <GovRow
                                    key={it.id}
                                    item={it}
                                    selected={sel && sel.id === it.id}
                                    onSelect={onSelect}
                                    checked={checked.has(it.id)}
                                    onCheck={toggle}
                                />
                            ))}
                        </div>
                    </div>
                </div>
                <div className="pane">
                    <div className="pane-scroll">
                        <GovInspector
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

function GovInspector({ item, layout, onAct }) {
    if (!item)
        return (
            <div className="empty">
                <span className="ico">○</span>
                <h3>No item selected</h3>
            </div>
        );
    return (
        <div className="inspector">
            <div className="insp-head">
                <span className="insp-title">{item.title}</span>
                <span className="insp-scope">{item.namespace}</span>
                <span className="insp-badges">
                    <span
                        className={
                            'badge ' + (item.severity === 'block' ? 'bad' : item.severity === 'warn' ? 'warn' : '')
                        }
                    >
                        {item.severity}
                    </span>
                    <span className="badge">{item.decision}</span>
                    {item.sensitivity === 'sensitive' && <span className="badge">sensitive</span>}
                    {item.encrypted && <span className="badge encrypted">encrypted at rest</span>}
                </span>
            </div>
            <div
                style={{ padding: '4px 0 0', color: 'var(--fg-3)', fontFamily: 'var(--font-mono)', fontSize: '10.5px' }}
            >
                {item.id}
            </div>

            <div className={'insp-grid' + (layout === 'narrow' ? ' narrow' : '')}>
                <div>
                    <div className="section-label">Why this is here</div>
                    <p className="body-text">
                        {item.reason} Rule path <code>{item.rule_path}</code> ran on capture and produced the decision{' '}
                        <span className="mono">{item.decision}</span>.
                    </p>

                    <div className="section-label">
                        Policy decision trace{' '}
                        <span className="meta">
                            {item.trace.length} rules · {item.trace.reduce((a, b) => a + b.ms, 0).toFixed(1)}ms total
                        </span>
                    </div>
                    <div className="trace">
                        {item.trace.map((t, i) => (
                            <div
                                key={i}
                                className={
                                    'trace-row ' +
                                    (t.action === 'deny' || t.action === 'block' || t.action === 'quarantine'
                                        ? 'deny'
                                        : t.action === 'match'
                                          ? 'match'
                                          : 'ok')
                                }
                            >
                                <span className="trace-step mono">{i + 1}</span>
                                <span className="trace-rule mono">{t.rule}</span>
                                <span className="trace-action">{t.action}</span>
                                <span className="trace-outcome">{t.outcome}</span>
                                <span className="trace-ms mono">{t.ms.toFixed(1)}ms</span>
                            </div>
                        ))}
                    </div>

                    <div className="action-bar">
                        <button
                            className="btn primary"
                            onClick={() => onAct && onAct('approve')}
                        >
                            <span className="key">a</span>Approve decision
                        </button>
                        <button
                            className="btn"
                            onClick={() => onAct && onAct('override')}
                        >
                            <span className="key">o</span>Override…
                        </button>
                        <button
                            className="btn"
                            onClick={() => onAct && onAct('defer')}
                        >
                            <span className="key">d</span>Defer 7d
                        </button>
                        <button
                            className="btn danger"
                            onClick={() => onAct && onAct('tombstone')}
                        >
                            <span className="key">x</span>Tombstone
                        </button>
                    </div>
                </div>

                <div className="sidecar">
                    <div className="card">
                        <div className="card-head">
                            <span>Provenance · 1 entry</span>
                        </div>
                        <dl className="kv">
                            <dt>captured</dt>
                            <dd>{item.meta} ago</dd>
                            <dt>by</dt>
                            <dd className="mono">claude-code</dd>
                            <dt>session</dt>
                            <dd>
                                <a
                                    href="#"
                                    className="mono"
                                >
                                    a8b3f2c
                                </a>
                            </dd>
                            <dt>device</dt>
                            <dd className="mono">mbp</dd>
                        </dl>
                    </div>
                    <div className="card">
                        <div className="card-head">
                            <span>Policy</span>
                        </div>
                        <dl className="kv">
                            <dt>rule</dt>
                            <dd className="mono">{item.rule_path}</dd>
                            <dt>decision</dt>
                            <dd>{item.decision}</dd>
                            <dt>severity</dt>
                            <dd>{item.severity}</dd>
                        </dl>
                    </div>
                    <div className="card">
                        <div className="card-head">
                            <span>Privacy scan</span>
                            <span className="card-meta">{item.sensitivity || 'plaintext'}</span>
                        </div>
                        <div
                            style={{ color: 'var(--fg-3)', fontFamily: 'var(--font-mono)', fontSize: 'var(--text-xs)' }}
                        >
                            {item.sensitivity === 'sensitive'
                                ? 'labels detected · storage action: ' +
                                  (item.encrypted ? 'encrypt-at-rest' : 'plaintext')
                                : 'no sensitive labels detected'}
                        </div>
                    </div>
                </div>
            </div>
        </div>
    );
}

Object.assign(window, { GovernanceView, GovInspector });
