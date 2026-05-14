import { expect, test } from '../support/playwright';

const themes = ['warm-dark', 'warm-light', 'cool-dark', 'cool-light', 'monochrome', 'high-contrast'];
const states = ['all', 'proposed', 'queued', 'running'];

// Brief §View 4 splits Dreams into Journal / Questions / Cleanup sub-tabs.
// The visual matrix exercises status filters within the default Journal tab;
// the per-tab variants get exercised by the e2e + unit tests.
for (const state of states) {
    for (const theme of themes) {
        test(`@visual dreams ${state} ${theme}`, async ({ page }) => {
            await page.goto(`/?view=dreams&dreamTab=journal&dreamState=${state}&theme=${theme}`);
            await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
            await expect(page.getByTestId(`dreams-view-journal-${state}`)).toBeAttached();
            await expect(page.getByRole('tablist', { name: 'Dream status filters' })).toBeVisible();
            await expect(page.getByRole('tablist', { name: 'Dream sub-tabs' })).toBeVisible();
        });
    }
}
