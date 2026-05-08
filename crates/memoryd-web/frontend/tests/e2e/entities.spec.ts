import { expect, test } from '../support/playwright';

test('entities table filters by kind and search while keeping the entity-detail inspector', async ({ page }) => {
    await page.goto('/?view=entities');
    await page.getByRole('tab', { name: /tool/i }).click();
    await page.getByLabel('Entity search').fill('pnpm');
    await expect(page.getByTestId('entities-view-tool')).toContainText('pnpm');
    await expect(page.getByRole('region', { name: 'Inspector' })).toContainText('entity');
});
