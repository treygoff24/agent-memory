import { expect, test } from '../support/playwright';

const themes = [
  'warm-dark',
  'warm-light',
  'cool-dark',
  'cool-light',
  'monochrome',
  'high-contrast',
];
const states = ['default', 'agent-filter', 'device-filter', 'heavy'];

for (const state of states) {
  for (const theme of themes) {
    test(`@visual recall ${state} ${theme}`, async ({ page }) => {
      await page.goto(`/?view=recall&recallState=${state}&theme=${theme}`);
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
      await expect(page.getByTestId(`recall-ledger-${state}`)).toBeAttached();
      await expect(page.getByRole('group', { name: 'Timeline' })).toBeVisible();
    });
  }
}
