import { useMemo, useState } from 'react';

import { inboxItems, type InboxItem } from '../data/fixtures';
import { Badge, Card, EmptyState } from '../ui';

const filters = ['all', 'review', 'recall', 'conflict', 'dream', 'due'] as const;
type InboxFilter = (typeof filters)[number];
function ItemInspector({ item }: { item: InboxItem }) {
    return (
        <Card title="Inspector">
            <h2>{item.title}</h2>
            <p>{item.body}</p>
            <p className="mono">
                confidence {item.confidence.toFixed(2)} · {item.namespace}
            </p>
        </Card>
    );
}
export function Inbox() {
    const [filter, setFilter] = useState<InboxFilter>('all');
    const [selectedId, setSelectedId] = useState(inboxItems[0]?.id ?? '');
    const visible = useMemo(
        () => (filter === 'all' ? inboxItems : inboxItems.filter((item) => item.kind === filter)),
        [filter],
    );
    const selected = visible.find((item) => item.id === selectedId) ?? visible[0];
    return (
        <>
            <div className="view-header">
                <span className="view-title">Inbox</span>
                <span className="view-subtitle">· {visible.length} items</span>
                <span className="spacer" />{' '}
                <div className="pills">
                    {filters.map((pill) => (
                        <button
                            key={pill}
                            className={`pill ${filter === pill ? 'active' : ''}`}
                            onClick={() => setFilter(pill)}
                        >
                            {pill}
                            <span className="n">
                                {pill === 'all'
                                    ? inboxItems.length
                                    : inboxItems.filter((item) => item.kind === pill).length}
                            </span>
                        </button>
                    ))}
                </div>
            </div>
            <div className="panes-2">
                <section className="pane left">
                    <div className="list">
                        {visible.map((item) => (
                            <button
                                key={item.id}
                                className={`list-item ${selected?.id === item.id ? 'selected' : ''}`}
                                onClick={() => setSelectedId(item.id)}
                            >
                                <span className={`glyph ${item.kind}`}>◆</span>
                                <span className="body">
                                    <span className="title-line">{item.title}</span>
                                    <span className="sub">
                                        {item.namespace} · {item.meta}
                                    </span>
                                </span>
                                <Badge
                                    tone={item.kind === 'conflict' ? 'bad' : item.kind === 'dream' ? 'warn' : 'neutral'}
                                >
                                    {item.kind}
                                </Badge>
                            </button>
                        ))}
                    </div>
                    {visible.length === 0 && (
                        <EmptyState
                            title="Inbox is clear"
                            body="No matching review items."
                        />
                    )}
                </section>
                <section className="pane">
                    <div className="pane-scroll">
                        {selected ? (
                            <ItemInspector item={selected} />
                        ) : (
                            <EmptyState
                                title="No item selected"
                                body="Choose a memory to inspect."
                            />
                        )}
                    </div>
                </section>
            </div>
        </>
    );
}
