import { expect, type Page } from '@playwright/test';

export const themes = ['warm-dark', 'warm-light', 'cool-dark', 'cool-light', 'monochrome', 'high-contrast'] as const;

export const apiSurfaceViews = [
    { id: 'inbox', label: 'Inbox', title: 'Inbox' },
    { id: 'reality', label: 'Reality Check', testId: /^reality-check-/ },
    { id: 'recall', label: 'Recall ledger', title: 'Recall ledger' },
    { id: 'dreams', label: 'Dreams', title: 'Dreams' },
    { id: 'peers', label: 'Peers', title: 'Peers' },
    { id: 'governance', label: 'Governance', title: 'Governance' },
    { id: 'entities', label: 'Entities', title: 'Entities' },
] as const;

export const allSurfaceViews = [...apiSurfaceViews, { id: 'settings', label: 'Settings', title: 'Settings' }] as const;

export async function gotoSurfaceView(page: Page, view: { id: string }, params: Record<string, string> = {}) {
    const query = new URLSearchParams({ view: view.id, ...params });
    await page.goto(`/?${query.toString()}`);
}

export async function expectSurfaceReady(page: Page, view: { title?: string; testId?: RegExp }) {
    await expect(page.getByRole('main')).toBeVisible();
    if (view.title) {
        await expect(page.getByRole('main').getByText(view.title, { exact: true })).toBeVisible();
        return;
    }
    if (view.testId) {
        await expect(page.getByTestId(view.testId)).toBeAttached();
    }
}
