import { useCallback, useEffect, useMemo, useState } from 'react';

import { useRealityCheckQuery, useRealityCheckRespondMutation, type RealityCheckApiItem } from '../api';
import { hashParams } from '../router';
import { EmptyState } from '../ui';
import { QueryErrorBanner, QueryLoadingBanner } from './QueryFeedback';
import {
    CompletionCard,
    FocusStrip,
    QuestionStage,
    SessionSidebar,
    type RealityCheckActionName,
    type RealityCheckMemory,
    type RealityCheckRespondPayload,
    type RealityCheckSession,
    type RealityCheckSessionItem,
    type RealityCheckVariant,
} from './realityMode';

interface RealityCheckProps {
    variant?: RealityCheckVariant;
    session?: RealityCheckSession;
    onExit?: () => void;
    onRespond?: (payload: RealityCheckRespondPayload) => void | Promise<void>;
}

const variants = ['default', 'encrypted', 'refused', 'score-open', 'complete'] as const;

function variantFromUrl(): RealityCheckVariant {
    const raw = hashParams(window.location.hash).get('variant');
    return variants.find((candidate) => candidate === raw) ?? 'default';
}

function isTextInputTarget(target: unknown): boolean {
    if (!(target instanceof HTMLElement)) return false;
    const tagName = target.tagName.toLowerCase();
    return tagName === 'input' || tagName === 'textarea' || target.isContentEditable;
}

function daysSince(iso: string): number {
    const then = new Date(iso).getTime();
    if (Number.isNaN(then)) return 0;
    return Math.max(0, Math.round((Date.now() - then) / 86_400_000));
}

function shortWritten(iso: string): string {
    const days = daysSince(iso);
    if (days === 0) return 'today';
    if (days === 1) return '1d ago';
    return `${days}d ago`;
}

function toRealityCheckMemory(item: RealityCheckApiItem, variant: RealityCheckVariant): RealityCheckMemory {
    const encrypted = item.encrypted || variant === 'encrypted';
    const namespace = variant === 'refused' ? 'personal/family' : item.namespace;
    return {
        id: item.memory_id,
        namespace,
        title: item.title,
        question: `Is this still true: ${item.title}?`,
        think: `${item.title}. Status ${item.status}; recall count ${item.recall_count_30d}.`,
        source: `${namespace} · source capture`,
        written: shortWritten(item.last_observed_at),
        last_verified_days: daysSince(item.last_observed_at),
        score: item.score,
        component_scores: {
            recency: item.component_scores.days_since_observed_norm,
            recall_frequency: item.component_scores.recall_frequency_norm,
            corroboration: item.component_scores.cross_source_corroboration,
            confidence_decay: item.component_scores.confidence_decay,
            sensitivity: item.component_scores.sensitivity_weight,
        },
        encrypted,
    };
}

function toSessionItem(item: RealityCheckApiItem, index: number, complete: boolean): RealityCheckSessionItem {
    return {
        id: item.memory_id,
        title: item.title,
        status: complete ? 'done' : index === 0 ? 'now' : 'queued',
    };
}

function sessionFromApi(
    items: RealityCheckApiItem[],
    sessionId: string,
    variant: RealityCheckVariant,
): RealityCheckSession | null {
    if (items.length === 0) return null;
    const complete = variant === 'complete';
    return {
        session_id: sessionId,
        progress: { current: complete ? items.length : 1, total: items.length },
        current: toRealityCheckMemory(items[0]!, variant),
        items: items.map((item, index) => toSessionItem(item, index, complete)),
    };
}

