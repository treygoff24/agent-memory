#!/usr/bin/env bash
set -euo pipefail
profile=""; results=""; baseline=""
while [ $# -gt 0 ]; do
  case "$1" in
    --profile) profile="${2:?}"; shift ;;
    --results) results="${2:?}"; shift ;;
    --baseline) baseline="${2:?}"; shift ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
  shift
done
[ -n "$profile" ] || { echo "--profile required" >&2; exit 2; }
results="${results:-bench/results.${profile}.json}"
baseline="${baseline:-bench/baseline.${profile}.json}"
[ -f "$baseline" ] || { echo "error: $baseline not found; commit a placeholder via Task 1 scaffolding" >&2; exit 1; }
[ -f "$results" ] || { echo "error: $results not found" >&2; exit 1; }
python3 - "$results" "$baseline" <<'PY'
import json, pathlib, shutil, sys
results=pathlib.Path(sys.argv[1]); baseline=pathlib.Path(sys.argv[2])
cur=json.loads(results.read_text()); base=json.loads(baseline.read_text())
if cur.get('runs',0) < 9:
    raise SystemExit('error: current.runs < 9')
if base.get('runs') == 0:
    proposed=baseline.with_suffix(baseline.suffix + '.proposed')
    proposed.write_text(json.dumps(cur, indent=2)+"\n")
    print(f'warning: baseline is a placeholder; first-release bootstrap path active; wrote {proposed}')
    raise SystemExit(0)
for name, metric in cur.get('metrics', {}).items():
    b=base['metrics'][name]
    noise=b.get('noise_floor_ms', base.get('noise_floor_ms', 0))
    if metric['p95_ms'] > 1.10*b['p95_ms'] and (metric['p95_ms']-b['p95_ms']) > noise:
        raise SystemExit(f'regression: {name}')
print('bench regression check ok')
PY
