import { describe, expect, it } from 'vitest';

import { builtAssets } from './dist';

const cssBudgetBytes = 80 * 1024;
const jsBudgetBytes = 250 * 1024;

describe('budgets', () => {
    it('keeps gzipped CSS bundles under the 80 KB budget', () => {
        const assets = builtAssets('.css');
        expect(assets.length).toBeGreaterThan(0);
        for (const asset of assets) {
            expect(asset.gzipBytes, `${asset.file} raw=${asset.rawBytes}`).toBeLessThanOrEqual(cssBudgetBytes);
        }
    });

    it('keeps gzipped JS bundles under the 250 KB budget', () => {
        const assets = builtAssets('.js');
        expect(assets.length).toBeGreaterThan(0);
        for (const asset of assets) {
            expect(asset.gzipBytes, `${asset.file} raw=${asset.rawBytes}`).toBeLessThanOrEqual(jsBudgetBytes);
        }
    });
});
