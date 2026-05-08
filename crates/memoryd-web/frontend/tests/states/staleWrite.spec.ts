import { expect, test } from '../support/playwright';
import { apiSurfaceViews, gotoSurfaceView } from '../support/surface';

test.use({ apiScenario: 'conflict409' });

for (const view of apiSurfaceViews) {
  test(`state stale-write-409 ${view.id}`, async ({ page }) => {
    await gotoSurfaceView(page, view);
    await expect(page.locator('.banner').first()).toContainText('conflict');
    await expect(page.locator('.banner').first()).toContainText('changed state');
  });
}
