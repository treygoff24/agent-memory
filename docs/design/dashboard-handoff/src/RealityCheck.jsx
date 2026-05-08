// Reality Check focus mode
const { useState: useStateRC } = React;

function RealityCheck({ session, onExit, onRespond, variant = 'default' }) {
    const cur = session.current;
    const [showScore, setShowScore] = useStateRC(variant === 'score-open');

    const isComplete = variant === 'complete';
    const isEncrypted = variant === 'encrypted';
    const isRefused = variant === 'refused';

    const progressCurrent = isComplete ? session.progress.total : session.progress.current;
    const progress = (progressCurrent / session.progress.total) * 100;

    return (
        <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
            <div className="rc-strip">
                <span className="brand">
                    <span className="sigil">◆</span>
                    <span className="word">memorum</span>
                </span>
                <span className="sep">·</span>
                <span className="label">reality check</span>
                <span className="sep">·</span>
                <span className="scope">{cur.namespace}</span>
                <div className="gauge">
                    <i style={{ width: progress + '%' }} />
                </div>
                <span className="progress-text">
                    {progressCurrent} of {session.progress.total}
                </span>
                <span
                    className="exit"
                    onClick={onExit}
                >
                    esc · pause
                </span>
            </div>

            <div className="rc-stage">
                {isComplete ? (
                    <div className="rc-card rc-complete">
                        <div className="rc-complete-mark">✓</div>
                        <h2 className="rc-complete-title">Reality Check complete.</h2>
                        <div className="rc-complete-stats">
                            <div>
                                <span className="n">11</span>
                                <span className="lbl">confirmed</span>
                            </div>
                            <div>
                                <span className="n bad">1</span>
                                <span className="lbl">forgotten</span>
                            </div>
                            <div>
                                <span className="n">0</span>
                                <span className="lbl">deferred</span>
                            </div>
                        </div>
                        <div className="rc-complete-meta">Next session due in 7 days · session_id rc_20260507_001</div>
                        <button
                            className="rc-action primary"
                            style={{ maxWidth: 280 }}
                            onClick={onExit}
                        >
                            <span className="key">↵</span>
                            <span>Dismiss</span>
                            <span className="desc">return to inbox</span>
                        </button>
                    </div>
                ) : (
                    <div className="rc-card">
                        <div className="rc-scope-line">
                            <span className="scope">{cur.namespace}</span>
                            <span className="sep">·</span>
                            <span>written {cur.written}</span>
                            <span className="sep">·</span>
                            <span>last verified {cur.last_verified_days}d</span>
                        </div>

                        <h2 className="rc-question">{cur.question}</h2>

                        {isEncrypted ? (
                            <div className="rc-think rc-think--encrypted">
                                <div className="head">What memorum thinks</div>
                                <div className="body">
                                    <span className="enc-glyph">⌬</span>
                                    <span>encrypted memory · score 0.78</span>
                                </div>
                                <div className="enc-help">
                                    reveal externally to confirm/correct — body is sealed in this surface
                                </div>
                            </div>
                        ) : (
                            <div className="rc-think">
                                <div className="head">What memorum thinks</div>
                                <div className="body">{cur.think}</div>
                                <div className="source">Source: {cur.source}</div>
                            </div>
                        )}

                        {isRefused ? (
                            <div className="rc-refused">
                                <div className="head">Refused</div>
                                <div className="body">
                                    This memory cannot be confirmed because{' '}
                                    <span className="reason">
                                        a tombstone in the personal/family namespace blocks mutations on entities tagged
                                        minor.
                                    </span>
                                </div>
                                <div className="rc-refused-meta">
                                    policy_id family.minor.no_mutate · trace_id pdt_20260507_8f2a
                                </div>
                                <button
                                    className="rc-action"
                                    style={{ maxWidth: 280 }}
                                    onClick={() => onRespond('next')}
                                >
                                    <span className="key">n</span>
                                    <span>Next item</span>
                                    <span className="desc">skip and continue</span>
                                </button>
                            </div>
                        ) : (
                            <div className="rc-actions">
                                <button
                                    className={'rc-action primary' + (isEncrypted ? ' disabled' : '')}
                                    onClick={() => !isEncrypted && onRespond('confirm')}
                                    title={
                                        isEncrypted
                                            ? 'Cannot confirm encrypted memories from this surface — reveal externally first.'
                                            : ''
                                    }
                                    disabled={isEncrypted}
                                >
                                    <span className="key">y</span>
                                    <span>Confirm — still true</span>
                                    <span className="desc">
                                        {isEncrypted ? 'requires external reveal' : 'keep, refresh verified-at'}
                                    </span>
                                </button>
                                <button
                                    className="rc-action"
                                    onClick={() => onRespond('correct')}
                                >
                                    <span className="key">k</span>
                                    <span>Correct — replace with…</span>
                                    <span className="desc">opens text input</span>
                                </button>
                                <button
                                    className="rc-action"
                                    onClick={() => onRespond('forget')}
                                >
                                    <span className="key">f</span>
                                    <span>Forget</span>
                                    <span className="desc">tombstone, no recall</span>
                                </button>
                                <button
                                    className="rc-action"
                                    onClick={() => onRespond('skip')}
                                >
                                    <span className="key">s</span>
                                    <span>Skip — ask later</span>
                                    <span className="desc">defer 30 days</span>
                                </button>
                            </div>
                        )}

                        <details
                            className="rc-score"
                            open={showScore}
                            onToggle={(e) => setShowScore(e.target.open)}
                        >
                            <summary>
                                <span>Score breakdown</span>
                                <span className="s-meta">total {cur.score.toFixed(2)}</span>
                            </summary>
                            <div className="scorebars">
                                {Object.entries(cur.component_scores).map(([k, v]) => (
                                    <div
                                        className="row"
                                        key={k}
                                    >
                                        <span className="label">{k}</span>
                                        <span className="track">
                                            <span
                                                className="fill"
                                                style={{ width: v * 100 + '%' }}
                                            />
                                        </span>
                                        <span className="val">{v.toFixed(2)}</span>
                                    </div>
                                ))}
                            </div>
                        </details>
                    </div>
                )}

                <aside className="rc-side">
                    <h3>Session</h3>
                    <ul>
                        {(isComplete
                            ? session.items.map((i) => ({
                                  ...i,
                                  status: i.status === 'queued' ? 'done' : i.status === 'now' ? 'done' : i.status,
                              }))
                            : session.items
                        )
                            .slice(0, 8)
                            .map((it) => (
                                <li
                                    key={it.id}
                                    className={it.status}
                                >
                                    <span className="mark">
                                        {it.status === 'done' ? '✓' : it.status === 'now' ? '▸' : '·'}
                                    </span>
                                    <span>{it.title}</span>
                                </li>
                            ))}
                        {session.items.length > 8 && (
                            <li>
                                <span className="mark">·</span>
                                <span>+ {session.items.length - 8} more</span>
                            </li>
                        )}
                    </ul>
                </aside>
            </div>
        </div>
    );
}

Object.assign(window, { RealityCheck });
