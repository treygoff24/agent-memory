import { expect, test } from '@playwright/test';

test('inbox keyboard filters and row navigation drive the Inspector', async ({ page }) => {
    await page.goto('/?view=inbox');
    await page.keyboard.press('3');
    await expect(page.getByRole('tab', { name: /conflicts.*3/i })).toHaveAttribute('aria-selected', 'true');
    await page.keyboard.press('j');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('region', { name: 'Inspector' })).toContainText('Editor preference disagreement');
});
