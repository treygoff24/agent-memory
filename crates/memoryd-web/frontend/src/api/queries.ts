import { useQuery } from '@tanstack/react-query';

import type {
    AuditMemoryResponse,
    EntityDetailResponse,
    EntityGraphResponse,
    PolicyEditorResponse,
    ProvenanceWalkResponse,
    RealityCheckHistoryResponse,
    RealityCheckStatusResponse,
    RecallHitsResponse,
    ReviewQueueResponse,
    DashboardRoiResponse,
    DashboardSearchResponse,
    StatusResponse,
    SyncDashboardResponse,
    TemporalStateResponse,
} from './types';

import { apiJson } from './client';

export type ReviewQueueParams = {
    status?: string;
    namespace?: string;
    limit?: number;
    offset?: number;
};

export type RecallHitsParams = {
    since?: string;
    limit?: number;
};

export const queryKeys = {
    status: ['status'] as const,
    entityGraph: ['entityGraph'] as const,
    entityDetail: (id: string) => ['entityGraph', id] as const,
    roi: (windowDays?: number) => ['roi', windowDays ?? null] as const,
    realityCheck: ['realityCheck'] as const,
    realityCheckHistory: (limit?: number) => ['realityCheckHistory', limit ?? null] as const,
    recallHits: (params: RecallHitsParams = {}) => ['recallHits', params] as const,
    search: (query: string) => ['search', query] as const,
    audit: (id: string) => ['audit', id] as const,
    auditWalk: (id: string, direction?: string, depth?: number) =>
        ['auditWalk', id, direction ?? null, depth ?? null] as const,
    auditTemporal: (id: string, at?: string) => ['auditTemporal', id, at ?? null] as const,
    review: (params: ReviewQueueParams = {}) => ['review', params] as const,
    policy: ['policy'] as const,
    sync: ['sync'] as const,
};

type QueryParamValue = string | number | boolean | undefined;

function withParams(path: string, params: Record<string, QueryParamValue>): string {
    const search = new URLSearchParams();
    for (const [key, value] of Object.entries(params)) {
        if (value !== undefined) search.set(key, String(value));
    }
    const query = search.toString();
    return query ? `${path}?${query}` : path;
}

export function useStatusQuery() {
    return useQuery({
        queryKey: queryKeys.status,
        queryFn: () => apiJson<StatusResponse>('/api/status'),
    });
}

export function useEntityGraphQuery() {
    return useQuery({
        queryKey: queryKeys.entityGraph,
        queryFn: () => apiJson<EntityGraphResponse>('/api/entity-graph'),
    });
}

export function useEntityDetailQuery(id: string) {
    return useQuery({
        queryKey: queryKeys.entityDetail(id),
        queryFn: () => apiJson<EntityDetailResponse>(`/api/entity-graph/${encodeURIComponent(id)}`),
        enabled: id.length > 0,
    });
}

export function useRoiQuery(windowDays?: number) {
    return useQuery({
        queryKey: queryKeys.roi(windowDays),
        queryFn: () => apiJson<DashboardRoiResponse>(withParams('/api/roi', { window: windowDays })),
    });
}

export function useRealityCheckQuery() {
    return useQuery({
        queryKey: queryKeys.realityCheck,
        queryFn: () => apiJson<RealityCheckStatusResponse>('/api/reality-check'),
    });
}

export function useRealityCheckHistoryQuery(limit?: number) {
    return useQuery({
        queryKey: queryKeys.realityCheckHistory(limit),
        queryFn: () => apiJson<RealityCheckHistoryResponse>(withParams('/api/reality-check/history', { limit })),
    });
}

export function useRecallHitsQuery(params: RecallHitsParams = {}) {
    return useQuery({
        queryKey: queryKeys.recallHits(params),
        queryFn: () => apiJson<RecallHitsResponse>(withParams('/api/recall-hits', params)),
    });
}

export function searchMemories(query: string, limit = 5) {
    return apiJson<DashboardSearchResponse>(withParams('/api/search', { q: query, limit }));
}

export function useAuditQuery(id: string) {
    return useQuery({
        queryKey: queryKeys.audit(id),
        queryFn: () => apiJson<AuditMemoryResponse>(`/api/audit/${encodeURIComponent(id)}`),
        enabled: id.length > 0,
    });
}

export function useAuditWalkQuery(id: string, direction?: string, depth?: number) {
    return useQuery({
        queryKey: queryKeys.auditWalk(id, direction, depth),
        queryFn: () =>
            apiJson<ProvenanceWalkResponse>(
                withParams(`/api/audit/${encodeURIComponent(id)}/walk`, { direction, depth }),
            ),
        enabled: id.length > 0,
    });
}

export function useAuditTemporalQuery(id: string, at?: string) {
    return useQuery({
        queryKey: queryKeys.auditTemporal(id, at),
        queryFn: () =>
            apiJson<TemporalStateResponse>(withParams(`/api/audit/${encodeURIComponent(id)}/temporal`, { at })),
        enabled: id.length > 0,
    });
}

export function useReviewQueueQuery(params: ReviewQueueParams = {}) {
    return useQuery({
        queryKey: queryKeys.review(params),
        queryFn: () => apiJson<ReviewQueueResponse>(withParams('/api/review', params)),
    });
}

export function usePolicyEditorQuery() {
    return useQuery({
        queryKey: queryKeys.policy,
        queryFn: () => apiJson<PolicyEditorResponse>('/api/policy-editor'),
    });
}

export function useSyncDashboardQuery() {
    return useQuery({
        queryKey: queryKeys.sync,
        queryFn: () => apiJson<SyncDashboardResponse>('/api/sync-dashboard'),
    });
}
