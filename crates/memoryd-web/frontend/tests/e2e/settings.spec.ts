import { expect, test } from '../support/playwright';

test('settings tabs expose theme editor and notification controls', async ({ page }) => {
  await page.goto('/?view=settings');

  await expect(page.getByRole('tab', { name: 'Appearance' })).toBeVisible();
  await page.getByRole('tab', { name: 'Theme editor' }).click();
  await expect(page.getByRole('region', { name: 'Theme editor' })).toContainText(
    'Custom theme preview',
  );
  await page.getByRole('tab', { name: 'Notifications' }).click();
  await expect(page.getByRole('region', { name: 'Notifications' })).toContainText(
    'Daemon health alerts',
  );
});

test('settings tweaks mode opens the dev tweaks panel', async ({ page }) => {
  await page.goto('/?tweaks=1');

  await expect(page.getByRole('main').getByText('Settings', { exact: true })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Dev tweaks' })).toContainText(
    'Experimental dashboard controls',
  );
});
