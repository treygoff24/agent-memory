import { expect, test } from '../support/playwright';

test.use({ apiScenario: 'heavy' });

test('recallScroll heavy ledger stays within the 60fps mean frame budget', async ({ page }) => {
  await page.goto('/?view=recall&recallState=heavy');
  const scroller = page.getByLabel('Recall event ledger');
  await expect(page.getByRole('main')).toContainText('9,000 events');
  await expect(scroller).toBeVisible();

  const meanFrameMs = await scroller.evaluate(async (element) => {
    const frameDeltas: number[] = [];
    let previous = globalThis.performance.now();
    const maxScroll = Math.max(0, element.scrollHeight - element.clientHeight);
    for (let step = 0; step < 24; step += 1) {
      await new Promise<void>((resolve) => {
        globalThis.requestAnimationFrame((now) => {
          frameDeltas.push(now - previous);
          previous = now;
          element.scrollTop = Math.round((maxScroll * step) / 23);
          resolve();
        });
      });
    }
    return frameDeltas.reduce((sum, value) => sum + value, 0) / frameDeltas.length;
  });

  expect(meanFrameMs).toBeLessThanOrEqual(16.6);
});
