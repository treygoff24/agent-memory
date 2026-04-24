# Shared Memory Layer — Draft Spec v0.1

**Status:** blue-sky brainstorm output, 2026-04-23. Every section below reflects a deliberate design choice Trey and Claude made in a single long brainstorming session. Not a finalized spec; a coherent design that could be turned into one.

**Working name:** `memoryd` for the daemon, no product name yet (flagged as open thread).

**Companion documents:**

- `agent-harness-memory-context-handbook-v2.2.md` — the worldview, vocabulary, threat model, and design principles this spec inherits. Read that first if you haven't.
- `handoff.md` — quick-start context for picking up this work in the next session.

**What this is.** A local-first, harness-agnostic, daemon-backed shared memory layer that works across Claude Code, Codex CLI, Cursor, and any other MCP-speaking harness without forking, modifying, or wrapping the harness. It provides durable memory, passive recall, cross-harness coordination, and governance primitives as a single system the user runs once and every agent on the machine can talk to.

**What this is not.** A replacement for OpenClaw's internal memory. A cloud service. A general-purpose vector DB. A secret store. A scheduling system. A chat log archive.

---

## 1. Purpose and non-purpose

Purpose:

1. One durable memory surface shared across every agent harness on the machine, so what Codex learns is immediately available to Claude Code, what Cursor notices is visible to both, and the user never has to rebuild context across tools.
2. Structured memory with real governance — candidate/promote, supersession chains, temporal validity, tombstones, sensitivity tiers, provenance chains — because shared memory without governance is a durable poisoning vector.
3. Passive recall so memory feels memoryful, not merely queryable. Rich in harnesses that support hooks; gracefully degraded elsewhere.
4. Cross-session coordination that only a daemon can provide: peer sessions see each other's candidates and substrate notes, subagents can be shared instead of duplicated, live investigation state crosses harness boundaries.
5. Drift-fighting as a first-class feature via the Weekly Relationship Reality Check and active tombstones, not as an aspiration.
6. Local-first, user-owned, offline-capable. Git-synced for multi-device. No cloud backend required.

Non-purpose:

1. Not a replacement for in-harness memory systems. OpenClaw's `memory-core` plugin and similar in-harness layers still exist for session-scoped state; this system is the durable layer underneath and across them.
2. Not a general-purpose vector DB. SQLite + FTS + local embeddings are derived indexes over files; the files are canonical.
3. Not a secret manager. `secret`-tier content is refused; use 1Password / `pass` / equivalent and reference by id.
4. Not a scheduler. Prospective memory surfaces commitments; launchd/cron/systemd fire them.
5. Not a multi-tenant SaaS. Designed for a single user across their own devices and harnesses.

---

## 2. Design principles (inherited from the handbook)

These are load-bearing and non-negotiable in v1. Every spec decision below flows from them.

1. **Solve data quality at write time, not retrieval time.** Frontmatter is the foundation. Validator runs on every write. Structure isn't cosmetic; it's what makes every downstream layer cheap.
2. **Separate memory systems by access pattern.** Three namespaces (me / project / agent); within each, further subdivision by access pattern (identity vs. relationship-facts vs. episodic, etc.).
3. **Retrieval alone is not enough.** Passive recall is the thing that makes memory feel memoryful. Push-by-default via hooks where the harness supports it; pull-on-convention otherwise.
4. **Memory and compaction collaborate.** Memory reconstructs what compaction discarded. This spec doesn't own compaction (the harness does), but compaction-aware writes and pre-compaction flush are first-class.
5. **Preserve state, not sludge.** Tool outputs, browser snapshots, giant logs are artifacted elsewhere. Memory holds state, not payloads.
6. **Identity context matters and is tri-partite.** Stable role / operating principles / relationship facts each get different review cadences and write gates.
7. **Maintenance ≠ synthesis.** Cleanup is janitorial. Dreaming is cognitive. Three-layer pipeline (substrate / journal / cleanup) keeps them distinct.
8. **Untrusted inputs propose; they don't promote.** No exceptions. Promotion is gated by deterministic policy.
9. **Cache stability is architectural.** Passive recall is assembled to preserve prefix caching; dynamic content goes in the suffix.
10. **Every memory has an evaluation story.** Eval harness is built from day one, not retrofitted.

---

## 3. High-level architecture

```
  ┌─────────────────────────────────────────────────────────────────┐
  │                    User's Devices (each runs)                    │
  │                                                                   │
  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐            │
  │  │ Claude Code  │  │  Codex CLI   │  │    Cursor    │  ...       │
  │  │   session    │  │   session    │  │   session    │            │
  │  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘            │
  │         │                 │                 │                     │
  │   MCP (universal)   MCP + hooks       MCP + rules                 │
  │         │                 │                 │                     │
  │         └─────────────────┼─────────────────┘                     │
  │                           │                                        │
  │                  ┌────────▼────────┐                              │
  │                  │     memoryd     │  (single local daemon)       │
  │                  │                 │                               │
  │                  │  ┌───────────┐  │                              │
  │                  │  │ Governance│  │                              │
  │                  │  │ + Policies│  │                              │
  │                  │  └───────────┘  │                              │
  │                  │  ┌───────────┐  │                              │
  │                  │  │ Privacy   │  │  (opt-in: OpenAI PF)         │
  │                  │  │ Filter    │  │                              │
  │                  │  └───────────┘  │                              │
  │                  │  ┌───────────┐  │                              │
  │                  │  │ Indexer   │──┼──► SQLite (derived)          │
  │                  │  │ + Embedder│  │                              │
  │                  │  └───────────┘  │                              │
  │                  │  ┌───────────┐  │                              │
  │                  │  │ Dreaming  │  │                              │
  │                  │  │ (3-layer) │  │                              │
  │                  │  └───────────┘  │                              │
  │                  │  ┌───────────┐  │                              │
  │                  │  │ Event log │──┼──► JSONL (per-device)         │
  │                  │  │ + Sync    │  │                              │
  │                  │  └───────────┘  │                              │
  │                  └────────┬────────┘                              │
  │                           │                                        │
  │                  ┌────────▼────────┐                              │
  │                  │  Memory tree    │  (git repo: ~/.memory/)       │
  │                  │  (Markdown +    │                              │
  │                  │   frontmatter)  │                              │
  │                  └────────┬────────┘                              │
  │                           │                                        │
  └───────────────────────────┼────────────────────────────────────────┘
                              │
                    ┌─────────▼──────────┐
                    │  git remote (sync) │  private repo, any host
                    └────────────────────┘
                              ▲
                              │ pull/push per device
  ┌───────────────────────────┼────────────────────────────────────────┐
  │  Other device — identical daemon architecture, pulls/pushes same   │
  │  remote. Frontmatter merge driver resolves semantic conflicts.     │
  └────────────────────────────────────────────────────────────────────┘
```

Key properties:

- **Daemon is the single point of write.** All MCP calls route through `memoryd`. No harness writes directly to files. This guarantees every write runs through governance, Privacy Filter, contradiction detection, and event log append.
- **Files are canonical.** SQLite is derived; event log is durable audit; everything else is reconstructable from the Markdown tree.
- **Git is the sync transport.** Commits are fine-grained; a custom frontmatter merge driver handles semantic merges.
- **The harness is untouched.** MCP server and hook scripts are the only integration surface.

