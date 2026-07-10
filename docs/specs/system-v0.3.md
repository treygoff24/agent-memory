# Memorum — System Spec v0.3

**Status:** v1 release contract. Supersedes `system-v0.2.md` (which stays on disk for history). Stream A–F implementation contracts (`stream-a-core-substrate-v1.1.md`, `stream-c-governance-v0.1.md`, `stream-d-privacy-v0.1.md`, `stream-e-passive-recall-v0.7.md`, `stream-f-dreaming-v0.3.md`) override this document on any conflict — those are the live, shipped contracts. This document is the system-level frame and the contract for v1 release shape.

**Date:** 2026-07-08 (v0.3); 2026-05-01 (v0.2 base).

## Revision goal (v0.2 → v0.3)

v0.3 records the CLI-first agent surface. Under v0.2 the tier model made the MCP bridge the day-one active surface for every harness; in practice agents drive the `memoryd` CLI plus the `using-memorum` skill better than an MCP tool schema, and passive recall already arrives via lifecycle hooks. v0.3 makes that official and fixes one internal inconsistency. Concretely:

1. **Tier 1 is "hooks + skill/CLI," not "hooks + MCP."** The active agent surface is the hardened `memoryd` CLI (v1 envelope + exit-code contract, `docs/api/memoryd-cli-contract-v1.md`) paired with the `using-memorum` skill, plus the passive-recall lifecycle hooks. See §10.1.
2. **The MCP bridge is an optional compatibility surface.** It still ships, still forwards to the same daemon socket, and stays frozen at the ten tools in §14.1 — but it is no longer wired by default. `memoryd init` wires passive-recall hooks by default and wires MCP only on explicit request (`--wire-mcp <harness>`). This is a behavior change to the v0.2 §10 / §19 bootstrap flow; the plan approving it is the explicit direction. See §10, §19.
3. **Tool-count consistency fixed.** v0.2 enumerated ten agent-facing tools in §14.1 but one later line called them "the agent-facing nine." v0.3 says ten everywhere.
4. Everything else in v0.2 stands. No contract change to the substrate, governance, privacy, recall, or dreaming streams; the daemon socket protocol frame shape is unchanged.

**Sources:** `system-v0.1.md`, the shipped Stream A–F contracts, the long grilling session that locked v1 scope, name, license, harness tier policy, dogfood plan, and Stream I architecture.

**Non-source:** `docs/handoff-2026-04-23.md`, `docs/reference/handbook-v2.2.md`, and `docs/reference/gpt-deep-research-2026-04-23.md` are background. They are not normative.

**Working title locked:** `Memorum`. Latin: "of memory." See §22 for namespace clearance and rationale.

## Revision goal (v0.1 → v0.2)

v0.1 was a blue-sky design output. v0.2 is the v1 release contract. Streams A–F shipped under v0.1's design intent. v0.2 freezes the system shape for v1 release, names the product, drops one piece of v0.1 architecture that does not survive contact with how LLM agent harnesses actually run, and turns "open threads" into either "in v1" or "deferred to v2/v3" with no ambiguity in between.

Concretely:

