// Inbox view: filter pills + list + inspector (right rail / drawer / modal)
const { useState: useStateInbox, useMemo: useMemoInbox } = React;

const PILLS = [
    { id: 'all', label: 'all', key: '1' },
    { id: 'review', label: 'review', key: '2' },
    { id: 'conflicts', label: 'conflicts', key: '3' },
    { id: 'recall', label: 'recall', key: '4' },
    { id: 'dreams', label: 'dreams', key: '5' },
    { id: 'due', label: 'due', key: '6' },
];

const KIND_TO_PILL = {
    review: 'review',
    conflict: 'conflicts',
    recall: 'recall',
    dream: 'dreams',
    due: 'due',
    memory: 'all',
};

function FilterPills({ items, active, onChange }) {
    const counts = items.reduce((acc, it) => {
        const p = KIND_TO_PILL[it.kind];
        acc[p] = (acc[p] || 0) + 1;
        acc.all = (acc.all || 0) + 1;
        return acc;
    }, {});
    return (
        <div
            className="pills"
            role="tablist"
        >
            {PILLS.map((p) => {
                const c = counts[p.id];
                if (c == null && p.id !== 'recall') return null;
                return (
                    <button
                        key={p.id}
                        className={'pill' + (active === p.id ? ' active' : '')}
                        onClick={() => onChange(p.id)}
                        role="tab"
                        aria-selected={active === p.id}
                    >
                        <span>{p.label}</span>
                        {c != null && <span className="n">{c}</span>}
                        <span className="pillkey">{p.key}</span>
                    </button>
                );
            })}
        </div>
    );
}

function ListRow({ item, selected, onClick }) {
    return (
        <div
            className={'list-item' + (selected ? ' selected' : '')}
            onClick={onClick}
            role="row"
            tabIndex={0}
        >
            <span
                className={'glyph ' + item.kind}
                aria-hidden
            >
                {item.glyph}
            </span>
            <div className="body">
                <div className="title-line">{item.title}</div>
                <div className="sub">
                    <span className="scope">{item.namespace}</span>
                    {(item.sub || []).map((s, i) => (
                        <React.Fragment key={i}>
                            <span className="sep">·</span>
                            <span>{s}</span>
                        </React.Fragment>
                    ))}
                </div>
            </div>
            <div className="meta">{item.meta}</div>
        </div>
    );
}

function MemoryList({ items, selectedId, onSelect, dense }) {
    // Group by today / earlier for typical (or by kind for heavy)
    return (
        <div className="list">
            {items.map((it) => (
                <ListRow
                    key={it.id}
                    item={it}
                    selected={selectedId === it.id}
                    onClick={() => onSelect(it.id)}
                />
            ))}
            {items.length === 0 && (
                <div className="empty">
                    <span className="ico">○</span>
                    <h3>Inbox is clear.</h3>
                    <p>All review items processed. Last activity: 2 hours ago.</p>
                </div>
            )}
        </div>
    );
}

