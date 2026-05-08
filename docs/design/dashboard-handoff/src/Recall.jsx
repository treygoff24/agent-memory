// Recall ledger: timeline strip (24h or 30d) + filtered list of recall events
const { useState: useStateR, useMemo: useMemoR } = React;

function TimelineStrip({ buckets, mode, selectedHour, onPick }) {
    const max = Math.max(1, ...buckets.map((b) => b.count));
    return (
        <div className="rl-strip">
            <div className="rl-strip-head">
                <span className="section-label">{mode === '24h' ? '24-hour scrubber' : '30-day scrubber'}</span>
                <span className="rl-strip-meta">
                    peak {max}/{mode === '24h' ? 'hr' : 'day'}
                </span>
            </div>
            <div
                className="rl-strip-bars"
                role="group"
                aria-label="Timeline"
            >
                {buckets.map((b, i) => {
                    const h = Math.max(2, Math.round((b.count / max) * 56));
                    const isSel =
                        selectedHour != null && (mode === '24h' ? b.hour === selectedHour : b.day === selectedHour);
                    return (
                        <button
                            key={i}
                            className={'rl-bar' + (isSel ? ' selected' : '')}
                            style={{ height: h }}
                            title={
                                mode === '24h'
                                    ? `${String(b.hour).padStart(2, '0')}:00 — ${b.count} recalls`
                                    : `day -${29 - b.day} — ${b.count} recalls`
                            }
                            onClick={() => onPick(mode === '24h' ? b.hour : b.day)}
                        >
                            <span className="rl-bar-cap" />
                        </button>
                    );
                })}
            </div>
            <div className="rl-strip-axis">
                {mode === '24h' ? (
                    <>
                        <span>00</span>
                        <span>06</span>
                        <span>12</span>
                        <span>18</span>
                        <span>23</span>
                    </>
                ) : (
                    <>
                        <span>30d ago</span>
                        <span>21d</span>
                        <span>14d</span>
                        <span>7d</span>
                        <span>today</span>
                    </>
                )}
            </div>
        </div>
    );
}

function RecallRow({ ev, selected, onSelect }) {
    const t = new Date(ev.recalled_at);
    const hh = String(t.getUTCHours()).padStart(2, '0');
    const mm = String(t.getUTCMinutes()).padStart(2, '0');
    const ss = String(t.getUTCSeconds()).padStart(2, '0');
    return (
        <div
            className={'rl-row' + (selected ? ' selected' : '')}
            tabIndex={0}
            onClick={() => onSelect(ev.id)}
        >
            <span className="rl-time">
                {hh}:{mm}:{ss}
            </span>
            <span className="rl-seq">#{ev.seq}</span>
            <span className="rl-dev">{ev.device}</span>
            <span className="rl-agent">{ev.agent}</span>
            <span className="rl-summary">{ev.summary}</span>
            <span className="rl-ns">{ev.namespace}</span>
            <span className="rl-lat">{ev.latency_ms}ms</span>
            <span className="rl-score">{ev.score.toFixed(2)}</span>
        </div>
    );
}

function RecallView({ events, dayBuckets, hourBuckets, heavy, selectedId, onSelect }) {
    const [agent, setAgent] = useStateR('all');
    const [device, setDevice] = useStateR('all');
    const [pickedBucket, setPickedBucket] = useStateR(null);
    const stripMode = heavy ? '30d' : '24h';

    const visible = useMemoR(() => {
        return events.filter((e) => {
            if (agent !== 'all' && e.agent !== agent) return false;
            if (device !== 'all' && e.device !== device) return false;
            return true;
        });
    }, [events, agent, device]);

    const sel = visible.find((e) => e.id === selectedId) || visible[0];
    const total = heavy ? '≈ 9,142 events' : `${events.length} events`;

    return (
        <>
            <div className="view-header">
                <span className="view-title">Recall ledger</span>
                <span className="view-subtitle">
                    · {total} · {stripMode === '24h' ? 'last 24 hours' : 'last 30 days'}
                </span>
                <span className="spacer" />
                <div className="rl-filters">
                    <select
                        value={agent}
                        onChange={(e) => setAgent(e.target.value)}
                    >
                        <option value="all">all agents</option>
                        <option value="claude-code">claude-code</option>
                        <option value="codex-cli">codex-cli</option>
                        <option value="cursor">cursor</option>
                        <option value="manual">manual</option>
                    </select>
                    <select
                        value={device}
                        onChange={(e) => setDevice(e.target.value)}
                    >
                        <option value="all">all devices</option>
                        <option value="mbp">mbp</option>
                        <option value="mini">mini</option>
                        <option value="phone">phone</option>
                    </select>
                    <button className="btn">/ search</button>
                    <button className="btn">export csv</button>
                </div>
            </div>

            <TimelineStrip
                buckets={stripMode === '24h' ? hourBuckets : dayBuckets}
                mode={stripMode}
                selectedHour={pickedBucket}
                onPick={(b) => setPickedBucket((p) => (p === b ? null : b))}
            />

            <div className="panes-2">
                <div className="pane left rl-list-pane">
                    <div className="rl-table-head">
                        <span>time</span>
                        <span>seq</span>
                        <span>device</span>
                        <span>agent</span>
                        <span>memory</span>
                        <span>namespace</span>
                        <span>lat</span>
                        <span>score</span>
                    </div>
                    <div className="pane-scroll">
                        {visible.map((ev) => (
                            <RecallRow
                                key={ev.id}
                                ev={ev}
                                selected={sel && sel.id === ev.id}
                                onSelect={onSelect}
                            />
                        ))}
                        {heavy && (
                            <div className="rl-virt-hint">
                                <span className="mono">{visible.length}</span> visible · scrolling backed by 7-day
                                window · older buckets summarized in scrubber
                            </div>
                        )}
                    </div>
                </div>
                <div className="pane">
                    <div className="pane-scroll">
                        <RecallEventInspector
                            ev={sel}
                            layout="narrow"
                        />
                    </div>
                </div>
            </div>
        </>
    );
}

