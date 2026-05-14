import { expect, test } from '../support/playwright';

test('peers card layout renders cards and opens the peer-detail inspector', async ({ page }) => {
    await page.goto('/?view=peers');
    await expect(page.getByTestId('peers-view')).toContainText('Peers');
    await expect(page.getByTestId('peer-card').first()).toBeAttached();
    await expect(page.getByRole('region', { name: 'Inspector' })).toContainText('peer detail');
    await expect(page.getByTestId('peers-view')).toContainText('fenced');
});

test('peers table layout sorts and opens the peer-detail inspector', async ({ page }) => {
    await page.goto('/?view=peers&layout=table');
    await expect(page.getByTestId('peers-view')).toContainText('Peers');
    await page.getByRole('button', { name: /events 24h/i }).click();
    await expect(page.getByRole('region', { name: 'Inspector' })).toContainText('peer detail');
    await expect(page.getByTestId('peers-view')).toContainText('fenced');
});
