# Memorum dashboard frontend

```bash
pnpm install
pnpm run dev
pnpm run check:fast
pnpm run check:local
pnpm run build
```

Production assets are emitted to `frontend/dist/`. The Rust `memoryd-web` crate embeds that directory at compile time via `rust-embed`; `build.rs` runs `pnpm install --frozen-lockfile` and `pnpm run build` before Rust compilation.

## Test-suite catalog

| Command                                                          | Coverage                                                                                     |
| ---------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `pnpm run lint`                                                  | ESLint over source, tests, configs, and Playwright helpers.                                  |
| `pnpm run typecheck`                                             | Strict TypeScript project check.                                                             |
| `pnpm run test:gentle`                                           | Capped Vitest run (`--minWorkers=1 --maxWorkers=2`) excluding bundle-budget builds.          |
| `pnpm run test:e2e:gentle`                                       | Capped Playwright e2e run (`--workers=1 --reporter=line`) for local flow checks.             |
| `pnpm run test --run`                                            | Vitest unit/component/MSW contract tests.                                                    |
| `pnpm run test:budgets`                                          | Production bundle budget/CSP tests; run when bundle, assets, or final validation require it. |
| `pnpm run test --run budgets`                                    | Builds `dist/`, asserts gzip bundle budgets, and verifies CSP-strict HTML.                   |
| `pnpm run test:e2e`                                              | Playwright e2e, state matrix, and recall scroll perf smoke.                                  |
| `pnpm run test:visual --run`                                     | Theme/layout visual-regression probes. These are assertion probes, not screenshot baselines. |
| `pnpm run test:a11y`                                             | Axe scan for every dashboard surface across all six themes.                                  |
| `pnpm run test:perf`                                             | Recall heavy-ledger scroll performance smoke only.                                           |
| `cd ../../.. && cargo test -p memoryd-web --test frontend_smoke` | Rust embed smoke: CSRF rewrite, CSP, hashed assets, and gzip bundle budgets after embedding. |

Tiered frontend gates:

```bash
pnpm run check:fast   # typecheck + capped Vitest; use during normal implementation
pnpm run check:local  # lint + typecheck + capped Vitest; run before claiming a view/task done
pnpm run check:full   # lint + typecheck + Vitest + visual + a11y + perf + e2e
```

Add targeted Playwright suites when the touched surface requires them: routing and user flows usually need `pnpm run test:e2e:gentle` or a grepped `pnpm run test:e2e -- --grep ...`; theme/layout changes need `pnpm run test:visual`; accessibility changes need `pnpm run test:a11y`. Run `cd ../../.. && cargo test -p memoryd-web --test frontend_smoke` when verifying the Rust-embedded production dist.

## Visual baselines and probes

Visual tests currently assert stable surface structure under each theme rather than storing screenshots. If screenshot baselines are reintroduced, keep Playwright's platform-scoped `snapshotPathTemplate` and regenerate intentionally with:

```bash
pnpm run test:visual -- --update-snapshots
```

Review generated diffs per platform. Do not bless baseline drift caused by data fixture changes unless the user-facing layout change is intentional.

## Reviewing a11y violations

Run:

```bash
pnpm run test:a11y -- --reporter=list
```

For each violation, capture:

1. view and theme from the test title,
2. axe rule id and impact,
3. affected selector/snippet,
4. whether the fix belongs in tokens, component markup, focus management, or copy.

Color contrast is explicitly enabled and is not a nuisance rule for this dashboard.

## Bundle budget policy

Budgets are enforced twice: Vitest reads built `dist/assets/*`, and `frontend_smoke.rs` checks the embedded bytes exposed by Rust.

Current budgets:

- CSS gzip: 80 KB per CSS asset.
- JS gzip: 250 KB per JS asset.

Only bump a budget when a reviewed feature deliberately adds user-visible capability that cannot be split or removed. Budget bumps must include the measured before/after gzip sizes and the reason code-splitting or dependency removal is not the better answer.

## CI integration notes

- Run `pnpm run check:local` from `crates/memoryd-web/frontend` after `pnpm install --frozen-lockfile` for local confidence.
- Run `pnpm run check:full` only for final/pre-merge/CI validation or when visual/a11y/perf/e2e coverage is directly required.
- Run Rust `frontend_smoke` from the workspace root so `build.rs` embeds the same production `dist/` that the budget tests inspected.
- `test:e2e` includes `tests/e2e`, `tests/states`, and `tests/perf`; use `--grep` for scoped debugging.
- `test:visual` and `test:a11y` run separate Playwright suites so failures are easier to triage.
