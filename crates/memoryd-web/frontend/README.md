# Memorum dashboard frontend

```bash
pnpm install
pnpm run dev
pnpm run check:all
pnpm run build
```

Production assets are emitted to `frontend/dist/`. The Rust `memoryd-web` crate embeds that directory at compile time via `rust-embed`; `build.rs` runs `pnpm install --frozen-lockfile` and `pnpm run build` before Rust compilation.

Visual baselines use platform-scoped Playwright snapshots. Regenerate intentionally with:

```bash
pnpm run test:visual -- --update-snapshots
```
