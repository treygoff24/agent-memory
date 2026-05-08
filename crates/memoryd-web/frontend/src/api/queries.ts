import { useQuery } from '@tanstack/react-query';

import type { EntityGraphResponse, PolicyEditorResponse, StatusResponse, SyncDashboardResponse } from './types';

import { apiJson } from './client';

export const queryKeys = {
    status: ['status'] as const,
    entityGraph: ['entityGraph'] as const,
    policy: ['policy'] as const,
    sync: ['sync'] as const,
};

export function useStatusQuery() {
    return useQuery({ queryKey: queryKeys.status, queryFn: () => apiJson<StatusResponse>('/api/status') });
}
export function useEntityGraphQuery() {
    return useQuery({
        queryKey: queryKeys.entityGraph,
        queryFn: () => apiJson<EntityGraphResponse>('/api/entity-graph'),
    });
}
export function usePolicyEditorQuery() {
    return useQuery({ queryKey: queryKeys.policy, queryFn: () => apiJson<PolicyEditorResponse>('/api/policy-editor') });
}
export function useSyncDashboardQuery() {
    return useQuery({ queryKey: queryKeys.sync, queryFn: () => apiJson<SyncDashboardResponse>('/api/sync-dashboard') });
}