---

## 4. The three namespaces

```
me      — personal memory; follows the user across any tool, any project
project — project memory; scoped to a git repo (or yaml-declared logical project); shared across harnesses touching that project
agent   — global pool; cross-cutting patterns, playbooks, regressions, postmortems accumulated from agent work across all projects and harnesses
```

**Scope resolution at tool-call time:**

1. User identity = OS user + device id.
2. Project identity = `git remote get-url origin` → normalize → SHA256 → `proj_<hex>`. `.memory-project.yaml` in the repo tree overrides with a human-readable alias and can split a monorepo into multiple logical projects.
3. Agent identity = the harness id + session id (attribution only; authority is global-pool-wide).

Sessions resolve their binding on connection:

```json5
{
  session_id: "sess_...",
  user: "user:trey@newayfunds.com",
  device: "dev_...<macbook-hash>",
  cwd: "/Users/treygoff/code/atlasos",
  project: {
    canonical_id: "proj_<hex>",
    alias: "prospera/atlasos",        // from .memory-project.yaml if present
    resolved_via: "yaml_override",    // or "git_remote" or "none"
  },
  harness: "claude-code",
  harness_version: "0.7.3",
  namespaces_in_scope: ["me", "project:proj_<hex>", "agent"],
}
```

---

## 5. Process architecture

**Single local daemon, `memoryd`.**

- Auto-starts at login via `launchd` (macOS) / `systemd` (Linux) / `NSSM` (Windows, if/when supported).
- Unix socket at `$XDG_RUNTIME_DIR/memoryd.sock` (`~/.memoryd/socket` fallback on macOS).
- MCP servers (one per connecting harness session) are thin clients: they expose the MCP protocol to the harness, forward calls over the socket to `memoryd`, return responses.
- `memoryd` owns:
  - File watcher on the memory tree (chokidar/notify).
  - SQLite writer (single-writer model; no lock contention).
  - Embedding worker (background; configurable provider: local `embeddinggemma-300m-qat-Q8_0.gguf` default, OpenAI/Voyage/etc. optional).
  - Indexer (chunk + embed + insert on file change, debounced).
  - Privacy Filter process (if enabled; warm on boot).
  - Dreaming scheduler (three-layer pipeline, leased).
  - Sync manager (git fetch/push, merge driver).
  - Event log appender.
  - Policy loader and validator.
  - Review queue, reality-check scheduler, notification dispatcher.
  - Local web dashboard (opt-in, `http://localhost:7137`).

**Upgrade path:** daemon supports graceful reload. New daemon binary starts on a new socket path; clients reconnect on version-mismatch signal; old daemon drains and exits.

**Lazy-start fallback:** if `memoryd` isn't running when an MCP client connects (e.g., during install), the client spawns it. Same daemon, lazy-initialized.

---

## 6. Storage substrate

**Canonical form: Markdown + YAML frontmatter, on disk.**

**Derived index: SQLite** at `~/.memory/index.sqlite`. Device-local. Never synced. Rebuilt from files on demand (`memory reindex`). FTS5 for keyword; sqlite-vec for vectors (or pgvector-adjacent local store depending on availability).

**Event log: per-device JSONL** at `~/.memory/events/<device-id>.jsonl`. Append-only. Every write, note, supersede, forget, conflict, and promotion event is logged with ordering. Event log is synced via git (separate files per device means no merge conflicts in the log itself — merges are concatenations).

**Why files canonical:**

- Human-readable, greppable without tooling.
- Git-friendly, diffable, reviewable.
- Vim/VSCode/any editor can open, edit, commit by hand.
- SQLite corruption is a recoverable nuisance, not data loss.

**Memory tree root:** `~/.memory/` (git repo).

**Full tree:**

```
~/.memory/
├── .git/                              # history, sync transport
├── .memory-project.yaml               # optional: override project binding
├── me/
│   ├── identity/
│   │   ├── role.md                    # who Trey is, what he does
│   │   └── principles.md              # operating principles
│   ├── relationship/
│   │   ├── facts/<entity>.md          # what agents learned about Trey
│   │   ├── preferences/
│   │   ├── corrections/<id>.md        # things agents got wrong
│   │   └── patterns/                  # meta-patterns about Trey
│   ├── knowledge/<topic>.md           # Trey-authored knowledge
│   ├── episodic/YYYY-MM-DD.md         # daily me-scoped notes
│   └── prospective/<id>.md            # future commitments
│
├── projects/
│   └── <namespace>/                   # e.g., prospera/atlasos/
│       ├── state.md                   # current working state
│       ├── decisions/YYYY-MM-DD-<slug>.md
│       ├── open-questions/<id>.md
│       ├── playbooks/<name>.md
│       ├── entities/<entity>.md
│       ├── episodic/YYYY-MM-DD.md
│       ├── invariants.md
│       └── regressions/<id>.md        # project-specific regression fingerprints
│
├── agent/
│   ├── patterns/<id>.md
│   ├── playbooks/<name>.md
│   ├── postmortems/<id>.md
│   ├── anti-patterns/<id>.md
│   ├── heuristics/<id>.md
│   ├── regressions/<id>.md            # cross-project regression fingerprints
│   └── episodic/YYYY-MM-DD.md
│
├── dreams/
│   ├── journal/YYYY-MM-DD.md          # narrative; NOT promotion source
│   ├── questions/YYYY-MM-DD.md        # Pass-3 uncomfortable questions
│   └── reports/<phase>/YYYY-MM-DD.md
│
├── substrate/
│   └── <device-id>/YYYY-MM-DD.jsonl   # per-device substrate fragments
│
├── encrypted/
│   └── <namespace>/...                 # age-encrypted parallel tree
│
├── tombstones/
│   └── YYYY-MM-DD.jsonl                # active deletion rules
│
├── events/
│   └── <device-id>.jsonl               # per-device event log
│
├── policies/
│   ├── me-strict.yaml
│   ├── project-standard.yaml
│   ├── agent-strict.yaml
│   └── dreaming-strict.yaml
│
├── leases/
│   └── journal.lease                   # advisory journal run lock
│
└── config.yaml                         # daemon config: sync remote, provider keys, toggles
```

Index file (`~/.memory/index.sqlite`) and daemon runtime files live outside the git repo (`~/.memoryd/`).

---

## 7. Frontmatter schema (target, A′ discipline)

Every memory carries the full schema. Unused fields are explicit `null`, not missing.

