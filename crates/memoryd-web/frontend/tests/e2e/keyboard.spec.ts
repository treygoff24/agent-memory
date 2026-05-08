import { expect, test } from '../support/playwright';

test('keyboard g-prefix navigation reaches settings', async ({ page }) => {
  await page.goto('/?view=inbox');

  await page.keyboard.press('g');
  await page.keyboard.press('s');

  await expect(page.getByRole('main').getByRole('tab', { name: 'Appearance' })).toBeVisible();
});

test('keyboard shortcuts do not fire from text inputs', async ({ page }) => {
  await page.goto('/?view=inbox');

  await page.getByRole('textbox', { name: 'Search memories' }).focus();
  await page.keyboard.press('g');
  await page.keyboard.press('s');

  await expect(page.getByRole('main').getByText('Inbox', { exact: true })).toBeVisible();
  await expect(page.getByRole('main').getByRole('tab', { name: 'Appearance' })).toHaveCount(0);
});
