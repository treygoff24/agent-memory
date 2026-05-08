import { spawnSync } from 'node:child_process';

const mode = process.argv[2] ?? 'e2e';
const suites = {
    e2e: ['tests/e2e', 'tests/states', 'tests/perf'],
    visual: ['tests/visual'],
    a11y: ['tests/a11y'],
    perf: ['tests/perf'],
};
const suite = suites[mode] ?? suites.e2e;
const rawArgs = process.argv.slice(3).filter((arg) => arg !== '--run' && arg !== '--');
const passthrough = [];
for (let index = 0; index < rawArgs.length; index += 1) {
    const arg = rawArgs[index];
    if (!arg.startsWith('-')) {
        passthrough.push('--grep', arg);
        continue;
    }
    passthrough.push(arg);
    const next = rawArgs[index + 1];
    if (next && !next.startsWith('-')) {
        passthrough.push(next);
        index += 1;
    }
}
const result = spawnSync('pnpm', ['exec', 'playwright', 'test', ...suite, ...passthrough], { stdio: 'inherit' });
process.exit(result.status ?? 1);
