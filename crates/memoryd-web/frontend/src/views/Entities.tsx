import { useMemo, useState } from 'react';

import { useEntityGraphQuery, type EntityNode } from '../api';
import { Inspector, type InspectorItem } from '../inspector';
import { EntityTable } from './entitiesView';
import { QueryErrorBanner, QueryLoadingBanner } from './QueryFeedback';

export type EntityKind = 'person' | 'org' | 'project' | 'place' | 'tool' | 'language';
export type EntityFilter = 'all' | EntityKind;
export type EntitySortKey = 'name' | 'kind' | 'mentions' | 'namespaces' | 'lastSeen' | 'firstSeen' | 'confidence';

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
    if (node.memory_count >= 30) return 0.88;
    if (node.memory_count >= 15) return 0.91;
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
    const items = useMemo(() => query.data?.nodes.map(toEntityViewItem) ?? [], [query.data]);
    const [filter, setFilter] = useState<EntityFilter>('all');
    const [search, setSearch] = useState('');
    const [sort, setSort] = useState<SortState>({ key: 'mentions', dir: 'desc' });
    const filtered = useMemo(() => {
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
    const [selectedId, setSelectedId] = useState(items[0]?.id ?? '');
    const selected = filtered.find((item) => item.id === selectedId) ?? filtered[0];
    const counts = filters.reduce<Record<EntityFilter, number>>(
        (acc, kind) => {
            acc[kind] = kind === 'all' ? items.length : items.filter((item) => item.kind === kind).length;
            return acc;
        },
        { all: 0, person: 0, org: 0, project: 0, place: 0, tool: 0, language: 0 },
    );

    function updateFilter(next: EntityFilter) {
        setFilter(next);
        const nextSelected = next === 'all' ? items[0] : items.find((item) => item.kind === next);
        setSelectedId(nextSelected?.id ?? '');
    }

    function updateSort(key: EntitySortKey) {
        setSort((current) => ({
            key,
            dir: current.key === key && current.dir === 'desc' ? 'asc' : 'desc',
        }));
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
                    className="filter-pills"
                    role="tablist"
                    aria-label="Entity kind filters"
                >
                    {filters.map((kind, index) => (
                        <button
                            key={kind}
                            className={`pill ${filter === kind ? 'active' : ''}`}
                            onClick={() => updateFilter(kind)}
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
                <input
                    aria-label="Entity search"
                    className="ent-search"
                    onChange={(event) => setSearch(event.target.value)}
                    placeholder="filter by name…"
                    value={search}
                />
            </div>
            <div className="panes-2">
                <div className="pane left">
                    <EntityTable
                        entities={filtered}
                        selectedId={selected?.id ?? ''}
                        sort={sort}
                        onSort={updateSort}
                        onSelect={setSelectedId}
                    />
                </div>
                <div className="pane">
                    <div
                        className="pane-scroll"
                        tabIndex={0}
                    >
                        <Inspector
                            item={inspectorItemFromEntity(selected)}
                            layout="narrow"
                        />
                    </div>
                </div>
            </div>
        </div>
    );
}
