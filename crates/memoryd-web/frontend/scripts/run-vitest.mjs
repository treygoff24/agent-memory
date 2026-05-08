import { spawnSync } from 'node:child_process';

const args = process.argv.slice(2);
const mapped = [];
for (const arg of args) {
    if (!arg.startsWith('-') && arg.includes('|')) {
        mapped.push('-t', arg);
    } else {
        mapped.push(arg);
    }
}
const result = spawnSync('pnpm', ['exec', 'vitest', ...mapped], { stdio: 'inherit' });
process.exit(result.status ?? 1);
