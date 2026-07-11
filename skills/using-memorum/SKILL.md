---
name: using-memorum
description: Use when operating Memorum from the memoryd CLI — orienting in a new session, searching and reading memories, recording notes and governed memories, superseding or forgetting, revealing encrypted content, checking daemon health, or importing/backing up prior Claude Code / Codex CLI memory. Covers the full agent operating loop and the JSON envelope + exit-code contract every command follows.
---

# Operating Memorum from the CLI

Memorum is a local-first daemon that gives Claude Code, Codex CLI, and any shell-capable harness one shared, governed memory layer. You drive it with the `memoryd` CLI. This skill is the operating loop: how to orient, read, write, and stay out of trouble.

The CLI is the Tier-1 agent surface. The MCP bridge still ships but is an opt-in compatibility path — you do not need it. Passive recall is injected for you by lifecycle hooks (see "Recall interplay" below), so a lot of context arrives without you asking.

## Paths and env vars

```bash
MEMORUM_REPO="${MEMORUM_REPO:-$HOME/memorum}"
MEMORUM_SOCKET="${MEMORUM_SOCKET:-$MEMORUM_REPO/.memoryd/memoryd.sock}"
```

Flag convention: daemon-backed commands (`status`, `search`, `get`, `write`, `write-note`, `supersede`, `forget`, `source`, `reveal`, `observe`, `review`, `export`) take `--socket`. `doctor` reads the substrate directly and takes `--repo`/`--runtime` (it also tolerates `--socket` so one scripted loop can pass the same flags everywhere).

## The envelope and exit-code contract

Every covered command speaks one machine contract. Learn it once and you can branch on any command's outcome.

- **Success** → one JSON object on **stdout**, exit `0`:
  `{"ok":true,"data":{...},"meta":{"schema_version":"1.0","warnings":[]}}`
- **Failure** → one JSON object on **stderr**, nonzero exit:
  `{"ok":false,"error":{"code","message","retryable","suggested_fix"},"meta":{...}}`

Parse stdout on exit 0, stderr otherwise. `data` is the payload itself (e.g. `data.hits`), never a daemon wrapper. `meta.warnings` carries non-fatal advisories — read them.

Exit codes you will branch on:

| Exit | Meaning | React |
| ---: | --- | --- |
| 0 | success (incl. empty result, and queued writes) | check `meta.warnings` and `data.status` |
| 2 | usage / bad argument | fix the command |
| 65 | invalid input or **governance refusal** | read `error.message` + `suggested_fix` |
| 66 | `not_found` — well-formed id, no such memory | `search` for the right id |
| 75 | daemon unreachable / transient | retry, or `doctor` |
| 77 | client gate (e.g. `reveal` without `--allow-reveal`) | pass the named flag only with authority |

`doctor` (0/1) and `recall *` (their own dictionary) are documented exceptions. **`doctor` also differs in output *shape*, not just its exit codes:** it emits the raw daemon frame `{"id":...,"result":{"success":{"doctor":{...}}}}`, not the `{"ok","data","meta"}` envelope — read `.result.success.doctor.healthy`, and do not parse it with the envelope's `.ok`/`.data`. Run `memoryd schema --json` for the whole machine contract, or `memoryd schema exit-codes` for just the tables.

## 1. Orient (start of a session)

```bash
memoryd status --socket "$MEMORUM_SOCKET"    # daemon reachable? (exit 0 = yes)
memoryd doctor --repo "$MEMORUM_REPO"         # substrate healthy? (exit 0, healthy:true)
memoryd schema --json                          # the full command/envelope/exit contract
```

If `status` exits 75, the daemon isn't running — start it (or ask the user) before anything else. If `doctor` exits 1, it prints the specific finding and the fix.

