import type { DreamViewItem } from './types';

export function DreamList({
    items,
    selectedId,
    onSelect,
}: {
    items: DreamViewItem[];
    selectedId: string;
    onSelect: (id: string) => void;
}) {
    return (
        <div className="list">
            {items.map((item) => (
                <button
                    key={item.id}
                    className={`list-item ${selectedId === item.id ? 'selected' : ''} ${item.kind === 'dream-run' ? 'dream-run-row' : ''}`}
                    onClick={() => onSelect(item.id)}
                    type="button"
                >
                    <span className={`glyph ${item.kind === 'dream-run' ? 'run' : 'dream'}`}>
                        {item.kind === 'dream-run' ? '◈' : '◇'}
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
                    <span className={`dream-status ${item.status}`}>{item.status}</span>
                    <span className="li-meta mono">{item.confidence.toFixed(2)}</span>
                    <span className="li-meta">{item.meta}</span>
                </button>
            ))}
        </div>
    );
}
