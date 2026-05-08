export type RealityCheckVariant = 'default' | 'encrypted' | 'refused' | 'score-open' | 'complete';

export type RealityCheckActionName = 'confirm' | 'correct' | 'forget' | 'not_relevant' | 'skip_this_week';

export interface RealityCheckRespondPayload {
    session_id: string;
    memory_id: string;
    action: RealityCheckActionName;
    correction?: string;
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
