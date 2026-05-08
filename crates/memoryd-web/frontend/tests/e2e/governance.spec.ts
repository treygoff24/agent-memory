import { expect, test } from '@playwright/test';

test('governance review queue filters consent items and exposes batch actions', async ({ page }) => {
    await page.goto('/?view=governance');
    await page.getByRole('tab', { name: /consent/i }).click();
    await expect(page.getByTestId('governance-view-consent_required')).toContainText('Family detail consent required');
    await page.getByLabel(/select Family detail consent required/i).check();
    await expect(page.getByText(/1 selected/i)).toBeVisible();
    await expect(page.getByRole('region', { name: 'Inspector' })).toContainText('governance');
});
