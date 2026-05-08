import { describe, expect, it } from 'vitest';

import { scenarioHandlers } from '../msw/handlers';
import { apiRouteIds, apiScenarioNames, payloadForApiRequest } from '../msw/payloads';

describe('MSW API scenario matrix', () => {
    it('defines named overrides for every dashboard route', () => {
        for (const scenario of apiScenarioNames) {
            expect(scenarioHandlers[scenario]).toHaveLength(apiRouteIds.length);
        }
    });

    it('covers the mandated 403, 409, and 503 error statuses', () => {
        expect(payloadForApiRequest('GET', '/api/review', 'forbidden403').status).toBe(403);
        expect(payloadForApiRequest('POST', '/api/review/action', 'conflict409').status).toBe(409);
        expect(payloadForApiRequest('GET', '/api/status', 'unavailable503').status).toBe(503);
    });
});
