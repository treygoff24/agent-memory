import { spawnSync } from 'node:child_process';

const mode = process.argv[2] ?? 'e2e';
const suites = { e2e: 'tests/e2e', visual: 'tests/visual', a11y: 'tests/a11y' };
const suite = suites[mode] ?? 'tests/e2e';
const passthrough = process.argv.slice(3).filter((arg) => arg !== '--run' && arg !== '--');
const result = spawnSync('pnpm', ['exec', 'playwright', 'test', suite, ...passthrough], { stdio: 'inherit' });
process.exit(result.status ?? 1);
