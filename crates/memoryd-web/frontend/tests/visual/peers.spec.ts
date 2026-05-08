import { expect, test } from '@playwright/test';

const themes = ['warm-dark', 'warm-light', 'cool-dark', 'cool-light', 'monochrome', 'high-contrast'];

for (const theme of themes) {
    test(`@visual peers ledger ${theme}`, async ({ page }) => {
        await page.goto(`/?view=peers&theme=${theme}`);
        await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
        await expect(page.getByTestId('peers-view')).toBeAttached();
        await expect(page.getByRole('region', { name: 'Inspector' })).toContainText('peer detail');
    });
}
