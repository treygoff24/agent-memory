import { expect, test } from '../support/playwright';
import { apiSurfaceViews, gotoSurfaceView } from '../support/surface';

test.use({ apiScenario: 'unavailable503' });

for (const view of apiSurfaceViews) {
  test(`state daemon-down ${view.id}`, async ({ page }) => {
    await gotoSurfaceView(page, view);
    await expect(page.locator('.banner').first()).toContainText('backend unavailable');
    await expect(page.locator('.banner').first()).toContainText('memoryd daemon is not reachable');
  });
}
