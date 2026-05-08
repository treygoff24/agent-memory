import AxeBuilder from '@axe-core/playwright';

import { expect, test } from '../support/playwright';
import { allSurfaceViews, themes } from '../support/surface';

for (const view of allSurfaceViews) {
    for (const theme of themes) {
        test(`@a11y ${view.id} ${theme}`, async ({ page }) => {
            await page.goto(`/?view=${view.id}&theme=${theme}`);
            const results = await new AxeBuilder({ page })
                .options({ rules: { 'color-contrast': { enabled: true } } })
                .analyze();
            expect(results.violations).toEqual([]);
        });
    }
}
