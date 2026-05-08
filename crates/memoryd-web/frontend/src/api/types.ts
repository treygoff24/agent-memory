export interface ApiErrorBody {
    error?: string;
    message?: string;
    route?: string;
}
export interface StatusResponse {
    daemon: { version: string; pid: number; uptime_seconds: number };
    socket: string;
    sync: { ahead: number; behind: number; remote: string; last_push: string };
    active_sessions: Array<{ harness: string; session_id: string }>;
}
export interface EntityGraphResponse {
    nodes: Array<{ id: string; label: string; kind?: string; memory_count: number }>;
    edges: Array<{ source: string; target: string; kind: string; weight: number }>;
}
export interface SyncDashboardResponse {
    sync: StatusResponse['sync'];
    peer_presence: { active_session_count: number };
    claim_locks: { active_count: number };
}
export interface PolicyEditorResponse {
    source: string;
    raw_yaml: string;
    writable: boolean;
    policies: Array<{ scope: string; selected_policy: string }>;
}
