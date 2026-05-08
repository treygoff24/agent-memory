import { expect, test } from '../support/playwright';

test('realityCheck keyboard opens inline correction editor', async ({ page }) => {
    await page.goto('/?view=reality');
    await page.keyboard.press('k');
    await expect(page.getByRole('textbox', { name: 'Corrected memory body' })).toBeVisible();
    await expect(page.getByTestId('reality-check-default')).toContainText('Save correction');
});