**Embedding lanes:** the default is the local on-device model. An opt-in Gemini API lane exists
(`memoryd config embedding-lane --lane gemini-api`, ~30 MB daemon instead of ~1.3 GB): it requires an
explicit consent ceremony — scripted runs must pass `--consent`, and only plaintext-eligible
(public/internal) content plus query text ever reach the API; confidential/personal content is held local
by the embedding fence. Never pass `--consent` on the user's behalf without their instruction. Switching
lanes requires a daemon restart; `doctor` explains API-lane problems (`embedding_api_*` findings). Operator
detail: `docs/runbooks/api-embedding-lane.md`.

## Recall interplay — don't re-search what you already have

When Memorum's lifecycle hooks are wired (the default), relevant memories are **already injected into your context** at session start and as the conversation shifts — you receive them without running a command. Before you `search`, check whether the answer is already in the recall block you were given. Reach for `search` when you need something the passive block didn't surface, or to confirm before a write.

## 2. Read path: search → get

```bash
memoryd search "delegate droid alias" --socket "$MEMORUM_SOCKET"
memoryd search "onboarding" --limit 5 --include-body --socket "$MEMORUM_SOCKET"
```

`search` returns bounded summaries by default; `--include-body` inlines full bodies. An empty result is still exit 0 with `data.hits: []` and a broadening hint in `meta.warnings` — widen the query rather than treating it as an error.

To read one memory in full, follow a hit's id into `get`:

```bash
memoryd get <id> --socket "$MEMORUM_SOCKET"
memoryd get <id> --include-provenance --socket "$MEMORUM_SOCKET"
```

Bodies are bounded server-side (4 KiB); `data.truncated: true` marks a cut body. A well-formed id that doesn't exist exits 66 — `search` for the right one.

## 3. Write etiquette

**Search before you write.** A write that contradicts an existing memory is refused (see below); confirm what's already recorded first.

**Note vs. governed write.** A note is low-friction and lands immediately; a governed write goes through policy, grounding, and contradiction checks.

```bash
# Note — quick, immediate, no governance candidate step
memoryd write-note "react-doctor flakes on cold start; a rerun fixes it" --socket "$MEMORUM_SOCKET"

# Governed structured write — carries title/tags/meta, subject to governance
memoryd write "The dashboard defaults to port 7137; override with --port." \
  --title "Dashboard default port" --tag config \
  --meta '{"namespace":"project","type":"claim","confidence":0.88,"abstraction":"Dashboard port configuration","cues":["Dashboard port","Port override"]}' \
  --socket "$MEMORUM_SOCKET"
```

When semantic metadata helps future retrieval, add an `abstraction` of at most 8 words and 0–3 cues. Write cues as `[Main Entity] + [Key Aspect]`, usually 2–4 words each (for example, `Dashboard port` or `Port override`); omit them rather than padding weak cues.

Malformed `--meta` JSON exits 65 with a minimal valid example in `suggested_fix`.

**Read the write outcome — a queued write is not a live one.** A governed write returns `data.status`:

- `promoted` — live and recall-visible. Done.
- `candidate` / `quarantined` — accepted into the review queue, **exit 0 but not yet active**. `meta.warnings` says so. It will not appear in search until a human approves it (`memoryd review queue`, then `memoryd review approve <id>`). Do not report it as a completed write.
- `refused` — **exit 65, `ok:false`**. The write did not happen. `error.code` is the reason (`contradiction`, `tombstone`, `policy`, `grounding`, `privacy`, `superseded`, `review_required`) and `suggested_fix` names the next move — e.g. for `contradiction`, `search` for the conflicting memory and `supersede` it instead of writing fresh.

**Supersede vs. forget.** To replace an outdated memory with a corrected one, `supersede` (keeps the chain); to remove a memory that should no longer exist, `forget` (tombstones it).

```bash
memoryd supersede <old-id> "The dashboard now defaults to port 7137 and honors --port." \
  --reason "clarify override mechanism" --socket "$MEMORUM_SOCKET"

memoryd forget <id> --reason "keyboard preference no longer holds" --socket "$MEMORUM_SOCKET"
```

