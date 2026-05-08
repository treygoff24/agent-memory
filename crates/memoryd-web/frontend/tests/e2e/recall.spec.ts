import { expect, test } from '../support/playwright';

test('recall ledger filters and inspector render', async ({ page }) => {
    await page.goto('/?view=recall');
    await page.getByLabel('Agent filter').selectOption('codex');
    await page.getByLabel('Recall search').fill('pnpm');
    await expect(page.getByTestId('recall-ledger-default')).toContainText('Recall ledger');
    await expect(page.getByRole('region', { name: 'Inspector' })).toContainText('recall event');
});