```yaml
---
# Identity
id: mem_20260423_014
type: project | person | procedure | episode | claim | artifact | prospective | pattern | playbook | postmortem | anti-pattern | heuristic | regression | correction | invariant | decision | open-question
scope: user | project | org | agent | subagent
namespace: prospera/atlasos         # human-readable alias; canonical id in metadata
canonical_namespace_id: proj_<hex>

# Content metadata
tags: [project, policy]
entities:
  - id: ent_trey_goff
    label: Trey Goff
aliases: []
summary: "One sentence operational summary."

# Provenance
source:
  kind: user | agent-primary | agent-subagent | tool | web | email | file | synthesis
  ref: "session id, file path, URL handle, artifact id"
  harness: claude-code | codex | cursor | cli | null
  session_id: sess_...
  subagent_id: null
  device: dev_<hash>
author: user:trey | agent:claude-code:sess_... | dreaming:journal:2026-04-23
evidence:
  - quote: "quoted support"
    ref: "file:line or artifact handle"
    weight: 1.0
    observed_at: 2026-04-23T14:22:10Z

# Governance
confidence: 0.85                     # 0.0 – 1.0, required
trust_level: trusted | untrusted | candidate | quarantined | pinned
sensitivity: public | internal | confidential | secret | personal
status: candidate | active | pinned | superseded | archived | tombstoned
review_state: null | pending | approved | rejected
requires_user_confirmation: false

# Privacy scan (populated when Privacy Filter is enabled)
privacy_scan:
  model: openai/privacy-filter@v1.0
  ran_at: 2026-04-23T14:22:10Z
  spans_detected: 2
  labels: [private_person, private_email]
  span_details_ref: sidecar://privacy-scans/mem_20260423_014.json

# Temporal
created_at: 2026-04-23T14:22:10Z
updated_at: 2026-04-23T14:22:10Z
observed_at: 2026-04-23T14:22:10Z
valid_from: null
valid_until: null
ttl: null

# Supersession
supersedes: []
superseded_by: []
related: [mem_20260401_007]

# Policies
retrieval_policy:
  passive_recall: true
  max_scope: project
  mask_personal_for_synthesis: true
write_policy:
  human_review_required: false
  policy_applied: me-strict@v3

# Regression-specific fields (only for type: regression)
regression:
  detection_signature:
    error_string_regex: null
    stack_fingerprint: null
    tool_output_hash: null
    behavioral_marker: null
  fire_on_attempt: true               # surface in recall when signature is about to recur
  first_observed: 2026-03-12T10:03:00Z
  last_observed: 2026-04-20T15:41:00Z
  occurrence_count: 4
---

Body of the memory — free-form Markdown for the actual content.
```

Validator runs at the daemon boundary. Missing required fields = refuse write with structured error. Fields with null values = allowed, flagged by lint, sometimes gated by policy (e.g., policy can require `valid_until` be inferred for certain types).

---

## 8. Identity and scope resolution

**User identity:** OS user + device id. Device id is generated at first run, stored in `~/.memoryd/device-id`. Paired with the user's canonical id in `config.yaml`.

**Project identity:**

```
1. Walk upward from cwd.
2. Find nearest `.memory-project.yaml`.
3. If found:
     use its declared namespace alias → canonical id derived from alias hash.
4. Else find nearest `.git/`:
     canonical_id = SHA256(normalize(git remote get-url origin))
     alias = derived from remote's human-readable path (e.g., "prospera/atlasos")
5. Else:
     no project scope. namespaces_in_scope = ["me", "agent"] only.
```

Normalization of git remotes:

- Strip `.git` suffix.
- Lowercase the host.
- Collapse `git@host:org/repo` and `https://host/org/repo` to the same canonical form.
- Strip trailing slashes, query strings.

**`.memory-project.yaml` schema:**

```yaml
namespace: prospera/atlasos
aliases: [atlasos]                    # secondary matchers
canonical_id: proj_<explicit hex>     # optional; if omitted, derived from namespace
subprojects:
  - path: services/api
    namespace: prospera/atlasos/api
  - path: services/web
    namespace: prospera/atlasos/web
policy:
  project: project-standard
  agent: agent-strict
```

Subprojects handle monorepos: a session at `services/api/...` resolves to `prospera/atlasos/api`; at repo root it resolves to `prospera/atlasos`.

**Agent identity for attribution:**

```
source.harness: claude-code | codex | cursor | ...
source.harness_version: "0.7.3"
source.session_id: sess_...
source.subagent_id: null              # if delegated work
author: agent:claude-code:sess_...
```

Authority is global-pool-wide — all agents write to the same `agent/` namespace — but every write carries attribution sufficient to roll back or investigate a specific harness/session/subagent's contributions.

---

## 9. Multi-device sync

**Transport:** git. Memory tree is a git repo. Sync via `git fetch && git merge` on a configurable cadence (default: every 2 minutes when daemon is idle, on-demand via `memory sync`).

**Remote:** user's choice. Private GitHub repo, self-hosted Gitea, private GitLab, or even a file remote mounted via SSHFS between personal machines. Remote is configured at install.

**Auto-commit policy:** daemon coalesces writes. On any write/note/supersede/forget/tombstone event, start a 30-second debounce timer. When timer fires, commit all pending changes with an auto-generated message:

```
auto: 3 writes, 2 notes, 1 supersede [namespace: prospera/atlasos]

- writes: mem_..., mem_..., mem_...
- notes: sub_..., sub_...
- supersedes: mem_... -> mem_...
```

Commit author is the user's configured git identity. Commit has a `memoryd-version` trailer for debuggability.

**Merge driver for frontmatter:**

Custom `gitattributes`:

```
*.md merge=memory-frontmatter-merge
```

Merge driver is a Node/Go/Rust binary installed by the daemon. Behavior:

- Parse frontmatter from base, ours, theirs.
- Field-level merge:
  - Scalars (`confidence`, `status`, `summary`, `updated_at`): newer `updated_at` wins; if tied, quarantine.
  - Arrays (`evidence`, `supersedes`, `superseded_by`, `related`, `tags`): union by id.
  - Enum transitions (`status`): transitions follow a poset; `tombstoned` dominates; `archived` > `superseded` > `active` > `candidate`. Irreconcilable transitions (e.g., both sides tombstoned with different reasons) go to quarantine.
  - `valid_from`/`valid_until`: newer `observed_at` wins.
- Body merge:
  - Attempt textual 3-way merge.
  - On conflict, leave conflict markers in the body, set `status: quarantined`, emit event, surface in `memory conflicts`.
- Privacy scan metadata: newer scan wins; keep both if from different model versions (audit).

**Event log merge:**

Each device writes to its own `events/<device-id>.jsonl`. Different files per device = no merge conflicts. Fetch unions the files naturally.

**SQLite not synced.** Each device rebuilds its index from files. `memory reindex` is always idempotent.

**Conflict escalation:** when the driver produces a quarantine, the daemon posts a notification, adds to the review queue, and the TUI / web dashboard surfaces a side-by-side resolution UI with field-level accept/reject.

---

## 10. Passive recall — per-harness capability mapping

Passive recall is assembled by the daemon and delivered through harness-specific integration layers. Three concrete integrations; degraded pull-only for the rest.

**Integration tier 1 (full) — Claude Code:**

```
.claude/hooks/memory-session-start.sh      # SessionStart hook
.claude/hooks/memory-user-prompt.sh        # UserPromptSubmit hook
.claude/.mcp.json                           # registers memoryd MCP server
```

- `SessionStart` hook calls `memoryd cli startup-block --session=<sid> --cwd=<cwd>`. Output is the base recall block (stable across the session). Injected as a system message.
- `UserPromptSubmit` hook calls `memoryd cli delta-block --session=<sid> --message=<text>`. If entity matcher finds new hits, output is a small delta block (400-token budget) injected as a suffix system message. If no new hits, output is empty (no cache impact).

