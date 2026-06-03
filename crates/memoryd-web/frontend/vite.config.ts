import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig({
    plugins: [react()],
    build: {
        outDir: 'dist',
        assetsDir: 'assets',
        manifest: true,
        rollupOptions: {
            output: {
                // Deliberate single-bundle build. This dashboard is a
                // localhost-only operator tool served from RustEmbed over
                // loopback, so there is no network latency and the cache is
                // always warm; route-level code-splitting (React.lazy +
                // Suspense) would add loading states for no real first-paint
                // win. Without dynamic imports Rollup would emit one chunk
                // anyway, so this override only makes the intent explicit.
                // Revisit (lazy-load route views) only if first paint ever
                // moves off loopback.
                manualChunks: undefined,
            },
        },
    },
    server: {
        proxy: {
            '/api': 'http://127.0.0.1:7137',
        },
    },
});
