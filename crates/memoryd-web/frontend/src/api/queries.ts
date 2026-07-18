import { useQuery } from '@tanstack/react-query';

import type {
    EntityDetailResponse,
    EntityGraphResponse,
    PolicyEditorResponse,
    RealityCheckStatusResponse,
    RecallHitsResponse,
    ReviewQueueResponse,
    DashboardSearchResponse,
    StatusResponse,
    SyncDashboardResponse,
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
    realityCheck: ['realityCheck'] as const,
    recallHits: (params: RecallHitsParams = {}) => ['recallHits', params] as const,
    search: (query: string) => ['search', query] as const,
    review: (params: ReviewQueueParams = {}) => ['review', params] as const,
    policy: ['policy'] as const,
    sync: ['sync'] as const,
};

type SearchParams = { q: string; limit: number };
type QueryParams = RecallHitsParams | ReviewQueueParams | SearchParams;

function withParams(path: string, params: QueryParams): string {
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

export function useRealityCheckQuery() {
    return useQuery({
        queryKey: queryKeys.realityCheck,
        queryFn: () => apiJson<RealityCheckStatusResponse>('/api/reality-check'),
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
