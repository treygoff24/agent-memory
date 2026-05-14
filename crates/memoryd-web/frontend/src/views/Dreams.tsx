import { useMemo, useState } from 'react';

import { useReviewQueueQuery, useStatusQuery, type ReviewQueueItem } from '../api';
import { Inspector, type InspectorItem } from '../inspector';
import { hashParams } from '../router';
import { EmptyState } from '../ui';
import { DreamList } from './dreams/DreamList';
import { QueryErrorBanner, QueryLoadingBanner } from './QueryFeedback';

export type DreamStatus = 'all' | 'proposed' | 'queued' | 'accepted' | 'completed' | 'dismissed' | 'running';
export type DreamTab = 'journal' | 'questions' | 'cleanup';

export interface DreamViewItem {
    id: string;
    status: Exclude<DreamStatus, 'all'>;
    title: string;
    confidence: number;
    namespace: string;
    sub: string[];
    meta: string;
    pass: string;
    evidence: Array<{ id: string; title: string; score: number }>;
    kind: 'pattern' | 'question' | 'cleanup' | 'dream-run';
}

const statuses: DreamStatus[] = ['all', 'proposed', 'queued', 'accepted', 'completed', 'dismissed', 'running'];

// Brief §View 4 mandates Journal / Questions / Cleanup tabs. Status pills
// nest under the active tab as within-tab filters.
const tabs: Array<{ id: DreamTab; label: string; kinds: ReadonlyArray<DreamViewItem['kind']> }> = [
    { id: 'journal', label: 'Journal', kinds: ['pattern', 'dream-run'] },
    { id: 'questions', label: 'Questions', kinds: ['question'] },
    { id: 'cleanup', label: 'Cleanup', kinds: ['cleanup'] },
];

function tabFromUrl(): DreamTab {
    const raw = hashParams(window.location.hash).get('dreamTab');
    return tabs.find((tab) => tab.id === raw)?.id ?? 'journal';
}

function statusFromUrl(): DreamStatus {
    const raw = hashParams(window.location.hash).get('dreamState') as DreamStatus | null;
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

function formatDreamHeader(
    items: DreamViewItem[],
    lastRunAt: string | undefined,
    nextScheduled: string | undefined,
): string {
    const promoted = items.filter((item) => item.status === 'accepted').length;
    const queued = items.filter((item) => item.status === 'queued').length;
    const dropped = items.filter((item) => item.status === 'dismissed').length;
    const parts = [
        `Last dream: ${lastRunAt ?? '—'}`,
        'scope: all',
        `promoted ${promoted} / queued ${queued} / dropped ${dropped}`,
    ];
    if (nextScheduled) parts.push(`next scheduled ${nextScheduled}`);
    return parts.join(' · ');
}

export function Dreams() {
    const query = useReviewQueueQuery({ status: 'dream', limit: 100 });
    const status = useStatusQuery();
    const [activeTab, setActiveTab] = useState<DreamTab>(() => tabFromUrl());
    const [statusFilter, setStatusFilter] = useState<DreamStatus>(() => statusFromUrl());
    const items = useMemo(() => query.data?.items.filter(isDreamItem).map(toDreamViewItem) ?? [], [query.data]);

    const tab = tabs.find((candidate) => candidate.id === activeTab) ?? tabs[0]!;
    const tabItems = useMemo(() => items.filter((item) => tab.kinds.includes(item.kind)), [items, tab]);
    const visible = useMemo(
        () => (statusFilter === 'all' ? tabItems : tabItems.filter((item) => item.status === statusFilter)),
        [statusFilter, tabItems],
    );
    const [selectedId, setSelectedId] = useState(visible[0]?.id ?? '');
    const selected = visible.find((item) => item.id === selectedId) ?? visible[0];

    const tabCounts = useMemo(
        () =>
            tabs.reduce<Record<DreamTab, number>>(
                (acc, candidate) => {
                    acc[candidate.id] = items.filter((item) => candidate.kinds.includes(item.kind)).length;
                    return acc;
                },
                { journal: 0, questions: 0, cleanup: 0 },
            ),
        [items],
    );

    const statusCounts = statuses.reduce<Record<DreamStatus, number>>(
        (acc, key) => {
            acc[key] = key === 'all' ? tabItems.length : tabItems.filter((item) => item.status === key).length;
            return acc;
        },
        { all: 0, proposed: 0, queued: 0, accepted: 0, completed: 0, dismissed: 0, running: 0 },
    );

    const lastRunAt = status.data?.dreaming.last_run.at;
    const nextScheduled = status.data?.dreaming.next_run;

    return (
        <div
            className="view"
            data-testid={`dreams-view-${activeTab}-${statusFilter}`}
        >
            {query.isLoading ? <QueryLoadingBanner label="Dreams" /> : null}
            <QueryErrorBanner
                error={query.error}
                label="Dreams"
            />
            <div className="view-header">
                <span className="view-title">Dreams</span>
                <span className="view-subtitle">· {formatDreamHeader(items, lastRunAt, nextScheduled)}</span>
            </div>
            <div
                className="dream-tabs"
                role="tablist"
                aria-label="Dream sub-tabs"
            >
                {tabs.map((candidate) => (
                    <button
                        key={candidate.id}
                        className={`dream-tab ${activeTab === candidate.id ? 'active' : ''}`}
                        role="tab"
                        aria-selected={activeTab === candidate.id}
                        type="button"
                        onClick={() => {
                            setActiveTab(candidate.id);
                            setStatusFilter('all');
                            setSelectedId('');
                        }}
                    >
                        <span>{candidate.label}</span>
                        <span className="count">{tabCounts[candidate.id]}</span>
                    </button>
                ))}
            </div>
            <div
                className="filter-pills dream-status-filters"
                role="tablist"
                aria-label="Dream status filters"
            >
                {statuses.map((key, index) => (
                    <button
                        key={key}
                        className={`pill ${statusFilter === key ? 'active' : ''}`}
                        onClick={() => {
                            setStatusFilter(key);
                            const next = key === 'all' ? tabItems[0] : tabItems.find((item) => item.status === key);
                            setSelectedId(next?.id ?? '');
                        }}
                        role="tab"
                        aria-selected={statusFilter === key}
                        type="button"
                    >
                        <span>{key}</span>
                        <span className="count">{statusCounts[key]}</span>
                        <span className="kbd-hint">{index + 1}</span>
                    </button>
                ))}
            </div>
            <div className="panes-2">
                <div className="pane left">
                    <div className="pane-scroll">
                        {visible.length === 0 ? (
                            <EmptyState
                                title="No dream output yet."
                                body="No dream output for this scope yet. Dreams run nightly at 03:00 by default."
                            />
                        ) : (
                            <DreamList
                                items={visible}
                                selectedId={selected?.id ?? ''}
                                onSelect={setSelectedId}
                            />
                        )}
                    </div>
                </div>
                <div className="pane">
                    <div
                        className="pane-scroll"
                        tabIndex={0}
                    >
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
