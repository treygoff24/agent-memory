import type { InspectorItem } from '../../inspector';
import type { InboxFilterId, InboxItem, InboxKind, InboxViewItem } from './types';

export const inboxFilters = [
    { id: 'all', label: 'all', key: '1' },
    { id: 'review', label: 'review', key: '2' },
    { id: 'conflicts', label: 'conflicts', key: '3' },
    { id: 'recall', label: 'recall', key: '4' },
    { id: 'dreams', label: 'dreams', key: '5' },
    { id: 'due', label: 'due', key: '6' },
] as const;

export const filterForKind: Record<InboxKind, InboxFilterId> = {
    review: 'review',
    conflict: 'conflicts',
    recall: 'recall',
    dream: 'dreams',
    due: 'due',
};

const glyphForKind: Record<InboxKind, string> = {
    review: '●',
    conflict: '⚠',
    recall: '▸',
    dream: '◇',
    due: '▣',
};

export function toInboxViewItem(item: InboxItem): InboxViewItem {
    return {
        ...item,
        glyph: glyphForKind[item.kind],
        sub: [item.meta],
    };
}

export function inspectorItemFromInbox(item: InboxViewItem): InspectorItem {
    const base = {
        id: item.id,
        title: item.title,
        namespace: item.namespace,
        body: item.body,
        confidence: item.confidence,
        meta: item.meta,
        provenance: {
            written: item.meta,
            confidence: item.confidence.toFixed(2),
            device: 'local',
            peers: '0 of 0',
        },
        policy: {
            privacy: 'plaintext',
            governance: item.kind === 'conflict' ? 'manual-review' : 'auto-approve',
            tombstone: 'none',
        },
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
                evidence: [{ id: 'mem_20260507_a1b2c3d4e5f60718_000011', title: item.title, score: item.confidence }],
            };
        case 'due':
            return { ...base, kind: 'inbox-due' };
    }
}

export function filterItems(items: InboxViewItem[], filter: InboxFilterId): InboxViewItem[] {
    if (filter === 'all') return items;
    return items.filter((item) => filterForKind[item.kind] === filter);
}
