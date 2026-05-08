import { useMutation, useQueryClient } from '@tanstack/react-query';

import { apiJson } from './client';
import { queryKeys } from './queries';
import type {
  PolicyEditorPostRequest,
  PolicyEditorPostResponse,
  PolicyEditorResponse,
  RealityCheckActionResponse,
  RealityCheckRespondRequest,
  RealityCheckStatusResponse,
  ReviewActionRequest,
  ReviewActionResponse,
  ReviewQueueResponse,
} from './types';

export function useRealityCheckRespondMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: RealityCheckRespondRequest) =>
      apiJson<RealityCheckActionResponse>('/api/reality-check/respond', {
        method: 'POST',
        body: JSON.stringify(input),
      }),
    onMutate: async (input) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.realityCheck });
      const previous = queryClient.getQueryData<RealityCheckStatusResponse>(queryKeys.realityCheck);
      queryClient.setQueryData<RealityCheckStatusResponse>(queryKeys.realityCheck, (current) => {
        if (!current) return current;
        const items = current.items.filter((item) => item.memory_id !== input.memory_id);
        return { ...current, items, total_scored: Math.max(0, current.total_scored - 1) };
      });
      return { previous };
    },
    onError: (_error, _input, context) => {
      if (context?.previous) queryClient.setQueryData(queryKeys.realityCheck, context.previous);
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: queryKeys.realityCheck }),
  });
}

export function useReviewActionMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: ReviewActionRequest) =>
      apiJson<ReviewActionResponse>('/api/review/action', {
        method: 'POST',
        body: JSON.stringify(input),
      }),
    onMutate: async (input) => {
      await queryClient.cancelQueries({ queryKey: ['review'] });
      const snapshots = queryClient.getQueriesData<ReviewQueueResponse>({ queryKey: ['review'] });
      for (const [key, current] of snapshots) {
        if (!current) continue;
        queryClient.setQueryData<ReviewQueueResponse>(key, {
          ...current,
          items: current.items.map((item) =>
            item.id === input.id
              ? { ...item, status: input.action, reason: input.reason ?? item.reason ?? null }
              : item,
          ),
        });
      }
      return { snapshots };
    },
    onError: (_error, _input, context) => {
      for (const [key, data] of context?.snapshots ?? []) queryClient.setQueryData(key, data);
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['review'] }),
  });
}

export function usePolicyEditorMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: PolicyEditorPostRequest) =>
      apiJson<PolicyEditorPostResponse>('/api/policy-editor', {
        method: 'POST',
        body: JSON.stringify(input),
      }),
    onMutate: async (input) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.policy });
      const previous = queryClient.getQueryData<PolicyEditorResponse>(queryKeys.policy);
      queryClient.setQueryData<PolicyEditorResponse>(queryKeys.policy, (current) => {
        if (!current) return current;
        return { ...current, raw_yaml: input.raw_yaml };
      });
      return { previous };
    },
    onError: (_error, _input, context) => {
      if (context?.previous) queryClient.setQueryData(queryKeys.policy, context.previous);
    },
    onSuccess: (response) => {
      queryClient.setQueryData<PolicyEditorResponse>(queryKeys.policy, (current) => {
        if (!current) return current;
        return { ...current, policies: response.policies };
      });
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: queryKeys.policy }),
  });
}
