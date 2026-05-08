export interface InboxItem {
    id: string;
    kind: 'review' | 'recall' | 'conflict' | 'dream' | 'due';
    title: string;
    namespace: string;
    meta: string;
    body: string;
    confidence: number;
}

export interface RecallEvent {
    id: string;
    time: string;
    device: string;
    agent: string;
    memory: string;
    namespace: string;
    score: number;
}

export interface DreamItem {
    id: string;
    status: 'proposed' | 'queued' | 'accepted' | 'completed' | 'dismissed' | 'running';
    title: string;
    kind: string;
    confidence: number;
}

export interface PeerItem {
    id: string;
    label: string;
    trust: string;
    sync: string;
    locksHeld: number;
    locksPending: number;
    events24h: number;
}

export interface GovernanceItem {
    id: string;
    title: string;
    severity: 'block' | 'warn' | 'info';
    decision: string;
    namespace: string;
}

export interface EntityItem {
    id: string;
    name: string;
    kind: 'person' | 'org' | 'project' | 'place' | 'tool' | 'language';
    mentions: number;
    namespaces: string[];
    firstSeen: string;
    lastSeen: string;
    confidence: number;
}

export const inboxItems: InboxItem[] = [
    {
        id: 'mem_20260507_a1b2c3d4e5f60718_000010',
        kind: 'review',
        title: 'Project uses pnpm, never npm',
        namespace: 'coding/typescript',
        meta: '2m',
        body: 'Project uses pnpm for package management. Avoid package-lock.json drift.',
        confidence: 0.84,
    },
    {
        id: 'mem_conflict_editor',
        kind: 'conflict',
        title: 'Editor preference disagreement',
        namespace: 'prefs/editor',
        meta: '1h',
        body: 'Helix and Neovim claims conflict across devices.',
        confidence: 0.61,
    },
    {
        id: 'mem_recall_evt_28f1a4',
        kind: 'recall',
        title: 'Acme renewal date — Mar 14, 2026',
        namespace: 'work/clients/acme',
        meta: '12m',
        body: 'Recalled while drafting renewal email.',
        confidence: 0.92,
    },
    {
        id: 'mem_due_school',
        kind: 'due',
        title: "Daughter's school name (verify)",
        namespace: 'personal/family',
        meta: '1d',
        body: 'Reality Check due because last observation is stale.',
        confidence: 0.72,
    },
    {
        id: 'mem_dream_rust',
        kind: 'dream',
        title: 'Pattern: prefers Rust over Go for systems work',
        namespace: 'meta/preferences',
        meta: '3h',
        body: 'Dream synthesis candidate with low confidence.',
        confidence: 0.62,
    },
];

export const recallEvents: RecallEvent[] = Array.from({ length: 80 }, (_, index) => ({
    id: `recall_${index}`,
    time: `2026-05-${String((index % 28) + 1).padStart(2, '0')}T12:00:00Z`,
    device: index % 2 === 0 ? 'mbp' : 'mini',
    agent: index % 3 === 0 ? 'codex' : 'claude-code',
    memory: inboxItems[index % inboxItems.length].title,
    namespace: inboxItems[index % inboxItems.length].namespace,
    score: 0.5 + (index % 50) / 100,
}));

export const dreams: DreamItem[] = [
    { id: 'dream_run_1', status: 'running', title: 'Nightly synthesis pass', kind: 'dream-run', confidence: 1 },
    {
        id: 'dream_pattern_1',
        status: 'proposed',
        title: 'Pattern: favors evidence-first reviews',
        kind: 'pattern',
        confidence: 0.74,
    },
    {
        id: 'dream_question_1',
        status: 'queued',
        title: 'Question: which laptop is primary now?',
        kind: 'question',
        confidence: 0.56,
    },
    {
        id: 'dream_cleanup_1',
        status: 'completed',
        title: 'Cleanup stale fragments older than 45d',
        kind: 'cleanup',
        confidence: 0.91,
    },
];

export const peers: PeerItem[] = [
    {
        id: 'peer_mbp',
        label: 'MacBook Pro',
        trust: 'trusted',
        sync: 'in-sync',
        locksHeld: 2,
        locksPending: 0,
        events24h: 128,
    },
    {
        id: 'peer_mini',
        label: 'Mac mini',
        trust: 'trusted',
        sync: 'behind',
        locksHeld: 0,
        locksPending: 1,
        events24h: 42,
    },
    {
        id: 'peer_old',
        label: 'Old laptop',
        trust: 'revoked',
        sync: 'revoked',
        locksHeld: 0,
        locksPending: 0,
        events24h: 0,
    },
];

export const governance: GovernanceItem[] = [
    {
        id: 'gov_secret',
        title: 'Possible secret in source capture',
        severity: 'block',
        decision: 'redact_proposed',
        namespace: 'privacy/source',
    },
    {
        id: 'gov_low_conf',
        title: 'Low confidence dream candidate',
        severity: 'warn',
        decision: 'review_required',
        namespace: 'dreams/patterns',
    },
    {
        id: 'gov_info',
        title: 'Grounding evidence attached',
        severity: 'info',
        decision: 'auto_approve',
        namespace: 'project:agent-memory',
    },
];

export const entities: EntityItem[] = [
    {
        id: 'ent_agent_memory',
        name: 'agent-memory',
        kind: 'project',
        mentions: 42,
        namespaces: ['project:agent-memory'],
        firstSeen: '2026-04-01',
        lastSeen: '2026-05-07',
        confidence: 0.96,
    },
    {
        id: 'ent_pnpm',
        name: 'pnpm',
        kind: 'tool',
        mentions: 18,
        namespaces: ['coding/typescript'],
        firstSeen: '2026-03-22',
        lastSeen: '2026-05-07',
        confidence: 0.91,
    },
    {
        id: 'ent_rust',
        name: 'Rust',
        kind: 'language',
        mentions: 31,
        namespaces: ['coding/rust'],
        firstSeen: '2026-02-15',
        lastSeen: '2026-05-05',
        confidence: 0.88,
    },
    {
        id: 'ent_acme',
        name: 'Acme',
        kind: 'org',
        mentions: 9,
        namespaces: ['work/clients/acme'],
        firstSeen: '2026-01-10',
        lastSeen: '2026-05-01',
        confidence: 0.84,
    },
];
