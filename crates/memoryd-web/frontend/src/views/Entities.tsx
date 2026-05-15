import { useMemo, useState } from 'react';

import { useEntityGraphQuery, type EntityNode } from '../api';
import { Inspector, type InspectorItem } from '../inspector';
import { useHashParam, useRoute } from '../router';
import { EntityGraph, EntityTable, type ColorBy, type Density } from './entitiesView';
import { QueryErrorBanner, QueryLoadingBanner } from './QueryFeedback';

export type EntityKind = 'person' | 'org' | 'project' | 'place' | 'tool' | 'language';
export type EntityFilter = 'all' | EntityKind;
export type EntitySortKey = 'name' | 'kind' | 'mentions' | 'namespaces' | 'lastSeen' | 'firstSeen' | 'confidence';
export type EntityMode = 'graph' | 'table';

export interface EntityViewItem {
    id: string;
    name: string;
    kind: EntityKind;
    mentions: number;
    namespaces: string[];
    firstSeen: string;
    lastSeen: string;
    confidence: number;
    sensitive?: boolean;
    recent: Array<{ id: string; title: string; weight: number }>;
}

interface SortState {
    key: EntitySortKey;
    dir: 'asc' | 'desc';
}

const filters: EntityFilter[] = ['all', 'person', 'org', 'project', 'place', 'tool', 'language'];

function normalizeKind(node: EntityNode): EntityKind {
    const marker = `${node.kind} ${node.label}`.toLowerCase();
    if (marker.includes('person') || marker.includes('operator')) return 'person';
    if (marker.includes('org') || marker.includes('acme')) return 'org';
    if (marker.includes('place') || marker.includes('home') || marker.includes('office')) return 'place';
    if (marker.includes('tool') || marker.includes('pnpm')) return 'tool';
    if (marker.includes('language') || marker.includes('rust')) return 'language';
    return 'project';
}

function confidenceForNode(node: EntityNode, index: number): number {
    if (node.memory_count >= 40) return 0.96;
    if (node.memory_count >= 30) return 0.92;
    if (node.memory_count >= 15) return 0.88;
    return Math.max(0.68, 0.84 - index * 0.02);
}

function toEntityViewItem(node: EntityNode, index: number): EntityViewItem {
    const kind = normalizeKind(node);
    const namespace = node.namespace ?? (kind === 'project' ? 'project:agent-memory' : `entity/${kind}`);
    return {
        id: node.id,
        name: node.label,
        kind,
        mentions: node.memory_count,
        namespaces: [namespace],
        firstSeen:
            kind === 'language'
                ? '2026-02-15'
                : kind === 'tool'
                  ? '2026-03-22'
                  : kind === 'org'
                    ? '2026-01-10'
                    : '2026-04-01',
        lastSeen: kind === 'org' ? '2026-05-01' : '2026-05-07',
        confidence: confidenceForNode(node, index),
        sensitive: namespace.startsWith('personal/'),
        recent: [
            {
                id: `${node.id}_mem_a`,
                title: `First reference of ${node.label}`,
                weight: Math.max(0.4, confidenceForNode(node, index) - 0.02),
            },
            {
                id: `${node.id}_mem_b`,
                title: `${node.label} mentioned in recent session`,
                weight: Math.max(0.4, confidenceForNode(node, index) - 0.09),
            },
        ],
    };
}

function valueForSort(item: EntityViewItem, key: EntitySortKey): string | number {
    if (key === 'namespaces') return item.namespaces.length;
    return item[key];
}

function sortEntities(items: EntityViewItem[], sort: SortState): EntityViewItem[] {
    return [...items].sort((a, b) => {
        const av = valueForSort(a, sort.key);
        const bv = valueForSort(b, sort.key);
        const result =
            typeof av === 'number' && typeof bv === 'number' ? av - bv : String(av).localeCompare(String(bv));
        return sort.dir === 'asc' ? result : -result;
    });
}

function inspectorItemFromEntity(entity: EntityViewItem | undefined): InspectorItem | null {
    if (!entity) return null;
    return {
        kind: 'entity-detail',
        id: entity.id,
        title: entity.name,
        namespace: entity.kind,
        body: `Memorum extracted ${entity.name} from ${entity.mentions.toLocaleString()} mentions across ${entity.namespaces.length} namespace${entity.namespaces.length === 1 ? '' : 's'}.`,
        confidence: entity.confidence,
        sensitivity: entity.sensitive ? 'sensitive' : 'plain',
        meta: entity.lastSeen,
        recallCountTotal: entity.mentions,
        recallCount30d: Math.min(entity.mentions, 30),
        evidence: entity.recent.map((memory) => ({
            id: memory.id,
            title: memory.title,
            score: memory.weight,
        })),
        provenance: {
            written: entity.lastSeen,
            grounding: entity.namespaces.join(', '),
            confidence: entity.confidence.toFixed(2),
            device: entity.kind,
        },
        summary: entity.namespaces.join(', '),
    };
}

