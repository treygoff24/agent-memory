import { expect, test } from '../support/playwright';
import { apiSurfaceViews, expectSurfaceReady, gotoSurfaceView } from '../support/surface';

test.use({ apiScenario: 'empty' });

for (const view of apiSurfaceViews) {
    test(`state empty ${view.id}`, async ({ page }) => {
        await gotoSurfaceView(page, view);
        await expectSurfaceReady(page, view);
        if (view.id === 'inbox') {
            await expect(page.getByText('Inbox is clear')).toBeVisible();
        }
    });
}
