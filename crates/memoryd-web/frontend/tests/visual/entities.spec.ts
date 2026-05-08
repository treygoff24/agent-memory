import { expect, test } from '../support/playwright';

const themes = ['warm-dark', 'warm-light', 'cool-dark', 'cool-light', 'monochrome', 'high-contrast'];

for (const theme of themes) {
    test(`@visual entities table ${theme}`, async ({ page }) => {
        await page.goto(`/?view=entities&theme=${theme}`);
        await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
        await expect(page.getByTestId('entities-view-all')).toBeAttached();
        await expect(page.getByRole('region', { name: 'Inspector' })).toContainText('entity');
    });
}
