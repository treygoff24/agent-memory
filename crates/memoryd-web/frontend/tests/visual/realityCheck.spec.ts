import { expect, test } from '@playwright/test';

const themes = ['warm-dark', 'warm-light', 'cool-dark', 'cool-light', 'monochrome', 'high-contrast'];
const variants = ['default', 'encrypted', 'refused', 'score-open', 'complete'];

for (const variant of variants) {
    for (const theme of themes) {
        test(`@visual realityCheck ${variant} ${theme}`, async ({ page }) => {
            await page.goto(`/?view=reality&variant=${variant}&theme=${theme}`);
            await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
            await expect(page.getByTestId(`reality-check-${variant}`)).toBeAttached();
            await expect(page.getByTestId(`reality-check-${variant}`).getByText('reality check', { exact: true })).toBeVisible();
        });
    }
}
