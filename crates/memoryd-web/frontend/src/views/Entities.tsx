import { entities } from '../data/fixtures';
export function Entities() {
    return (
        <>
            <div className="view-header">
                <span className="view-title">Entities</span>
                <span className="view-subtitle">· {entities.length} extracted</span>
            </div>
            <div className="ent-table">
                <div className="ent-thead">
                    <span>name</span>
                    <span>kind</span>
                    <span>mentions</span>
                    <span>namespaces</span>
                    <span>last seen</span>
                    <span>first seen</span>
                    <span>confidence</span>
                </div>
                {entities.map((entity) => (
                    <div
                        className="ent-row"
                        key={entity.id}
                    >
                        <span className="ent-name">{entity.name}</span>
                        <span>{entity.kind}</span>
                        <span className="mono">{entity.mentions}</span>
                        <span>{entity.namespaces.join(', ')}</span>
                        <span className="mono">{entity.lastSeen}</span>
                        <span className="mono">{entity.firstSeen}</span>
                        <span className="mono">{entity.confidence.toFixed(2)}</span>
                    </div>
                ))}
            </div>
        </>
    );
}
