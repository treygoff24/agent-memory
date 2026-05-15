import { expect, test } from '../support/playwright';

test('entities table filters by kind and search while keeping the entity-detail inspector', async ({ page }) => {
    await page.goto('/?view=entities');
    // Graph is the default mode; switch to table for the search-input flow.
    await page.getByRole('tab', { name: /^table$/i }).click();
    await page.getByRole('tab', { name: /^tool\s/i }).click();
    await page.getByLabel('Entity search').fill('pnpm');
    await expect(page.getByTestId('entities-view-tool')).toContainText('pnpm');
    await expect(page.getByRole('region', { name: 'Inspector' })).toContainText('entity');
});

test('entities defaults to graph mode and renders the SVG entity layout', async ({ page }) => {
    await page.goto('/?view=entities');
    await expect(page.getByRole('group', { name: /entity relationship graph/i })).toBeVisible();
    await page.getByRole('button', { name: /select entity pnpm/i }).press('Enter');
    await expect(page).toHaveURL(/#\/entities\/ent_pnpm$/);
    await expect(page.getByRole('region', { name: 'Inspector' })).toContainText('pnpm');
    await expect(page.getByRole('tab', { name: /^graph$/i })).toHaveAttribute('aria-selected', 'true');
});

test('legacy entities table-mode URL migrates mode into the hash query', async ({ page }) => {
    await page.goto('/?view=entities&mode=table');
    await expect(page).toHaveURL(/#\/entities\?mode=table$/);
    await expect(page.getByLabel('Entity search')).toBeVisible();
    await expect(page.getByRole('tab', { name: /^table$/i })).toHaveAttribute('aria-selected', 'true');
});
