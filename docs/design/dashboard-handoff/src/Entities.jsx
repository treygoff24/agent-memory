// Entities: sortable table
const { useState: useStateE, useMemo: useMemoE } = React;

const ENTITY_KIND_GLYPH = {
    person: '◉',
    org: '▣',
    project: '◆',
    place: '○',
    tool: '▶',
    language: '▤',
};

function EntitiesView({ entities, selectedId, onSelect }) {
    const [sort, setSort] = useStateE({ key: 'mentions', dir: 'desc' });
    const [filter, setFilter] = useStateE('all');
    const [search, setSearch] = useStateE('');

    const visible = useMemoE(() => {
        let arr = entities;
        if (filter !== 'all') arr = arr.filter((e) => e.kind === filter);
        if (search) arr = arr.filter((e) => e.name.toLowerCase().includes(search.toLowerCase()));
        arr = [...arr].sort((a, b) => {
            const av = a[sort.key],
                bv = b[sort.key];
            if (typeof av === 'number' && typeof bv === 'number') return sort.dir === 'asc' ? av - bv : bv - av;
            return sort.dir === 'asc' ? String(av).localeCompare(String(bv)) : String(bv).localeCompare(String(av));
        });
        return arr;
    }, [entities, sort, filter, search]);

    const sel = visible.find((e) => e.id === selectedId) || visible[0];

    function head(key, label) {
        const active = sort.key === key;
        return (
            <button
                className={'th' + (active ? ' active' : '')}
                onClick={() => setSort((s) => ({ key, dir: s.key === key && s.dir === 'desc' ? 'asc' : 'desc' }))}
            >
                <span>{label}</span>
                {active && <span className="th-arrow mono">{sort.dir === 'asc' ? '↑' : '↓'}</span>}
            </button>
        );
    }

    const counts = {};
    entities.forEach((e) => {
        counts[e.kind] = (counts[e.kind] || 0) + 1;
    });

    return (
        <>
            <div className="view-header">
                <span className="view-title">Entities</span>
                <span className="view-subtitle">
                    · {entities.length} extracted · auto-detected from{' '}
                    {entities.reduce((a, e) => a + e.mentions, 0).toLocaleString()} mentions
                </span>
                <span className="spacer" />
                <div className="filter-pills">
                    {['all', 'person', 'org', 'project', 'place', 'tool', 'language'].map((k, i) => (
                        <button
                            key={k}
                            className={'pill' + (filter === k ? ' active' : '')}
                            onClick={() => setFilter(k)}
                        >
                            <span>{k}</span>
                            <span className={'count' + (filter === k ? ' accent' : '')}>
                                {k === 'all' ? entities.length : counts[k] || 0}
                            </span>
                        </button>
                    ))}
                </div>
                <input
                    className="ent-search"
                    placeholder="filter by name…"
                    value={search}
                    onChange={(e) => setSearch(e.target.value)}
                />
            </div>

            <div className="panes-2">
                <div className="pane left">
                    <div className="ent-table">
                        <div className="ent-thead">
                            <span></span>
                            {head('name', 'name')}
                            {head('kind', 'kind')}
                            {head('mentions', 'mentions')}
                            {head('namespaces', 'namespaces')}
                            {head('last_seen', 'last seen')}
                            {head('first_seen', 'first seen')}
                            {head('confidence', 'confidence')}
                        </div>
                        <div className="pane-scroll">
                            {visible.map((e) => (
                                <div
                                    key={e.id}
                                    className={'ent-row' + (sel && sel.id === e.id ? ' selected' : '')}
                                    onClick={() => onSelect(e.id)}
                                >
                                    <span className="ent-glyph">{ENTITY_KIND_GLYPH[e.kind] || '·'}</span>
                                    <span className="ent-name">
                                        {e.name}{' '}
                                        {e.sensitive && (
                                            <span
                                                className="badge"
                                                style={{ marginLeft: 6 }}
                                            >
                                                sensitive
                                            </span>
                                        )}
                                    </span>
                                    <span className="ent-kind">{e.kind}</span>
                                    <span className="ent-mentions mono">{e.mentions.toLocaleString()}</span>
                                    <span className="ent-ns mono">
                                        {e.namespaces[0]}
                                        {e.namespaces.length > 1 ? ` +${e.namespaces.length - 1}` : ''}
                                    </span>
                                    <span className="ent-time mono">{e.last_seen}</span>
                                    <span className="ent-time mono">{e.first_seen}</span>
                                    <span className="ent-conf mono">
                                        <span className="conf-bar">
                                            <span
                                                className="conf-fill"
                                                style={{ width: e.confidence * 100 + '%' }}
                                            />
                                        </span>
                                        {e.confidence.toFixed(2)}
                                    </span>
                                </div>
                            ))}
                        </div>
                    </div>
                </div>
                <div className="pane">
                    <div className="pane-scroll">
                        <EntityInspector
                            e={sel}
                            layout="narrow"
                        />
                    </div>
                </div>
            </div>
        </>
    );
}

