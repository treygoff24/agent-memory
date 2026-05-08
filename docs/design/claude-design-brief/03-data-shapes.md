# Memorum Dashboard — Real Data Shapes

These are **actual** JSON responses from the running daemon. Use these instead of lorem ipsum so designs reflect real volume, real lengths, and real edge cases. Field names are stable per the Stream G spec.

## API base

All routes are served at `http://127.0.0.1:7137` by default (configurable). Static SPA at `/`; API at `/api/*`. Mutations require `X-Memorum-CSRF` header (token in `<meta name="csrf-token">`).

## `GET /api/status` — daemon health

```json
{
  "state": "running",
  "guidance": "ok",
  "recall": {
    "startup_blocks_emitted": 42,
    "delta_blocks_emitted": 119,
    "peer_updates_inserted": 8,
    "candidate_attention_count": 3,
    "quarantine_attention_count": 1
  },
  "dreams": {
    "last_run_started_at": "2026-05-07T03:00:12Z",
    "last_run_completed_at": "2026-05-07T03:04:38Z",
    "last_promoted": 3,
    "last_queued": 1,
    "last_dropped": 0,
    "next_scheduled_at": "2026-05-08T03:00:00Z"
  },
  "passive_notifications": [{ "message": "Reality Check due in 3 days", "created_at": "2026-05-04T12:00:00Z" }]
}
```

## `GET /api/review?status=&namespace=&limit=&offset=` — review queue page

```json
{
  "total": 7,
  "candidate_count": 3,
  "quarantined_count": 2,
  "dream_low_confidence_count": 2,
  "items": [
    {
      "memory_id": "mem_20260507_a1b2c3d4e5f60718_000010",
      "status": "candidate",
      "title": "Project uses pnpm, never npm",
      "namespace": "coding/typescript",
      "sensitivity": "plaintext",
      "encrypted": false,
      "body_preview": "Project uses pnpm for all package management. Running npm install or yarn will create stray package-lock.json…",
      "added_at": "2026-05-07T14:23:11Z",
      "source_kind": "agent",
      "source_harness": "claude-code",
      "session_id": "a8b3f2c",
      "confidence": 0.84,
      "policy": "project-standard@v2",
      "next_action": "requires_user_confirmation",
      "reason": null
    },
    {
      "memory_id": "mem_20260506_def_000004",
      "status": "quarantined",
      "title": "SSH key rotation every 90d",
      "namespace": "me/security",
      "sensitivity": "sensitive",
      "encrypted": true,
      "body_preview": "[encrypted — body redacted]",
      "added_at": "2026-05-06T09:11:02Z",
      "source_kind": "agent",
      "source_harness": "codex-cli",
      "session_id": "b1d92a4",
      "confidence": 0.5,
      "policy": "personal-default@v1",
      "next_action": "manual_review",
      "reason": "grounding_rehydration_failed"
    },
    {
      "memory_id": "mem_20260507_ghi_000002",
      "status": "candidate",
      "title": "Database connection pool size",
      "namespace": "project:atlasos",
      "sensitivity": "plaintext",
      "encrypted": false,
      "body_preview": "Use 20 connections per worker process. Going above 50 saturates the Postgres replica…",
      "added_at": "2026-05-07T11:02:44Z",
      "source_kind": "agent",
      "source_harness": "claude-code",
      "session_id": "c7e21f8",
      "confidence": 0.71,
      "policy": "project-atlasos@v3",
      "next_action": "merge_conflict",
      "reason": "merge_conflict"
    }
  ]
}
```

## `POST /api/review/action` — approve / reject / forget

Request body:

```json
{ "id": "mem_20260507_a1b2c3d4e5f60718_000010", "action": "approve", "reason": null }
```

Action values: `"approve" | "reject" | "forget" | "quarantine"`. `reason` is required for `forget`, optional otherwise.

Response (success):

```json
{
  "ok": true,
  "memory_id": "mem_20260507_a1b2c3d4e5f60718_000010",
  "new_status": "active",
  "applied_at": "2026-05-07T14:25:08Z"
}
```

Response (refused):

