import { expect, test } from '../support/playwright';

test('dreams Questions tab surfaces the question dream output in the inspector', async ({ page }) => {
    // Brief §View 4 splits Dreams into Journal/Questions/Cleanup sub-tabs.
    // Question-kind dream outputs live in the Questions tab; the queued status
    // filter applies within whichever sub-tab is active.
    await page.goto('/?view=dreams');
    await page.getByRole('tab', { name: /Questions/i }).click();
    await expect(page.getByTestId('dreams-view-questions-all')).toContainText('Question: which laptop is primary now?');
    await expect(page.getByRole('region', { name: 'Inspector' })).toContainText('dream output');
});
