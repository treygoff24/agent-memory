import { expect, test } from '../support/playwright';

const themes = ['warm-dark', 'warm-light', 'cool-dark', 'cool-light', 'monochrome', 'high-contrast'];
const states = ['all', 'proposed', 'queued', 'running'];

for (const state of states) {
    for (const theme of themes) {
        test(`@visual dreams ${state} ${theme}`, async ({ page }) => {
            await page.goto(`/?view=dreams&dreamState=${state}&theme=${theme}`);
            await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
            await expect(page.getByTestId(`dreams-view-${state}`)).toBeAttached();
            await expect(page.getByRole('tablist', { name: 'Dream status filters' })).toBeVisible();
        });
    }
}
