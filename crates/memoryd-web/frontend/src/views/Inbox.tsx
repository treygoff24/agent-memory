import { useMemo, useState } from 'react';

import { inboxItems, type InboxItem } from '../data/fixtures';
import { Inspector, type InspectorItem } from '../inspector';
import { Badge, EmptyState } from '../ui';

const filters = ['all', 'review', 'recall', 'conflict', 'dream', 'due'] as const;
type InboxFilter = (typeof filters)[number];

function inspectorItemFromInbox(item: InboxItem): InspectorItem {
    const base = {
        id: item.id,
        title: item.title,
        namespace: item.namespace,
        body: item.body,
        confidence: item.confidence,
        meta: item.meta,
    };
    switch (item.kind) {
        case 'review':
            return { ...base, kind: 'inbox-review' };
        case 'recall':
            return { ...base, kind: 'inbox-recall', memoryId: item.id, summary: item.title };
        case 'conflict':
            return {
                ...base,
                kind: 'inbox-conflict',
                diff: {
                    local: {
                        device: 'local',
                        body: item.body,
                        written: item.meta,
                        session: 'local',
                    },
                    remote: {
                        device: 'remote',
                        body: 'Remote side has a competing assertion for this memory.',
                        written: item.meta,
                        session: 'remote',
                    },
                },
            };
        case 'dream':
            return {
                ...base,
                kind: 'inbox-dream',
                evidence: [
                    { id: 'mem_20260507_a1b2c3d4e5f60718_000011', title: item.title, score: item.confidence },
                ],
            };
        case 'due':
            return { ...base, kind: 'inbox-due' };
    }
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
                            <Inspector item={inspectorItemFromInbox(selected)} />
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
