import { expect, test } from '../support/playwright';
import { allSurfaceViews, expectSurfaceReady, gotoSurfaceView } from '../support/surface';

for (const view of allSurfaceViews) {
  test(`state palette-open ${view.id}`, async ({ page }) => {
    await gotoSurfaceView(page, view);
    await expectSurfaceReady(page, view);
    await page.keyboard.press('Shift+Semicolon');
    await expect(page.getByPlaceholder('Type a command…')).toBeVisible();
    await expect(page.locator('.palette-row').first()).toContainText(/Open Settings|Go to Inbox/);
  });
}
