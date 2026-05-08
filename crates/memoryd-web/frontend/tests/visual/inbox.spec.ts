import { expect, test } from '../support/playwright';

const themes = ['warm-dark', 'warm-light', 'cool-dark', 'cool-light', 'monochrome', 'high-contrast'];
const layouts = ['two-pane', 'three-pane', 'drawer', 'modal'];

for (const layout of layouts) {
    for (const theme of themes) {
        test(`@visual inbox ${layout} ${theme}`, async ({ page }) => {
            await page.goto(`/?view=inbox&layout=${layout}&theme=${theme}`);
            await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
            await expect(page.getByTestId(`inbox-layout-${layout}`)).toBeAttached();
            await expect(page.getByRole('tablist', { name: 'Inbox filters' })).toBeVisible();
        });
    }
}
