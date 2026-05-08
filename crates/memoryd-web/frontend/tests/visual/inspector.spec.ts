import { expect, test } from '@playwright/test';

const themes = ['warm-dark', 'warm-light', 'cool-dark', 'cool-light', 'monochrome', 'high-contrast'];
const kinds = [
    'inbox-review',
    'inbox-recall',
    'inbox-conflict',
    'inbox-due',
    'inbox-dream',
    'recall-event',
    'dream-output',
    'peer-detail',
    'governance-decision',
    'entity-detail',
];

for (const kind of kinds) {
    for (const theme of themes) {
        test(`@visual inspector ${kind} ${theme}`, async ({ page }) => {
            await page.goto(`/?view=inbox&theme=${theme}&inspectorKind=${kind}`);
            await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
            await expect(page.getByRole('region', { name: /inspector/i })).toBeAttached();
        });
    }
}
