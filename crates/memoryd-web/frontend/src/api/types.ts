export interface ApiErrorBody {
    error?: string;
    message?: string;
    route?: string;
    code?: string;
    action?: string;
}

export interface DaemonStatus {
    version: string;
    pid: number;
    uptime_seconds: number | null;
}

export interface IndexStatus {
    active_memories: number;
    last_reindex: string | null;
}

export interface SyncStatus {
    ahead: number;
    behind: number;
    remote: string;
    last_push: string;
}

export interface ReviewStatus {
    candidate: number;
    quarantined: number;
    dream_low_confidence: number;
}

export interface ActiveSession {
    harness: string;
    session_id: string;
}

export interface DreamRunSummary {
    at: string | null;
    promoted: number | null;
    queued: number | null;
    dropped: number | null;
}

export interface DreamingStatus {
    status: string;
    next_run: string | null;
    last_run: DreamRunSummary;
}

export interface RecallStatus {
    startup_total: number;
    delta_total: number;
    peer_update_snapshot_count: number;
}

export interface StatusResponse {
    degraded: boolean;
    warnings: string[];
    daemon: DaemonStatus;
    socket: string;
    index: IndexStatus;
    sync: SyncStatus;
    review: ReviewStatus;
    conflicts: number;
    active_sessions: ActiveSession[];
    dreaming: DreamingStatus;
    recall: RecallStatus;
}

export interface EntityNode {
    id: string;
    label: string;
    kind: string;
    namespace?: string;
    memory_count: number;
}

export interface EntityEdge {
    source: string;
    target: string;
    kind: string;
    weight: number;
    temporal_from?: string | null;
    temporal_to?: string | null;
}

export interface EntityGraphResponse {
    nodes: EntityNode[];
    edges: EntityEdge[];
}

export interface EntityMemorySummary {
    id: string;
    namespace: string;
    status: string;
    confidence: number;
}

export interface EntityDetailResponse {
    entity_id: string;
    label: string;
    mentions: string[];
    related_memories: EntityMemorySummary[];
    first_seen?: string | null;
    last_seen?: string | null;
    memories: EntityMemorySummary[];
    supersession_chain: string[];
    recall_history: Array<{ at: string; count: number }>;
}

export interface RoiResponse {
    window_days: number;
    promotion_rate: number;
    promotion_precision: number;
    refusal_breakdown: Record<string, number>;
    dreaming: {
        candidates_generated: number;
        promoted_silent: number;
        entered_review_queue: number;
        dropped: number;
        review_queue_approval_rate: number;
    };
    reality_check_adherence: {
        weeks_completed: number;
        weeks_skipped: number;
    };
}

export interface RealityCheckComponentScores {
    days_since_observed_norm: number;
    recall_frequency_norm: number;
    cross_source_corroboration: number;
    confidence_decay: number;
    sensitivity_weight: number;
}

export interface RealityCheckApiItem {
    memory_id: string;
    title: string;
    namespace: string;
    status: string;
    sensitivity?: string | null;
    score: number;
    component_scores: RealityCheckComponentScores;
    encrypted: boolean;
    last_observed_at: string;
    recall_count_30d: number;
    last_recalled_at?: string | null;
}

export interface RealityCheckStatusResponse {
    kind: string;
    session_id: string;
    items: RealityCheckApiItem[];
    total_scored: number;
    last_completed_at?: string | null;
}

export interface RealityCheckHistoryResponse {
    sessions: Array<{
        session_id: string;
        started_at: string;
        completed_at: string;
        items_total: number;
        reviewed: number;
        confirmed: number;
        corrected: number;
        forgotten: number;
        not_relevant: number;
        deferred: number;
        remaining: number;
    }>;
}

export interface RealityCheckRespondRequest {
    session_id: string;
    memory_id: string;
    action: string;
    correction?: string;
}

export interface RealityCheckActionResponse {
    accepted: boolean;
    session_id: string;
    memory_id: string;
    action: string;
    completion: unknown;
}

export interface RecallHitSummary {
    event_id: string;
    device: string;
    seq: number;
    memory_id: string;
    recalled_at: string;
    summary?: string | null;
}

export interface RecallHitsResponse {
    since?: string | null;
    limit: number;
    hits: RecallHitSummary[];
}

export interface AuditMemoryResponse {
    memory_id: string;
    title: string;
    body: string;
    status: string;
    namespace: string;
    confidence: number;
    confidence_reason?: string | null;
    recall_count_total: number;
    recall_count_30d: number;
    last_recalled?: string | null;
    provenance_chain: unknown[];
    policy_decisions: unknown[];
    privacy_scan: unknown;
    supersession_history: unknown[];
    sync_state: unknown;
}

export interface ProvenanceWalkResponse {
    memory_id: string;
    direction: string;
    depth: number;
    nodes: Array<{ id: string; kind: string; label: string }>;
    edges: Array<{ source: string; target: string; kind: string }>;
}

export interface TemporalStateResponse {
    memory_id: string;
    at?: string | null;
    viewing_historical_state: boolean;
    artifact: unknown;
}

export interface ReviewQueueItem {
    id: string;
    summary: string;
    status: string;
    namespace: string;
    policy_applied: string;
    reason?: string | null;
    next_actions: string[];
}

export interface ReviewQueueResponse {
    items: ReviewQueueItem[];
    limit: number;
    offset: number;
}

export interface ReviewActionRequest {
    id: string;
    action: string;
    reason?: string;
}

export interface ReviewActionResponse {
    ok: boolean;
    id: string;
    action: string;
}

export interface GovernancePolicySummary {
    scope: string;
    selected_policy: string;
    policy_source?: string;
    confidence_floor?: number;
    review_gates?: string[];
    requires_grounding?: boolean;
}

export interface PolicyEditorResponse {
    source: string;
    raw_yaml: string;
    writable: boolean;
    files: string[];
    policies: GovernancePolicySummary[];
}

export interface PolicyEditorPostRequest {
    raw_yaml: string;
    file_name?: string;
}

export interface PolicyEditorPostResponse {
    accepted: boolean;
    file_name: string;
    policies: GovernancePolicySummary[];
}

export interface PeerSessionStatus {
    session_id: string;
    harness: string;
    namespace: string;
    salient_entities: string[];
    started_at?: string | null;
    last_heartbeat_age_seconds: number;
}

export interface ClaimLockInfo {
    id?: string;
    namespace?: string;
    holder?: string;
    held_by?: string;
    memory_id?: string;
    age_seconds?: number;
}

export interface SyncDashboardResponse {
    sync: SyncStatus;
    last_commit?: string | null;
    peer_presence: {
        coordination_level: number;
        active_session_count: number;
        active_sessions: PeerSessionStatus[];
        recent_delivery_count: number;
    };
    claim_locks: {
        active_count: number;
        locks: ClaimLockInfo[];
    };
}

export interface NotificationSnapshotItem {
    id: string;
    title: string;
    body: string;
    tone?: 'ok' | 'warn' | 'bad';
    created_at?: string;
}

export interface NotificationsHeartbeat {
    kind: 'heartbeat';
    notifications: NotificationSnapshotItem[];
}
