import { expect, test } from '../support/playwright';
import { allSurfaceViews, expectSurfaceReady, gotoSurfaceView } from '../support/surface';

// Reality Check is fullbleed by design (brief §View 2): chrome dissolves and the
// topbar is hidden so the question can fill the viewport. The bell lives in the
// topbar, so it isn't reachable while RC is the active view.
const viewsWithBell = allSurfaceViews.filter((view) => view.id !== 'reality');

for (const view of viewsWithBell) {
    test(`state bell-open ${view.id}`, async ({ page }) => {
        await gotoSurfaceView(page, view);
        await expectSurfaceReady(page, view);
        await page.getByRole('button', { name: 'Notifications' }).click();
        await expect(page.locator('.notif')).toBeVisible();
        await expect(page.locator('.notif')).toContainText('Notifications');
    });
}
