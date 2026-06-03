import { useMemo, useState } from 'react';

import type { RecallLedgerEvent } from './recall/types';

import { useRecallHitsQuery, type RecallHitSummary } from '../api';
import { Inspector, type InspectorItem } from '../inspector';
import { QueryErrorBanner, QueryLoadingBanner } from './QueryFeedback';
import { RecallList } from './recall/RecallList';
import { TimelineStrip, type TimelineBucket } from './recall/TimelineStrip';

export type { RecallLedgerEvent } from './recall/types';

interface RecallProps {
    events?: RecallLedgerEvent[];
    heavy?: boolean;
}

interface RawRecallEvent {
    id: string;
    time: string;
    device: string;
    agent: string;
    memory: string;
    namespace: string;
    score: number;
}

function stateFromUrl(): string {
    return new URLSearchParams(window.location.search).get('recallState') ?? 'default';
}

function normalizeAgent(agent: string): string {
    if (agent === 'codex') return 'codex';
    if (agent === 'claude-code') return 'claude-code';
    return agent;
}

function toRecallLedgerEvent(event: RawRecallEvent, index: number): RecallLedgerEvent {
    const date = new Date(event.time);
    const hh = String(date.getUTCHours()).padStart(2, '0');
    const mm = String(date.getUTCMinutes()).padStart(2, '0');
    const ss = String(date.getUTCSeconds()).padStart(2, '0');
    return {
        id: event.id,
        seq: index + 1,
        isoTime: event.time,
        time: `${hh}:${mm}:${ss}`,
        device: event.device,
        agent: normalizeAgent(event.agent),
        memory: event.memory,
        namespace: event.namespace,
        score: event.score,
        latencyMs: 18 + (index % 31),
        session: `session_${String(index % 12).padStart(2, '0')}`,
    };
}

function fromRecallHit(hit: RecallHitSummary): RecallLedgerEvent {
    const summary = hit.summary ?? hit.memory_id;
    const date = new Date(hit.recalled_at);
    const hh = String(date.getUTCHours()).padStart(2, '0');
    const mm = String(date.getUTCMinutes()).padStart(2, '0');
    const ss = String(date.getUTCSeconds()).padStart(2, '0');
    return {
        id: hit.event_id,
        seq: hit.seq,
        isoTime: hit.recalled_at,
        time: `${hh}:${mm}:${ss}`,
        device: hit.device,
        agent: 'unknown',
        memory: summary,
        namespace: 'unknown',
        score: null,
        latencyMs: null,
        session: 'unknown',
    };
}

export function makeHeavyRecallEvents(count = 9000): RecallLedgerEvent[] {
    return Array.from({ length: count }, (_, index) =>
        toRecallLedgerEvent(
            {
                id: `heavy_recall_${index}`,
                time: `2026-05-${String((index % 28) + 1).padStart(2, '0')}T${String(index % 24).padStart(2, '0')}:00:00Z`,
                device: index % 3 === 0 ? 'mini' : index % 3 === 1 ? 'mbp' : 'phone',
                agent: index % 2 === 0 ? 'codex' : 'claude-code',
                memory: index % 5 === 0 ? 'Project uses pnpm, never npm' : `Recall event ${index}`,
                namespace: index % 5 === 0 ? 'coding/typescript' : 'project:agent-memory',
                score: 0.45 + (index % 50) / 100,
            },
            index,
        ),
    );
}

function makeHourBuckets(events: RecallLedgerEvent[]): TimelineBucket[] {
    const counts = Array.from({ length: 24 }, (_, key) => ({
        key,
        label: `${String(key).padStart(2, '0')}:00`,
        count: 0,
    }));
    for (const event of events) counts[new Date(event.isoTime).getUTCHours()]!.count += 1;
    return counts;
}

function makeDayBuckets(events: RecallLedgerEvent[]): TimelineBucket[] {
    const counts = Array.from({ length: 30 }, (_, key) => ({
        key,
        label: `day ${key + 1}`,
        count: 0,
    }));
    for (const event of events) counts[new Date(event.isoTime).getUTCDate() % 30]!.count += 1;
    return counts;
}

function inspectorItemFromRecall(event: RecallLedgerEvent | undefined): InspectorItem | null {
    if (!event) return null;
    return {
        kind: 'recall-event',
        id: event.id,
        title: 'Recall event',
        namespace: event.namespace,
        body: event.memory,
        memoryId: event.id,
        summary: event.memory,
        sessionId: event.session,
        recallCountTotal: event.seq,
        recallCount30d: event.seq % 30,
        provenance: {
            written: event.time,
            session: event.session,
            confidence: event.score === null ? 'unknown' : event.score.toFixed(2),
            device: event.device,
        },
    };
}