function RecallEventInspector({ ev, layout }) {
    if (!ev)
        return (
            <div
                className="empty"
                style={{ paddingTop: 120 }}
            >
                <span className="ico">○</span>
                <h3>Pick an event</h3>
                <p>Use ↑↓ or click a row in the ledger.</p>
            </div>
        );
    const t = new Date(ev.recalled_at);
    return (
        <div className="inspector">
            <div className="insp-head">
                <span className="insp-title">Recall event</span>
                <span className="insp-scope">{ev.namespace}</span>
                <span className="insp-badges">
                    <span className="badge info">recall_event</span>
                </span>
            </div>
            <div
                style={{ padding: '4px 0 0', color: 'var(--fg-3)', fontFamily: 'var(--font-mono)', fontSize: '10.5px' }}
            >
                {ev.id} · seq {ev.seq}
            </div>

            <div className={'insp-grid' + (layout === 'narrow' ? ' narrow' : '')}>
                <div>
                    <div className="section-label">Memory recalled</div>
                    <div className="card">
                        <div className="card-head">
                            <a
                                href="#"
                                className="mono"
                                style={{ color: 'var(--info)' }}
                            >
                                {ev.memory_id.slice(0, 32)}…
                            </a>
                            <span className="card-meta">open trust artifact →</span>
                        </div>
                        <div
                            className="body-text"
                            style={{ fontSize: 'var(--text-sm)' }}
                        >
                            {ev.summary}
                        </div>
                    </div>

                    <div className="section-label">Surrounding context</div>
                    <p className="body-text">
                        Agent <span className="mono">{ev.agent}</span> on <span className="mono">{ev.device}</span>{' '}
                        retrieved this memory while operating in session{' '}
                        <a
                            href="#"
                            className="mono"
                        >
                            {ev.session}
                        </a>
                        . Score above retrieval threshold (0.40); no rerank triggered.
                    </p>

                    <div className="action-bar">
                        <button className="btn">
                            <span className="key">o</span>Open memory
                        </button>
                        <button className="btn">
                            <span className="key">s</span>Open session
                        </button>
                        <button className="btn">
                            <span className="key">c</span>Copy event id
                        </button>
                    </div>
                </div>
                <div className="sidecar">
                    <div className="card">
                        <div className="card-head">
                            <span>Event</span>
                        </div>
                        <dl className="kv">
                            <dt>recalled_at</dt>
                            <dd className="mono">{t.toISOString().slice(0, 19)}Z</dd>
                            <dt>seq</dt>
                            <dd className="mono">{ev.seq}</dd>
                            <dt>device</dt>
                            <dd className="mono">{ev.device}</dd>
                            <dt>agent</dt>
                            <dd className="mono">{ev.agent}</dd>
                            <dt>session</dt>
                            <dd>
                                <a
                                    href="#"
                                    className="mono"
                                >
                                    {ev.session}
                                </a>
                            </dd>
                            <dt>latency</dt>
                            <dd className="mono">{ev.latency_ms} ms</dd>
                            <dt>tokens</dt>
                            <dd className="mono">{ev.tokens}</dd>
                        </dl>
                    </div>
                    <div className="card">
                        <div className="card-head">
                            <span>Score</span>
                        </div>
                        <div className="scorebars">
                            <div className="row">
                                <span className="label">retrieval_score</span>
                                <span className="track">
                                    <span
                                        className="fill"
                                        style={{ width: ev.score * 100 + '%' }}
                                    />
                                </span>
                                <span className="val">{ev.score.toFixed(2)}</span>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    );
}

Object.assign(window, { RecallView, RecallEventInspector });