**Supersede runs the full governance gate — and can be *stricter* than the original write.** It goes through the same policy / grounding / privacy / contradiction checks as `write`, so a claim that promoted ungrounded may be refused on supersede if its namespace demands grounding. If you get `error.code: "grounding"`, capture evidence and cite it:

```bash
# 1. Capture a source (a URL is cleanest; --file works for local artifacts)
memoryd source capture --url "https://example.com/changelog" \
  --excerpt "default port is now 7137" --socket "$MEMORUM_SOCKET"
# → data.source_refs: ["webcap:src_...#quote_0001"]

# 2. Cite it in --meta under the key `source_ref` (a single string, not `source_refs`)
memoryd supersede <old-id> "..." --reason "..." \
  --meta '{"source_ref":"webcap:src_...#quote_0001"}' --socket "$MEMORUM_SOCKET"
```

Prefer a **public URL** as the source: a local `--file` artifact whose path/contents the privacy classifier flags (e.g. anything under a home directory) will pass grounding but then trip a `privacy` refusal, because a plaintext governed write can't carry a privacy-flagged descriptor. If both gates box you in, surface it to the user rather than looping.

**Observations (Stream F).** Record a low-level observation/pattern/signal for later synthesis:

```bash
memoryd observe "the deploy step flakes on cold caches" --kind signal --socket "$MEMORUM_SOCKET"
```

`--kind` is required (`observation`/`pattern`/`signal`); text is bounded to 16 KiB and entity ids must be `ent_*` — violations exit 65.

## 4. Reveal encrypted content (audited, gated)

Some memories are encrypted at rest (PII, contacts). Reading their plaintext is an **audited** action: a successful reveal writes an `EncryptedContentRevealed` event. The CLI refuses unless you pass `--allow-reveal`, and it refuses **before** contacting the daemon (exit 77):

```bash
memoryd reveal <id> --reason "user asked to see their saved address" --allow-reveal --socket "$MEMORUM_SOCKET"
```

Only pass `--allow-reveal` when the user has directed you to unmask that specific content.

## 5. Import / back up prior memory

Back up everything the user taught Claude Code and Codex CLI into Memorum — one idempotent, non-destructive command:

```bash
memoryd import --repo "$MEMORUM_REPO" --socket "$MEMORUM_SOCKET"
```

By default it imports the union of all Claude profile roots (`~/.claude*/projects`) plus `~/.codex/memories`, gives non-git-cwd memories a derived project namespace (saved and active, never silently skipped), and recovers malformed frontmatter leniently. Re-runs skip unchanged sources by content hash. Preview with `--dry-run`; pin exact roots with repeatable `--from-claude`.

`import` has its **own** exit contract (not the envelope): it exits **0 even when some writes were refused, recovered, or skipped** — those are reported in the summary, not failures. Nonzero means a hard failure only (daemon unreachable, `AnotherImportInProgress { pid: N }`, unreadable repo). Privacy-blocked counts are by design; report them and move on. Full detail: `docs/agent-import-guide.md`.

## 6. Review queue

```bash
memoryd review queue --socket "$MEMORUM_SOCKET"          # candidates + quarantined items
memoryd review approve <id> --socket "$MEMORUM_SOCKET"   # promote one to active
```

## Gotchas

- Passive recall already injects relevant memory — check the recall block before searching.
- A `candidate`/`quarantined` write is exit 0 but **not active**; only `promoted` is live. Never read a queued write as done.
- A refused write is exit 65 — `error.suggested_fix` tells you the next move (usually `search` then `supersede`).
- Privacy refusals (on import or write) are expected output, not a bug. Report and move on.
- Reveal is audited; pass `--allow-reveal` only with user-directed authority.
- Deeper reference: `docs/api/memoryd-cli-contract-v1.md` (the contract), `docs/agent-import-guide.md` (import), `docs/troubleshooting.md` (symptoms → fixes).
