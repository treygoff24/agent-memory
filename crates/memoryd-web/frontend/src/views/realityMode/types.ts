import type { RealityCheckRespondRequest } from '../../api';

export const realityCheckVariants = ['default', 'encrypted', 'refused', 'score-open', 'complete'] as const;

export type RealityCheckVariant = (typeof realityCheckVariants)[number];

export type RealityCheckActionName = 'confirm' | 'correct' | 'forget' | 'not_relevant' | 'skip_this_week';

/**
 * Same wire shape as the API's {@link RealityCheckRespondRequest} (the payload is
 * submitted directly to the respond mutation), but narrows `action` to the known
 * action names the UI emits.
 */
export type RealityCheckRespondPayload = Omit<RealityCheckRespondRequest, 'action'> & {
    action: RealityCheckActionName;
};

export interface RealityCheckScoreBreakdown {
    recency: number;
    recall_frequency: number;
    corroboration: number;
    confidence_decay: number;
    sensitivity: number;
}

export interface RealityCheckMemory {
    id: string;
    namespace: string;
    title: string;
    question: string;
    think: string;
    source: string;
    written: string;
    last_verified_days: number;
    score: number;
    component_scores: RealityCheckScoreBreakdown;
    encrypted?: boolean;
}

export interface RealityCheckSessionItem {
    id: string;
    title: string;
    status: 'done' | 'now' | 'queued';
}

export interface RealityCheckSession {
    session_id: string;
    progress: { current: number; total: number };
    current: RealityCheckMemory;
    items: RealityCheckSessionItem[];
}
