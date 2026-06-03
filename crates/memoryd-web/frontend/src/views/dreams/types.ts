// Shared view-model types for the Dreams view. Extracted to a leaf module so the
// `Dreams` container and its `DreamList` child can both depend on them without
// forming an import cycle.

export type DreamStatus = 'all' | 'proposed' | 'queued' | 'accepted' | 'completed' | 'dismissed' | 'running';

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
