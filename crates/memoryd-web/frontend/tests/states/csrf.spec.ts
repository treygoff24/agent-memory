import { expect, test } from '../support/playwright';
import { apiSurfaceViews, gotoSurfaceView } from '../support/surface';

test.use({ apiScenario: 'forbidden403' });

for (const view of apiSurfaceViews) {
  test(`state csrf-403 ${view.id}`, async ({ page }) => {
    await gotoSurfaceView(page, view);
    await expect(page.locator('.banner').first()).toContainText('permission required');
    await expect(page.locator('.banner').first()).toContainText('Dashboard policy forbids this request');
  });
}
