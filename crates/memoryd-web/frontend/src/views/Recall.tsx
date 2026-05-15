import { useMemo, useState } from 'react';

import { useRecallHitsQuery, type RecallHitSummary } from '../api';
import { Inspector, type InspectorItem } from '../inspector';
import { hashParams } from '../router';
import { EmptyState } from '../ui';
import { QueryErrorBanner, QueryLoadingBanner } from './QueryFeedback';
import { RecallList } from './recall/RecallList';
import { TimelineStrip, type TimelineBucket } from './recall/TimelineStrip';

export interface RecallLedgerEvent {
    id: string;
    seq: number;
    isoTime: string;
    time: string;
    device: string;
    agent: string;
    memory: string;
    /** The raw memory_id from the recall hit — used for encrypted summary display. */
    memory_id: string;
    /** True when the original recall hit had a null summary (encrypted memory). */
    isEncrypted: boolean;
    namespace: string;
    score: number;
    latencyMs: number;
    session: string;
}

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
    /** The canonical memory_id for this event — kept separately from the display text. */
    memory_id?: string;
    /** Whether the original hit had a null summary (encrypted memory). */
    isEncrypted?: boolean;
    namespace: string;
    score: number;
}

function stateFromUrl(): string {
    return hashParams(window.location.hash).get('recallState') ?? 'default';
}

function normalizeAgent(agent: string): string {
    if (agent === 'codex') return 'codex';
    if (agent === 'claude-code') return 'claude-code';
    return agent;
}

function agentFromDevice(device: string): string {
    if (device === 'mbp') return 'codex';
    if (device === 'mini') return 'claude-code';
    return 'manual';
}

function namespaceFromSummary(summary: string): string {
    if (/pnpm|typescript/i.test(summary)) return 'coding/typescript';
    if (/editor/i.test(summary)) return 'prefs/editor';
    if (/acme/i.test(summary)) return 'work/clients/acme';
    if (/daughter|school|family/i.test(summary)) return 'personal/family';
    if (/rust|go/i.test(summary)) return 'meta/preferences';
    return 'project:agent-memory';
}

export function toRecallLedgerEvent(event: RawRecallEvent, index: number): RecallLedgerEvent {
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
        memory_id: event.memory_id ?? event.id,
        isEncrypted: event.isEncrypted ?? false,
        namespace: event.namespace,
        score: event.score,
        latencyMs: 18 + (index % 31),
        session: `session_${String(index % 12).padStart(2, '0')}`,
    };
}

function fromRecallHit(hit: RecallHitSummary, index: number): RecallLedgerEvent {
    const encrypted = hit.summary == null;
    const summary = hit.summary ?? hit.memory_id;
    return toRecallLedgerEvent(
        {
            id: hit.event_id,
            time: hit.recalled_at,
            device: hit.device,
            agent: agentFromDevice(hit.device),
            memory: summary,
            memory_id: hit.memory_id,
            isEncrypted: encrypted,
            namespace: namespaceFromSummary(summary),
            score: 0.5 + (index % 50) / 100,
        },
        index,
    );
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
        memoryId: event.memory_id,
        summary: event.memory,
        sessionId: event.session,
        recallCountTotal: event.seq,
        recallCount30d: event.seq % 30,
        provenance: {
            written: event.time,
            session: event.session,
            confidence: event.score.toFixed(2),
            device: event.device,
        },
    };
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
    const bucketMode = heavyMode ? '30d' : '24h';

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
                if (selectedBucket !== null) {
                    const d = new Date(event.isoTime);
                    const bucketKey = bucketMode === '24h' ? d.getUTCHours() : d.getUTCDate() % 30;
                    if (bucketKey !== selectedBucket) return false;
                }
                return true;
            }),
        [agent, device, search, sourceEvents, selectedBucket, bucketMode],
    );
    const selected = visible.find((event) => event.id === selectedId) ?? visible[0];
    const buckets = bucketMode === '30d' ? makeDayBuckets(sourceEvents) : makeHourBuckets(sourceEvents);

    return (
        <div
            className="view"
            data-testid={`recall-ledger-${urlState}`}
        >
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
                    {visible.length === 0 && !query.isLoading ? (
                        <EmptyState
                            title="No recall events yet."
                            body="Recall events appear here once an agent retrieves a memory."
                        />
                    ) : (
                        <RecallList
                            events={visible}
                            selectedId={selected?.id ?? ''}
                            onSelect={setSelectedId}
                            heavy={heavyMode}
                        />
                    )}
                </div>
                <div className="pane">
                    <div
                        className="pane-scroll"
                        tabIndex={0}
                    >
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
