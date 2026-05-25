import { http, HttpResponse, type HttpHandler } from 'msw';

import { apiScenarioNames, payloadForApiRequest, type ApiScenario } from './payloads';

const routes = [
    ['GET', '/api/status'],
    ['GET', '/api/entity-graph'],
    ['GET', '/api/entity-graph/:entityId'],
    ['GET', '/api/roi'],
    ['GET', '/api/reality-check'],
    ['GET', '/api/reality-check/history'],
    ['POST', '/api/reality-check/respond'],
    ['GET', '/api/recall-hits'],
    ['GET', '/api/search'],
    ['GET', '/api/audit/:id'],
    ['GET', '/api/audit/:id/walk'],
    ['GET', '/api/audit/:id/temporal'],
    ['GET', '/api/review'],
    ['POST', '/api/review/action'],
    ['GET', '/api/notifications/stream'],
    ['GET', '/api/policy-editor'],
    ['POST', '/api/policy-editor'],
    ['GET', '/api/sync-dashboard'],
] as const;

async function requestBody(request: globalThis.Request) {
    if (request.method === 'GET') return undefined;
    return request.json().catch(() => undefined) as Promise<unknown>;
}

function handlerFor(method: string, path: string, scenario: ApiScenario): HttpHandler {
    const resolver = async ({ request }: { request: globalThis.Request }) => {
        const payload = payloadForApiRequest(method, request.url, scenario, await requestBody(request));
        return new HttpResponse(payload.body, {
            status: payload.status,
            headers: { 'content-type': payload.contentType },
        });
    };
    if (method === 'POST') return http.post(path, resolver);
    return http.get(path, resolver);
}

export function handlersForScenario(scenario: ApiScenario = 'happy'): HttpHandler[] {
    return routes.map(([method, path]) => handlerFor(method, path, scenario));
}

export const scenarioHandlers = Object.fromEntries(
    apiScenarioNames.map((scenario) => [scenario, handlersForScenario(scenario)]),
) as Record<ApiScenario, HttpHandler[]>;

export const handlers = handlersForScenario('happy');
