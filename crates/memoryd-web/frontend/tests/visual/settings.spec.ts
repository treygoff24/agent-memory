import { expect, test } from '../support/playwright';
import { themes } from '../support/surface';

for (const theme of themes) {
  test(`@visual settings tabs ${theme}`, async ({ page }) => {
    await page.goto(`/?view=settings&theme=${theme}`);
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
    await expect(page.getByRole('tab', { name: 'Appearance' })).toBeVisible();
    await expect(page.getByRole('slider', { name: 'Base font size' })).toBeVisible();
  });
}
