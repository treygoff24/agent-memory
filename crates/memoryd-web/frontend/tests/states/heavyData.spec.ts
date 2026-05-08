import { expect, test } from '../support/playwright';
import { apiSurfaceViews, expectSurfaceReady, gotoSurfaceView } from '../support/surface';

test.use({ apiScenario: 'heavy' });

for (const view of apiSurfaceViews) {
  test(`state heavy-data ${view.id}`, async ({ page }) => {
    const params = view.id === 'recall' ? { recallState: 'heavy' } : {};
    await gotoSurfaceView(page, view, params);
    await expectSurfaceReady(page, view);
    if (view.id === 'recall') {
      await expect(page.getByRole('main')).toContainText('9,000 events');
      await expect(page.getByTestId('recall-virtual-list')).toBeAttached();
    }
  });
}