```json
{
  "ok": false,
  "error_kind": "governance_refused",
  "message": "Tombstone match for memory_id=mem_… still pending; cannot approve until cleared.",
  "memory_id": "mem_20260507_a1b2c3d4e5f60718_000010"
}
```

## `GET /api/reality-check` — pending list

```json
{
  "session_id": "rc_20260507_001",
  "items": [
    {
      "memory_id": "mem_20260101_xyz_000003",
      "title": "Maeve started kindergarten at Pacific Crest Montessori",
      "namespace": "personal/family",
      "status": "active",
      "sensitivity": "plaintext",
      "score": 0.82,
      "component_scores": {
        "days_since_observed_norm": 0.91,
        "recall_frequency_norm": 0.45,
        "cross_source_corroboration": 0.2,
        "confidence_decay": 0.62,
        "sensitivity_weight": 1.0
      },
      "encrypted": false,
      "last_observed_at": "2026-02-04T18:30:00Z",
      "recall_count_30d": 4,
      "last_recalled_at": "2026-04-29T22:14:00Z"
    }
  ],
  "total_scored": 12,
  "last_completed_at": "2026-05-03T10:00:00Z"
}
```

## `POST /api/reality-check/respond`

Request body:

```json
{
  "session_id": "rc_20260507_001",
  "memory_id": "mem_20260101_xyz_000003",
  "action": "confirm"
}
```

Action shapes:

- `{ "action": "confirm" }`
- `{ "action": "correct", "new_body": "Maeve attends Pacific Crest Montessori, K-1 multiage classroom, since Sept 2025." }`
- `{ "action": "forget", "reason": "kid changed schools — Aug 2026 transfer" }`
- `{ "action": "not_relevant" }`
- `{ "action": "skip_this_week" }`

Response (success): `{ "session_id": "...", "memory_id": "...", "next_item": <RealityCheckItem | null>, "completion": { "kind": "progress", "remaining": 9, "deferred": 0 } }`

## `GET /api/recall-hits?since=&limit=` — recall ledger feed

```json
{
  "since": "2026-05-06T00:00:00Z",
  "limit": 50,
  "hits": [
    {
      "event_id": "evt_28f1a4",
      "device": "mbp",
      "seq": 1281,
      "memory_id": "mem_20260507_a1b2c3d4e5f60718_000010",
      "recalled_at": "2026-05-07T13:42:18Z",
      "summary": "Project uses pnpm, never npm"
    },
    {
      "event_id": "evt_28f1a3",
      "device": "mbp",
      "seq": 1280,
      "memory_id": "mem_20260507_jkl_000007",
      "recalled_at": "2026-05-07T13:38:02Z",
      "summary": "Ghostty is the daily-driver terminal"
    }
  ]
}
```

`limit` clamps to 1..=500. `summary` may be `null` for encrypted items.

## `GET /api/audit/:memory_id` — trust artifact

```json
{
  "memory_id": "mem_20260507_a1b2c3d4e5f60718_000010",
  "title": "Project uses pnpm, never npm",
  "body": "Project uses pnpm for all package management. Running npm install or yarn will create stray package-lock.json / yarn.lock files that conflict with pnpm-lock.yaml on the next install. Always use pnpm verbs.",
  "status": "active",
  "namespace": "coding/typescript",
  "confidence": 0.84,
  "confidence_reason": "deterministic agent write, governance auto-approved",
  "recall_count_total": 28,
  "recall_count_30d": 12,
  "last_recalled": "2026-05-07T13:42:18Z",
  "provenance_chain": [
    {
      "kind": "agent_write",
      "at": "2026-05-07T14:23:11Z",
      "actor": "claude-code",
      "session_id": "a8b3f2c",
      "grounding_ref": { "kind": "git_commit", "sha": "8a3f2c1d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b" }
    },
    {
      "kind": "governance_decision",
      "at": "2026-05-07T14:23:12Z",
      "policy": "project-standard@v2",
      "decision": "promote_candidate",
      "rationale": "agent write with grounding; passes confidence threshold 0.80"
    }
  ],
  "policy_decisions": [{ "policy": "project-standard@v2", "result": "approve", "at": "2026-05-07T14:23:12Z" }],
  "privacy_scan": {
    "labels_detected": [],
    "storage_action": "plaintext"
  },
  "supersession_history": [],
  "sync_state": {
    "devices": ["mbp", "mini"],
    "merge_status": "clean",
    "claim_lock_status": "Stream I active · no contention"
  }
}
```

