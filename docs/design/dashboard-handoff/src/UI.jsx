// Command palette, notification dropdown, banner, toast, empty state
const { useState: useStateUI, useEffect: useEffectUI, useRef: useRefUI, useMemo: useMemoUI } = React;

function CommandPalette({ open, onClose, commands, onRun }) {
    const [q, setQ] = useStateUI('');
    const [idx, setIdx] = useStateUI(0);
    const inputRef = useRefUI(null);

    useEffectUI(() => {
        if (open) {
            setQ('');
            setIdx(0);
            setTimeout(() => inputRef.current?.focus(), 50);
        }
    }, [open]);

    const filtered = useMemoUI(() => {
        const ql = q.toLowerCase();
        if (!ql) return commands;
        return commands.filter((c) => c.name.toLowerCase().includes(ql) || c.cat.toLowerCase().includes(ql));
    }, [q, commands]);

    if (!open) return null;

    const onKey = (e) => {
        if (e.key === 'Escape') onClose();
        else if (e.key === 'ArrowDown') {
            setIdx((i) => Math.min(filtered.length - 1, i + 1));
            e.preventDefault();
        } else if (e.key === 'ArrowUp') {
            setIdx((i) => Math.max(0, i - 1));
            e.preventDefault();
        } else if (e.key === 'Enter') {
            filtered[idx] && onRun(filtered[idx]);
        }
    };

    return (
        <div
            className="modal-veil"
            onClick={onClose}
        >
            <div
                className="modal"
                onClick={(e) => e.stopPropagation()}
                onKeyDown={onKey}
            >
                <div className="palette-input">
                    <span className="prompt">:</span>
                    <input
                        ref={inputRef}
                        value={q}
                        onChange={(e) => {
                            setQ(e.target.value);
                            setIdx(0);
                        }}
                        placeholder="Type a command…"
                    />
                    <kbd>esc</kbd>
                </div>
                <div className="palette-list">
                    {filtered.length === 0 && (
                        <div
                            style={{
                                padding: '24px',
                                textAlign: 'center',
                                color: 'var(--fg-3)',
                                fontSize: 'var(--text-sm)',
                            }}
                        >
                            No matching commands.
                        </div>
                    )}
                    {filtered.map((c, i) => (
                        <div
                            key={c.id}
                            className={'palette-row' + (i === idx ? ' selected' : '')}
                            onMouseEnter={() => setIdx(i)}
                            onClick={() => onRun(c)}
                        >
                            <span className="cat">{c.cat[0]}</span>
                            <span className="cmd-name">
                                {c.name}
                                <span className="scope">{c.scope}</span>
                            </span>
                            {c.kbd && <kbd>{c.kbd}</kbd>}
                        </div>
                    ))}
                </div>
                <div className="palette-foot">
                    <span>
                        <kbd>↑↓</kbd> navigate
                    </span>
                    <span>
                        <kbd>enter</kbd> run
                    </span>
                    <span>
                        <kbd>tab</kbd> cycle category
                    </span>
                    <span style={{ marginLeft: 'auto' }}>{filtered.length} commands</span>
                </div>
            </div>
        </div>
    );
}

function NotificationDropdown({ open, onClose, items, onAction }) {
    if (!open) return null;
    return (
        <>
            <div
                style={{ position: 'fixed', inset: 0, zIndex: 30 }}
                onClick={onClose}
            />
            <div
                className="notif"
                role="menu"
            >
                <div className="notif-head">
                    Notifications · {items.length}
                    <span
                        className="clear"
                        onClick={onClose}
                    >
                        clear all
                    </span>
                </div>
                {items.map((n) => (
                    <div
                        key={n.id}
                        className="notif-row"
                        onClick={() => onAction(n)}
                    >
                        <span
                            className="ico"
                            style={{
                                color:
                                    n.iconRole === 'review'
                                        ? 'var(--accent)'
                                        : n.iconRole === 'dream'
                                          ? 'var(--warn)'
                                          : n.iconRole === 'due'
                                            ? 'var(--warn)'
                                            : n.iconRole === 'conflict'
                                              ? 'var(--bad)'
                                              : 'var(--fg-3)',
                                fontFamily: 'var(--font-mono)',
                            }}
                        >
                            {n.icon}
                        </span>
                        <div>
                            <div className="nt-title">{n.title}</div>
                            <div className="nt-meta">
                                {n.at} · {n.meta}
                            </div>
                            <div className="nt-action">{n.action.label}</div>
                        </div>
                    </div>
                ))}
            </div>
        </>
    );
}

function Banner({ kind = 'bad', label, msg, onDismiss, actions }) {
    return (
        <div className="banner">
            <span className="label">{label}</span>
            <span className="msg">{msg}</span>
            <span className="actions">
                {(actions || []).map((a, i) => (
                    <button
                        key={i}
                        className="btn"
                        onClick={a.onClick}
                    >
                        {a.label}
                    </button>
                ))}
                {onDismiss && (
                    <button
                        className="icon-btn"
                        onClick={onDismiss}
                    >
                        <Icon
                            name="x"
                            size={14}
                        />
                    </button>
                )}
            </span>
        </div>
    );
}

function Toast({ toast, onDismiss }) {
    return (
        <div className={'toast ' + (toast.kind || '')}>
            <div>
                <div className="t-title">{toast.title}</div>
                <div>{toast.msg}</div>
            </div>
            <div className="t-actions">
                {toast.action && (
                    <button
                        className="btn"
                        onClick={toast.action.onClick}
                    >
                        {toast.action.label}
                    </button>
                )}
                <button
                    className="icon-btn"
                    onClick={onDismiss}
                >
                    <Icon
                        name="x"
                        size={14}
                    />
                </button>
            </div>
        </div>
    );
}

Object.assign(window, { CommandPalette, NotificationDropdown, Banner, Toast });
