#!/usr/bin/env bash
# Pinned LLM judge for the Memora-lessons arc (plan DP5, approved 2026-07-10).
# Contract: one BenchmarkJudgeInput JSON record on stdin ->
#           {"score": number, "rationale": string} on stdout.
# FROZEN for the arc at first scoring run: model=luna (gpt-5.6), effort=low,
# rubric below. Any change invalidates cross-run comparability — don't.
set -euo pipefail
input=$(cat)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cat > "$tmp/schema.json" << 'EOF'
{"type":"object","properties":{"score":{"type":"number"},"rationale":{"type":"string"}},"required":["score","rationale"],"additionalProperties":false}
EOF

{
  cat << 'EOF'
You are a retrieval-quality judge for a memory-system benchmark. The input record contains a question, the gold answer, the memory snippets a retrieval system surfaced (retrieved_context), and the answer basis it assembled.

Score how well the retrieved context supports answering the question — exactly one of:
- 1.0 — the context contains the information needed to state the gold answer.
- 0.5 — the context contains partial or indirect support (some but not all needed facts).
- 0.0 — the context does not support the gold answer.

Judge ONLY whether retrieved_context/answer_basis supports the gold answer. Do not reward fluent but fact-free context. Output only the JSON object required by the schema.

INPUT RECORD:
EOF
  printf '%s\n' "$input"
} > "$tmp/prompt.txt"

delegate --json codex call --read-only --model luna --reasoning-effort low \
  --output-schema "$tmp/schema.json" --prompt-file "$tmp/prompt.txt" > "$tmp/out.json"

# Extract the model's final message (schema-validated JSON) from the envelope.
python3 - "$tmp/out.json" << 'PY'
import json, sys
env = json.load(open(sys.argv[1]))
# Envelope shape probe: prefer known fields, fall back to scanning strings.
for key in ("text", "finalMessage", "output", "result", "message", "stdout"):
    v = env.get(key)
    if isinstance(v, str) and v.strip().startswith("{"):
        obj = json.loads(v)
        print(json.dumps({"score": float(obj["score"]), "rationale": str(obj.get("rationale", ""))}))
        sys.exit(0)
print(json.dumps(env, indent=2), file=sys.stderr)
sys.exit(3)
PY