1. **Product name locked: `Memorum`.** v0.1 §18.6 listed naming as an open thread. Closed.
2. **`memory_subscribe` is removed from the v1 MCP surface.** v0.1 §14.2 specified a long-lived streaming subscription tool. LLM agents only "think" inside the harness's generation loop; events streamed mid-generation have nowhere to go. The right primitive for cross-session awareness is the harness's existing pre-turn hook (already in production via Stream E's `memoryd recall delta-block`). v1 implements all three coordination levels via that hook plus per-call metadata. See §14.2 (replaced) and §15 (replaced).
3. **Cross-harness peer-update surface is specified.** v0.1 §15 sketched three sharing levels but did not lock the relevance gate. v0.2 §15 specifies the score function, threshold, recency window, per-turn cap, and the in-recall XML shape that delivers a peer's salient writes to a sibling agent without flooding context.
4. **Harness tier policy is explicit.** v1 ships full hook integration for Tier 1 (Claude Code, Codex CLI). All other harnesses get the raw MCP surface only — Tier 3 in v1. Tier 2 (rich-but-not-bespoke integrations: Factory Droid, OpenCode, Cursor, OpenClaw native) is the v2 scope, not v1. See §10.
5. **Anti-features are listed explicitly.** v0.1 §1 had a non-purpose list; v0.2 §1 expands it with six items the v1 release will refuse to add even if asked, plus rationale. This is durability-of-intent, not nuance.
6. **License: Apache 2.0.** v0.1 didn't pick one. v0.2 §21 explains why Apache over MIT (explicit patent grant; some of what Memorum does — drift-risk scoring, masked synthesis grounding rehydration, peer-update relevance gating — is patentable and the grant defangs that on day one).
7. **Versioning starts at SemVer 1.0.0.** Public release is 1.0.0 after the dogfood window. v1 is the durable contract; v1.x releases are bug-fixes and additive; v2.0.0 is the next breaking shape change. See §21.
8. **Implementation phasing replaced with status.** v0.1 §19 enumerated streams; v0.2 §19 records what shipped and what's left. Streams G/H/I are the only remaining v1 work. Stream J (open threads) is post-v1.
9. **Dogfood plan is part of the contract.** §20 specifies the 1-week multi-machine dogfood gate between code-complete and 1.0.0 release, including the recursive condition (Memorum tracks its own development; if it can't carry the project's own context across a week of multi-machine, multi-harness work, it isn't ready).
10. **MCP tool surface is updated to ten tools.** v0.1 listed seven agent-facing tools. Stream D added `memory_reveal` (eighth); Stream F added `memory_observe` (ninth) and left `memory_note` unchanged; source grounding added `memory_capture_source` (tenth). v0.2 §14.1 enumerates the live ten and forbids further tool-count creep in v1.
11. **Open threads are resolved.** v0.1 §18 listed six unresolved areas. v0.2 §18 records: prospective memory deferred to v2; eval harness is Stream H (in v1); bootstrap UX is in v1 (interactive wizard + flags + `--non-interactive`); policy migration is in v1 (additive only, no breaking schema changes); multi-user is post-v2; naming is closed.
12. **Reality Check is passive-only.** v0.1 §16.4 said "Slack/email reminder when due." v0.2 §16.4 makes the reminder Sunday morning Slack by default, with no active interruption — Reality Check surfaces silently in `memory status`, the next session's `<pending-attention>`, and the dashboard. Active OS notification is opt-in.
13. **Dream-output disposition.** v0.1 §12 said grounded promotion happens; it didn't say where the candidate lands. v0.2 §12 aligns with Stream F v0.3 §13: every grounded Pass 2 candidate enters the review queue, no silent auto-promotion in v1. A `review_min` floor (default 0.65) drops obviously-low-confidence candidates to the cleanup log instead of the queue. v1.1 may add silent promotion once dogfood produces calibration data — see §12.
14. **Drift-risk weights.** v0.1 §16.4 left `w1..w5` symbolic. v0.2 §16.4 picks: 0.35 staleness, 0.20 inverse-recall-frequency, 0.20 cross-source corroboration, 0.15 confidence decay, 0.10 sensitivity weight. These are dogfood-tunable but ship as defaults.
15. **Bootstrap CLI shape locked.** Interactive wizard by default; flag overrides for every prompt; `--non-interactive` for scripts; dry-run plan + `y/N` confirm before any disk effect; age-key backup blocks until user confirms they have it. See §18.3.

The substance of Stream A–F's design — files canonical, SQLite derived, Markdown+YAML frontmatter, three namespaces, governance gates, masked synthesis, three-layer dreaming, age-encrypted privacy boundary, harness-CLI delegation for LLM passes — is unchanged. v0.2 is the system frame around the shipped streams and the v1 release contract, not a re-design.

---

## 1. Purpose, non-purpose, and anti-features

### 1.1 Purpose

Memorum is a local-first, harness-agnostic, daemon-backed shared memory layer that works across Claude Code, Codex CLI, Cursor, OpenClaw, Factory, and any other MCP-speaking harness without forking, modifying, or wrapping the harness. It provides durable memory, passive recall, cross-harness coordination, and governance primitives as a single system the user runs once and every agent on the machine can talk to.

**Review-fix decision policy:** when review finds a gap between implementation and the live spec, the default is to fix the implementation. Spec amendments are valid only when the contract was technically wrong, unsafe, or explicitly deferred with user-visible behavior and tracking. See `docs/review-fix-policy.md`; review-fix commits that amend a contract must identify whether they are a code fix, spec correction, or explicit deferral.

**Spec amendment/versioning policy:** additive public-surface amendments may stay in-version with a dated amendment block when they add no new required behavior for existing callers. Behavior-changing changes, return-shape changes, removed or renamed surface, and new enforced invariants require a version bump unless Trey explicitly directs otherwise.

Concretely:

1. **One durable memory surface shared across every agent harness on the machine.** What Codex learns is immediately available to Claude Code; what Cursor notices is visible to both; the user never has to rebuild context across tools.
2. **Structured memory with real governance.** Candidate/promote, supersession chains, temporal validity, tombstones, sensitivity tiers, provenance chains. Shared memory without governance is a durable poisoning vector.
3. **Passive recall, not just queryable retrieval.** Rich in harnesses that support hooks (Tier 1); MCP-only fallback elsewhere (Tier 3). See §10.
4. **Cross-session coordination only a daemon can provide.** Peer sessions see each other's salient writes via a relevance-gated peer-update surface, without flooding context. Subagents can be shared instead of duplicated. Live investigation state crosses harness boundaries.
5. **Drift-fighting as a first-class feature.** Weekly Reality Check + active tombstones, not as aspirations.
6. **Local-first, user-owned, offline-capable.** Git-synced for multi-device. No cloud backend required.
7. **Recursive.** Memorum tracks its own development. Specs, plans, and reviews about Memorum are valid Memorum content; the project is its own first dogfood subject. Pass §20's dogfood gate by surviving its own multi-device, multi-harness use.

### 1.2 Non-purpose

1. **Not a replacement for in-harness memory systems.** OpenClaw's `memory-core` plugin and Claude Code's session-scoped memory still exist for session-scoped state; Memorum is the durable layer underneath and across them.
2. **Not a general-purpose vector DB.** SQLite + FTS + local embeddings are derived indexes over Markdown files; the files are canonical.
3. **Not a secret manager.** `secret`-tier content is refused at the substrate boundary (Stream A `WriteFailureKind::SecretRefused`). Use 1Password / `pass` / equivalent and reference by id.
4. **Not a scheduler.** Prospective memory surfaces commitments (post-v1); launchd/cron/systemd fire them.
5. **Not a multi-tenant SaaS.** Designed for a single user across their own devices and harnesses.
6. **Not an event bus.** v0.1's `memory_subscribe` is gone. Cross-session coordination is poll-based via the harness's pre-turn hook plus per-call metadata. Streaming push has no clean delivery path inside an LLM agent's generation loop.

### 1.3 Anti-features (v1 will refuse to add)

These are not "we haven't gotten to them." They are "we have decided not to do them in v1, and we will say no when asked."

1. **No long-lived streaming MCP surface.** No `memory_subscribe`, no `memory_watch`, no SSE, no WebSocket bridge. Coordination is poll-on-hook plus per-call metadata. If the harness can't poll, it doesn't get cross-session awareness in v1.
2. **No remote inference for daemon-internal LLM calls.** Daemon-internal LLM calls (governance contradiction tiebreak, dream passes) delegate to the user's already-installed harness CLI (`claude -p`, `codex exec`, …). Memorum never ships its own provider abstraction in v1, never holds a user's API key, never bills inference.
3. **No second-user multi-tenancy.** v1 is single-user-multi-device. Sharing memory across humans is its own design problem (privacy, principal model, ACLs, cross-human conflict resolution). Post-v2.
4. **No editor plugins.** v1 ships CLI, TUI, and a localhost web dashboard. No VSCode extension, no Cursor extension, no JetBrains plugin. Tier 1 harness integration is the surface for editor work.
5. **No cloud sync feature.** v1 sync is git only. Bring your own remote (GitHub, Gitea, GitLab, SSHFS file remote). Memorum's hosted offering (if any) is not a v1 product.
6. **No "memory marketplace" or shared knowledge packs.** Curated playbook/heuristic libraries are tempting and bad: provenance dilutes, governance becomes opaque, and the threat model breaks. v1 memory is what the user's agents wrote on the user's machines.

---

## 2. Design principles (inherited from the handbook, unchanged from v0.1)

These are load-bearing and non-negotiable in v1.

1. **Solve data quality at write time, not retrieval time.** Frontmatter is the foundation. Validator runs on every write. Structure isn't cosmetic; it's what makes every downstream layer cheap.
2. **Separate memory systems by access pattern.** Three namespaces (`me` / `project` / `agent`); within each, further subdivision by access pattern.
3. **Retrieval alone is not enough.** Passive recall is the thing that makes memory feel memoryful. Push-by-default via hooks where the harness supports it; pull-on-convention otherwise.
4. **Memory and compaction collaborate.** Memory reconstructs what compaction discarded. Memorum doesn't own compaction (the harness does), but compaction-aware writes and pre-compaction flush are first-class.
5. **Preserve state, not sludge.** Tool outputs, browser snapshots, giant logs are artifacted elsewhere. Memory holds state, not payloads.
6. **Identity context matters and is tri-partite.** Stable role / operating principles / relationship facts each get different review cadences and write gates.
7. **Maintenance ≠ synthesis.** Cleanup is janitorial. Dreaming is cognitive. Three-layer pipeline (substrate / journal / cleanup) keeps them distinct.
8. **Untrusted inputs propose; they don't promote.** No exceptions. Promotion is gated by deterministic policy.
9. **Cache stability is architectural.** Passive recall is assembled to preserve prefix caching; dynamic content goes in the suffix.
10. **Every memory has an evaluation story.** Eval harness is built from day one (Stream H), not retrofitted.

---

## 3. High-level architecture

```
  ┌─────────────────────────────────────────────────────────────────┐
  │                    User's Devices (each runs)                    │
  │                                                                   │
  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐            │
  │  │ Claude Code  │  │  Codex CLI   │  │  Any MCP     │  ...       │
  │  │   session    │  │   session    │  │   harness    │            │
  │  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘            │
  │         │                 │                 │                     │
  │   MCP + hooks       MCP + hooks         MCP only                  │
  │   (Tier 1)          (Tier 1)            (Tier 3)                  │
  │         │                 │                 │                     │
  │         └─────────────────┼─────────────────┘                     │
  │                           │                                        │
  │                  ┌────────▼────────┐                              │
  │                  │     memoryd     │  (single local daemon)       │
  │                  │                 │                               │
  │                  │  ┌───────────┐  │                              │
  │                  │  │ Governance│  │  (Stream C, shipped)         │
  │                  │  └───────────┘  │                              │
  │                  │  ┌───────────┐  │                              │
  │                  │  │ Privacy   │  │  (Stream D, shipped)         │
  │                  │  │ Filter    │  │                              │
  │                  │  └───────────┘  │                              │
  │                  │  ┌───────────┐  │                              │
  │                  │  │ Indexer   │──┼──► SQLite (derived)          │
  │                  │  │ + Embedder│  │  (Stream A, shipped)         │
  │                  │  └───────────┘  │                              │
  │                  │  ┌───────────┐  │                              │
  │                  │  │ Dreaming  │  │  (Stream F, shipped)         │
  │                  │  │ (3-layer) │  │  delegates LLM to harness CLI│
  │                  │  └───────────┘  │                              │
  │                  │  ┌───────────┐  │                              │
  │                  │  │ Recall    │  │  (Stream E, shipped)         │
  │                  │  │ assembly  │  │  + (Stream I, in v1)         │
  │                  │  │ + peer    │  │     peer-update relevance    │
  │                  │  │ updates   │  │     gate                     │
  │                  │  └───────────┘  │                              │
  │                  │  ┌───────────┐  │                              │
  │                  │  │ Event log │──┼──► JSONL (per-device)         │
  │                  │  │ + Sync    │  │  (Stream A, shipped)         │
  │                  │  └───────────┘  │                              │
  │                  │  ┌───────────┐  │                              │
  │                  │  │ TUI + Web │  │  (Stream G, in v1)           │
  │                  │  │ dashboard │  │                              │
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

Key properties (unchanged from v0.1, validated by what shipped):

- **Daemon is the single point of write.** All MCP calls route through `memoryd`. No harness writes directly to files. This guarantees every write runs through governance, Privacy Filter, contradiction detection, and event log append.
- **Files are canonical.** SQLite is derived; event log is durable audit; everything else is reconstructable from the Markdown tree.
- **Git is the sync transport.** Commits are fine-grained; a custom frontmatter merge driver handles semantic merges.
- **The harness is untouched.** MCP server and hook scripts are the only integration surface. No forks, no wrappers, no per-harness daemons.

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

Unchanged from v0.1 §4 and shipped per Stream A v1.1 §3 + Stream B's `RequestPayload::Startup` handler.

---

## 5. Process architecture

**Single local daemon, `memoryd`.**

- Auto-starts at login via `launchd` (macOS) / `systemd` (Linux) / `NSSM` (Windows; v1 platform support is macOS + Linux only — Windows is v2).
- Unix socket at `$XDG_RUNTIME_DIR/memoryd.sock` (`~/.memoryd/socket` fallback on macOS). Owner-only chmod after bind (Stream D §13.x).
- MCP servers (one per connecting harness session) are thin clients: they expose the MCP protocol to the harness, forward calls over the socket to `memoryd`, return responses. The shipped stdio entrypoint is `memoryd mcp --socket <socket_path>`, which keeps stdout reserved for newline-delimited JSON-RPC protocol frames and writes diagnostics to stderr.
- `memoryd` owns:
  - File watcher on the memory tree (notify on Linux/macOS).
  - SQLite writer (single-writer model; no lock contention).
  - Embedding worker (background; configurable provider, defaults specified by Stream A). Two lanes: the default local `fastembed-candle` provider (all sensitivity tiers), and an opt-in `gemini-api` provider (`gemini-embedding-2`) behind a privacy fence — API lanes embed only persisted `public`/`internal` content, require an explicit `api_embedding_consent: true` recorded by the `memoryd config embedding-lane` consent ceremony, and hold everything else local. Contract: Stream A spec v1.1, Amendment 2026-07-09; operator runbook `docs/runbooks/api-embedding-lane.md`.
  - Indexer (chunk + embed + insert on file change, debounced).
  - Privacy classification (Layer 1 deterministic; ONNX Layer 2 deferred per Stream D v0.1).
  - Dreaming scheduler (three-layer pipeline, leased; harness-CLI-delegated LLM calls per Stream F v0.3).
  - Sync manager (git fetch/push, merge driver).
  - Event log appender.
  - Policy loader and validator.
  - Review queue, reality-check scheduler, notification dispatcher.
  - Recall assembly (startup + delta) and peer-update relevance gate (Stream I).
  - Local web dashboard (opt-in, `http://localhost:7137`, Stream G).
  - TUI rendering (`memoryd ui`, Stream G).

**Upgrade path:** daemon supports graceful reload. New daemon binary starts on a new socket path; clients reconnect on version-mismatch signal; old daemon drains and exits.

**Current alpha MCP startup:** operators start `memoryd serve` first, then MCP clients run the stdio bridge `memoryd mcp --socket <socket_path>`. Lazy-start fallback remains a release-target behavior unless implemented and covered by the MCP bridge tests.

Unchanged from v0.1 §5 and validated by Stream B's shipped supervisor (`serve_substrate_with(socket, substrate, options, shutdown_rx)`).

---

## 6. Storage substrate

Unchanged in shape from v0.1 §6 and shipped per Stream A v1.1.

**Canonical form:** Markdown + YAML frontmatter, on disk.

**Derived index:** SQLite at `~/.memory/index.sqlite`. Device-local. Never synced. Rebuilt from files on demand (`memoryd reindex`). FTS5 for keyword; sqlite-vec for vectors.

**Event log:** per-device JSONL at `~/.memory/events/<device-id>.jsonl`. Append-only. Synced via git (separate files per device means no merge conflicts in the log itself — merges are concatenations).

**Memory tree root:** `~/.memory/` (git repo). Full tree as v0.1 §6, plus the Stream F additions:

```
~/.memory/
├── .git/                              # history, sync transport
├── .memory-project.yaml               # optional: override project binding
├── me/
│   ├── identity/
│   │   ├── role.md
│   │   └── principles.md
│   ├── relationship/
│   │   ├── facts/<entity>.md
│   │   ├── preferences/
│   │   ├── corrections/<id>.md
│   │   └── patterns/
│   ├── knowledge/<topic>.md
│   ├── episodic/YYYY-MM-DD.md
│   └── prospective/<id>.md            # post-v1 surface
│
├── projects/
│   └── <namespace>/
│       ├── state.md
│       ├── decisions/YYYY-MM-DD-<slug>.md
│       ├── open-questions/<id>.md
│       ├── playbooks/<name>.md
│       ├── entities/<entity>.md
│       ├── episodic/YYYY-MM-DD.md
│       ├── invariants.md
│       └── regressions/<id>.md
│
├── agent/
│   ├── patterns/<id>.md
│   ├── playbooks/<name>.md
│   ├── postmortems/<id>.md
│   ├── anti-patterns/<id>.md
│   ├── heuristics/<id>.md
│   ├── regressions/<id>.md
│   └── episodic/YYYY-MM-DD.md
│
├── dreams/
│   ├── journal/<scope_path>/YYYY-MM-DD.md      # Pass 1; not canonical, not indexed
│   ├── questions/<scope_path>/YYYY-MM-DD.jsonl # Pass 3; entity-bearing
│   └── cleanup/<device_id>/YYYY-MM-DD.json     # cleanup run reports
│
├── substrate/
│   └── <device_id>/YYYY-MM-DD.jsonl   # per-device substrate fragments
│
├── encrypted/
│   ├── <namespace>/...                 # age-encrypted parallel tree (Stream D)
│   └── substrate/<device_id>/YYYY-MM-DD.jsonl  # encrypted substrate
│
├── tombstones/
│   └── YYYY-MM-DD.jsonl                # active deletion rules
│
├── events/
│   └── <device_id>.jsonl               # per-device event log
│
├── policies/
│   ├── me-strict.yaml
│   ├── project-standard.yaml
│   ├── agent-strict.yaml
│   └── dreaming-strict.yaml
│
├── leases/
│   └── journal.lease                   # Stream F leased journal lock
│
└── config.yaml                         # daemon config
```

Index file (`~/.memory/index.sqlite`) and daemon runtime files live outside the git repo (`~/.memoryd/`).

---

## 7. Frontmatter schema

Unchanged from v0.1 §7. Shipped per Stream A v1.1 §7 (validator at the daemon boundary; missing required fields = refuse write with structured error). v0.2 adds no new frontmatter fields. Schema evolution policy: additive-only in v1.x; new required fields = breaking change = wait for v2.0.0.

---

## 8. Identity and scope resolution

Unchanged from v0.1 §8. Shipped per Stream A v1.1 §8 and Stream B's session-binding handler.

---

## 9. Multi-device sync

Unchanged from v0.1 §9 in shape. Shipped per Stream A v1.1 §13 (two-clone convergence as canonical-content equality, not raw `git diff`) and §14 (merge driver). v0.2 adds:

- **Auto-commit policy** is unchanged: 30-second debounce, structured commit message.
- **Daemon-authored commits** (Stream F lease + cleanup writes; Stream I peer-update metadata writes if any) follow the conventions specified in `stream-f-dreaming-v0.3.md` §1.1: explicit author identity (`memoryd lease-bot` / `memoryd cleanup-bot`), structured message prefix, dirty-tree handling.
- **`memoryd-version` trailer** on every auto-commit for debuggability.

---

## 10. Passive recall — harness tier policy

v0.1 §10 enumerated per-harness capabilities case-by-case. v0.2 normalizes that into three tiers with explicit v1 / v2 / v3 ownership.

### 10.1 Tier 1 — Hooks + skill/CLI (v1)

Harnesses with a shell and both **session-start** and **pre-turn** hooks, where Memorum can (a) inject recall content into the agent's context window before the turn, and (b) let the agent drive the store through the `memoryd` CLI.

**v1 Tier 1: Claude Code, Codex CLI.**

The Tier-1 active surface is the **`memoryd` CLI plus the `using-memorum` skill**, not the MCP bridge. The CLI composes with pipes and scripts, loads zero schema tokens into sessions that never touch memory, works identically across every shell-capable harness, and pairs with a skill that carries judgment a tool schema cannot ("when is something worth a governed write," "search before writing"). It speaks the v1 agent envelope + exit-code contract (`docs/api/memoryd-cli-contract-v1.md`). The MCP bridge (§14) remains available as an opt-in compatibility surface but is not wired by default.

Passive-recall integration surface (wired by default):

- `memoryd recall startup-block --harness <name>` emits the `<memory-recall version="stream-e-v0.5">` XML block on stdout. Wired to the harness's SessionStart hook.
- `memoryd recall delta-block --harness <name>` emits `<memory-delta>` (or `<memory-delta empty="true" />` for no-match). Wired to the harness's UserPromptSubmit / pre-turn hook.
- Both blocks include Stream I peer-update insertions when the relevance gate fires (§15).
- Recall counters update in `memory status` for observability.

These two hooks are sufficient to deliver: session-startup recall, per-turn delta recall, pending-attention surfacing, and cross-session peer updates. No per-harness business logic in `memoryd` — the harness invokes `memoryd recall …` via a configured hook and consumes the rendered block verbatim.

### 10.2 Tier 3 — Raw MCP only (v1)

Harnesses that speak MCP but do not expose hooks Memorum can wire into. Examples in v1: Cursor, Factory Droid (today), OpenCode (today), OpenClaw native (today), Cline, Continue, anything else MCP-compliant.

Integration surface:

- All ten MCP tools (§14.1) work normally.
- The agent must call `memory_search` / `memory_get` / `memory_startup` itself to retrieve recall content. Memorum does not push to it.
- Cross-session peer updates: not delivered (no hook to inject into).

This is "Memorum works on day one with any MCP-speaking harness, but the experience is poll-only." It's the floor, not the goal. Tier 3 in v1 is what makes the OSS release land: install the daemon, point the harness's MCP config at it, get governed durable memory across sessions even without hook integration.

### 10.3 Tier 2 — Rich, non-bespoke integrations (v2 follow-up)

Harnesses that support hooks but where the integration is harness-specific enough to need its own glue. v2 elevations:

- **Factory / Droid** — native rule + hook system; Memorum integration probably looks like a Droid plugin.
- **OpenCode** — has a hook surface emerging; v2 will implement.
- **Cursor** — rule files + extension hooks; v2 promotion target.
- **OpenClaw** — has its own `memory-core` plugin and dreaming concept; the v2 integration needs to coexist (Memorum is the durable layer underneath, OpenClaw's `memory-core` is the session layer).

v2 work plan: ship Tier 2 promotions for these four harnesses as additive feature releases (1.1.0, 1.2.0, …). No spec changes required; each is a hook adapter on the integration side.

### 10.4 Beyond v2 (v3+)

Anything else that wants Tier 2 status follows the same playbook: a hook adapter, no daemon changes. Long tail. Not in any release plan.

---

## 11. Governance — A′ architecture

Unchanged from v0.1 §11. Shipped per Stream C v0.1 (`crates/memory-governance/`):

- §11.1 Machinery (unconditional, every write): grounding verification, contradiction detection, tombstone matching, supersession chain check, sensitivity policy gate.
- §11.2 Policy files (per namespace): `me-strict.yaml`, `project-standard.yaml`, `agent-strict.yaml`, `dreaming-strict.yaml`. Loaded with fail-closed validation. Built-in policies only when no policy YAML exists.
- §11.3 Human-review gate: `requires_user_confirmation: true` paths refuse writes until approved via `memoryd review`.
- §11.4 Subagent writes: attribution-only. Authority unchanged.

v0.2 additions:

- **Contradiction tiebreak provider remains an open trait.** Stream C ships the `ContradictionTiebreaker` trait; no production tiebreaker is wired in v1. Conflict resolution falls back to deterministic policy (newer + higher-confidence wins; ties → quarantine). Whether the v1.x tiebreaker delegates to harness CLI like Stream F's dream passes is an explicit follow-up tracked in §18, not v1 work.
- **Dream auto-promotion thresholds (added to §12.2 below).** Stream F ships grounded promotion; v0.2 specifies the confidence boundary handling.

---

## 12. Dreaming — three-layer pipeline

Shipped per Stream F v0.3. Substrate / journal / cleanup layers as specified there, with harness-CLI-delegated LLM calls.

**v1 dream-output disposition: review queue only, no silent auto-promotion.** Stream F v0.3 §13 explicitly defers Pass 2 auto-promotion ("Pass 2 auto-promotion … is explicitly out of scope"). Every grounded Pass 2 candidate enters `pending` review with reason `dream_pass_2_candidate` (or `dream_pass_2_low_confidence` if the candidate's self-reported confidence is below `dreaming.review_min` and the candidate is logged-and-dropped instead). The user batches through accept / reject / edit in the TUI or web dashboard.

The default config exposes one tunable boundary — the floor below which Pass 2 outputs are dropped without being shown:

```yaml
dreaming:
  review_min: 0.65   # candidates below this are dropped + logged, not queued
```

Candidates with confidence `< review_min` are dropped to `dreams/cleanup/<device_id>/<date>.json` in the `dropped_low_confidence` array with `{candidate_id, confidence, reasons[]}` and can be re-surfaced via `memoryd dream review --include-dropped`. There is no `silent_min` and no silent-promotion path in v1.

**Why no auto-promotion in v1.** Dream-pass confidence is a model self-report and its calibration vs. ground-truth correctness is unverified. Auto-promotion silently writes potentially-false memory the user has no signal to catch. The dogfood week (§20) collects the calibration data — confidence-vs-was-actually-correct on a per-candidate basis from review decisions — that would justify a v1.1 silent-promotion path. Until then, every grounded dream candidate gets a human eye on it.

Other Stream F mechanics (lease semantics, scope path encoding, scheduled-vs-manual lease handling, Pass 3 entity sidecar, masked synthesis, harness-CLI status surfacing) are unchanged from `stream-f-dreaming-v0.3.md`.

---

## 13. Secrets and Privacy Filter

Shipped per Stream D v0.1 (`crates/memory-privacy/`). Unchanged in v0.2:

- Layer 1 deterministic classifier (regex + entropy) is the live path.
- Layer 2 ONNX OpenAI Privacy Filter remains opt-in / deferred — the trait exists, no production wiring in v1.
- Layer 3 age-X25519 encryption for `RequiresEncryption` writes.
- Layer 4 commit hook (gitleaks-style) fires on every commit, blocks pushes containing detected secrets.
- `secret`-tier writes are refused at the substrate boundary (`WriteFailureKind::SecretRefused`).
- Per-tier storage routing: URL/date stay plaintext; phone/email/address/person/account encrypt at rest without `Personal` tier elevation; SSN/Luhn-valid card/credential-like labels refuse before disk.
- `memory_reveal` is the audited unmask surface (8th MCP tool, Stream D).
- Masked synthesis views: dream Pass 2 prompts run on Stream D-masked text; restoration on Pass 2 candidate write-back only.

---

## 14. MCP tool surface

### 14.1 Agent-facing — exactly ten tools (frozen for v1)

The v1 contract is ten MCP tools. No further tool addition or removal in v1.x. New tools require v2.0.0.

**2026-05-07 amendment:** v1 MCP surface ratified at 10 tools (adds `memory_capture_source`, shipped 2026-05-06 in commit `ab66a34`). Surface frozen at 10 for v1.x. Daemon-protocol commands (`Status`, `Doctor`, `RealityCheck`, peer admin, etc.) are not part of the MCP surface and are exposed via the daemon socket only.

**2026-07-08 (v0.3):** the MCP bridge is now an **opt-in compatibility surface**, not the default agent transport. The ten tools below are unchanged and stay frozen; the bridge still forwards verbatim to the daemon socket. What changed is wiring: `memoryd init` no longer wires MCP by default (it wires passive-recall hooks instead), and the CLI + `using-memorum` skill is the Tier-1 active surface (§10.1). Wire the bridge explicitly with `memoryd init --wire-mcp <harness>` for a harness that needs it.

**2026-05-25 alpha hardening note:** the canonical library manifest remains the
ten-tool v1 contract, but the shipped `memoryd mcp` stdio bridge hides
`memory_reveal` unless launched with `--allow-reveal`. This narrows normal
dogfood sessions without changing the explicit reveal contract for
user-authorized encrypted-content access.

| # | Tool | Stream | Purpose |
|---|---|---|---|
| 1 | `memory_search` | A/B | hybrid keyword + vector + recency search across in-scope namespaces |
| 2 | `memory_get` | A/B | fetch one memory by id, optionally with provenance |
| 3 | `memory_write` | C | governed write with full meta; returns promoted/candidate/quarantined/refused |
| 4 | `memory_supersede` | C | replace an existing memory with a new one; chains supersession |
| 5 | `memory_forget` | C | tombstone; may require user confirmation |
| 6 | `memory_startup` | E | session binding + initial recall block |
| 7 | `memory_note` | A/B | substrate-layer write; cheap, no governance gates beyond Privacy Filter; feeds dreaming |
| 8 | `memory_reveal` | D | audited unmask of an encrypted memory; bounded reason validation |
| 9 | `memory_observe` | F | dream-substrate fragment write; entity-bearing; never becomes a canonical memory directly |
| 10 | `memory_capture_source` | Source grounding | capture supported deterministic sources (`http_static`, `local_artifact`) as local verified `webcap:` artifacts; richer browser/PDF/auth modes are typed unsupported in alpha |

Schemas as documented in:

- `docs/api/stream-a-public-api.md` (substrate types)
- `docs/api/stream-c-governance-api.md` (`memory_write`, `memory_supersede`, `memory_forget`)
- `docs/api/stream-d-privacy-api.md` (`memory_reveal`)
- `docs/api/stream-e-passive-recall-api.md` (`memory_startup`)
- `docs/api/stream-f-dreaming-api.md` (`memory_observe`)
- `docs/api/web-source-grounding-api.md` (`memory_capture_source`)

### 14.2 Event subscription — REMOVED in v0.2

v0.1 §14.2 specified `memory_subscribe` as a long-lived streaming MCP tool. **Removed in v0.2.**

Rationale: an LLM agent only "thinks" inside the harness's generation loop. Events streamed mid-generation have no consumer — the agent isn't running, and even if it were, mid-token-generation injection has no architectural surface in any current harness (Anthropic, OpenAI, Anthropic via OpenClaw, Cursor's wrapper, Factory's wrapper, none of them).

The right primitive for cross-session awareness is the harness's existing pre-turn hook (Tier 1: Stream E's `memoryd recall delta-block`, already shipping). For Tier 3 (raw MCP), the agent polls `memory_search` / `memory_startup` itself if it cares.

If a v2 use case emerges where streaming push genuinely makes sense (e.g., a TUI watching live state, or a non-LLM consumer), it goes through a different surface — likely a Server-Sent Events endpoint on the daemon's localhost HTTP port — not as an MCP tool. That's v2 work.

### 14.3 Admin surface (CLI + slash commands, NOT MCP)

CLI commands (Tier-agnostic; runs anywhere `memoryd` is installed):

```
memoryd status
memoryd review [--namespace X] [--quarantined] [--include-dropped]
memoryd diff --since 7d
memoryd audit <id>
memoryd lint
memoryd conflicts
memoryd rollback <id> --to-version N
memoryd pin <id>
memoryd unpin <id>
memoryd export <filter>
memoryd policy show|edit|test
memoryd health
memoryd doctor
memoryd reality-check [run|skip|snooze]
memoryd dream {status,now,review,enable,disable}
memoryd sync [--now]
memoryd reindex
memoryd device {onboard,rotate-keys,revoke}
memoryd privacy {scan,classify,decisions}
memoryd privacy-filter {enable,disable,status}
memoryd recall {startup-block,delta-block}     # Tier 1 hook entry points
memoryd peer {status,activity}                 # Stream I surfaces
memoryd serve --init                            # current alpha bootstrap
# memoryd init                                  # release-target bootstrap wizard, not current alpha
memoryd ui                                      # Stream G TUI
memoryd web {enable,disable,status}             # Stream G dashboard
memoryd {start,stop,restart,reload,logs}
```

Plus harness-specific slash commands (Tier 1 only):

```
# Claude Code, Codex CLI
/memory-status
/memory-review
/memory-pin
/memory-forget
/memory-reality-check
/memory-conflicts
```

**Admin commands are explicitly rejected from MCP** (Stream B + D). An agent cannot call `memoryd privacy decisions` via MCP. This is a hard boundary: agents see the agent-facing ten; humans run the admin surface.

---

## 15. Live cross-session coordination — Stream I architecture

Replaces v0.1 §15. The shape changed substantially when we dropped `memory_subscribe`.

### 15.1 The mental model

Two scenarios drive the design:

1. **You are working on a TUI in Claude Code while Codex is fixing database bugs in the same project.** When Codex writes a memory that affects your TUI work (a schema change, a tooling decision, an infrastructure invariant), you want to find out — but token-efficiently and with clear attribution, not "the user told me X."
2. **You wake up in Claude Code on Tuesday.** Yesterday on the laptop you (via Codex) decided something. You need to know that today, in this session, on the desktop. Same problem, different timescale.

Both are the same primitive: **a sibling agent wrote something relevant; surface it before the next turn, gated by relevance, with clear "this came from elsewhere" framing.**

### 15.2 Three levels — all poll-based, all hook-delivered

Same three-level taxonomy as v0.1, all delivered via the existing pre-turn hook (`memoryd recall delta-block`) with no streaming:

**Level 1 — writes only.** Sessions see peers' promoted memories on the next recall refresh. The default. Shipped via Stream E (promoted memories appear in `<memory-recall>` and `<memory-delta>` blocks on entity / topic match). No Stream I work needed for Level 1.

**Level 2 — writes + candidates + notes (Stream I default).** Sessions see in-flight proposals, substrate notes, and `memory_observe` fragments from peers, surfaced in the recall delta block when the relevance gate fires. This is what Stream I implements.

**Level 3 — presence + intent (opt-in, project-configurable).** Sessions broadcast "I'm working on entity X." Daemon surfaces "another session is also touching X" in recall. Claim locks (memory under revision) prevent stale-truth reliance. Configurable per project: `concurrent_session_mode: collaborative` in `.memory-project.yaml`.

**Tier scope.** All three levels' active surfacing — peer-update items, peer-presence elements, claim-lock annotations — flow through the `memoryd recall delta-block` hook. Tier 1 harnesses (Claude Code, Codex CLI per §10.1) receive them every turn. **Tier 3 harnesses do not call the hook**; their cross-session awareness is whatever surfaces through normal `memory_search` and `memory_get` MCP calls picking up peers' promoted memories on next read. There is no peer-update relevance gating, no peer-presence visibility, and no claim-lock annotation for Tier 3 in v1. Tier 3 sessions also do not *contribute* to peer-presence — Level 3 presence requires the heartbeat worker which is owned by the daemon side of the hook integration. This is consistent with Tier 3's explicit positioning in §10.2 as "raw MCP only, intentional minimum viable."

### 15.3 The peer-update relevance gate

The hard part is filtering. Without a gate, every peer write floods every other session's context. With too strict a gate, the feature might as well not exist.

**Score function** (computed per peer-write candidate per active recall assembly):

```
score(peer_write, current_session) =
    0.5 * entity_overlap(peer_write.entities, current_session.salient_entities)
  + 0.3 * path_overlap(peer_write.paths, current_session.salient_paths)
  + 0.2 * topic_similarity(peer_write.summary_embedding, current_session.recent_query_embedding)
```

Where:

- `entity_overlap` is Jaccard similarity over entity ids, in `[0, 1]`.
- `path_overlap` is the fraction of `peer_write.paths` (file paths or namespace paths the write touches) that intersect the current session's `salient_paths` (computed from the session's recent file mentions, project state, and recall block contents). In `[0, 1]`.
- `topic_similarity` is cosine similarity between the peer write's summary embedding and the current session's recent-query embedding. In `[0, 1]`.

**Threshold:** `score ≥ 0.6` to surface.

**Property: entity overlap is a *necessary* condition for surface.** With weights `(0.5, 0.3, 0.2)` and threshold `0.6`, the maximum achievable score with `entity_overlap = 0` is `0.3 + 0.2 = 0.5`, below threshold. This is intentional precision-first design: peer-update is a high-prominence, low-frequency surface (cap of 2/turn, embedded conspicuously in the delta block), and a peer write that shares no entities with the current session is almost always noise from the agent's perspective. Path-only and topic-only "near-misses" are still discoverable through normal `memory_search`. v1.1 may revisit (a disjunctive trigger like `entity_jaccard ≥ 0.3 OR (path_jaccard ≥ 0.5 AND topic ≥ 0.5)`) once dogfood produces evidence of genuinely-missed cross-session signals.

**Recency window:** 30 minutes, measured from **sync-arrival** (the timestamp this device first observed the peer write in its index, i.e. `local_observed_at`), not from the peer's wall-clock write timestamp. The semantic question is "did the agent recently learn about this?" not "was this written recently?" — using the peer's authored time produces silent drops on slow cross-device syncs (e.g. Device A offline for 90 minutes, then reconnects: peer writes are 90 min old by wall-clock but 0 min old by the device's own observation). Peer writes whose `local_observed_at` is older than 30 minutes never surface as "live updates" — they appear in normal recall via Stream E if they remain relevant.

**Per-turn cap:** 2 peer-update insertions max. If more than 2 candidates pass the threshold, surface the top 2 by score; the rest are dropped (counted in `<pending-attention>` for completeness, but not embedded as full peer-update items).

**Cool-down:** the same peer-write isn't surfaced twice to the same session. Once a session has seen it via peer-update, future deliveries fall back to normal recall on entity match.

### 15.4 The on-wire shape

Peer-updates are embedded inside the existing `<memory-delta>` block, so Tier 1 harnesses pick them up via the same hook (`memoryd recall delta-block`) without any new integration:

```xml
<memory-delta>
  <peer-update from="codex" session="abc1234" ts="15:23" relevance="0.84">
    <summary>Migrated `users.email` from VARCHAR(255) to CITEXT in atlasos. Tooling assumes CITEXT now.</summary>
    <ref>mem_20260501_021</ref>
    <namespace>project:proj_a3f2</namespace>
  </peer-update>
  <!-- normal recall delta items -->
  <memory ref="mem_..."> ... </memory>
</memory-delta>
```

`relevance` is the score, rounded to 2 decimals, for the agent's information (not a directive).

The `from`, `session`, and `ts` framing is load-bearing: agents must not confuse a peer-update with a user message. Stream I §3 (in `stream-i-cross-session-v0.1.md`) specifies the exact framing language and the test suite that asserts agents distinguish correctly under sampling.

### 15.5 Presence and claim-lock semantics (Level 3)

**Presence heartbeat:** sessions configured for Level 3 send a heartbeat every 60s containing `{session_id, harness, namespace, salient_entities, salient_paths, started_at}`. Daemon caches in memory (not persisted; presence does not survive daemon restart). Missed heartbeats for 5 minutes = daemon marks session stale; presence events clear.

**Claim lock:** when a session opens `memory_supersede` workflow on a memory, the daemon emits a claim-lock metadata flag for that `memory_id` (TTL: 5 minutes; renewable; cleared on supersede completion or session end). Other sessions reading that memory in their recall block see `claim_locked: { holder: "claude-code:sess_def567" }`.

**Presence projection in recall:** when Level 3 is enabled, the recall delta block carries a `<peer-presence>` element listing other live sessions touching salient entities:

```xml
<memory-delta>
  <peer-presence>
    <session harness="codex" id="def567" entities="ent_users_table,ent_atlasos" started="14:02" />
  </peer-presence>
  <peer-update from="codex" ... />
</memory-delta>
```

Per-turn cap: 4 presence entries; further sessions counted in `<pending-attention>`.

### 15.6 Shared substrate pool

All sessions on the same device-user-project write to the same `substrate/<device_id>/YYYY-MM-DD.jsonl`. No segregation by harness or session. Tagged with `harness` and `session_id` for attribution. The journal pass reads the combined pool — this is how cross-harness pattern recognition works at the synthesis layer (Stream F).

### 15.7 What Stream I delivers in v1

- The relevance gate implementation (score function, threshold, recency window, per-turn cap, cool-down).
- The `<peer-update>` and `<peer-presence>` block additions to the recall delta XML.
- Level 2 default-on (with per-namespace opt-out via `.memory-project.yaml`'s `concurrent_session_mode: minimal`).
- Level 3 opt-in via `.memory-project.yaml`'s `concurrent_session_mode: collaborative`.
- Tests asserting agents reliably frame peer-updates as third-party (not user input) under several sampling temperatures.

What Stream I does NOT deliver in v1:

- Cross-device live presence (presence is per-device; cross-device awareness is post-pull only).
- Conflict-prevention beyond claim-lock (no operational transform, no real-time merge).
- Multi-session collaborative editing of a single memory (use supersede chains).

Full contract in `stream-i-cross-session-v0.1.md`.

---

## 16. Observability — Stream G surfaces

Replaces v0.1 §16. Locked panel layouts, dashboard sections, and notification policy.

### 16.1 CLI

Already enumerated in §14.3. Token-efficient output, scriptable, stays in terminal.

### 16.2 TUI (`memoryd ui`)

`ratatui`-based terminal UI in the `lazygit` / `k9s` style. Eight panels, toggleable with number keys 1–8:

1. **Overview** — daemon health, pending review count, conflicts, sync lag, active sessions, dreaming run status.
2. **Review queue** — quarantined / pending items including dream-low-confidence promotions; `j/k` navigate, `a/r/f/q` approve/reject/forget/quarantine, `e` edit.
3. **Conflicts** — side-by-side merge conflict resolver; field-level accept/reject; `q` send to quarantine.
4. **Entities** — `/entity-name` search; see all memories attached, supersession chains, recall history.
5. **Timeline** — scrollable event feed with filter controls.
6. **Namespace explorer** — tree view of `me/` `project/` `agent/`; inspect any memory; quick-jump from search results.
7. **Policy inspector** — active policies, recent decisions, refusal reasons, policy editor escape (drops to `$EDITOR`).
8. **Reality check** — launch / snooze / browse the weekly Reality Check ritual; see drift-score breakdown per memory.

Keyboard-first, zero mouse. Renders in any terminal. Full keymap in `stream-g-observability-v0.1.md`.

### 16.3 Local web dashboard (opt-in)

`memoryd web enable` starts HTTP server on `localhost:7137`. Browser is localhost-only; no external network exposure by default. Opt-in remote access (SSH tunnel recommended, not built-in port exposure).

v1 ships **four of six** sections from v0.1's dashboard concept:

1. **Entity graph** — force-directed visualization of entity relationships; click to explore; supersession chains rendered as temporal edges.
2. **Synthesis ROI dashboard** — over 30/90/365 days: promotion rate, promotion precision (recall-after-promote), refusal breakdown, dreaming value metrics.
3. **Reality-check UI** — swipe/click through items more ergonomically than TUI; same underlying queue.
4. **Audit explorer** — walk provenance graphs visually; time-scrub temporal validity.

**Deferred to v1.x or v2:**

5. **Policy editor** — syntax-highlighted YAML editing with live validation and dry-run. Useful but not essential; v1 users edit via `$EDITOR` on `policies/*.yaml` directly.
6. **Sync status** — which devices have what; lease state; commit history. Useful but TUI panel #1 covers daemon health, and `git log` covers commits.

### 16.4 Weekly Reality Check

**Schedule:** Sunday morning, configurable. **Passive default — no active interruption.** Slack reminder via configured webhook is the default delivery; no OS notification unless explicitly opted in.

**Algorithm — drift-risk scoring with v0.2-locked weights:**

```
score(m) = 0.35 * days_since_observed_norm(m)
        + 0.20 * (1 - recall_frequency_norm(m))
        + 0.20 * (1 - cross_source_corroboration(m))
        + 0.15 * confidence_decay(m)
        + 0.10 * sensitivity_weight(m)
```

Where:

- `days_since_observed_norm(m)` is `min(1, days_since_observed / 90)` — saturates at 90 days.
- `days_since_observed_norm(m)` is `min(1, days_since_observed / 90)` — saturates at 90 days. Source: `memories.observed_at` (already in shipped index).
- `recall_frequency_norm(m)` is `recall_count_30d(m) / max(recall_count_30d across active memories, 1)`. **Data source: derived at score time via SQL query against the `events_log` SQLite table** (a Stream A surface addition introduced in v0.2 to back this drift score and future telemetry — see §19's cross-stream surface authorization table). The shipped Stream A event log is per-device JSONL on disk; v0.2 adds a SQLite mirror table `events_log` as a derived projection with backfill on first migration and dual-write on every event emission. The covering index `events_log(kind, memory_id, ts)` keeps the per-memory query at sub-millisecond. The JSONL files remain canonical; SQLite is rebuildable by `memoryd doctor --reindex`.
- `cross_source_corroboration(m)` is `1` if at least 2 distinct `source.harness` values exist across the memory and its supersession ancestors, else `0`. Source: `memories.source_harness` (already in shipped index) joined recursively through the `memory_supersession(memory_id, supersedes_id)` join table — a new v0.2 derived projection added to Stream A's index alongside `events_log`, backfilled from each `Frontmatter.supersedes` array and synced on every write through the existing `sync_auxiliary_tables` path (the project's own doc-comment listed this table as deferred from initial Stream A; v0.2 promotes it to shipped because drift scoring depends on it). The recursive walk uses an explicitly bounded CTE (`WHERE depth < 8`) which doubles as the cycle guard. **There is no `memories.supersedes_ids` column** — references to such a column in earlier drafts of this spec were a fiction; the join table replaces it. **NULL `source_harness` is excluded from the distinct count** by SQL convention. This is intentional: `Source.harness` is `Option<String>` in the shipped model, NULL means "unknown harness," and an unknown harness is not corroborating evidence (a `memory_note` written without harness attribution does not corroborate a separate write with `harness = 'codex'`). **Not** derived from the events log: `WriteCommitted` events do not carry harness/session_id today and there is no need to add them — the data is already on `memories`.
- `confidence_decay(m)` is `max(0, original_confidence - current_confidence) / 1.0`. **Data source:** v0.2 adds an `original_confidence: f64` field to the `Frontmatter` model (Stream A surface addition, authorized in §19). It is set on initial promotion and never mutated thereafter. Pre-v0.2 memories that lack the field default `original_confidence = current_confidence` on first read, producing decay = 0.0 (a conservative floor — they cannot drift "from" anything because we have no prior baseline).
- `sensitivity_weight(m)` is `0.0` for `public`, `0.3` for `internal`, `0.6` for `confidential`, `1.0` for `personal`.

Top N memories (default 12) surface per session. User responds:

- **confirm** → refresh `observed_at`, slight confidence bump.
- **correct** → prompts for new content; triggers supersession chain.
- **forget** → tombstone with user-provided reason.
- **not relevant** → lower passive-recall weight; skip in future reality checks (doesn't tombstone — just de-prioritize).
- **skip this week** → come back next Sunday.

These weights are dogfood-tunable. Final values land before 1.0.0.

### 16.5 Notifications

Three channels, per-event:

- **Passive** (default, always on): appears in `memoryd status` and in the next session's recall block as a `<pending-attention>` line.
- **OS notification** (urgent, opt-in): leaked secret detected, merge conflict blocking sync, queue over threshold. Off by default; explicit opt-in via `notifications.os.enabled: true`.
- **External** (scheduled): Slack webhook / email for weekly reality check, daily synthesis summary.

Configurable in `config.yaml`:

```yaml
notifications:
  passive: always
  os:
    enabled: false
    triggers: [leaked_secret, blocking_merge_conflict, review_queue_over:50]
  external:
    channel: slack
    webhook_url: https://hooks.slack.com/...
    triggers: [reality_check_due, daily_synthesis_summary]
```

### 16.6 Trust artifacts

Every memory's detail view (TUI panel 4 / dashboard audit explorer) shows:

- Provenance chain (walk backward to source).
- Confidence with reason.
- Recall count + last-recalled timestamp.
- Policy decisions taken.
- Privacy scan results (span labels detected).
- Supersession history.
- Sync state (which devices have this, merge status).

No black boxes.

Full surface in `stream-g-observability-v0.1.md`.

---

## 17. Internal namespace taxonomy

Unchanged from v0.1 §17 (me-memory, project-memory, agent-memory subtrees with review-cadence policies). The taxonomy is shipped via Stream A's tree validator and Stream C's policy files.

---

## 18. Open threads — resolved or deferred

v0.1 §18 listed six open threads. v0.2 records each as either "in v1," "deferred to v2," or "post-v2."

### 18.1 Prospective memory surface

**v0.2 disposition:** deferred to v2. v1 ships the namespace path (`me/prospective/<id>.md`) and frontmatter type but does not implement the trigger / scheduler / standing-order injection. Documented as `experimental` in v1 release notes.

**v2 design will lock:** commitment schema, external scheduler integration (launchd / systemd / cron), injection into recall as standing orders, silent-completion guard, conditional triggers, integration with Slack / calendar / email as external event sources.

### 18.2 Evaluation harness

**v0.2 disposition:** in v1 as Stream H (`stream-h-eval-harness-v0.1.md`).

Stream H ships:

- 12 handbook tests (exact identifier recall after compactions, superseded-fact handling, cross-project entity collision, abstention, poisoned candidate, tool-output preservation, subagent writeback, deletion and tombstone, recall budget pressure, compaction resumption, self-poisoning, temporal validity).
- 6 domain-specific tests (cross-harness substrate sharing, merge-driver semantic correctness, Privacy Filter refusal → error path → agent retry, reality-check drift scoring sanity, lease contention resolution, encrypted tier key rotation).
- Real-harness e2e for two specific tests (cross-harness substrate sharing; Privacy Filter refusal→retry) using actual `claude -p` and `codex exec` invocations against a sandbox tree.
- CI integration: runs on every release-candidate tag, blocking.
- Regression-as-test: any production failure that the eval harness didn't catch becomes a new test.

### 18.3 Bootstrap / cold-start UX

**v0.2 disposition:** in v1.

Implementation note as of 2026-05-24: alpha bootstrap is `memoryd serve --init` plus `scripts/install-memorum.sh`; the full interactive `memoryd init` wizard remains a release-shape target and is not the current alpha entrypoint.

Release-target `memoryd init` is an **interactive wizard by default**, with **flag overrides for every prompt** and **`--non-interactive` for scripts**. Flow:

1. **Welcome + license acknowledgment.** Apache 2.0 displayed; user acknowledges.
2. **Memory tree location.** Prompt with default `~/.memory/`. `--memory-dir <path>` overrides.
3. **Sync remote.** Prompt for remote URL or "skip for now." `--remote <url>` / `--no-remote` overrides.
4. **Privacy: age key generation.** Generates X25519 keypair to `~/.memoryd/keys/`. Prints recovery instructions and **blocks until user explicitly confirms** they have written the recovery instructions somewhere safe (`yes, I have it backed up [type EXACTLY: "I have my key backed up"]`). `--age-recovery-acknowledged` skips the gate (for scripted environments where the operator has already handled it).
5. **First harness link.** Detects installed harnesses (`claude`, `codex`, `cursor`, …); wires the passive-recall lifecycle hooks (the Tier-1 active surface pairs those with the `memoryd` CLI + skill). The MCP bridge is opt-in and off by default — `--wire-mcp <harness>` wires it explicitly for a harness that needs the compatibility surface. `--wire-hooks <harness>` / `--no-link` override hook wiring.
6. **Policy bundle.** Prompts standard / strict / custom. `--policy-bundle <name>` overrides.
7. **Reality-check cadence.** Default Sunday 09:00, Slack passive. Prompts to confirm / change. `--reality-check-cadence` / `--reality-check-channel` overrides.
8. **Dry-run plan.** Shows every disk effect (paths to create, files to write, daemon config to install, hooks to register) and prompts `y/N` confirmation. `--yes` skips the confirm.
9. **Apply.** Disk effects, daemon start, smoke test (`memoryd doctor`).
10. **Done.** Prints next-step suggestions.

Migration from existing systems (OpenClaw `memory-core`, Letta, etc.) is documented in `docs/migration-from-other-tools.md` (post-1.0.0 addendum, not in the wizard).

Backup and restore: `memoryd export <filter>` and `memoryd import <archive>` are part of the v1 admin surface (already in §14.3).

### 18.4 Policy versioning and migration

**v0.2 disposition:** in v1, but constrained.

v1 ships the policy schema with a `version: <int>` field. Policy evolution within v1.x is **additive only** — new fields default to no-op behavior. Old memories written under policy v1 read fine under policy v3 because v3 only added new gates, never tightened old ones in a breaking way.

`memoryd policy migrate --from v1 --to v3 --dry-run` runs a re-evaluation pass, surfacing newly-quarantined items for review without applying. `--apply` writes the changes.

Field-level forward-compat: daemon reads frontmatter even when newer daemon wrote fields this daemon doesn't know about. Unknown fields preserved on round-trip (Stream A v1.1 §7).

Schema evolution that requires a breaking change waits for v2.0.0.

### 18.5 Multi-user future-proofing

**v0.2 disposition:** post-v2. v1 explicitly does not design for it. v2 ships only Tier 2 harness elevations.

The principal model (human principals, shared memory, ACLs per path), shared-as-collaboration design, second-human onboarding, cross-human conflict resolution, and privacy implications are all post-v2 work. Not documented further until then.

### 18.6 Naming

**v0.2 disposition:** closed. Product is `Memorum`. See §22.

---

## 19. Implementation phasing — v1 status and remaining work

Replaces v0.1 §19. As of 2026-05-01:

### Shipped

- **Stream A — Core substrate.** Shipped in `d227dce` on `main`. Live contract: `stream-a-core-substrate-v1.1.md`.
- **Stream B — Daemon + MCP.** Shipped in `f9d9c2b` on `main` as the daemon/socket protocol and MCP forwarder; post-shipping remediation added the launchable stdio MCP server (`memoryd mcp --socket <path>`) on 2026-05-02. 10 MCP tools across A/C/D/E/F/source-grounding. Reviews: `docs/reviews/stream-b-*`.
- **Stream C — Governance.** Shipped in `6f583ec` on `main`. Live contract: `stream-c-governance-v0.1.md`.
- **Stream D — Privacy.** Shipped in `17a0a04` and `5f7d926` on `main`. Live contract: `stream-d-privacy-v0.1.md`.
- **Stream E — Passive recall.** Shipped 2026-04-30. Live contract: `stream-e-passive-recall-v0.5.md`. API: `docs/api/stream-e-passive-recall-api.md`.
- **Stream F — Dreaming.** Shipped 2026-05-01. Live contract: `stream-f-dreaming-v0.3.md`. API: `docs/api/stream-f-dreaming-api.md`. Final gate evidence: `docs/reviews/stream-f-final-gate-report.md`.

### Remaining for v1 (parallel tracks)

- **Stream G — Observability.** TUI (`memoryd ui`), localhost web dashboard, Reality Check ritual, notifications, trust artifacts. Spec: `stream-g-observability-v0.1.md`.
- **Stream H — Eval harness.** 12 + 6 tests, 2 real-harness e2e, CI integration, regression-as-test pattern. Spec: `stream-h-eval-harness-v0.1.md`.
- **Stream I — Cross-session coordination.** Peer-update relevance gate, presence/claim-lock semantics, `<peer-update>` / `<peer-presence>` recall block additions. Spec: `stream-i-cross-session-v0.1.md`.

### Topology

Streams G, H, I run in parallel after their specs are written, plan-reviewed, and revised. Stream G's TUI work and dashboard work are parallelizable internally. Stream H's tests are independent of each other.

Primary ownership:

- **Stream G** owns `crates/memoryd-ui/` (new), CLI command additions for `memoryd ui`, the localhost HTTP server, the Reality Check scoring library, and notification dispatch.
- **Stream H** owns `crates/memorum-eval/` (new), the CI workflow under `.github/workflows/`, the test orchestrator binary, and harness-runner subprocess plumbing for the two real-harness e2e tests.
- **Stream I** owns the new `crates/memorum-coordination/` crate (relevance-gate scoring, `framing_tests` helpers consumed by Stream H, peer-update emission helpers) plus daemon-side wiring (peer-presence heartbeat worker, claim-lock registry, lifecycle handlers). The standalone crate keeps the scoring logic's dependencies out of `memoryd` core; daemon-side modules live under `crates/memoryd/src/peer/`.

Trunk gate (`scripts/check.sh`) runs once after all three streams merge, not per-stream-per-task.

### Cross-stream surface authorizations

Streams G/H/I touch shipped-stream surfaces. Each touch listed here is **authorized as additive-only** — no rename, no removal, no semantic change to existing fields/events/protocol variants. Any cross-stream surface touch not enumerated here is unauthorized and must be brought back to the system spec for an amendment before implementation.

**Stream A surface additions** (substrate — frozen contract, additive only):

| Surface | Change | Authorized for |
|---|---|---|
| `EventKind` enum (`crates/memory-substrate/src/events/log.rs`) | Add five variants: `RecallHit { id: MemoryId, recalled_at: DateTime<Utc> }`, `RealityCheckConfirmed { id: MemoryId, session_id: String }`, `RealityCheckForgotten { id: MemoryId, session_id: String, reason: String }`, `RealityCheckNotRelevant { id: MemoryId, session_id: String }`, and `ClaimLockContention { memory_id: MemoryId, holder: String, contender: String }` | `RecallHit` is Stream G drift-score data (§16.4 inverse-recall-frequency); RC variants record §16.4 user response actions; `ClaimLockContention` records Stream I §7.4 advisory contention events. Emission of `RecallHit` is owned by Stream E (see below). |
| `events_log` **SQLite mirror table** (`crates/memory-substrate/src/index/schema.rs`) | New table `events_log(seq INTEGER PRIMARY KEY, kind TEXT NOT NULL, memory_id TEXT, ts TEXT NOT NULL, payload_json TEXT NOT NULL)` plus covering index `idx_events_log_kind_memory_ts(kind, memory_id, ts)`. Population: backfill from per-device JSONL files on first migration to schema v4; dual-write on every `events::log::append`. JSONL remains canonical; SQLite is a derived projection. **Mirror staleness must be observable**: Stream A exposes `Substrate::events_log_mirror_health()` returning `(jsonl_max_seq, sqlite_max_seq, lag)`, and `memoryd doctor` emits a `events_log_mirror_lag` `DoctorFinding` whenever lag > 0. `memoryd doctor --reindex` rebuilds the mirror from JSONL. | Stream G drift-score queries (`SELECT COUNT(*) FROM events_log WHERE kind='recall_hit' AND memory_id=? AND ts > ?`); Stream I cross-device device-attribution queries; durable foundation for future telemetry that needs SQL access to events. |
| `memory_supersession` **SQLite derived projection** (`crates/memory-substrate/src/index/schema.rs`) | New table `memory_supersession(memory_id TEXT NOT NULL, supersedes_id TEXT NOT NULL, PRIMARY KEY(memory_id, supersedes_id), FOREIGN KEY ... ON DELETE CASCADE)` plus reverse-lookup index `idx_memory_supersession_supersedes_id`. Population: backfill from each `Frontmatter.supersedes` array on migration to schema v4; sync wholesale on every memory write through the existing `sync_auxiliary_tables` path (extended to handle supersession edges alongside tags/aliases/entities/evidence). The shipped Stream A index left this as a documented "deferred" projection (`crates/memory-substrate/src/index/query.rs` doc-comment); v0.2 promotes it to shipped because Stream G drift scoring depends on it. | Stream G drift-score corroboration (recursive CTE walking supersession chains depth-bounded at 8). Replaces the phantom `memories.supersedes_ids` column referenced in earlier drafts of this spec; that column does not exist and never did. |
| `INDEX_SUPPORTED_SCHEMA_VERSION` constant | Bump from 3 to 4. Migration v4 adds `events_log` + covering index + backfill from JSONL, adds `memory_supersession` + reverse-lookup index + backfill from frontmatter, and adds `memories.original_confidence REAL` via `add_column_if_missing` + backfill from frontmatter where present. | Stream G's events_log mirror, supersession projection, and original_confidence column. |
| `Frontmatter` model (`crates/memory-substrate/src/model.rs`) | Add field `original_confidence: Option<f64>` (Option for backward compatibility with pre-v0.2 memories). Set on initial promotion; never mutated thereafter. Frontmatter parser default-on-absent; index column `original_confidence REAL` added in same migration. | Stream G drift-score component `confidence_decay(m) = max(0, original_confidence - current_confidence)`. |
| `RecallIndexRow` struct (`crates/memory-substrate/src/model.rs`) | Add two fields: `indexed_at: DateTime<Utc>` (already a NOT NULL column on `memories`; pure struct/hydration surface) and `source_device: Option<String>` (already a TEXT NULL column on `memories`; pure struct/hydration surface). | `indexed_at` for Stream I sync-arrival recency window (§4.2 / §5.3); `source_device` for Stream I cross-device peer-update filtering. **No new columns** — both already exist on `memories`. |
| Runtime layout (`stream-a-core-substrate-v1.1.md` §5.2) | Add three new daemon state files: `<runtime_root>/state/state.json`, `state/reality-check-pending.json`, `state/reality-check-session.json` (per-device, not synced) | Stream G persistent UI state, RC pending queue, RC session continuity across daemon restarts. Stream G must specify crash-recovery semantics (load-or-default; no recovery from a partial write). |

**Stream B / daemon protocol additions** (newline-delimited JSON over Unix socket — additive only):

| Surface | Change | Authorized for |
|---|---|---|
| `RequestPayload` enum | Add variants for Stream G reality-check (`RealityCheckRun`, `RealityCheckRespond`, `RealityCheckList`, `RealityCheckAcknowledge`) plus `RecallHits { since, limit }` for the web recall-hit consumer; Stream I peer-state (`PeerPresenceHeartbeat`, `PeerClaimAcquire`, `PeerClaimRelease`); and `TestInjectEvent` gated behind the `test-utils` cargo feature (Stream H — production builds reject with `MethodNotAllowedOnMcp`). | Stream G: TUI/dashboard wire and recall-hit observability; Stream I: cross-device coordination; Stream H: deterministic events-log fixture. Wire shapes in each stream's spec. |
| `ResponsePayload` enum | Companion variants for the above | Same. |
| Daemon error taxonomy (`crates/memoryd/src/protocol.rs`) | Add `MethodNotAllowedOnMcp` error variant. Returned by the MCP forwarder when an admin/UI surface variant (RC, peer-state, privacy admin, device admin, review admin, `TestInjectEvent`) is invoked through the MCP tool path. CLI/socket access remains permitted; only MCP is rejected. | Authoritative rejection for admin variants per §14.3. Single error variant rather than per-stream ad-hoc rejection text. |
| `DoctorResponse` (`crates/memoryd/src/protocol.rs`) and `doctor_response` handler (`crates/memoryd/src/handlers.rs`) | Additive: `doctor_response` calls `Substrate::events_log_mirror_health()` and on `lag > 0` appends a `DoctorFinding { code: "events_log_mirror_lag", repair: Some("memoryd doctor --reindex") }` and sets `healthy = false`. No struct schema change — the existing `Vec<DoctorFinding>` is used. | Surfacing dual-write fail-soft mode so a stale SQLite mirror (the failure mode where JSONL succeeds, SQLite write fails, WARN logged) is observable instead of silently corrupting drift scores. Owned by Stream G plan Task 4. |
| `NotificationEvent` broadcast channel | Stream G defines seven variants (declared in `stream-g-observability-v0.1.md` §1.3). Frozen at seven in v1. | Stream G TUI live updates and dashboard SSE stream. |
| MCP tool surface | **No Stream G/H/I changes.** Frozen at the ten tools in §14.1 after source grounding's `memory_capture_source` addition. Stream G/H/I add nothing to MCP. | Anti-feature 1 in §1.3. |

**Stream E surface additions** (passive recall — frozen contract, additive only):

| Surface | Change | Authorized for |
|---|---|---|
| Recall response builder (`crates/memoryd/src/recall/`) | Emits `EventKind::RecallHit` for each memory included in a rendered startup or delta block (one event per included memory per response, deduplicated within a single response) | Stream G drift-score data source. |
| `<memory-delta>` block schema | Adds optional `<peer-update>` and `<peer-presence>` child elements (additive; absence preserves Stream E's existing no-peer behavior) | Stream I cross-session coordination. Element schemas in `stream-i-cross-session-v0.1.md` §5. |
| `.memory-project.yaml` parser whitelist (`crates/memoryd/src/recall/project.rs:81`) | Extend `matches!(key, …)` whitelist to include `concurrent_session_mode` | Stream I per-project mode selection (Level 1/2/3). Without this whitelist update, projects that set the field crash at startup. |
| Recall index hydration | (RecallIndexRow `indexed_at` and `source_device` field surfacing handled by the Stream A row above — both columns already exist; this is the consuming side, no new Stream E surface here.) | — |

**Stream C / Stream D surfaces:** No additions for Streams G/H/I, with one exception: Stream H test #18 asserts forward secrecy across `memoryd device rotate-keys`, which requires Stream D's spec to explicitly state the rotation contract (key decommission semantics, atomicity of pointer swap, what happens to existing ciphertext). This is a Stream D v0.1.1 amendment — not a Stream H surface — and is authorized for that purpose only.

**Stream F surface:** No additions. Stream F shipped 2026-05-01; v1 leaves it as-is.

Any surface change not listed in these tables is out of scope for v1 and must be raised as a system spec amendment before implementation.

### Stream J — Open threads (post-v1)

Prospective memory, multi-user future-proofing, additional harness Tier 2 promotions. Post-v1 release backlog.

---

## 20. Dogfood gate (between code-complete and 1.0.0)

This is part of the v1 contract. Code-complete does not equal release. The dogfood gate has to pass before 1.0.0 publishes.

### 20.1 Window

**1 week.** Multi-machine from hour zero (laptop + desktop). Multi-harness from hour zero (Claude Code + Codex CLI; Cursor wired as Tier 3 for at least one session).

### 20.2 What gets dogfooded

- **Memorum tracks Memorum.** The `agent-memory` repo is its own first project. Specs, plans, reviews, design discussions all run through `memory_write` / `memory_observe` / `memory_note`. If the project's own context can't survive the week, the system isn't ready.
- **Active dev work.** Any other project Trey touches during the dogfood week is a second test surface. No special handling.

### 20.3 Daily eval

Every day during the dogfood week, the eval harness (Stream H) runs on the live tree. Pass criterion: no regression vs. the previous day's run. Failure triggers root-cause investigation before the day is "good."

### 20.4 Pass criteria

**Objective:**

- Eval harness has 19 tests; daily run reports zero failures. In real-harness mode all 19 run; in mock mode tests #13, #15, and #19 are reported as `partial: true` (skipped) and the remaining 16 pass — see `stream-h-eval-harness-v0.1.md` §6.3 for the test-mode matrix and §10.1 for the canonical catalog.
- No data loss across a forced multi-machine merge collision.
- No `secret` write reaches disk under any path.
- Dream pipeline runs at least 5 of 7 nights without manual intervention.
- Cross-harness substrate sharing demonstrably works (Codex writes; Claude Code reads on next turn; entity match drives surfacing).

**Subjective:**

- Trey reports: "I felt like memory was working, not getting in the way."
- Cross-session peer-update fires at least once and is correctly framed as third-party (no agent confused it with user input).
- Recall blocks land in cache-stable positions; no observable cache thrash.
- Reality Check fires Sunday morning; output is useful, not noise.

### 20.5 Failure mode

Any failure during the week is allowed to trigger a spec revision. v0.2 stays editable until 1.0.0 ships. If a structural problem emerges (e.g., the relevance gate threshold of 0.6 is too strict in practice), the spec changes, the affected stream re-implements, the dogfood week extends.

### 20.6 Ship gate

After the dogfood week passes:

1. Tag `v1.0.0-rc.1`.
2. CI release pipeline runs (Stream H eval, full `scripts/check.sh`, two-clone convergence, durability probe, bench gate).
3. Publish split repo to GitHub (`memorum/memorum`).
4. Publish 1.0.0 to crates.io (`memorum`, `memorum-substrate`, `memorum-governance`, `memorum-privacy`, `memorum-eval`).
5. Documentation under `/docs/` in the public repo (no separate site for v1).

---

## 21. License, versioning, public release

### 21.1 License — Apache 2.0

Memorum is **Apache 2.0**.

Rationale: some of what Memorum does is patentable surface — masked synthesis with grounding rehydration, drift-risk scoring with cross-source corroboration weights, peer-update relevance gating with the entity/path/topic blend, and the harness-CLI delegation pattern as a generic LLM-call substrate for daemons. MIT does not include a patent grant. Apache 2.0's explicit patent grant defangs the issue on day one — anyone using Memorum gets the patent license; anyone suing over Memorum loses theirs.

If contributors want their changes covered, contributions are inbound under Apache 2.0 (DCO sign-off; no CLA required for v1).

### 21.2 Versioning — SemVer starting at 1.0.0

- **1.0.0** — initial public release after dogfood gate.
- **1.x.y** — additive feature releases (Tier 2 harness adapters, dashboard sections 5–6, prospective memory v1, policy migration tools) and bug fixes. No breaking schema changes.
- **2.0.0** — next breaking change (frontmatter schema additions that are required, MCP tool surface changes, cross-stream contract revisions). No timeline; driven by what dogfood + production use teaches.

Stream specs continue to use the `-v<major>.<minor>.md` suffix versioning (e.g., `stream-a-core-substrate-v1.1.md`). Spec versions are independent of product semver — the live spec for a stream may be v1.1 or v0.5 even when the product is at 1.0.0.

### 21.3 Repository layout

**Development repo:** `agent-memory` (private, this repo). Specs, plans, reviews, internal coordination.

**Release repo:** `memorum/memorum` (public, GitHub). Contains:

- All `crates/*` source.
- `docs/api/*` (public API references; rendered in repo, no separate site for v1).
- `docs/specs/*` for the live versions only (older versions stay in the dev repo for history).
- `README.md`, `INSTALL.md`, `CONTRIBUTING.md`, `LICENSE-APACHE`, `NOTICE`.
- Examples directory with sample `.memory-project.yaml`, sample policies, sample harness hook configurations.

Release flow: development happens on `agent-memory`; release branches are mirrored to `memorum/memorum` with the dev-only history (older spec versions, internal handoffs, Codex worktree artifacts) stripped.

### 21.4 Public README shape

```markdown
# Memorum

Local-first, harness-agnostic shared memory for AI coding agents.

One daemon. Every harness. Memory that survives compaction, persists across
machines, and stays governed enough not to poison itself.

## Why

(short — the problem statement: agents forget, every harness has its own
sandbox, governance is usually retrofitted)

## What

(short — durable memory, passive recall, governance, dreaming, privacy,
all behind one local daemon)

## Status

v1.0.0. Apache 2.0. macOS + Linux. Claude Code and Codex CLI ship with
full hook integration; any other MCP-speaking harness gets the raw MCP
surface.

## Install

`brew install memorum` / `cargo install memorum`

## Quickstart

`memorum init`

## Docs

`/docs/` in this repo. Start with `docs/getting-started.md`.

## License

Apache 2.0.
```

No marketing site, no demo video, no logo for v1. Words on a page. The README is the surface.

---

## 22. Naming — Memorum

`Memorum` is Latin: the genitive plural of `memor` ("mindful, remembering, having presence of mind"). Literally "of the mindful ones" or "of those who remember." Not the genitive plural of `memoria` (which would be `memoriarum`). This is a deliberate choice, not a misderivation:

- **The system is about active mindfulness, not passive storage.** A pile of files isn't memory; agents grounding against, recalling, dreaming over, and superseding those files is. `memor` foregrounds the *act* of remembering, not the artifact. That matches the architecture: governance, dreaming, drift detection, recall — none of these are properties of dead bytes.
- **Directionally meaningful.** A stranger reading "Memorum" gets that this is something in the memory space. Doesn't tell them it's an agent memory tool, but the README handles that.
- **Pronounceable.** "muh-MORE-um." Three syllables. Doesn't fight its own letters.
- **Namespace-clear (as of 2026-05-01).** `memorum` is unclaimed on crates.io, npm (no relevant collisions), PyPI (one unrelated 2018 abandoned package), `github.com/memorum` is wide open, `@memorum` on most social handles is free. **Re-verify within 7 days of v1.0.0 tag.** Namespace state on 2026-05-01 is not a guarantee of namespace state on release day.
- **Domain check pending** — `memorum.dev`, `memorum.io`, `memorum.com`, `memorum.app` to be verified via Porkbun MCP next session. Domain selection happens before public release; if a primary is unavailable, fallback to `memorum.tools` or `memorum.sh`.

Earlier candidates (Engram, Hivemind, Reverie, Memora, Memento, Lore, Noosphere, Folkmind, Memeplex, Memorica) all had directional clarity but each carried a name collision that would force ambiguity at first contact. Memorum has none.

If during dogfood it becomes clear the name actively confuses users (very low probability; rename cost climbs sharply after public release), v1.0.0 is the last reasonable rename window. After that, the name is the name.

---

## 23. References

- `agent-harness-memory-context-handbook-v2.2.md` — worldview, vocabulary, threat model, design principles. The philosophical source.
- `docs/specs/stream-a-core-substrate-v1.1.md` — substrate contract.
- `docs/specs/stream-c-governance-v0.1.md` — governance contract.
- `docs/specs/stream-d-privacy-v0.1.md` — privacy contract.
- `docs/specs/stream-e-passive-recall-v0.5.md` — recall contract.
- `docs/specs/stream-f-dreaming-v0.3.md` — dreaming contract.
- `docs/specs/stream-g-observability-v0.1.md` — observability/UX contract (in this revision).
- `docs/specs/stream-h-eval-harness-v0.1.md` — evaluation contract (in this revision).
- `docs/specs/stream-i-cross-session-v0.1.md` — cross-session coordination contract (in this revision).
- OpenAI Privacy Filter: https://openai.com/index/introducing-openai-privacy-filter/ (Apache 2.0).
- `age` encryption: https://github.com/FiloSottile/age.
- gitleaks ruleset: https://github.com/gitleaks/gitleaks.

---

**End of v0.2 release contract.** Streams A–F shipped; Streams G/H/I are the remaining v1 work with their own spec contracts; v1 ships after a 1-week multi-machine dogfood gate as Memorum 1.0.0 under Apache 2.0.
