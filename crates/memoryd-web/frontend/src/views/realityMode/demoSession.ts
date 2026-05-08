import type { RealityCheckSession, RealityCheckVariant } from './types';

const baseMemory = {
    id: 'mem_20260507_a1b2c3d4e5f60718_000010',
    namespace: 'coding/typescript',
    title: 'Project uses pnpm, never npm',
    question: 'Is this still true: project uses pnpm and should avoid package-lock.json drift?',
    think: 'Project uses pnpm for package management. Avoid package-lock.json drift.',
    source: 'session codex-2026-05-07 · source capture',
    written: '2m ago',
    last_verified_days: 37,
    score: 0.82,
    component_scores: {
        recency: 0.72,
        recall_frequency: 0.66,
        contradiction_risk: 0.41,
        user_impact: 0.9,
    },
};

export function demoRealityCheckSession(variant: RealityCheckVariant = 'default'): RealityCheckSession {
    const current = {
        ...baseMemory,
        encrypted: variant === 'encrypted',
        namespace: variant === 'refused' ? 'personal/family' : baseMemory.namespace,
    };
    return {
        session_id: 'rc_20260507_001',
        progress: { current: variant === 'complete' ? 5 : 1, total: 5 },
        current,
        items: [
            { id: current.id, title: current.title, status: variant === 'complete' ? 'done' : 'now' },
            { id: 'mem_due_school', title: "Daughter's school name (verify)", status: variant === 'complete' ? 'done' : 'queued' },
            { id: 'mem_recall_evt_28f1a4', title: 'Acme renewal date — Mar 14, 2026', status: variant === 'complete' ? 'done' : 'queued' },
            { id: 'mem_conflict_editor', title: 'Editor preference disagreement', status: variant === 'complete' ? 'done' : 'queued' },
            { id: 'mem_dream_rust', title: 'Pattern: prefers Rust over Go', status: variant === 'complete' ? 'done' : 'queued' },
        ],
    };
}