## `GET /api/entity-graph?namespace=&depth=&focus=`

```json
{
  "nodes": [
    { "id": "ent_pnpm", "label": "pnpm", "kind": "tool", "memory_count": 4 },
    { "id": "ent_typescript", "label": "TypeScript", "kind": "language", "memory_count": 12 },
    { "id": "ent_acme", "label": "Acme Corp", "kind": "org", "memory_count": 8 }
  ],
  "edges": [
    { "from": "ent_pnpm", "to": "ent_typescript", "kind": "co_mention", "weight": 4 },
    { "from": "ent_typescript", "to": "ent_acme", "kind": "co_mention", "weight": 2 }
  ]
}
```

## Stream I peer status (subset of status response)

```json
{
  "coordination_level": 2,
  "active_sessions": [
    {
      "device": "mbp",
      "session_id": "a8b3f2c",
      "harness": "claude-code",
      "started_at": "2026-05-07T14:00:00Z",
      "scope": "project:atlasos"
    },
    {
      "device": "mini",
      "session_id": "d1e9f4a",
      "harness": "codex-cli",
      "started_at": "2026-05-07T13:50:22Z",
      "scope": "me/personal"
    }
  ],
  "claim_locks": [
    {
      "device": "mbp",
      "scope": "project:atlasos",
      "acquired_at": "2026-05-07T14:00:01Z",
      "expires_at": "2026-05-07T14:30:01Z"
    }
  ],
  "recent_deliveries": [
    {
      "device": "mini",
      "delivered_at": "2026-05-07T13:51:00Z",
      "kind": "peer_update",
      "summary": "session started: codex-cli on me/personal"
    }
  ]
}
```

## Notification dropdown items (web-rendered from broadcast feed)

```json
[
  {
    "kind": "review_queue_over_threshold",
    "at": "2026-05-07T13:00:00Z",
    "fields": { "count": 12, "threshold": 10 },
    "primary_action": { "label": "Review queue", "route": "/governance" }
  },
  {
    "kind": "dream_run_completed",
    "at": "2026-05-07T03:04:38Z",
    "fields": { "scope": "all", "promoted": 3, "queued": 1, "dropped": 0 },
    "primary_action": { "label": "Open Dreams", "route": "/dreams" }
  },
  {
    "kind": "reality_check_due",
    "at": "2026-05-07T00:00:00Z",
    "fields": { "due_at": "2026-05-07T00:00:00Z" },
    "primary_action": { "label": "Run Reality Check", "route": "/reality-check" }
  }
]
```

## Volumes you're designing for

These are realistic v1 volumes for a single user with ~6 months of dogfooding under their belt:

- **Total active memories:** ~1,200
- **Pending review at any moment:** 0–20 items typical, occasionally 30+
- **Recall hits per day:** 50–300
- **Dream outputs per night:** 0–5 promotions, 0–10 questions, 0–3 cleanup proposals
- **Peers:** 1–3 devices typical
- **Conflicts:** 0–2 most days; rare spikes after a long-offline device syncs back
- **Reality Check items per week:** 8–15

Design for these typical volumes. The dashboard should still be legible at 5× these numbers (a power user with 5+ years of memory) but should not be overweight at 0.1× (a user on day 2).

## Empty / error / loading states to design

- **Daemon down:** `GET /api/status` returns 503 or connection refused. Design the page-level banner.
- **CSRF failure (403):** mutation rejected. Design the toast + recovery affordance ("refresh and retry").
- **Stale-write (409):** mutation conflicted with another mutation. Design the toast.
- **Empty inbox / empty review queue / empty dream feed / no peers / no recall events.** Each view has its own empty state.
- **First-run state:** zero memories, zero events. Design the "welcome to memorum" panel that the inbox falls back to.
