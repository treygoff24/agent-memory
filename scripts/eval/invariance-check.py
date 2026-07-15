#!/usr/bin/env python3
"""W4b T3 invariance control: assert two benchmark artifacts retrieved identical context sets.

Usage: invariance-check.py <reference.json> <candidate.json>

Decision rule 1 (docs/plans/2026-07-12-w4b-context-enrichment-regate.md): arm L run on the v2
corpus must retrieve the SAME context set per item as the v1 reference — enrichment metadata must
not perturb legacy retrieval. Any divergence → STOP the re-gate.

Exit 0: identical. Exit 1: divergence (details printed). Exit 2: artifacts not comparable.
"""
import json
import sys


def per_item_contexts(path):
    with open(path) as f:
        report = json.load(f)
    items = report.get("items", [])
    inputs = report.get("judge_inputs", [])
    if len(items) != len(inputs):
        print(f"{path}: items ({len(items)}) and judge_inputs ({len(inputs)}) misaligned", file=sys.stderr)
        sys.exit(2)
    return {item["id"]: inp["retrieved_context"] for item, inp in zip(items, inputs)}


def main():
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    ref = per_item_contexts(sys.argv[1])
    cand = per_item_contexts(sys.argv[2])
    if set(ref) != set(cand):
        only_ref = sorted(set(ref) - set(cand))[:5]
        only_cand = sorted(set(cand) - set(ref))[:5]
        print(f"DIVERGENT item sets: ref-only {only_ref} cand-only {only_cand}")
        sys.exit(1)
    diverged = [qid for qid in ref if ref[qid] != cand[qid]]
    if diverged:
        print(f"DIVERGENT retrieved contexts on {len(diverged)}/{len(ref)} items: {diverged[:10]}")
        qid = diverged[0]
        print(f"--- example {qid} ---")
        print(f"ref : {ref[qid][:400]!r}")
        print(f"cand: {cand[qid][:400]!r}")
        sys.exit(1)
    print(f"INVARIANT: {len(ref)} items, retrieved contexts identical")


if __name__ == "__main__":
    main()
