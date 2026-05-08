import { expect, test } from '../support/playwright';
import { themes } from '../support/surface';

for (const theme of themes) {
  test(`@visual palette open ${theme}`, async ({ page }) => {
    await page.goto(`/?theme=${theme}`);
    await page.keyboard.press('Shift+Semicolon');
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
    await expect(page.getByPlaceholder('Type a command…')).toBeVisible();
    await expect(page.locator('.palette-row').first()).toBeVisible();
  });
}
