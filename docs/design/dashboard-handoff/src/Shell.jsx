// Shell: top bar, sidebar, footer
const { useState, useEffect, useRef, useMemo, useCallback } = React;

function StatusDot({ kind = 'ok', pulse = false }) {
    return (
        <span
            className={'status-dot ' + kind}
            aria-hidden
        />
    );
}

function TopBar({ view, onNav, onPalette, onBell, bellOpen, daemon, csrfBad, fullbleed }) {
    return (
        <div className="topbar">
            <div className="brand">
                <span className="sigil">◆</span>
                <span>memorum</span>
                {fullbleed && <span className="brand-divider">· reality check</span>}
            </div>
            {!fullbleed && (
                <div className="search">
                    <Icon
                        name="search"
                        size={14}
                    />
                    <input placeholder="Search memories, namespaces, ids…" />
                    <kbd>/</kbd>
                </div>
            )}
            {fullbleed && <div />}
            <div className="topbar-right">
                <button
                    className="icon-btn"
                    title="Command palette (:)"
                    onClick={onPalette}
                >
                    <Icon
                        name="command"
                        size={16}
                    />
                </button>
                <button
                    className={'icon-btn' + (bellOpen ? ' active' : '')}
                    title="Notifications"
                    onClick={onBell}
                >
                    <Icon
                        name="bell"
                        size={16}
                    />
                    <span className="dot" />
                </button>
                <div className="status-cluster">
                    <span
                        className="pair"
                        title="daemon"
                    >
                        <StatusDot kind={daemon === 'down' ? 'bad' : 'ok'} />
                        daemon
                    </span>
                    <span
                        className="pair"
                        title="sync · 2 peers"
                    >
                        <StatusDot kind="ok" />
                        sync · 2
                    </span>
                    <span
                        className="pair"
                        title="next dream 03:00"
                    >
                        <StatusDot kind="idle" />
                        03:00
                    </span>
                </div>
            </div>
        </div>
    );
}

const NAV_ITEMS = [
    { id: 'inbox', label: 'Inbox', icon: 'inbox', key: 'i', count: 14, primary: true },
    { id: 'reality', label: 'Reality Check', icon: 'eye', key: 'r', count: 12 },
    { id: 'recall', label: 'Recall', icon: 'clock', key: 'l' },
    { id: 'dreams', label: 'Dreams', icon: 'moon', key: 'd', count: 4 },
    { id: 'peers', label: 'Peers', icon: 'users', key: 'p', count: 2 },
    { id: 'governance', label: 'Governance', icon: 'shield', key: 'g', count: 7 },
    { id: 'entities', label: 'Entities', icon: 'graph', key: 'e' },
];

function Sidebar({ active, onNav, collapsed }) {
    return (
        <nav
            className="sidebar"
            aria-label="Primary"
        >
            <div className="sidebar-section">Workspace</div>
            {NAV_ITEMS.map((item) => (
                <button
                    key={item.id}
                    className={'nav-item' + (active === item.id ? ' active' : '')}
                    onClick={() => onNav(item.id)}
                    title={collapsed ? item.label : undefined}
                >
                    <span className="ico">
                        <Icon
                            name={item.icon}
                            size={16}
                        />
                    </span>
                    <span className="label">{item.label}</span>
                    {item.count != null && <span className="count">{item.count}</span>}
                </button>
            ))}
            <div className="sidebar-bottom">
                <button
                    className="nav-item"
                    onClick={() => onNav('settings')}
                >
                    <span className="ico">
                        <Icon
                            name="gear"
                            size={16}
                        />
                    </span>
                    <span className="label">Settings</span>
                </button>
            </div>
        </nav>
    );
}

function Footer({ view, daemon }) {
    const hints =
        {
            inbox: [
                ['↑↓', 'navigate'],
                ['enter', 'inspect'],
                ['a', 'accept'],
                ['r', 'reject'],
                ['e', 'edit'],
                ['f', 'forget'],
                [':', 'palette'],
            ],
            reality: [
                ['y', 'confirm'],
                ['k', 'correct'],
                ['f', 'forget'],
                ['s', 'skip'],
                ['esc', 'pause'],
            ],
            recall: [
                ['↑↓', 'navigate'],
                ['enter', 'inspect'],
                ['/', 'search'],
                [':', 'palette'],
            ],
            dreams: [
                ['↑↓', 'navigate'],
                ['p', 'promote'],
                ['x', 'dismiss'],
                [':', 'palette'],
            ],
            peers: [
                ['↑↓', 'navigate'],
                ['enter', 'inspect'],
                [':', 'palette'],
            ],
            governance: [
                ['↑↓', 'navigate'],
                ['enter', 'inspect'],
                ['a', 'approve'],
                ['r', 'reject'],
                ['b', 'batch'],
            ],
            entities: [
                ['↑↓', 'navigate'],
                ['enter', 'focus'],
                [':', 'palette'],
            ],
            settings: [
                ['↑↓', 'navigate'],
                ['enter', 'apply'],
                [':', 'palette'],
            ],
        }[view] || [];

    return (
        <div className="footer">
            <span className="vital">
                {daemon === 'down' ? (
                    <>
                        <StatusDot kind="bad" /> daemon — down
                    </>
                ) : (
                    <>
                        <StatusDot kind="ok" /> daemon
                    </>
                )}
            </span>
            <span className="vital">
                <StatusDot kind="ok" /> sync · 2 peers
            </span>
            <span className="sep">·</span>
            <span>next dream 03:00</span>
            <span className="sep">·</span>
            <span>
                filter <span style={{ color: 'var(--fg-2)' }}>all</span>
            </span>
            <div className="right">
                {hints.map(([k, l]) => (
                    <span key={k}>
                        <kbd>{k}</kbd>
                        {l}
                    </span>
                ))}
            </div>
        </div>
    );
}

Object.assign(window, { TopBar, Sidebar, Footer, StatusDot, NAV_ITEMS });