**Integration tier 1 (full) — Codex CLI:**

Codex supports `AGENTS.md` auto-inclusion and startup hooks. The installer writes:

```
AGENTS.md (prepended by installer):
  On session start, immediately call memory_startup with your current cwd and session id.
  Do this before reading any other context.
```

Plus whatever startup-hook mechanism Codex exposes (TBD based on current Codex capabilities at implementation time) to inject the base recall block natively.

**Integration tier 1 (partial) — Cursor:**

Rules file (`.cursor/rules/memory.mdc`) auto-invokes `memory_startup` on session open. Delta injection not available without hooks; agent calls `memory_search` on demand.

**Integration tier 2 (degraded) — any MCP-capable harness:**

Only MCP tools. Agent is instructed via `AGENTS.md` / `CLAUDE.md` / installer-added convention to call `memory_startup` as the first action. Pull-based; no true pre-prompt injection. Explicitly marked as degraded in the dashboard's integration status view.

**Base recall block composition (always the same shape):**

```
<memory-recall>
  <identity>
    {pinned items from me/identity/}
  </identity>

  <project state="active">
    {project.state.md for current project, trimmed}
    {open questions count + summaries}
    {recent decisions, last 7 days}
    {invariants, full}
    {regressions matching cwd/recent activity — fire_on_attempt=true}
  </project>

  <entity-recall entities="[...]" budget="3000">
    {entity summaries for mentioned / recently-touched entities}
    {one-hop related memories}
    {provenance for each: last_updated, confidence, recall_count}
  </entity-recall>

  <pending-attention>
    {reality check due? review queue count? conflicts?}
  </pending-attention>
</memory-recall>
```

Budget is configurable; default 3000-4000 tokens. Byte-identical across turns for a given session → prefix cache stays warm.

**Delta block (per-turn, suffix, 400 tokens max):**

```
<memory-delta>
  <new-entity id="..." matched-via="alias:X">
    {entity summary, one-hop related, provenance}
  </new-entity>
  <regression-fire id="regr_..." signature="error_string:X">
    Heads up — this pattern burned on 2026-03-12. See regr_... for fix.
  </regression-fire>
  <peer-activity>
    Codex session sess_... is currently touching auth.ts (started 4m ago).
  </peer-activity>
</memory-delta>
```

**Passive recall explanation metadata:** every block includes `<recall-explanation>` with matched entity ids, matching aliases, budget used, policy applied. Debuggable.

---

## 11. Governance — A′ architecture

**Full handbook machinery always running. Policy tunes strictness per namespace.**

### 11.1 Machinery (unconditional, every write)

1. **Grounding verification.** Non-user writes must have a `source.ref` that currently resolves. File exists / URL fetched within TTL and cached / tool-call transcript handle valid / subagent id in session spawn log. Dream prose is never a grounding source; only grounded snippets rehydrate from live daily files.

2. **Contradiction detection.**
   - Embed candidate.
   - Find top-K most-similar active memories in same namespace, same entity set.
   - If max similarity > threshold (default 0.85): run an LLM tiebreaker with candidate + top-K: "same claim / refinement / contradiction?"
   - `same`: refuse, return existing id as duplicate.
   - `refinement`: auto-merge into existing memory (append evidence, bump confidence, update body).
   - `contradiction`: auto-supersede (write supersession chain) OR quarantine for review, per policy.

3. **Tombstone matching.** Candidate content + entity set hashed; matched against active tombstones. Hit = refuse + log + (if sensitive) alert.

4. **Supersession chain maintenance.** Every contradiction-triggered supersession populates `supersedes`, `superseded_by`, caps old `valid_until`, opens new `valid_from`. Chain walkable in both directions. Old memory's status flips to `superseded`, not deleted.

5. **Sensitivity classification.**
   - Privacy Filter enabled: per-span labels → tier mapping per policy.
   - Privacy Filter disabled: regex + small heuristics → whole-document tier.
   - Sensitivity tier determines storage (plain vs. encrypted) and synthesis eligibility.

6. **Temporal validity inference.** Where possible: TTL hints in text ("until Q2"), extracted absolute dates, semantic cues. Otherwise null (no forced-populate).

7. **Quarantine as first-class status.** Any write that fails a check but shouldn't be discarded (contradicts but might be refinement; low-confidence but grounded; suspicious source) goes to `status: quarantined` with a reason code. Not surfaced in passive recall; visible in `memory review`.

### 11.2 Policy files (per namespace)

```yaml
# policies/me-strict.yaml
version: 3
confidence_floor: 0.75
grounding_required: true
contradiction_policy: supersede_with_chain   # or: quarantine_for_review
review_gate:
  triggers: [identity_claim, relationship_fact, preference_revision, principle_change]
  blocking: true
sensitivity_defaults:
  default_tier: personal
  max_tier_auto: personal        # anything above (secret) is refused regardless
privacy_filter:
  enabled: true
  policy_per_label:
    secret: refuse_write
    account_number: refuse_write
    private_person: allow
    private_email: allow
    private_address: allow_personal_tier
    private_phone: allow_personal_tier
    private_date: allow
    private_url: allow
tombstone_enforcement: strict
subagent_writes: quarantine_by_default
```

```yaml
# policies/agent-strict.yaml
version: 3
confidence_floor: 0.80                  # higher bar for cross-cutting pool
grounding_required: true
contradiction_policy: quarantine_for_review
review_gate:
  triggers: [pattern_revision, regression_new, playbook_change]
  blocking: false                       # log-only for now; tighten later
sensitivity_defaults:
  default_tier: internal
  max_tier_auto: internal
privacy_filter:
  enabled: true
  policy_per_label:
    secret: refuse_write
    account_number: refuse_write
    private_person: refuse_write        # no personal names in cross-cutting pool
    private_email: refuse_write
    private_address: refuse_write
    private_phone: refuse_write
    private_date: allow
    private_url: allow_with_host_only
tombstone_enforcement: strict
subagent_writes: quarantine_by_default
```

```yaml
# policies/project-standard.yaml
version: 2
confidence_floor: 0.60                  # lower bar for project-scoped memory
grounding_required: true
contradiction_policy: supersede_with_chain
review_gate:
  triggers: [invariant_change, decision_revision]
  blocking: false
sensitivity_defaults:
  default_tier: internal
  max_tier_auto: confidential
privacy_filter:
  enabled: true
  policy_per_label: { secret: refuse_write, account_number: refuse_write }
tombstone_enforcement: strict
subagent_writes: auto_promote
```

```yaml
# policies/dreaming-strict.yaml
version: 2
confidence_floor: 0.85
grounding_required: true
grounding_rehydration_required: true    # live-file rehydration at promote time
contradiction_policy: quarantine_for_review
dream_prose_as_source: refuse           # direct handbook rule
sensitivity_defaults:
  default_tier: internal
  max_tier_auto: internal
privacy_filter:
  enabled: true
  masked_synthesis_required: true       # dreams run on masked views only
```

