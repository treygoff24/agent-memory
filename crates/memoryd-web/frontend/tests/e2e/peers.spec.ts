import { expect, test } from '../support/playwright';

test('peers trust ledger sorts and opens the peer-detail inspector', async ({ page }) => {
  await page.goto('/?view=peers');
  await expect(page.getByTestId('peers-view')).toContainText('Peers');
  await page.getByRole('button', { name: /events 24h/i }).click();
  await expect(page.getByRole('region', { name: 'Inspector' })).toContainText('peer detail');
  await expect(page.getByTestId('peers-view')).toContainText('fenced');
});
