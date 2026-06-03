import { useMemo, useState } from 'react';

import type { EntityKind, EntitySortKey, EntityViewItem } from './entitiesView/types';

import { useEntityDetailQuery, useEntityGraphQuery, type EntityDetailResponse, type EntityNode } from '../api';
import { Inspector, type InspectorItem } from '../inspector';
import { EntityTable } from './entitiesView';
import { QueryErrorBanner, QueryLoadingBanner } from './QueryFeedback';

export type { EntityKind, EntitySortKey, EntityViewItem } from './entitiesView/types';

export type EntityFilter = 'all' | EntityKind;

interface SortState {
    key: EntitySortKey;
    dir: 'asc' | 'desc';
}

const filters: EntityFilter[] = ['all', 'person', 'org', 'project', 'place', 'tool', 'language', 'unknown'];

const KNOWN_KINDS = new Set<EntityKind>(['person', 'org', 'project', 'place', 'tool', 'language']);

// The daemon's InspectEntities response (see crates/memoryd-web/src/routes/entity_graph.rs
// node_from_entity) currently hardcodes kind: "entity" because EntitySummary has no
// kind field — the daemon doesn't classify entities by kind in alpha. Until a
// daemon-side classifier ships, every real entity will normalize to 'unknown' here,
// which is the honest reflection of the daemon's actual state.
function normalizeKind(node: EntityNode): EntityKind {
    const raw = node.kind.toLowerCase();
    return KNOWN_KINDS.has(raw as EntityKind) ? (raw as EntityKind) : 'unknown';
}

function toEntityViewItem(node: EntityNode): EntityViewItem {
    const kind = normalizeKind(node);
    const namespace = node.namespace ?? `entity/${kind}`;
    return {
        id: node.id,
        name: node.label,
        kind,
        mentions: node.memory_count,
        namespaces: [namespace],
        firstSeen: 'unknown',
        lastSeen: 'unknown',
        confidence: null,
        sensitive: namespace.startsWith('personal/'),
        recent: [],
    };
}

function valueForSort(item: EntityViewItem, key: EntitySortKey): string | number {
    if (key === 'namespaces') return item.namespaces.length;
    if (key === 'confidence') return item.confidence ?? -1;
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

function detailConfidence(detail: EntityDetailResponse | undefined): number | null {
    const confidences = detail?.related_memories
        .map((memory) => memory.confidence)
        .filter((value): value is number => typeof value === 'number');
    if (!confidences?.length) return null;
    return confidences.reduce((total, value) => total + value, 0) / confidences.length;
}

function inspectorItemFromEntity(
    entity: EntityViewItem | undefined,
    detail: EntityDetailResponse | undefined,
): InspectorItem | null {
    if (!entity) return null;
    const confidence = detailConfidence(detail);
    const recent =
        detail?.related_memories.map((memory) => ({
            id: memory.id,
            title: `${memory.status} · ${memory.namespace}`,
            score: memory.confidence ?? 0,
        })) ?? [];
    const lastSeen = detail?.last_seen ?? entity.lastSeen;
    const firstSeen = detail?.first_seen ?? entity.firstSeen;
    return {
        kind: 'entity-detail',
        id: entity.id,
        title: entity.name,
        namespace: entity.kind,
        body: `Memorum extracted ${entity.name} from ${entity.mentions.toLocaleString()} mentions across ${entity.namespaces.length} namespace${entity.namespaces.length === 1 ? '' : 's'}.`,
        ...(confidence === null ? {} : { confidence }),
        sensitivity: entity.sensitive ? 'sensitive' : 'plain',
        meta: lastSeen,
        recallCountTotal: entity.mentions,
        recallCount30d: Math.min(entity.mentions, 30),
        evidence: recent,
        provenance: {
            written: lastSeen,
            grounding: entity.namespaces.join(', '),
            confidence: confidence === null ? 'unknown' : confidence.toFixed(2),
            device: entity.kind,
            firstSeen,
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
    const detailQuery = useEntityDetailQuery(selected?.id ?? '');
    const counts = filters.reduce<Record<EntityFilter, number>>(
        (acc, kind) => {
            acc[kind] = kind === 'all' ? items.length : items.filter((item) => item.kind === kind).length;
            return acc;
        },
        { all: 0, person: 0, org: 0, project: 0, place: 0, tool: 0, language: 0, unknown: 0 },
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
        <div data-testid={`entities-view-${filter}`}>
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
                    <div className="pane-scroll">
                        <Inspector
                            item={inspectorItemFromEntity(selected, detailQuery.data)}
                            layout="narrow"
                        />
                    </div>
                </div>
            </div>
        </div>
    );
}
