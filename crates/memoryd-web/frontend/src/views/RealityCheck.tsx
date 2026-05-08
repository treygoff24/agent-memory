import { useCallback, useEffect, useMemo, useState } from 'react';

import { apiJson } from '../api/client';
import {
    CompletionCard,
    demoRealityCheckSession,
    FocusStrip,
    QuestionStage,
    SessionSidebar,
    type RealityCheckActionName,
    type RealityCheckRespondPayload,
    type RealityCheckSession,
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
    const raw = new URLSearchParams(window.location.search).get('variant');
    return variants.find((candidate) => candidate === raw) ?? 'default';
}

function isTextInputTarget(target: unknown): boolean {
    if (!(target instanceof HTMLElement)) return false;
    const tagName = target.tagName.toLowerCase();
    return tagName === 'input' || tagName === 'textarea' || target.isContentEditable;
}

async function postRealityCheckResponse(payload: RealityCheckRespondPayload) {
    await apiJson('/api/reality-check/respond', { method: 'POST', body: JSON.stringify(payload) });
}

export function RealityCheck({ variant, session, onExit = () => undefined, onRespond }: RealityCheckProps) {
    const resolvedVariant = variant ?? variantFromUrl();
    const resolvedSession = useMemo(
        () => session ?? demoRealityCheckSession(resolvedVariant),
        [resolvedVariant, session],
    );
    const [mode, setMode] = useState<'answer' | 'correct'>('answer');
    const complete = resolvedVariant === 'complete';
    const sessionId = resolvedSession.session_id;
    const memoryId = resolvedSession.current.id;

    const submitRespond = useCallback(
        (action: RealityCheckActionName, correction?: string) => {
            const payload: RealityCheckRespondPayload = {
                session_id: sessionId,
                memory_id: memoryId,
                action,
            };
            if (correction !== undefined) payload.correction = correction;
            // Web route maps these action strings to daemon RealityCheckRequest::Respond
            // with RealityCheckAction::{Confirm, Correct, Forget, NotRelevant, SkipThisWeek}.
            void Promise.resolve(onRespond ? onRespond(payload) : postRealityCheckResponse(payload));
            setMode('answer');
        },
        [memoryId, onRespond, sessionId],
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

    return (
        <div
            data-testid={`reality-check-${resolvedVariant}`}
            style={{ display: 'flex', flexDirection: 'column', height: '100%' }}
        >
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
                            const defaultCorrection = action === 'forget' ? 'operator requested forget during reality check' : correction;
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
