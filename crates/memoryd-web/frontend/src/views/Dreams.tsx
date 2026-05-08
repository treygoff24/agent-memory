import { useMemo, useState } from 'react';

import { dreams, type DreamItem } from '../data/fixtures';
import { Inspector, type InspectorItem } from '../inspector';
import { DreamList } from './dreams/DreamList';

export type DreamStatus = 'all' | 'proposed' | 'queued' | 'accepted' | 'completed' | 'dismissed' | 'running';

export interface DreamViewItem extends DreamItem {
    status: Exclude<DreamStatus, 'all'>;
    namespace: string;
    sub: string[];
    meta: string;
    pass: string;
    evidence: Array<{ id: string; title: string; score: number }>;
    kind: 'pattern' | 'question' | 'cleanup' | 'dream-run';
}

const statuses: DreamStatus[] = ['all', 'proposed', 'queued', 'accepted', 'completed', 'dismissed', 'running'];

function stateFromUrl(): DreamStatus {
    const raw = new URLSearchParams(window.location.search).get('dreamState') as DreamStatus | null;
    return raw && statuses.includes(raw) ? raw : 'all';
}

function toDreamViewItem(item: DreamItem, index: number): DreamViewItem {
    const kind = item.kind === 'dream-run' ? 'dream-run' : item.kind === 'question' ? 'question' : item.kind === 'cleanup' ? 'cleanup' : 'pattern';
    return {
        ...item,
        kind,
        status: item.status,
        namespace: kind === 'dream-run' ? 'dreams/runs' : 'dreams/patterns',
        sub: [kind === 'dream-run' ? 'dream run' : kind, `pass nightly-${index + 1}`],
        meta: index === 0 ? '03:04 today' : `${index + 1}h`,
        pass: `nightly-${index + 1}`,
        evidence: [
            { id: `mem_evidence_${index}_a`, title: item.title, score: item.confidence },
            { id: `mem_evidence_${index}_b`, title: 'Supporting recall cluster', score: Math.max(0.4, item.confidence - 0.12) },
        ],
    };
}

function dreamItems(): DreamViewItem[] {
    const base = dreams.map(toDreamViewItem);
    return [
        ...base,
        toDreamViewItem({ id: 'dream_accepted_1', status: 'accepted', title: 'Accepted: prefers evidence-first reviews', kind: 'pattern', confidence: 0.86 }, base.length),
        toDreamViewItem({ id: 'dream_dismissed_1', status: 'dismissed', title: 'Dismissed: stale editor inference', kind: 'pattern', confidence: 0.51 }, base.length + 1),
    ];
}

function inspectorItemFromDream(item: DreamViewItem | undefined): InspectorItem | null {
    if (!item) return null;
    return {
        kind: 'dream-output',
        id: item.id,
        title: item.title,
        namespace: item.namespace,
        body:
            item.kind === 'dream-run'
                ? `Dream run meta-entry for pass ${item.pass}; summarizes the scheduled synthesis pass separately from per-output entries.`
                : `Dream pass ${item.pass} synthesized this ${item.kind} from ${item.evidence.length} memories.`,
        confidence: item.confidence,
        evidence: item.evidence,
        summary: item.kind === 'dream-run' ? 'dream run meta-entry' : item.title,
        meta: item.status,
        provenance: {
            written: item.meta,
            confidence: item.confidence.toFixed(2),
            grounding: item.pass,
        },
    };
}

export function Dreams() {
    const initialFilter = stateFromUrl();
    const [filter, setFilter] = useState<DreamStatus>(initialFilter);
    const items = useMemo(dreamItems, []);
    const visible = useMemo(() => (filter === 'all' ? items : items.filter((item) => item.status === filter)), [filter, items]);
    const [selectedId, setSelectedId] = useState(visible[0]?.id ?? '');
    const selected = visible.find((item) => item.id === selectedId) ?? visible[0];
    const counts = statuses.reduce<Record<DreamStatus, number>>(
        (acc, status) => {
            acc[status] = status === 'all' ? items.length : items.filter((item) => item.status === status).length;
            return acc;
        },
        { all: 0, proposed: 0, queued: 0, accepted: 0, completed: 0, dismissed: 0, running: 0 },
    );

    return (
        <div data-testid={`dreams-view-${filter}`}>
            <div className="view-header">
                <span className="view-title">Dreams</span>
                <span className="view-subtitle">· {items.length} · last run 03:04 today</span>
                <span className="spacer" />
                <div
                    className="filter-pills"
                    role="tablist"
                    aria-label="Dream status filters"
                >
                    {statuses.map((status, index) => (
                        <button
                            key={status}
                            className={`pill ${filter === status ? 'active' : ''}`}
                            onClick={() => {
                                setFilter(status);
                                const next = status === 'all' ? items[0] : items.find((item) => item.status === status);
                                setSelectedId(next?.id ?? '');
                            }}
                            role="tab"
                            aria-selected={filter === status}
                            type="button"
                        >
                            <span>{status}</span>
                            <span className="count">{counts[status]}</span>
                            <span className="kbd-hint">{index + 1}</span>
                        </button>
                    ))}
                </div>
            </div>
            <div className="panes-2">
                <div className="pane left">
                    <div className="pane-scroll">
                        <DreamList
                            items={visible}
                            selectedId={selected?.id ?? ''}
                            onSelect={setSelectedId}
                        />
                    </div>
                </div>
                <div className="pane">
                    <div className="pane-scroll">
                        <Inspector
                            item={inspectorItemFromDream(selected)}
                            layout="narrow"
                        />
                    </div>
                </div>
            </div>
        </div>
    );
}
