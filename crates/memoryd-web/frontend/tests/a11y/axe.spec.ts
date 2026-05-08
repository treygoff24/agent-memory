import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';

for (const view of ['inbox', 'reality', 'recall', 'dreams', 'peers', 'governance', 'entities']) {
    test(`@a11y ${view}`, async ({ page }) => {
        await page.goto(`/?view=${view}`);
        const results = await new AxeBuilder({ page }).analyze();
        expect(results.violations).toEqual([]);
    });
}
