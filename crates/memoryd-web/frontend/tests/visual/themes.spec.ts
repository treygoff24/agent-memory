import { expect, test } from '@playwright/test';

for (const theme of ['warm-dark', 'warm-light', 'cool-dark', 'cool-light', 'monochrome', 'high-contrast']) {
    test(`@visual theme ${theme}`, async ({ page }) => {
        await page.goto(`/?theme=${theme}`);
        await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
        await expect(page.getByRole('heading', { name: 'Memorum Dashboard' })).toBeAttached();
    });
}
