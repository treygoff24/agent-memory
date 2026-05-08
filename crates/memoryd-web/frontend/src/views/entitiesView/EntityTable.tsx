import type { EntitySortKey, EntityViewItem } from '../Entities';

interface EntityTableProps {
    entities: EntityViewItem[];
    selectedId: string;
    sort: { key: EntitySortKey; dir: 'asc' | 'desc' };
    onSort: (key: EntitySortKey) => void;
    onSelect: (id: string) => void;
}

const columns: Array<{ key: EntitySortKey; label: string }> = [
    { key: 'name', label: 'name' },
    { key: 'kind', label: 'kind' },
    { key: 'mentions', label: 'mentions' },
    { key: 'namespaces', label: 'namespaces' },
    { key: 'lastSeen', label: 'last seen' },
    { key: 'firstSeen', label: 'first seen' },
    { key: 'confidence', label: 'confidence' },
];

export function EntityTable({ entities, selectedId, sort, onSort, onSelect }: EntityTableProps) {
    return (
        <div className="ent-table">
            <div className="ent-thead">
                {columns.map((column) => (
                    <button
                        key={column.key}
                        aria-label={`Sort by ${column.label}${sort.key === column.key ? ` ${sort.dir}` : ''}`}
                        aria-pressed={sort.key === column.key}
                        className={`th ${sort.key === column.key ? 'active' : ''}`}
                        onClick={() => onSort(column.key)}
                        type="button"
                    >
                        <span>{column.label}</span>
                        {sort.key === column.key ? (
                            <span className="th-arrow mono">{sort.dir === 'asc' ? '↑' : '↓'}</span>
                        ) : null}
                    </button>
                ))}
            </div>
            <div className="pane-scroll">
                {entities.map((entity) => (
                    <button
                        key={entity.id}
                        className={`ent-row ${selectedId === entity.id ? 'selected' : ''}`}
                        data-testid="entity-row"
                        onClick={() => onSelect(entity.id)}
                        type="button"
                    >
                        <span className="ent-name">
                            {entity.name}
                            {entity.sensitive ? <span className="badge bad">sensitive</span> : null}
                        </span>
                        <span className="ent-kind">{entity.kind}</span>
                        <span className="ent-mentions mono">{entity.mentions.toLocaleString()}</span>
                        <span className="ent-ns mono">
                            {entity.namespaces[0]}
                            {entity.namespaces.length > 1 ? ` +${entity.namespaces.length - 1}` : ''}
                        </span>
                        <span className="ent-time mono">{entity.lastSeen}</span>
                        <span className="ent-time mono">{entity.firstSeen}</span>
                        <span className="ent-conf mono">
                            <span className="conf-bar">
                                <span
                                    className="conf-fill"
                                    style={{ width: `${entity.confidence * 100}%` }}
                                />
                            </span>
                            {entity.confidence.toFixed(2)}
                        </span>
                    </button>
                ))}
            </div>
        </div>
    );
}