function csvCell(value: string | number): string {
    const text = String(value);
    return /[",\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
}

function recallCsv(events: RecallLedgerEvent[]): string {
    const header = ['time', 'seq', 'device', 'agent', 'memory', 'namespace', 'latency_ms', 'score', 'session'];
    const rows = events.map((event) =>
        [
            event.isoTime,
            event.seq,
            event.device,
            event.agent,
            event.memory,
            event.namespace,
            event.latencyMs ?? 'unknown',
            event.score ?? 'unknown',
            event.session,
        ]
            .map(csvCell)
            .join(','),
    );
    return [header.join(','), ...rows].join('\n');
}

function downloadRecallCsv(events: RecallLedgerEvent[]) {
    const link = document.createElement('a');
    link.href = `data:text/csv;charset=utf-8,${encodeURIComponent(recallCsv(events))}`;
    link.download = 'memorum-recall-visible.csv';
    document.body.append(link);
    link.click();
    link.remove();
}

export function Recall({ events, heavy }: RecallProps) {
    const urlState = stateFromUrl();
    const heavyMode = heavy ?? urlState === 'heavy';
    const query = useRecallHitsQuery({ limit: heavyMode ? 9000 : 500 });
    const sourceEvents = useMemo(() => events ?? query.data?.hits.map(fromRecallHit) ?? [], [events, query.data]);
    const [agent, setAgent] = useState(urlState === 'agent-filter' ? 'codex' : 'all');
    const [device, setDevice] = useState(urlState === 'device-filter' ? 'mbp' : 'all');
    const [search, setSearch] = useState('');
    const [selectedBucket, setSelectedBucket] = useState<number | null>(null);
    const [selectedId, setSelectedId] = useState(sourceEvents[0]?.id ?? '');

    const visible = useMemo(
        () =>
            sourceEvents.filter((event) => {
                if (agent !== 'all' && event.agent !== agent) return false;
                if (device !== 'all' && event.device !== device) return false;
                if (
                    search &&
                    !`${event.memory} ${event.namespace} ${event.agent} ${event.device}`
                        .toLowerCase()
                        .includes(search.toLowerCase())
                )
                    return false;
                return true;
            }),
        [agent, device, search, sourceEvents],
    );
    const selected = visible.find((event) => event.id === selectedId) ?? visible[0];
    const bucketMode = heavyMode ? '30d' : '24h';
    const buckets = bucketMode === '30d' ? makeDayBuckets(sourceEvents) : makeHourBuckets(sourceEvents);

    return (
        <div data-testid={`recall-ledger-${urlState}`}>
            {!events && query.isLoading ? <QueryLoadingBanner label="Recall ledger" /> : null}
            <QueryErrorBanner
                error={query.error}
                label="Recall ledger"
            />
            <div className="view-header">
                <span className="view-title">Recall ledger</span>
                <span className="view-subtitle">
                    · {sourceEvents.length.toLocaleString()} events ·{' '}
                    {bucketMode === '24h' ? 'last 24 hours' : 'last 30 days'}
                </span>
                <span className="spacer" />
                <div className="rl-filters">
                    <select
                        aria-label="Agent filter"
                        value={agent}
                        onChange={(event) => setAgent(event.target.value)}
                    >
                        <option value="all">all agents</option>
                        <option value="codex">codex</option>
                        <option value="claude-code">claude-code</option>
                        <option value="manual">manual</option>
                    </select>
                    <select
                        aria-label="Device filter"
                        value={device}
                        onChange={(event) => setDevice(event.target.value)}
                    >
                        <option value="all">all devices</option>
                        <option value="mbp">mbp</option>
                        <option value="mini">mini</option>
                        <option value="phone">phone</option>
                    </select>
                    <input
                        aria-label="Recall search"
                        value={search}
                        onChange={(event) => setSearch(event.target.value)}
                        placeholder="/ search"
                    />
                    <button
                        className="btn"
                        type="button"
                        onClick={() => downloadRecallCsv(visible)}
                    >
                        export csv
                    </button>
                </div>
            </div>
            <TimelineStrip
                buckets={buckets}
                mode={bucketMode}
                selected={selectedBucket}
                onPick={(key) => setSelectedBucket((current) => (current === key ? null : key))}
            />
            <div className="panes-2">
                <div className="pane left rl-list-pane">
                    <div className="rl-table-head">
                        <span>time</span>
                        <span>seq</span>
                        <span>device</span>
                        <span>agent</span>
                        <span>memory</span>
                        <span>namespace</span>
                        <span>lat</span>
                        <span>score</span>
                    </div>
                    <RecallList
                        events={visible}
                        selectedId={selected?.id ?? ''}
                        onSelect={setSelectedId}
                        heavy={heavyMode}
                    />
                </div>
                <div className="pane">
                    <div className="pane-scroll">
                        <Inspector
                            item={inspectorItemFromRecall(selected)}
                            layout="narrow"
                        />
                    </div>
                </div>
            </div>
        </div>
    );
}