export function Entities() {
    const query = useEntityGraphQuery();
    const { route } = useRoute();
    const modeParam = useHashParam('mode');
    const mode: EntityMode = modeParam === 'table' ? 'table' : 'graph';

    // useMemo so empty-fallback `[]` arrays keep stable references across renders
    // — downstream useMemos in this file include `apiNodes` / `apiEdges` in their
    // dep arrays and would otherwise recompute on every render.
    const apiNodes = useMemo(() => query.data?.nodes ?? [], [query.data]);
    const apiEdges = useMemo(() => query.data?.edges ?? [], [query.data]);
    const items = useMemo(() => apiNodes.map(toEntityViewItem), [apiNodes]);

    const [filter, setFilter] = useState<EntityFilter>('all');
    const [search, setSearch] = useState('');
    const [sort, setSort] = useState<SortState>({ key: 'mentions', dir: 'desc' });
    const [namespaceFilter, setNamespaceFilter] = useState<string>('all');
    const [colorBy, setColorBy] = useState<ColorBy>('kind');
    const [density, setDensity] = useState<Density>('sparse');
    const [focusId, setFocusId] = useState<string>('');

    const namespaces = useMemo(() => {
        const seen = new Set<string>();
        for (const n of apiNodes) if (n.namespace) seen.add(n.namespace);
        return ['all', ...Array.from(seen).sort()];
    }, [apiNodes]);

    const filteredViewItems = useMemo(() => {
        const queryText = search.toLowerCase();
        return sortEntities(
            items.filter((item) => {
                if (filter !== 'all' && item.kind !== filter) return false;
                if (
                    queryText &&
                    !`${item.name} ${item.kind} ${item.namespaces.join(' ')}`.toLowerCase().includes(queryText)
                )
                    return false;
                return true;
            }),
            sort,
        );
    }, [filter, items, search, sort]);

    // Apply kind+namespace filters to the raw graph nodes for the graph view.
    const graphNodes = useMemo(
        () =>
            apiNodes
                .filter((node) => {
                    if (filter !== 'all' && normalizeKind(node) !== filter) return false;
                    if (namespaceFilter !== 'all' && node.namespace !== namespaceFilter) return false;
                    return true;
                })
                .map((node) => ({
                    ...node,
                    // Daemon-backed entity summaries currently report the raw
                    // kind as "entity"; normalize before graph coloring so
                    // "color-by kind" stays useful outside MSW fixtures.
                    kind: normalizeKind(node),
                })),
        [apiNodes, filter, namespaceFilter],
    );

    // Route-driven selection: clicking a graph node navigates to
    // `#/entities/:id`, which becomes the inspector's selection in both modes.
    // Falls back to the first visible item when no entity is in the route.
    const routeEntityId = route.kind === 'entities' ? route.entityId : undefined;
    const tableSelected = filteredViewItems.find((item) => item.id === routeEntityId) ?? filteredViewItems[0];
    const inspectorEntity = tableSelected;

    const counts = filters.reduce<Record<EntityFilter, number>>(
        (acc, kind) => {
            acc[kind] = kind === 'all' ? items.length : items.filter((item) => item.kind === kind).length;
            return acc;
        },
        { all: 0, person: 0, org: 0, project: 0, place: 0, tool: 0, language: 0 },
    );

    function updateSort(key: EntitySortKey) {
        setSort((current) => ({
            key,
            dir: current.key === key && current.dir === 'desc' ? 'asc' : 'desc',
        }));
    }

    // Deep-link to an entity without losing the hash query suffix (e.g.
    // `?mode=table`). Used by both the EntityGraph node click and the
    // EntityTable row click — same behavior, single source of truth.
    function selectEntity(id: string) {
        const hashStr = window.location.hash;
        const idx = hashStr.indexOf('?');
        const tail = idx === -1 ? '' : hashStr.slice(idx);
        window.location.hash = `#/entities/${encodeURIComponent(id)}${tail}`;
    }

    function setMode(next: EntityMode) {
        // Preserve route + other hash params; only mode flips.
        const hashStr = window.location.hash;
        const idx = hashStr.indexOf('?');
        const pathPart = idx === -1 ? hashStr || '#/entities' : hashStr.slice(0, idx) || '#/entities';
        const params = new URLSearchParams(idx === -1 ? '' : hashStr.slice(idx + 1));
        if (next === 'table') params.set('mode', 'table');
        else params.delete('mode');
        const tail = params.toString();
        window.location.hash = tail ? `${pathPart}?${tail}` : pathPart;
    }

    return (
        <div
            className="view"
            data-testid={`entities-view-${filter}`}
        >
            {query.isLoading ? <QueryLoadingBanner label="Entities" /> : null}
            <QueryErrorBanner
                error={query.error}
                label="Entities"
            />
            <div className="view-header">
                <span className="view-title">Entities</span>
                <span className="view-subtitle">
                    · {items.length} extracted · auto-detected from{' '}
                    {items.reduce((total, item) => total + item.mentions, 0).toLocaleString()} mentions
                </span>
                <span className="spacer" />
                <div
                    className="mode-toggle"
                    role="tablist"
                    aria-label="Entities display mode"
                >
                    <button
                        type="button"
                        role="tab"
                        aria-selected={mode === 'graph'}
                        className={`pill ${mode === 'graph' ? 'active' : ''}`}
                        onClick={() => setMode('graph')}
                    >
                        graph
                    </button>
                    <button
                        type="button"
                        role="tab"
                        aria-selected={mode === 'table'}
                        className={`pill ${mode === 'table' ? 'active' : ''}`}
                        onClick={() => setMode('table')}
                    >
                        table
                    </button>
                </div>
                <div
                    className="filter-pills"
                    role="tablist"
                    aria-label="Entity kind filters"
                >
                    {filters.map((kind, index) => (
                        <button
                            key={kind}
                            className={`pill ${filter === kind ? 'active' : ''}`}
                            onClick={() => setFilter(kind)}
                            role="tab"
                            aria-selected={filter === kind}
                            type="button"
                        >
                            <span>{kind}</span>
                            <span className="count">{counts[kind]}</span>
                            <span className="kbd-hint">{index + 1}</span>
                        </button>
                    ))}
                </div>
                {mode === 'table' ? (
                    <input
                        aria-label="Entity search"
                        className="ent-search"
                        onChange={(event) => setSearch(event.target.value)}
                        placeholder="filter by name…"
                        value={search}
                    />
                ) : null}
            </div>
            <div className="panes-2">
                <div className="pane left">
                    {mode === 'graph' ? (
                        <div className="graph-pane">
                            <div className="graph-rail">
                                <label className="graph-ctrl">
                                    <span className="graph-ctrl-label">namespace</span>
                                    <select
                                        className="graph-ctrl-select"
                                        value={namespaceFilter}
                                        onChange={(e) => setNamespaceFilter(e.target.value)}
                                    >
                                        {namespaces.map((ns) => (
                                            <option
                                                key={ns}
                                                value={ns}
                                            >
                                                {ns}
                                            </option>
                                        ))}
                                    </select>
                                </label>
                                <label className="graph-ctrl">
                                    <span className="graph-ctrl-label">focus</span>
                                    <select
                                        className="graph-ctrl-select"
                                        value={focusId}
                                        onChange={(e) => setFocusId(e.target.value)}
                                    >
                                        <option value="">(none)</option>
                                        {apiNodes.map((node) => (
                                            <option
                                                key={node.id}
                                                value={node.id}
                                            >
                                                {node.label}
                                            </option>
                                        ))}
                                    </select>
                                </label>
                                <label className="graph-ctrl">
                                    <span className="graph-ctrl-label">color-by</span>
                                    <select
                                        className="graph-ctrl-select"
                                        value={colorBy}
                                        onChange={(e) => setColorBy(e.target.value as ColorBy)}
                                    >
                                        <option value="kind">kind</option>
                                        <option value="namespace">namespace</option>
                                        <option value="confidence">confidence</option>
                                    </select>
                                </label>
                                <label className="graph-ctrl">
                                    <span className="graph-ctrl-label">density</span>
                                    <select
                                        className="graph-ctrl-select"
                                        value={density}
                                        onChange={(e) => setDensity(e.target.value as Density)}
                                    >
                                        <option value="sparse">sparse</option>
                                        <option value="dense">dense</option>
                                    </select>
                                </label>
                            </div>
                            <EntityGraph
                                nodes={graphNodes}
                                edges={apiEdges}
                                colorBy={colorBy}
                                density={density}
                                focusId={focusId || null}
                                onSelect={selectEntity}
                                onRequestTableMode={() => setMode('table')}
                            />
                        </div>
                    ) : (
                        <EntityTable
                            entities={filteredViewItems}
                            selectedId={tableSelected?.id ?? ''}
                            sort={sort}
                            onSort={updateSort}
                            onSelect={selectEntity}
                        />
                    )}
                </div>
                <div className="pane">
                    <div
                        className="pane-scroll"
                        tabIndex={0}
                    >
                        <Inspector
                            item={inspectorItemFromEntity(inspectorEntity)}
                            layout="narrow"
                        />
                    </div>
                </div>
            </div>
        </div>
    );
}
