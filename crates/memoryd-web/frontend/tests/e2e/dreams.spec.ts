import { expect, test } from '../support/playwright';

test('dreams status filter drives the dream-output inspector', async ({ page }) => {
    await page.goto('/?view=dreams');
    await page.getByRole('tab', { name: /queued/i }).click();
    await expect(page.getByTestId('dreams-view-queued')).toContainText('Question: which laptop is primary now?');
    await expect(page.getByRole('region', { name: 'Inspector' })).toContainText('dream output');
});
