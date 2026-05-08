import { expect, test } from '../support/playwright';
import { allSurfaceViews, expectSurfaceReady, gotoSurfaceView } from '../support/surface';

for (const view of allSurfaceViews) {
  test(`state bell-open ${view.id}`, async ({ page }) => {
    await gotoSurfaceView(page, view);
    await expectSurfaceReady(page, view);
    await page.getByRole('button', { name: 'Notifications' }).click();
    await expect(page.locator('.notif')).toBeVisible();
    await expect(page.locator('.notif')).toContainText('Notifications');
  });
}