Policies are versioned and live in git. Changes are reviewable. `memory policy test` runs the policy against a candidate to preview behavior before applying.

### 11.3 Human-review gate

For policies with `review_gate.blocking: true`: writes matching trigger conditions go to `status: pending_review`. Surface in the review queue. User approves (becomes active), rejects (becomes archived with reason), or forgets (tombstone). The gate exists even in a single-user system; reviewing is optional but queue never auto-promotes.

### 11.4 Subagent writes

Default: `quarantine_by_default`. Subagent writes go to quarantine; parent agent (or policy) can promote them. Directly addresses the handbook's lesson about subagent memory silently failing — writes are always attempted, quarantine makes the attempt visible even if promotion is gated.

---

## 12. Dreaming — three-layer pipeline

### 12.1 Substrate layer (per-device, always on)

Substrate writes happen via `memory_note(observation)` tool and via passive event emission from the daemon itself (e.g., when passive recall fires on an entity, a substrate event logs the entity's recall context).

```jsonl
{"id":"sub_20260423_0001","ts":"2026-04-23T14:22:10Z","device":"dev_...","session":"sess_...","harness":"claude-code","namespace_scope":"project:proj_<hex>","entities":["ent_auth_flow"],"kind":"observation","text":"User corrected: we don't use HS256; we use RS256 with a rotating key.","source_ref":"session:sess_...:turn:47","privacy_spans":[]}
{"id":"sub_20260423_0002","ts":"2026-04-23T14:24:03Z","device":"dev_...","session":"sess_...","harness":"codex","namespace_scope":"project:proj_<hex>","entities":["ent_auth_flow","ent_jwt"],"kind":"pattern","text":"Third time investigating JWT validation in this repo — pattern emerging around key rotation.","source_ref":"session:sess_...:turn:12","privacy_spans":[]}
```

Fragment lifetime: 14 days default, configurable. Expired fragments are archived (moved to `substrate/archive/`), not deleted — dreaming's janitorial pass may recover them if they become relevant later.

### 12.2 Journal layer (daily, leased device, grounded promotion only)

Runs once per day per namespace scope. Device election via lease file:

```jsonl
// leases/journal.lease (committed + pushed)
{"device":"dev_macbook","acquired_at":"2026-04-23T03:00:00Z","expires_at":"2026-04-23T04:00:00Z","scope":"me"}
{"device":"dev_macbook","acquired_at":"2026-04-23T03:00:10Z","expires_at":"2026-04-23T04:00:00Z","scope":"agent"}
{"device":"dev_work","acquired_at":"2026-04-23T03:00:15Z","expires_at":"2026-04-23T04:00:00Z","scope":"project:proj_<hex>"}
```

A device attempting a dream for a scope: fetch, check for live lease. If none, append lease record, push, then run. If race on push, retry fetch; loser backs off.

**Three passes per run:**

Pass 1 — "Why did this happen this way?"

- Read all substrate fragments in scope from last N days (default 7 for daily, 30 for weekly).
- Cluster by entity co-occurrence.
- LLM pass identifies patterns, recurrences, cross-event narratives.
- Output: narrative prose → `dreams/journal/<namespace>/<date>.md`. NOT a promotion source.

Pass 2 — "What should change?"

- Using Pass 1 output + active memories in scope, propose candidate refinements.
- Each proposal has: candidate memory body, target namespace, evidence array pointing to substrate fragments, grounding requirement.
- Proposals enter the candidate queue under `dreaming-strict` policy.
- `grounding_rehydration_required: true` means at promote time, daemon re-reads cited source files; if deleted or content changed significantly, skip promotion.

Pass 3 — "What uncomfortable question is this system avoiding?"

- Adversarial self-critique pass.
- Output: questions → `dreams/questions/<namespace>/<date>.md`.
- NOT promoted. BUT: passive recall reads the most recent questions file for each scope-in-session; if topics intersect current activity, surface in the recall block's `<pending-attention>` section. The uncomfortable question gets heard without promoting to belief.

### 12.3 Cleanup layer (nightly, idempotent, any device)

Pure janitorial:

- Dedup by canonical claim hash.
- Archive stale candidates past retention window.
- Rebuild entity index.
- Run lint on all memories.
- Verify tombstone integrity (every tombstoned memory has an active rule; every active rule points to a memory).
- Check for provenance graph orphans (supersession chains with dangling ends).
- Refresh `observed_at` on memories whose supporting files are still live.
- Compact the event log (move entries older than N days into compressed archive, keeping tail fast to tail).

Safe to run concurrently on multiple devices because all operations are idempotent and commute.

### 12.4 Dreaming CLI

```
memory dream status                     # what's scheduled, last run times, lease state
memory dream now --scope me             # force a run for a scope
memory dream review --since 7d          # walk recent journal outputs + candidate proposals
memory dream disable / enable
```

---

## 13. Secrets and Privacy Filter

### 13.1 Four layers of detection

**Layer 1 — Regex + entropy (always on, offline).**

Gitleaks-compatible ruleset + entropy threshold heuristics. Catches:

- AWS/GCP/Azure keys
- GitHub/GitLab tokens
- Stripe keys
- SSH private keys (PEM headers)
- JWT-structured tokens
- Generic high-entropy strings > N chars

Hit = refuse write, log attempt, return structured error.

**Layer 2 — Privacy Filter (opt-in).**

OpenAI Privacy Filter (1.5B, Apache 2.0, Hugging Face, released 2026-04-22). Per-span labels:

```
private_person, private_address, private_email, private_phone,
private_url, private_date, account_number, secret
```

Daemon runs inference locally via ONNX runtime (or llama.cpp-compatible path if a quant ships). Span output drives per-label policy decisions (Q10).

Enable:

```
memoryd privacy-filter install          # download weights (~3GB)
memoryd privacy-filter enable
memoryd privacy-filter status
```

**Layer 3 — Storage routing by sensitivity.**

```
public      → plain Markdown, git-synced
internal    → plain Markdown, git-synced
confidential → age-encrypted Markdown under encrypted/, ciphertext git-synced
personal    → age-encrypted Markdown under encrypted/, ciphertext git-synced
secret      → refused; agent redirected to 1Password / pass / secretsmanager
```

**Layer 4 — Daemon commit hook.**

Before every git commit, daemon re-runs Layer 1 on the delta. Hit = refuse commit, quarantine the offending memory, alert.

### 13.2 Age-encrypted tier

Key management: `age` with recipients file (`~/.memory/encrypted/.age-recipients`), one public key per device. Private keys in OS keychain:

- macOS: Keychain Services, service `memoryd`, account `age-<device-id>`.
- Linux: secret-service (`libsecret`), collection `memoryd`.
- Windows: Credential Manager (if/when supported).

Device onboarding:

```
memoryd device onboard                   # generates new age key, stores private in keychain
memoryd device onboard --add-recipient PUBKEY    # for existing vaults
memoryd device rotate-keys               # re-encrypts encrypted/ for new recipient set
memoryd device revoke <device-id>        # remove device, rotate, force re-encryption
```

### 13.3 Masked synthesis views

When Privacy Filter is enabled, daemon maintains a session-scoped salt table mapping spans to stable tokens (`Person_A`, `Email_B`, `Address_C`, ...). Dreaming journal pass reads memories via `mask_personal` view:

- Spans replaced with stable tokens.
- LLM pass operates on masked text.
- Salt table is daemon-local, never written to disk, cleared at dreaming-run end.
- On write-back (candidate proposal), tokens are restored using the salt table.
- Restored proposals re-run Privacy Filter (belt and suspenders).

Prevents sensitive content from leaking into dream prose while still enabling pattern recognition.

### 13.4 Leak runbook

If detection fails and a secret enters the synced store:

```
memory forget <id> --reason "secret leaked" --leaked-secret-hash <hash>
# daemon:
#   1. tombstone the memory
#   2. emit user alert with incident details
#   3. generate runbook with specific commands:
#         git filter-repo --replace-text <leak-file> (in memory repo)
#         force-push to remote (with warning)
#         rotate credential via <issuer>
#         re-clone on other devices
#   4. add leaked_secret_hash to active tombstone rule
#         → future writes containing same hash auto-refused
```

---

## 14. MCP tool surface

### 14.1 Agent-facing (7 tools)

**`memory_search(query, filter?)`**

```typescript
input: {
  query: string,
  filter?: {
    namespace?: "me" | "project" | "agent" | "all",
    entities?: string[],
    types?: MemoryType[],
    since?: ISODateString,
    until?: ISODateString,
    source_kind?: SourceKind,
    status?: Status[],
    sensitivity_max?: SensitivityTier,
    limit?: number,                // default 10
    include_body?: boolean,         // default false; returns summaries only
  }
}

output: {
  results: Array<{
    id: string,
    namespace: string,
    type: MemoryType,
    summary: string,
    entities: string[],
    confidence: number,
    updated_at: ISODateString,
    score: number,
    score_breakdown: { vector: number, text: number, recency: number },
    matched_entities?: string[],    // which entities drove the match
    body?: string,                  // if include_body=true
  }>,
  total: number,
  budget_used_tokens: number,
  recall_explanation: {
    scopes_searched: string[],
    policy_applied: string,
    elapsed_ms: number,
  }
}
```

**`memory_get(id)`**

```typescript
input: { id: string, include_provenance?: boolean }

output: {
  id: string,
  frontmatter: Frontmatter,
  body: string,
  provenance?: {
    supersedes_chain: Array<{ id, summary, valid_until, superseded_reason }>,
    superseded_by_chain: Array<{ id, summary, valid_from }>,
    evidence: EvidenceEntry[],
    source: SourceDescriptor,
  }
}
```

**`memory_write(content, meta?)`**

```typescript
input: {
  content: string,                  // markdown body
  meta: {
    namespace?: "me" | "project" | "agent",   // default: project if bound, else me
    type: MemoryType,
    summary: string,
    entities?: Entity[],
    tags?: string[],
    confidence?: number,            // default policy-defined
    sensitivity?: SensitivityTier,
    evidence?: EvidenceEntry[],
    valid_from?: ISODateString,
    valid_until?: ISODateString,
  }
}

output:
  | { status: "promoted", id: string, namespace: string, supersedes?: string[] }
  | { status: "candidate", id: string, reason: string, next_actions: string[] }
  | { status: "quarantined", id: string, reason: string, next_actions: string[] }
  | { status: "refused", reason: "grounding"|"contradiction"|"tombstone"|"privacy"|"policy", details: object }
```

**`memory_supersede(old_id, content, reason)`**

```typescript
input: { old_id: string, content: string, reason: string, meta?: WriteMeta }
output: { status, new_id, chain: { supersedes: string[], capped_valid_until: string } } | RefusedResult
```

**`memory_forget(id, reason)`**

```typescript
input: { id: string, reason: string }
output: { status: "tombstoned", id, tombstone_ref }
        | { status: "requires_user_confirmation", reason, prompt_for_user: string }
        | RefusedResult
```

**`memory_startup(session_context)`**

```typescript
input: {
  cwd: string,
  session_id: string,
  harness: string,
  harness_version?: string,
  since_event_id?: string,          // for delta-since-last-startup
}

output: {
  session_binding: SessionBinding,
  recall_block: RecallBlock,        // identity + project state + entity recall + pending-attention
  budget_used_tokens: number,
  recall_explanation: RecallExplanation,
  peer_activity?: PeerActivity[],   // Level 3 coordination, if enabled
}
```

**`memory_note(observation)`**

```typescript
input: {
  text: string,
  entities?: Entity[],
  kind?: "observation" | "pattern" | "correction" | "question",
}

output: { status: "logged", id: string }
```

Substrate-layer write. Cheap, no governance gates beyond Privacy Filter, no promotion. Feeds dreaming journal.

### 14.2 Event subscription (Level 2/3 coordination)

```typescript
// memory_subscribe is a long-lived MCP tool for harnesses that support streaming.
// For non-streaming harnesses, use `since_event_id` on memory_search / memory_startup.

memory_subscribe({
  session_id: string,
  filter: {
    namespaces?: string[],
    entities?: string[],
    kinds?: EventKind[],           // write, note, supersede, forget, conflict, presence, claim_locked
  }
})
// → streams events as they occur. each event <200 bytes.
```

### 14.3 Admin surface (CLI + slash commands, NOT MCP)

```
memory status
memory review [--namespace X] [--quarantined]
memory diff --since 7d
memory audit <id>
memory lint
memory conflicts
memory rollback <id> --to-version N
memory pin <id>
memory unpin <id>
memory export <filter>
memory policy show|edit|test
memory health
memory reality-check [run|skip|snooze]
memory dream status|now|review|disable|enable
memory sync [--now]
memory reindex
memory device onboard|rotate-keys|revoke
memoryd status|reload|logs
```

Plus Claude Code slash commands:

```
/memory-status
/memory-review
/memory-pin
/memory-forget
/memory-reality-check
```

---

## 15. Live cross-session coordination

**Event-log-driven, three levels.**

### 15.1 Event types

```typescript
type Event =
  | { kind: "write", id, ts, namespace, entity_ids, actor, ref }
  | { kind: "note", id, ts, namespace, entity_ids, actor, ref }
  | { kind: "supersede", id, ts, namespace, old_id, new_id, actor }
  | { kind: "forget", id, ts, namespace, forgotten_id, actor, reason }
  | { kind: "conflict", id, ts, namespace, quarantined_id, reason }
  | { kind: "presence", id, ts, session_id, entity_ids, started_at }          // Level 3
  | { kind: "claim_locked", id, ts, namespace, memory_id, holder_session }    // Level 3
```

All events < 200 bytes serialized. Bounded per turn to protect cache.

### 15.2 Three levels of sharing

**Level 1 — writes only.** Sessions see peers' promoted memories on next recall refresh. Minimum viable.

**Level 2 — writes + candidates + notes (default).** Sessions see in-flight proposals and substrate notes from peers. Surfaces in recall delta when relevant.

**Level 3 — presence + intent.** Sessions broadcast "I'm working on entity X." Daemon surfaces "another session is also touching X" in recall. Claim locks (memory under revision) prevent stale-truth reliance. Configurable per project: `concurrent_session_mode: collaborative` in `.memory-project.yaml`.

### 15.3 Shared substrate pool

All sessions on the same device-user-project write to the same `substrate/<device-id>/YYYY-MM-DD.jsonl`. No segregation by harness or session. Tagged with `harness` and `session_id` for attribution. The journal pass reads the combined pool — this is how cross-harness pattern recognition works at the synthesis layer.

### 15.4 Session presence TTL

Sessions send heartbeats every 60s. Missed heartbeats for 5 minutes = daemon marks session stale; presence events clear. Prevents ghost presence from crashed sessions.

---

## 16. Observability and user-facing surfaces

### 16.1 CLI

Already enumerated in §14.3. Token-efficient output, scriptable, stays in terminal.

### 16.2 TUI (`memory ui`)

Terminal UI akin to `lazygit`/`k9s`. Panels (toggleable with number keys):

1. **Overview** — daemon health, pending review count, conflicts, sync lag, active sessions
2. **Review queue** — quarantined/pending items; j/k to navigate, a/r/f/q to approve/reject/forget/quarantine
3. **Conflicts** — side-by-side merge conflict resolver; field-level accept/reject
4. **Entities** — `/entity-name` search; see all memories attached, supersession chains, recall history
5. **Timeline** — scrollable event feed with filter controls
6. **Namespace explorer** — tree view of me/project/agent; inspect any memory
7. **Policy inspector** — active policies, recent decisions, refusal reasons
8. **Reality check** — launch or snooze the weekly ritual

Keyboard-first, zero mouse. Renders in any terminal.

### 16.3 Local web dashboard (opt-in)

`memoryd web enable` starts HTTP server on `localhost:7137`. Serves:

- **Entity graph** — force-directed visualization of entity relationships; click to explore; supersession chains rendered as temporal edges
- **Synthesis ROI dashboard** — over 30/90/365 days: promotion rate, promotion precision (recall-after-promote), refusal breakdown, dreaming value metrics
- **Richer reality-check UI** — swipe/click through items more ergonomically than TUI
- **Policy editor** — syntax-highlighted YAML editing with live validation and dry-run
- **Audit explorer** — walk provenance graphs visually; time-scrub temporal validity
- **Sync status** — which devices have what; lease state; commit history

Browser is localhost-only; no external network exposure by default. Opt-in remote access (SSH tunnel recommended, not built-in port exposure).

### 16.4 Weekly Relationship Reality Check

**Schedule:** Sunday evenings, configurable. Slack/email reminder when due.

**Algorithm — drift-risk scoring:**

```
score(m) = w1 * days_since_observed(m)
         + w2 * recall_frequency(m)
         + w3 * (1 - cross_source_corroboration(m))
         + w4 * confidence_decay(m)
         + w5 * sensitivity_weight(m)
```

Top N memories (default 12) surface per session. User responds:

- **confirm** → refresh `observed_at`, slight confidence bump
- **correct** → prompts for new content; triggers supersession chain
- **forget** → tombstone with user-provided reason
- **not relevant** → lower passive-recall weight; skip in future reality checks (doesn't tombstone — just de-prioritize)
- **skip this week** → come back next Sunday

### 16.5 Notifications

Three channels, per-event:

- **Passive** (default): appears in `memory status` and in the next session's recall block as a pending-attention line
- **OS notification** (urgent): leaked secret detected, merge conflict blocking sync, queue over threshold
- **External** (scheduled): Slack webhook / email for weekly reality check, daily synthesis summary

Configurable in `config.yaml`:

```yaml
notifications:
  passive: always
  os:
    triggers: [leaked_secret, blocking_merge_conflict, review_queue_over:50]
  external:
    channel: slack
    webhook_url: https://hooks.slack.com/...
    triggers: [reality_check_due, daily_synthesis_summary]
```

### 16.6 Trust artifacts

Every memory's detail view shows:

- Provenance chain (walk backward to source)
- Confidence with reason
- Recall count + last-recalled timestamp
- Policy decisions taken
- Privacy scan results (span labels detected)
- Supersession history
- Sync state (which devices have this, merge status)

No black boxes.

---

## 17. Internal namespace taxonomy — category details

### 17.1 Me-memory

**`identity/role.md`** — who you are, what you do. Pinned. Changes require `requires_user_confirmation: true`. Rarely written.

**`identity/principles.md`** — operating principles. Pinned. Deliberate revision through user action only.

**`relationship/facts/<entity>.md`** — e.g., `relationship/facts/claire.md`, `relationship/facts/prospera.md`. What agents have concluded about an entity in your life. Drift-prone; surfaced in Weekly Reality Check by drift-risk score.

**`relationship/preferences/`** — e.g., `relationship/preferences/code-review.md`, `relationship/preferences/communication-style.md`. Working style defaults. Auto-update with lower friction but log reversals in `corrections/`.

**`relationship/corrections/<id>.md`** — first-class category. When you correct an agent, the correction is a durable record. Schema:

```yaml
---
type: correction
entity_corrected: ent_...
original_belief: "agent thought X"
correction: "actually Y"
context: session/turn ref
observed_by: [session_id, ...]
---

Narrative of the correction and why it matters.
```

Corrections are high-leverage for agent-memory synthesis: "what do agents keep getting wrong about Trey?"

**`relationship/patterns/`** — meta-patterns about you. "You prefer durable over band-aid." "You expect pushback, not deference." Accumulated through synthesis on corrections + observed interactions.

**`knowledge/<topic>.md`** — things YOU'VE taught the system. Your views, your explicit assertions. Distinct from relationship/facts — different provenance, different review.

**`episodic/YYYY-MM-DD.md`** — daily notes scoped to your general activity (not project-bound).

**`prospective/<id>.md`** — future commitments (see §18).

### 17.2 Project-memory

**`state.md`** — current working state. High churn. Compaction takes snapshots into `episodic/`.

**`decisions/YYYY-MM-DD-<slug>.md`** — ADR-style records. Immutable. Supersession chains, not in-place edits.

**`open-questions/<id>.md`** — unresolved. Resolved → decision; abandoned → archived with reason.

**`playbooks/<name>.md`** — procedural, project-scoped.

**`entities/<entity>.md`** — project-specific (services, components, people-in-project-context).

**`episodic/YYYY-MM-DD.md`** — project-scoped daily notes.

**`invariants.md`** — things that must always be true about this project. Examples:
- "database migrations never drop columns"
- "all external endpoints require auth middleware"
- "no secrets in application logs"

Violations detected in substrate = incidents (written to regressions/).

**`regressions/<id>.md`** — project-specific regression fingerprints. Schema includes `detection_signature` with error string regex, stack fingerprint, tool-output hash, or behavioral marker. `fire_on_attempt: true` causes the regression to surface in recall when the signature is about to recur.

### 17.3 Agent-memory (global pool)

**`patterns/<id>.md`** — "when X observed, check Y before assuming Z." Cross-project, cross-harness.

**`playbooks/<name>.md`** — cross-project procedures. "How agents should handle PR review." "How agents should triage a production incident."

**`postmortems/<id>.md`** — what went wrong, from any session/harness/project. Narrative + root cause + lessons.

**`anti-patterns/<id>.md`** — explicit negative knowledge. "Don't do X, here's why."

**`heuristics/<id>.md`** — uncertain rules of thumb with confidence and observed-case count. Evolves as evidence accumulates.

**`regressions/<id>.md`** — cross-project regression fingerprints. Patterns recurring across multiple projects. Sourced from project-memory regressions escalated by the journal pass.

**`episodic/YYYY-MM-DD.md`** — cross-harness daily agent log.

### 17.4 Review cadence per subtree

Encoded in policy files, enforced by daemon:

```yaml
# policies/me-strict.yaml (snippet)
review_cadence:
  identity/role.md:
    require_user_confirmation: true
    change_notification: immediate
  identity/principles.md:
    require_user_confirmation: true
    change_notification: immediate
  relationship/facts/:
    weekly_reality_check: include
    drift_risk_scoring: enabled
  relationship/preferences/:
    auto_update: true
    log_reversals_to: relationship/corrections/
  relationship/corrections/:
    append_only: true
    synthesis_input: true
  knowledge/:
    user_authored_only: true
```

---

## 18. Open threads (not yet designed)

### 18.1 Prospective memory surface

**What needs designing:**

- Commitment schema (trigger: time/event/condition; owner; status lifecycle; requires_confirmation flag)
- External scheduler integration (launchd/systemd/cron-based firing)
- Injection of fired commitments into recall as "standing orders"
- Silent-completion guard (agent claims it did X; how do we verify?)
- Conditional triggers ("when PR #1234 merges") — how to detect; what subscribes
- Integration with Slack / calendar / email as external event sources

### 18.2 Evaluation harness

**What needs designing:**

- Project-level test suite with the 12 tests from the handbook:
  1. Exact identifier recall after three compactions
  2. Superseded fact handling
  3. Cross-project entity collision
  4. Abstention
  5. Poisoned candidate
  6. Tool-output preservation
  7. Subagent writeback
  8. Deletion and tombstone
  9. Recall budget pressure
  10. Compaction resumption
  11. Self-poisoning
  12. Temporal validity
- Domain-specific tests for this system:
  13. Cross-harness substrate sharing
  14. Merge-driver semantic correctness
  15. Privacy Filter refusal → error path → agent retry
  16. Reality-check drift scoring sanity
  17. Lease contention resolution
  18. Encrypted tier key rotation
- CI integration: run on every `memoryd` release
- Regression harness: failed assertions become new tests

### 18.3 Bootstrap / cold-start UX

**What needs designing:**

- `memoryd init` walkthrough: generate device key, create repo, configure remote, pick policies, link first harness
- First-harness linking (install MCP config + hooks for Claude Code / Codex / Cursor)
- Second-device onboarding (add recipient to age, clone repo, rebuild index, verify sync)
- First-run wizard: where does `~/.memory/` live? Sync remote? Privacy Filter install? Weekly reality check cadence?
- Migration from existing memory systems (OpenClaw `memory-core`, Letta, etc.)
- Backup and restore procedures

### 18.4 Policy versioning and migration

**What needs designing:**

- Policy evolution: old memories written under policy v1 are read under policy v3 — how do we handle the mismatch?
- Policy migration tool: re-evaluate all memories under new policy, surface newly-quarantined items for review
- Schema evolution: if we add a required field to frontmatter, how do we migrate existing memories?
- Field-level compatibility: daemon should read frontmatter even when newer daemon wrote fields this daemon doesn't know about (forward-compat)

### 18.5 Multi-user future-proofing

**What needs designing:**

- Principal model beyond single-user (human principals, shared memory, ACLs per path)
- Shared memory as collaboration surface (not just threat surface) — the design question I flagged in my initial handbook read
- Onboarding a second human principal to a shared memory repo
- Conflict resolution across humans (different from across devices)
- Privacy implications when memory leaves a single user's control

### 18.6 Naming

The system needs a real name. `memoryd` is the daemon; the system as a whole needs a name for packaging, documentation, brand.

---

## 19. Implementation phasing (proposed)

Given no limits on build capacity, parallel workstreams make sense rather than strict sequential phasing. Proposed streams:

**Stream A — Core substrate (foundational):**
1. File tree layout + frontmatter validator
2. SQLite indexer + file watcher
3. Event log + git auto-commit
4. Git merge driver for frontmatter

**Stream B — Daemon + MCP surface:**
1. Daemon skeleton (launchd/systemd)
2. MCP server (thin client to daemon)
3. Seven agent tools
4. CLI admin surface

**Stream C — Governance:**
1. Policy loader + validator
2. Grounding verification
3. Contradiction detection (vector + LLM tiebreak)
4. Tombstone matching
5. Supersession chain machinery
6. Quarantine queue

**Stream D — Privacy:**
1. Layer 1 regex detectors
2. Layer 4 commit hook
3. Privacy Filter integration (opt-in)
4. Age-encrypted tier
5. Masked synthesis views

**Stream E — Passive recall:**
1. Entity index + alias resolution
2. Base recall block assembly (budgeted)
3. Claude Code hooks (SessionStart + UserPromptSubmit)
4. Codex + Cursor integrations
5. Delta block on entity match

**Stream F — Dreaming:**
1. Substrate layer (always-on)
2. Journal layer (leased, three passes)
3. Cleanup layer (idempotent)
4. Dream review UI

**Stream G — Observability:**
1. CLI admin surface (shared with Stream B)
2. TUI (`memory ui`)
3. Web dashboard (opt-in)
4. Weekly Reality Check ritual
5. Notifications

**Stream H — Eval harness:**
1. 12 handbook tests
2. Domain-specific tests
3. CI integration
4. Regression harness pattern

**Stream I — Cross-session coordination:**
1. Event log tail + subscription
2. Level 2 defaults (candidates + notes visible)
3. Level 3 opt-in (presence + claim locks)

**Stream J — Open threads:**
1. Prospective memory
2. Bootstrap UX
3. Policy migration
4. Multi-user future-proofing

Dependencies: A is prerequisite to B, C, D, E. B gates Stream G. E depends on C for gate machinery. F depends on A, C, D. H runs in parallel to everything.

---

## 20. References

- `agent-harness-memory-context-handbook-v2.2.md` — everything philosophical, plus the threat model, evaluation chapter, and framework landscape.
- OpenAI Privacy Filter: https://openai.com/index/introducing-openai-privacy-filter/ (Apache 2.0, 1.5B, released 2026-04-22)
- OpenClaw docs: docs.openclaw.ai — especially `concepts/context-engine`, `concepts/dreaming`, `plugins/memory-wiki`
- LangChain Deep Agents — ACL primitives reference
- Graphiti / Zep — bi-temporal supersession reference
- Mastra Observational Memory — stable-prefix passive recall reference
- Letta Sleep-time Compute (arXiv 2504.13171) — dreaming ROI evidence
- `age` encryption: https://github.com/FiloSottile/age
- gitleaks ruleset: https://github.com/gitleaks/gitleaks

---

**End of v0.1 draft.** All 14 design forks from the 2026-04-23 brainstorming session are encoded. Open threads are named. Implementation streams are enumerated.
