import { expect, test } from '../support/playwright';

const themes = ['warm-dark', 'warm-light', 'cool-dark', 'cool-light', 'monochrome', 'high-contrast'];

for (const theme of themes) {
    test(`@visual inbox two-pane ${theme}`, async ({ page }) => {
        await page.goto(`/?theme=${theme}#/inbox`);
        await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
        await expect(page.getByTestId('inbox-layout-two-pane')).toBeAttached();
        await expect(page.getByRole('tablist', { name: 'Inbox filters' })).toBeVisible();
    });
}
