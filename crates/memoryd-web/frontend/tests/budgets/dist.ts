import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';
import { gzipSync } from 'node:zlib';

const projectRoot = process.cwd();
const distDir = join(projectRoot, 'dist');
const assetsDir = join(distDir, 'assets');
const cacheKey = projectRoot.replace(/[^a-zA-Z0-9_-]+/g, '-').slice(-80);
const cacheDir = join(tmpdir(), `memorum-dashboard-budget-${cacheKey}`);
const lockDir = join(cacheDir, 'build.lock');
const stampFile = join(cacheDir, 'build.done');

function sleep(ms: number): void {
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

function distIsReady(): boolean {
    if (!existsSync(join(distDir, 'index.html')) || !existsSync(assetsDir)) return false;
    return (
        readdirSync(assetsDir).some((file) => file.endsWith('.js')) &&
        readdirSync(assetsDir).some((file) => file.endsWith('.css'))
    );
}

export function ensureDist(): void {
    if (existsSync(stampFile) && distIsReady()) return;
    mkdirSync(cacheDir, { recursive: true });
    let ownsLock = false;
    for (let attempt = 0; attempt < 120; attempt += 1) {
        try {
            mkdirSync(lockDir);
            ownsLock = true;
            break;
        } catch {
            if (existsSync(stampFile) && distIsReady()) return;
            sleep(250);
        }
    }
    if (!ownsLock) throw new Error('Timed out waiting for frontend budget build lock.');
    try {
        execFileSync('pnpm', ['run', 'build'], { cwd: projectRoot, stdio: 'inherit' });
        writeFileSync(stampFile, new Date().toISOString());
    } finally {
        rmSync(lockDir, { recursive: true, force: true });
    }
}

export interface BundleAsset {
    file: string;
    rawBytes: number;
    gzipBytes: number;
}

export function builtAssets(extension: '.css' | '.js'): BundleAsset[] {
    ensureDist();
    return readdirSync(assetsDir)
        .filter((file) => file.endsWith(extension))
        .map((file) => {
            const path = join(assetsDir, file);
            const data = readFileSync(path);
            return {
                file: basename(path),
                rawBytes: statSync(path).size,
                gzipBytes: gzipSync(data).length,
            };
        });
}

export function builtIndexHtml(): string {
    ensureDist();
    return readFileSync(join(distDir, 'index.html'), 'utf8');
}
