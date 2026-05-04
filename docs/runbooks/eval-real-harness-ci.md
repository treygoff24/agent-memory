# Real-harness eval CI

The Stream H eval workflow can run mock coverage without secrets and real harness smoke coverage when GitHub secrets are present.

## Required secrets

- `MEMORUM_EVAL_CLAUDE_KEY`
- `MEMORUM_EVAL_CODEX_KEY`

`.github/workflows/stream-h-eval.yml` exports those values into the eval job. When either is absent, the workflow records a partial run instead of pretending real harness coverage passed.

## Workflow guard pattern

Use secret presence checks before running expensive real-harness work, for example:

```yaml
if: ${{ secrets.MEMORUM_EVAL_CLAUDE_KEY != '' && secrets.MEMORUM_EVAL_CODEX_KEY != '' }}
```

## Cost and cadence

The real-harness path is intentionally tiny: one Claude smoke and one Codex smoke, not the full catalog. Expect low single-digit model calls per scheduled run. The existing daily 03:00 schedule is enough; do not add high-frequency cron triggers unless Trey explicitly asks.

## Local smoke

```bash
MEMORUM_EVAL_CLAUDE_KEY=... cargo test -p memorum-eval --features live-harness -- live::claude_smoke
MEMORUM_EVAL_CODEX_KEY=... cargo test -p memorum-eval --features live-harness -- live::codex_smoke
```
