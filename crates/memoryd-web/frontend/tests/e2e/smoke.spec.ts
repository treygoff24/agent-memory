import { expect, test } from '../support/playwright';

test('dashboard bootstrap shell loads', async ({ page }) => {
    await page.goto('/');
    await expect(page).toHaveTitle('Memorum Dashboard');
    await expect(page.locator('meta[name="csrf-token"]')).toHaveAttribute('content', /.+/);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'warm-dark');
    await expect(page.getByRole('heading', { name: 'Memorum Dashboard' })).toBeVisible();
});
