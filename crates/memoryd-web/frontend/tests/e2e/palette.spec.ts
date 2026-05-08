import { expect, test } from '../support/playwright';

test('palette fuzzy search navigates with Enter', async ({ page }) => {
    await page.goto('/?view=inbox');

    await page.keyboard.press('Shift+Semicolon');
    const paletteInput = page.getByPlaceholder('Type a command…');
    await expect(paletteInput).toBeVisible();
    await paletteInput.fill('settings');
    await paletteInput.press('Enter');

    await expect(page.getByRole('main').getByRole('tab', { name: 'Appearance' })).toBeVisible();
});