function InboxView({ items, layout, selectedId, onSelect, drawerOpen, onCloseDrawer, modalOpen, onCloseModal, onAct }) {
    const [pill, setPill] = useStateInbox('all');

    const visible = useMemoInbox(() => {
        if (pill === 'all') return items;
        return items.filter((it) => KIND_TO_PILL[it.kind] === pill);
    }, [items, pill]);

    const selected = items.find((it) => it.id === selectedId) || visible[0];

    // Three-pane: left = pills/groups, mid = list, right = inspector
    if (layout === 'three') {
        return (
            <>
                <div className="view-header">
                    <span className="view-title">Inbox</span>
                    <span className="view-subtitle">· three-pane · {visible.length} items</span>
                    <span className="spacer" />
                    <FilterPills
                        items={items}
                        active={pill}
                        onChange={setPill}
                    />
                </div>
                <div className="panes-3">
                    <div className="pane left">
                        <div className="pane-scroll">
                            <div className="list-section">Filters</div>
                            <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                                {PILLS.map((p) => {
                                    const c = items.reduce(
                                        (n, it) => n + (KIND_TO_PILL[it.kind] === p.id || p.id === 'all' ? 1 : 0),
                                        0,
                                    );
                                    return (
                                        <button
                                            key={p.id}
                                            className={'nav-item' + (pill === p.id ? ' active' : '')}
                                            onClick={() => setPill(p.id)}
                                            style={{ margin: 0, height: 28, gridTemplateColumns: '24px 1fr auto' }}
                                        >
                                            <span
                                                className="ico"
                                                style={{ color: 'var(--fg-3)' }}
                                            >
                                                {p.id === 'review'
                                                    ? '●'
                                                    : p.id === 'conflicts'
                                                      ? '⚠'
                                                      : p.id === 'dreams'
                                                        ? '◇'
                                                        : p.id === 'due'
                                                          ? '▣'
                                                          : p.id === 'recall'
                                                            ? '▸'
                                                            : '○'}
                                            </span>
                                            <span
                                                className="label"
                                                style={{ color: 'inherit' }}
                                            >
                                                {p.label}
                                            </span>
                                            <span className="count">{c || 0}</span>
                                        </button>
                                    );
                                })}
                            </div>
                            <div className="list-section">Namespaces</div>
                            <div
                                style={{
                                    display: 'flex',
                                    flexDirection: 'column',
                                    gap: 2,
                                    fontFamily: 'var(--font-mono)',
                                    fontSize: 'var(--text-xs)',
                                }}
                            >
                                {[
                                    'coding/typescript',
                                    'project:atlasos',
                                    'personal/family',
                                    'me/security',
                                    'prefs/editor',
                                    'work/clients/acme',
                                    'tools/terminal',
                                    'meta/preferences',
                                ].map((n) => (
                                    <div
                                        key={n}
                                        style={{ padding: '4px 10px', color: 'var(--fg-3)' }}
                                    >
                                        {n}
                                    </div>
                                ))}
                            </div>
                        </div>
                    </div>
                    <div className="pane mid">
                        <div
                            className="pane-scroll"
                            style={{ padding: 0 }}
                        >
                            <MemoryList
                                items={visible}
                                selectedId={selected?.id}
                                onSelect={onSelect}
                            />
                        </div>
                    </div>
                    <div className="pane">
                        <div className="pane-scroll">
                            <Inspector
                                item={selected}
                                layout="narrow"
                                onAct={onAct}
                            />
                        </div>
                    </div>
                </div>
            </>
        );
    }

    // Modal: list full width, inspector pops as modal sheet
    if (layout === 'modal') {
        return (
            <>
                <div className="view-header">
                    <span className="view-title">Inbox</span>
                    <span className="view-subtitle">· modal · {visible.length} items</span>
                    <span className="spacer" />
                    <FilterPills
                        items={items}
                        active={pill}
                        onChange={setPill}
                    />
                </div>
                <div className="panes-single">
                    <div className="pane">
                        <div className="pane-scroll">
                            <MemoryList
                                items={visible}
                                selectedId={modalOpen ? selected?.id : null}
                                onSelect={onSelect}
                            />
                        </div>
                    </div>
                </div>
                {modalOpen && selected && (
                    <div
                        className="modal-veil"
                        onClick={onCloseModal}
                    >
                        <div
                            className="modal"
                            style={{ width: 760, maxHeight: '76vh', overflow: 'auto' }}
                            onClick={(e) => e.stopPropagation()}
                        >
                            <div style={{ display: 'flex', justifyContent: 'flex-end', padding: '10px 14px 0' }}>
                                <button
                                    className="icon-btn"
                                    onClick={onCloseModal}
                                    aria-label="Close"
                                >
                                    <Icon
                                        name="x"
                                        size={16}
                                    />
                                </button>
                            </div>
                            <Inspector
                                item={selected}
                                layout="narrow"
                                onAct={onAct}
                            />
                        </div>
                    </div>
                )}
            </>
        );
    }

    // Drawer: list full width, inspector slides in from right
    if (layout === 'drawer') {
        return (
            <>
                <div className="view-header">
                    <span className="view-title">Inbox</span>
                    <span className="view-subtitle">· drawer · {visible.length} items</span>
                    <span className="spacer" />
                    <FilterPills
                        items={items}
                        active={pill}
                        onChange={setPill}
                    />
                </div>
                <div className="panes-drawer">
                    <div className="pane left">
                        <div className="pane-scroll">
                            <MemoryList
                                items={visible}
                                selectedId={drawerOpen ? selected?.id : null}
                                onSelect={onSelect}
                            />
                        </div>
                    </div>
                    <div className={'drawer' + (drawerOpen ? '' : ' closed')}>
                        <div
                            className="pane-scroll"
                            style={{ paddingTop: 0 }}
                        >
                            <div style={{ display: 'flex', justifyContent: 'flex-end', padding: '10px 14px 0' }}>
                                <button
                                    className="icon-btn"
                                    onClick={onCloseDrawer}
                                    aria-label="Close drawer"
                                >
                                    <Icon
                                        name="x"
                                        size={16}
                                    />
                                </button>
                            </div>
                            <Inspector
                                item={selected}
                                layout="narrow"
                                onAct={onAct}
                            />
                        </div>
                    </div>
                </div>
            </>
        );
    }

    // Default: two-pane
    return (
        <>
            <div className="view-header">
                <span className="view-title">Inbox</span>
                <span className="view-subtitle">· {visible.length} items</span>
                <span className="spacer" />
                <FilterPills
                    items={items}
                    active={pill}
                    onChange={setPill}
                />
            </div>
            <div className="panes-2">
                <div className="pane left">
                    <div className="pane-scroll">
                        <MemoryList
                            items={visible}
                            selectedId={selected?.id}
                            onSelect={onSelect}
                        />
                    </div>
                </div>
                <div className="pane">
                    <div className="pane-scroll">
                        <Inspector
                            item={selected}
                            layout="wide"
                            onAct={onAct}
                        />
                    </div>
                </div>
            </div>

            {modalOpen && selected && (
                <div
                    className="modal-veil"
                    onClick={onCloseModal}
                >
                    <div
                        className="modal"
                        style={{ width: 760, maxHeight: '76vh', overflow: 'auto' }}
                        onClick={(e) => e.stopPropagation()}
                    >
                        <div style={{ display: 'flex', justifyContent: 'flex-end', padding: '10px 14px 0' }}>
                            <button
                                className="icon-btn"
                                onClick={onCloseModal}
                                aria-label="Close"
                            >
                                <Icon
                                    name="x"
                                    size={16}
                                />
                            </button>
                        </div>
                        <Inspector
                            item={selected}
                            layout="narrow"
                            onAct={onAct}
                        />
                    </div>
                </div>
            )}
        </>
    );
}

Object.assign(window, { InboxView });
