# Privacy leak response runbook

Stream D blocks known secret-like content before disk effects and routes
personal/confidential content through encrypted writes. If a leak is suspected,
handle it as an operator incident rather than a normal memory edit.

## Immediate containment

1. Stop `memoryd` and any harness writing memories.
2. Run `memoryd privacy scan-delta --repo <memory-repo>` before any commit.
3. Search the repo and runtime roots for the canary or leaked value.
4. Tombstone affected memories if they exist.
5. Rotate any exposed external credential outside `memoryd`.

## If plaintext reached git history

Normal `memory_forget` does not rewrite git history. Preserve evidence, rotate
credentials, then perform an explicit admin history rewrite outside the daemon.
After rewrite, run Stream A repair/reindex and re-run the privacy scan.

## Encrypted tier key handling

`memoryd device onboard` creates local Stream D key material for encrypted writes.
If a device is compromised, run `memoryd device revoke <device-id>` for operator
guidance, remove the device recipient from trusted devices, rotate keys, and
re-encrypt confidential/personal records.

## Validation checklist

- Secret canary is absent from repo, runtime, SQLite, pending queues, events,
  tombstones, and review queue metadata.
- Encrypted records live only under `encrypted/`.
- `memory_get` does not return decrypted encrypted bodies to agent-facing MCP.
- Raw confidential/personal body terms are not searchable.
