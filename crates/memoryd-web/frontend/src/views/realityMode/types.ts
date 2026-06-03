import type { RealityCheckRespondRequest } from '../../api';

export type RealityCheckVariant = 'default' | 'encrypted' | 'refused' | 'score-open' | 'complete';

export type RealityCheckActionName = 'confirm' | 'correct' | 'forget' | 'not_relevant' | 'skip_this_week';

/**
 * Same wire shape as the API's {@link RealityCheckRespondRequest} (the payload is
 * submitted directly to the respond mutation), but narrows `action` to the known
 * action names the UI emits.
 */
export type RealityCheckRespondPayload = Omit<RealityCheckRespondRequest, 'action'> & {
    action: RealityCheckActionName;
};

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
    component_scores: Record<string, number>;
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
