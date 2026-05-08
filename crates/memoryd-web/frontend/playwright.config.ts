import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
    testDir: './tests',
    retries: process.env.CI ? 1 : 0,
    snapshotPathTemplate: '{testDir}/__snapshots__/{platform}/{testFilePath}/{arg}{ext}',
    use: {
        baseURL: 'http://127.0.0.1:5173',
        trace: 'retain-on-failure',
    },
    expect: {
        toHaveScreenshot: { maxDiffPixelRatio: 0.01 },
    },
    projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
    webServer: {
        command: 'pnpm run dev',
        port: 5173,
        reuseExistingServer: !process.env.CI,
    },
});
