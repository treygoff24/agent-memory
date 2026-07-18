import { expect, test as base, type Page } from '@playwright/test';

import { payloadForApiRequest, type ApiRequestBody, type ApiScenario } from '../msw/payloads';

async function installApiMocks(page: Page, scenario: ApiScenario) {
    await page.route('**/api/**', async (route) => {
        const request = route.request();
        const pathname = new globalThis.URL(request.url()).pathname;
        if (!pathname.startsWith('/api/')) {
            await route.continue();
            return;
        }
        let body: ApiRequestBody | undefined;
        if (request.method() !== 'GET') {
            try {
                body = request.postDataJSON() as ApiRequestBody;
            } catch {
                body = undefined;
            }
        }
        const payload = payloadForApiRequest(request.method(), request.url(), scenario, body);
        await route.fulfill({
            status: payload.status,
            contentType: payload.contentType,
            body: payload.body,
        });
    });
}

export const test = base.extend<{ apiScenario: ApiScenario }>({
    apiScenario: ['happy', { option: true }],
    page: async ({ page, apiScenario }, run) => {
        await installApiMocks(page, apiScenario);
        await run(page);
    },
});

export { expect };
