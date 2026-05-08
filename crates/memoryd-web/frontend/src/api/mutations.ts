import { useMutation, useQueryClient } from '@tanstack/react-query';

import { apiJson } from './client';
import { queryKeys } from './queries';

export function useReviewActionMutation() {
    const queryClient = useQueryClient();
    return useMutation({
        mutationFn: (input: { id: string; action: string; reason?: string }) =>
            apiJson('/api/review/action', { method: 'POST', body: JSON.stringify(input) }),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: ['review'] }),
    });
}

export function usePolicyEditorMutation() {
    const queryClient = useQueryClient();
    return useMutation({
        mutationFn: (input: { raw_yaml: string; file_name?: string }) =>
            apiJson('/api/policy-editor', { method: 'POST', body: JSON.stringify(input) }),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.policy }),
    });
}
