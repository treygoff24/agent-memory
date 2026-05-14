import react from '@vitejs/plugin-react';
import { visualizer } from 'rollup-plugin-visualizer';
import { defineConfig, type PluginOption } from 'vite';

const plugins: PluginOption[] = [react()];
if (process.env.ANALYZE === '1') {
    plugins.push(
        visualizer({
            filename: 'dist/bundle-report.html',
            gzipSize: true,
            brotliSize: true,
            template: 'treemap',
            open: false,
        }),
    );
}

export default defineConfig({
    plugins,
    build: {
        outDir: 'dist',
        assetsDir: 'assets',
        manifest: true,
        rollupOptions: {
            output: {
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