export function RealityCheck({ variant, session, onExit = () => undefined, onRespond }: RealityCheckProps) {
    const resolvedVariant = variant ?? variantFromUrl();
    const query = useRealityCheckQuery();
    const respondMutation = useRealityCheckRespondMutation();
    const resolvedSession = useMemo(
        () => session ?? (query.data ? sessionFromApi(query.data.items, query.data.session_id, resolvedVariant) : null),
        [query.data, resolvedVariant, session],
    );
    const [mode, setMode] = useState<'answer' | 'correct'>('answer');
    const complete = resolvedVariant === 'complete';
    const sessionId = resolvedSession?.session_id ?? '';
    const memoryId = resolvedSession?.current.id ?? '';

    const submitRespond = useCallback(
        (action: RealityCheckActionName, correction?: string) => {
            if (!sessionId || !memoryId) return;
            const payload: RealityCheckRespondPayload = {
                session_id: sessionId,
                memory_id: memoryId,
                action,
            };
            if (correction !== undefined) payload.correction = correction;
            void Promise.resolve(onRespond ? onRespond(payload) : respondMutation.mutateAsync(payload));
            setMode('answer');
        },
        [memoryId, onRespond, respondMutation, sessionId],
    );

    useEffect(() => {
        const onKeyDown = (event: KeyboardEvent) => {
            if (complete || isTextInputTarget(event.target) || isTextInputTarget(document.activeElement)) return;
            if (event.key === 'Escape') {
                setMode('answer');
                return;
            }
            if (mode === 'correct') return;
            if (event.key === 'k') {
                event.preventDefault();
                setMode('correct');
                return;
            }
            if (resolvedVariant !== 'encrypted' && event.key === 'y') {
                event.preventDefault();
                submitRespond('confirm');
                return;
            }
            if (event.key === 'f') {
                event.preventDefault();
                submitRespond('forget', 'operator requested forget during reality check');
                return;
            }
            if (event.key === 'n') {
                event.preventDefault();
                submitRespond('not_relevant');
                return;
            }
            if (event.key === 's') {
                event.preventDefault();
                submitRespond('skip_this_week');
            }
        };
        window.addEventListener('keydown', onKeyDown);
        return () => window.removeEventListener('keydown', onKeyDown);
    }, [complete, mode, resolvedVariant, submitRespond]);

    if (!resolvedSession) {
        // Use `!!query.data` rather than `query.data && !query.isLoading` so the
        // empty state stays rendered during background refetches. React Query's
        // stale-while-revalidate keeps `data` available while `isLoading` flips
        // true; pairing those two flags suppresses the empty state mid-refresh.
        const hasData = !!query.data;
        const lastCompleted = query.data?.last_completed_at;
        // Brief §View 2 verbatim title; body uses the daemon's `last_completed_at`
        // when available. "Next due" isn't currently exposed by the daemon — we
        // show what we have without inventing the missing field.
        const emptyBody = lastCompleted
            ? `Last completed: ${shortWritten(lastCompleted)}.`
            : 'Last completed: never. Run Memorum agents to populate the queue.';
        return (
            <div
                className="view"
                data-testid={`reality-check-${resolvedVariant}`}
            >
                {query.isLoading ? <QueryLoadingBanner label="Reality Check" /> : null}
                <QueryErrorBanner
                    error={query.error}
                    label="Reality Check"
                />
                {hasData ? (
                    <EmptyState
                        title="No items due."
                        body={emptyBody}
                    />
                ) : null}
            </div>
        );
    }

    return (
        <div
            className="view"
            data-testid={`reality-check-${resolvedVariant}`}
        >
            <QueryErrorBanner
                error={query.error ?? respondMutation.error}
                label="Reality Check"
            />
            <FocusStrip
                session={resolvedSession}
                complete={complete}
                onExit={onExit}
            />
            <div className="rc-stage">
                {complete ? (
                    <CompletionCard onExit={onExit} />
                ) : (
                    <QuestionStage
                        memory={resolvedSession.current}
                        mode={mode}
                        variant={resolvedVariant}
                        onAction={(action, correction) => {
                            const defaultCorrection =
                                action === 'forget' ? 'operator requested forget during reality check' : correction;
                            submitRespond(action, defaultCorrection);
                        }}
                        onCorrectMode={() => setMode('correct')}
                        onCancelCorrect={() => setMode('answer')}
                    />
                )}
                <SessionSidebar
                    session={resolvedSession}
                    complete={complete}
                />
            </div>
        </div>
    );
}
