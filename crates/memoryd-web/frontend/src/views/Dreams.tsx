import { useMemo, useState } from 'react';

import { useReviewQueueQuery, type ReviewQueueItem } from '../api';
import { Inspector, type InspectorItem } from '../inspector';
import { DreamList } from './dreams/DreamList';
import type { DreamStatus, DreamViewItem } from './dreams/types';
import { QueryErrorBanner, QueryLoadingBanner } from './QueryFeedback';

export type { DreamStatus, DreamViewItem } from './dreams/types';

const statuses: DreamStatus[] = ['all', 'proposed', 'queued', 'accepted', 'completed', 'dismissed', 'running'];

function stateFromUrl(): DreamStatus {
    const raw = new URLSearchParams(window.location.search).get('dreamState') as DreamStatus | null;
    return raw && statuses.includes(raw) ? raw : 'all';
}

function dreamKind(item: ReviewQueueItem): DreamViewItem['kind'] {
    const marker = `${item.id} ${item.summary} ${item.namespace} ${item.reason ?? ''}`.toLowerCase();
    if (marker.includes('run')) return 'dream-run';
    if (marker.includes('question')) return 'question';
    if (marker.includes('cleanup')) return 'cleanup';
    return 'pattern';
}

function dreamStatus(item: ReviewQueueItem): DreamViewItem['status'] {
    const normalized = item.status as DreamViewItem['status'];
    if (['proposed', 'queued', 'accepted', 'completed', 'dismissed', 'running'].includes(normalized)) return normalized;
    if ((item.reason ?? '').includes('question')) return 'queued';
    if ((item.reason ?? '').includes('run')) return 'running';
    return 'proposed';
}

function isDreamItem(item: ReviewQueueItem): boolean {
    const marker =
        `${item.id} ${item.summary} ${item.namespace} ${item.reason ?? ''} ${item.policy_applied}`.toLowerCase();
    return marker.includes('dream');
}

function toDreamViewItem(item: ReviewQueueItem, index: number): DreamViewItem {
    const kind = dreamKind(item);
    return {
        id: item.id,
        title: item.summary,
        kind,
        status: dreamStatus(item),
        confidence: Math.max(0.5, 1 - index * 0.04),
        namespace: kind === 'dream-run' ? 'dreams/runs' : item.namespace,
        sub: [kind === 'dream-run' ? 'dream run' : kind, `pass nightly-${index + 1}`],
        meta: index === 0 ? '03:04 today' : `${index + 1}h`,
        pass: `nightly-${index + 1}`,
        evidence: [
            {
                id: `${item.id}_evidence_a`,
                title: item.summary,
                score: Math.max(0.5, 0.9 - index * 0.03),
            },
            {
                id: `${item.id}_evidence_b`,
                title: item.policy_applied,
                score: Math.max(0.4, 0.82 - index * 0.03),
            },
        ],
    };
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
    const query = useReviewQueueQuery({ status: 'dream', limit: 100 });
    const initialFilter = stateFromUrl();
    const [filter, setFilter] = useState<DreamStatus>(initialFilter);
    const items = useMemo(() => query.data?.items.filter(isDreamItem).map(toDreamViewItem) ?? [], [query.data]);
    const visible = useMemo(
        () => (filter === 'all' ? items : items.filter((item) => item.status === filter)),
        [filter, items],
    );
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
            {query.isLoading ? <QueryLoadingBanner label="Dreams" /> : null}
            <QueryErrorBanner
                error={query.error}
                label="Dreams"
            />
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