function EntityInspector({ e, layout }) {
    if (!e)
        return (
            <div className="empty">
                <span className="ico">○</span>
                <h3>No entity selected</h3>
            </div>
        );
    return (
        <div className="inspector">
            <div className="insp-head">
                <span className="insp-title">{e.name}</span>
                <span className="insp-scope">{e.kind}</span>
                <span className="insp-badges">
                    <span className="badge">{e.kind}</span>
                    {e.sensitive && <span className="badge bad">sensitive</span>}
                </span>
            </div>
            <div
                style={{ padding: '4px 0 0', color: 'var(--fg-3)', fontFamily: 'var(--font-mono)', fontSize: '10.5px' }}
            >
                {e.id} · confidence {e.confidence.toFixed(2)}
            </div>

            <div className={'insp-grid' + (layout === 'narrow' ? ' narrow' : '')}>
                <div>
                    <div className="section-label">
                        Mentions{' '}
                        <span className="meta">
                            {e.mentions.toLocaleString()} across {e.namespaces.length} namespace
                            {e.namespaces.length > 1 ? 's' : ''}
                        </span>
                    </div>
                    <p className="body-text">
                        Memorum extracted <span className="mono">{e.name}</span> automatically from memory text. Mention
                        count includes recall events and review-queue items where the entity was matched. Use this view
                        to find every memory that references this entity.
                    </p>

                    <div className="section-label">Namespaces</div>
                    <div className="ent-ns-list">
                        {e.namespaces.map((n, i) => (
                            <span
                                key={i}
                                className="ent-ns-chip mono"
                            >
                                {n}
                            </span>
                        ))}
                    </div>

                    <div className="section-label">
                        Recent memories <span className="meta">5 of {e.mentions.toLocaleString()}</span>
                    </div>
                    <div className="evidence-list">
                        {[
                            { id: 'mem_…0a1', title: 'First reference of ' + e.name, weight: 0.92 },
                            { id: 'mem_…0b3', title: e.name + ' mentioned in session a8b3f2c', weight: 0.87 },
                            { id: 'mem_…1c4', title: 'Recalled regarding ' + e.name, weight: 0.81 },
                        ].map((m, i) => (
                            <div
                                key={i}
                                className="ev-row"
                            >
                                <a
                                    href="#"
                                    className="mono ev-id"
                                >
                                    {m.id}
                                </a>
                                <span className="ev-title">{m.title}</span>
                                <span className="ev-weight mono">{m.weight.toFixed(2)}</span>
                                <span className="ev-bar">
                                    <span
                                        className="ev-fill"
                                        style={{ width: m.weight * 100 + '%' }}
                                    />
                                </span>
                            </div>
                        ))}
                    </div>

                    <div className="action-bar">
                        <button className="btn">
                            <span className="key">m</span>List all mentions
                        </button>
                        <button className="btn">
                            <span className="key">r</span>Rename entity
                        </button>
                        <button className="btn">
                            <span className="key">l</span>Merge / link
                        </button>
                    </div>
                </div>
                <div className="sidecar">
                    <div className="card">
                        <div className="card-head">
                            <span>Entity</span>
                        </div>
                        <dl className="kv">
                            <dt>name</dt>
                            <dd>{e.name}</dd>
                            <dt>kind</dt>
                            <dd>{e.kind}</dd>
                            <dt>mentions</dt>
                            <dd className="mono">{e.mentions.toLocaleString()}</dd>
                            <dt>first seen</dt>
                            <dd className="mono">{e.first_seen}</dd>
                            <dt>last seen</dt>
                            <dd className="mono">{e.last_seen}</dd>
                            <dt>confidence</dt>
                            <dd className="mono">{e.confidence.toFixed(2)}</dd>
                        </dl>
                    </div>
                    <div className="card">
                        <div className="card-head">
                            <span>Co-occurring</span>
                        </div>
                        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
                            {['Trey D.', 'atlasos', 'Helix', 'pnpm'].slice(0, 3).map((n, i) => (
                                <span
                                    key={i}
                                    className="ent-chip"
                                >
                                    {n}
                                </span>
                            ))}
                        </div>
                    </div>
                </div>
            </div>
        </div>
    );
}

Object.assign(window, { EntitiesView, EntityInspector });
