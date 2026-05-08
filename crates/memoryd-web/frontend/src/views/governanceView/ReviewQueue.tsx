import type { GovernanceViewItem } from '../Governance';

interface ReviewQueueProps {
    items: GovernanceViewItem[];
    selectedId: string;
    checked: Set<string>;
    onSelect: (id: string) => void;
    onCheck: (id: string) => void;
}

function severityTone(severity: GovernanceViewItem['severity']): string {
    if (severity === 'block') return 'bad';
    if (severity === 'warn') return 'warn';
    return 'ok';
}

export function ReviewQueue({ items, selectedId, checked, onSelect, onCheck }: ReviewQueueProps) {
    return (
        <div className="list">
            {items.map((item) => (
                <div
                    key={item.id}
                    className={`gov-row list-item ${selectedId === item.id ? 'selected' : ''}`}
                    data-testid="governance-row"
                    onClick={() => onSelect(item.id)}
                    onKeyDown={(event) => {
                        if (event.key === 'Enter' || event.key === ' ') {
                            event.preventDefault();
                            onSelect(item.id);
                        }
                    }}
                    role="button"
                    tabIndex={0}
                >
                    <span
                        className="gov-check"
                        onClick={(event) => event.stopPropagation()}
                    >
                        <input
                            aria-label={`Select ${item.title}`}
                            checked={checked.has(item.id)}
                            onChange={() => onCheck(item.id)}
                            type="checkbox"
                        />
                    </span>
                    <span className={`glyph ${item.severity === 'block' ? 'conflict' : 'due'}`}>
                        <span className={`status-dot ${severityTone(item.severity)}`} />
                    </span>
                    <span className="li-main">
                        <span className="li-title">{item.title}</span>
                        <span className="li-sub">
                            <span className="ns">{item.namespace}</span>
                            {item.sub.map((part) => (
                                <span key={part}>
                                    <span className="dot">·</span> {part}
                                </span>
                            ))}
                        </span>
                    </span>
                    <span className={`badge ${severityTone(item.severity)}`}>{item.decision}</span>
                    <span className="li-meta">{item.meta}</span>
                </div>
            ))}
        </div>
    );
}
