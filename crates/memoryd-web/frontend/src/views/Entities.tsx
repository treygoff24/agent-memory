import { useMemo, useState } from 'react';

import { entities, type EntityItem } from '../data/fixtures';
import { Inspector, type InspectorItem } from '../inspector';
import { EntityTable } from './entitiesView';

export type EntityFilter = 'all' | EntityItem['kind'];
export type EntitySortKey = 'name' | 'kind' | 'mentions' | 'namespaces' | 'lastSeen' | 'firstSeen' | 'confidence';

export interface EntityViewItem extends EntityItem {
    sensitive?: boolean;
    recent: Array<{ id: string; title: string; weight: number }>;
}

interface SortState {
    key: EntitySortKey;
    dir: 'asc' | 'desc';
}

const filters: EntityFilter[] = ['all', 'person', 'org', 'project', 'place', 'tool', 'language'];

function entityRows(): EntityViewItem[] {
    return [
        ...entities.map((entity, index) => ({
            ...entity,
            recent: [
                { id: `${entity.id}_mem_a`, title: `First reference of ${entity.name}`, weight: Math.max(0.4, entity.confidence - 0.02) },
                { id: `${entity.id}_mem_b`, title: `${entity.name} mentioned in recent session`, weight: Math.max(0.4, entity.confidence - 0.09) },
            ],
            sensitive: index === 3,
        })),
        {
            id: 'ent_operator',
            name: 'Operator',
            kind: 'person',
            mentions: 12,
            namespaces: ['me/preferences', 'project:agent-memory'],
            firstSeen: '2026-03-01',
            lastSeen: '2026-05-08',
            confidence: 0.82,
            recent: [
                { id: 'ent_operator_mem_a', title: 'Operator asked for evidence-first review', weight: 0.84 },
                { id: 'ent_operator_mem_b', title: 'Operator accepted concise status reports', weight: 0.76 },
            ],
        },
        {
            id: 'ent_home_office',
            name: 'Home office',
            kind: 'place',
            mentions: 7,
            namespaces: ['personal/logistics'],
            firstSeen: '2026-02-03',
            lastSeen: '2026-04-30',
            confidence: 0.73,
            recent: [
                { id: 'ent_home_office_mem_a', title: 'Home office device handoff note', weight: 0.71 },
                { id: 'ent_home_office_mem_b', title: 'Location-specific sync fence', weight: 0.66 },
            ],
        },
    ];
}

function valueForSort(item: EntityViewItem, key: EntitySortKey): string | number {
    if (key === 'namespaces') return item.namespaces.length;
    return item[key];
}

function sortEntities(items: EntityViewItem[], sort: SortState): EntityViewItem[] {
    return [...items].sort((a, b) => {
        const av = valueForSort(a, sort.key);
        const bv = valueForSort(b, sort.key);
        const result = typeof av === 'number' && typeof bv === 'number' ? av - bv : String(av).localeCompare(String(bv));
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
    const items = useMemo(entityRows, []);
    const [filter, setFilter] = useState<EntityFilter>('all');
    const [search, setSearch] = useState('');
    const [sort, setSort] = useState<SortState>({ key: 'mentions', dir: 'desc' });
    const filtered = useMemo(() => {
        const query = search.toLowerCase();
        return sortEntities(
            items.filter((item) => {
                if (filter !== 'all' && item.kind !== filter) return false;
                if (query && !`${item.name} ${item.kind} ${item.namespaces.join(' ')}`.toLowerCase().includes(query)) return false;
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
        setSort((current) => ({ key, dir: current.key === key && current.dir === 'desc' ? 'asc' : 'desc' }));
    }

    return (
        <div data-testid={`entities-view-${filter}`}>
            <div className="view-header">
                <span className="view-title">Entities</span>
                <span className="view-subtitle">
                    · {items.length} extracted · auto-detected from {items.reduce((total, item) => total + item.mentions, 0).toLocaleString()} mentions
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
                            item={inspectorItemFromEntity(selected)}
                            layout="narrow"
                        />
                    </div>
                </div>
            </div>
        </div>
    );
}
