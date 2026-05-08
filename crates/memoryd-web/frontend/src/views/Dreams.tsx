import { dreams } from '../data/fixtures';
import { Badge, Card } from '../ui';
export function Dreams() {
    return (
        <>
            <div className="view-header">
                <span className="view-title">Dreams</span>
                <span className="view-subtitle">· scheduled synthesis</span>
            </div>
            <div className="cards-grid">
                {dreams.map((dream) => (
                    <Card
                        key={dream.id}
                        title={dream.kind}
                    >
                        <h2>{dream.title}</h2>
                        <Badge
                            tone={dream.status === 'running' ? 'ok' : dream.status === 'proposed' ? 'warn' : 'neutral'}
                        >
                            {dream.status}
                        </Badge>
                        <p className="mono">confidence {dream.confidence.toFixed(2)}</p>
                    </Card>
                ))}
            </div>
        </>
    );
}
