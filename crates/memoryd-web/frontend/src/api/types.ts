export interface ApiErrorBody {
    error?: string;
    message?: string;
    route?: string;
    code?: string;
    action?: string;
}

interface DaemonStatus {
    version: string;
    pid: number;
    uptime_seconds: number | null;
}

interface IndexStatus {
    active_memories: number;
    last_reindex: string | null;
}

interface SyncStatus {
    ahead: number;
    behind: number;
    remote: string;
    last_push: string | null;
}

interface ReviewStatus {
    candidate: number;
    quarantined: number;
    dream_low_confidence: number;
}

interface ActiveSession {
    harness: string;
    session_id: string;
}

interface DreamRunSummary {
    at: string | null;
    promoted: number | null;
    queued: number | null;
    dropped: number | null;
}

interface DreamingStatus {
    status: string;
    next_run: string | null;
    last_run: DreamRunSummary;
}

interface RecallStatus {
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

interface EntityEdge {
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

interface EntityMemorySummary {
    id: string;
    namespace: string;
    status: string;
    confidence: number | null;
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

interface RealityCheckComponentScores {
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

export interface RealityCheckRespondRequest {
    session_id: string;
    memory_id: string;
    action: string;
    correction?: string;
}

type RealityCheckCompletion =
    | { progress: { remaining: number; deferred: number } }
    | { complete: { reviewed: number; deferred: number; completed_at: string } };

export interface RealityCheckActionResponse {
    accepted: boolean;
    session_id: string;
    memory_id: string;
    action: string;
    completion: RealityCheckCompletion;
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

export interface SearchHitSummary {
    id: string;
    summary: string;
    snippet: string;
    score: number;
}

export interface DashboardSearchResponse {
    hits: SearchHitSummary[];
    total: number;
    guidance: string;
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

interface GovernancePolicySummary {
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
    current_file?: string | null;
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
    kind: string;
    message: string;
    tone?: 'ok' | 'warn' | 'bad';
    created_at?: string;
}

export interface NotificationsHeartbeat {
    kind: 'heartbeat';
    notifications: NotificationSnapshotItem[];
    error?: {
        code: string;
        message: string;
    };
}
