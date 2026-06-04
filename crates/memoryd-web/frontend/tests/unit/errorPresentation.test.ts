import { describe, expect, it } from 'vitest';

import { ApiError } from '../../src/api/client';
import { apiErrorBody, apiErrorTitle, apiErrorTone } from '../../src/api/errorPresentation';

describe('api error presentation', () => {
    it('maps known HTTP statuses to stable titles', () => {
        expect(apiErrorTitle(new ApiError(403, { error: 'forbidden' }), 'Search')).toBe('Search: permission required');
        expect(apiErrorTitle(new ApiError(409, { error: 'conflict' }), 'Search')).toBe('Search: conflict');
        expect(apiErrorTitle(new ApiError(503, { error: 'daemon_unavailable' }), 'Search')).toBe(
            'Search: backend unavailable',
        );
    });

    it('prefers API message fields in the body copy', () => {
        expect(apiErrorBody(new ApiError(503, { error: 'daemon_unavailable', message: 'socket closed' }))).toBe(
            'socket closed',
        );
        expect(apiErrorBody(new Error('network reset'))).toBe('network reset');
    });

    it('treats conflicts as warning tone', () => {
        expect(apiErrorTone(new ApiError(409, { error: 'conflict' }))).toBe('warn');
        expect(apiErrorTone(new ApiError(503, { error: 'daemon_unavailable' }))).toBe('bad');
    });
});
