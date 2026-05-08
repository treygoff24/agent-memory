import { defineConfig } from 'vitest/config';

export default defineConfig({
    test: {
        include: ['tests/unit/**/*.test.ts', 'tests/unit/**/*.test.tsx'],
        environment: 'jsdom',
        setupFiles: ['./tests/setup.ts'],
        globals: true,
        coverage: {
            thresholds: {
                lines: 80,
                functions: 80,
                branches: 75,
                statements: 80,
            },
        },
    },
});
