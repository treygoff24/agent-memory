# OpenAI Codex CLI memory system — architecture report (2026-05-27)

This report describes the default OpenAI Codex CLI memory implementation as documented and implemented in the official Codex codebase on 2026-05-27. The most important default-user caveat is that Codex memories are opt-in: OpenAI’s public memory docs say the feature is off by default, can be enabled in settings or with `[features] memories = true`, and is unavailable in the EEA, UK, and Switzerland at launch.[^memories-docs] Therefore a fresh default install that has not enabled the feature writes no memory artifacts. The rest of this report describes the built-in memory architecture that a normal user receives after enabling the feature without further custom configuration.

The public docs give the user-facing overview; the exact file formats and lifecycle are mostly specified in the official `openai/codex` source tree, especially the memory write-path README, Phase 1 and Phase 2 prompt templates, and storage/runtime code.[^memory-readme]

## Storage layout

Codex stores local user state under `CODEX_HOME`; the official advanced configuration docs state that `CODEX_HOME` defaults to `~/.codex` and contains configuration, auth, history, logs, and other local state.[^config-advanced] The quickstart lists Codex CLI support for macOS, Windows, and Linux, so the default path is the user-home `.codex` directory on each supported OS, using the shell/platform meaning of `~`.[^quickstart] The user config file lives at `~/.codex/config.toml`, while a project can also have `.codex/config.toml`; project-level config overrides user config.[^config-basic]

With the memory feature enabled, the official memories docs say generated memory files live under the Codex home directory, with the main memory files in `~/.codex/memories/` by default.[^memories-docs] The current source confirms that `memory_root(codex_home)` is `codex_home.join("memories")`, and defines `rollout_summaries/` plus `raw_memories.md` beneath that root.[^write-lib-paths]

The Phase 2 consolidation prompt defines the memory folder structure as:

- `memory_summary.md`: always loaded into the prompt; first line must be exactly `v1`.[^consolidation-layout]
- `MEMORY.md`: the retrieval-oriented handbook of consolidated memory entries.[^consolidation-layout]
- `raw_memories.md`: a temporary/mechanical merge of Phase 1 raw memories used as Phase 2 input.[^consolidation-layout]
- `skills/<skill-name>/`: optional reusable procedures with `SKILL.md`, scripts, templates, or examples.[^consolidation-layout]
- `rollout_summaries/<rollout_slug>.md`: per-rollout recaps with lessons, reusable knowledge, references, and pruned evidence snippets.[^consolidation-layout]
- `extensions/`: memory-extension data; the source uses `codex_home/memories/extensions` for extension-owned artifacts, including ad hoc notes.[^write-lib-paths]

There is also a separate SQLite state store. The official source names the memories database `memories_1.sqlite`; the state runtime initializes the SQLite databases under the configured Codex home, while `CODEX_SQLITE_HOME` is the environment variable for overriding the SQLite state database home.[^state-db-paths] The initial memories migration creates `stage1_outputs` and `jobs` tables for extracted memories and memory jobs.[^sqlite-migration] The public docs are user-facing and do not document this DB schema, but it matters architecturally because the Markdown files are materialized from DB-backed Phase 1 outputs.

## The three-tier memory system (raw → MEMORY → summary)

“Three-tier memory system” is a useful descriptive label for the current design, but the official source names the write pipeline as Phase 1 “Rollout Extraction” and Phase 2 “Global Consolidation.”[^memory-readme] The tiers are:

1. **Raw per-rollout extraction**: Phase 1 reads eligible rollout traces and asks a model for structured JSON containing `raw_memory`, `rollout_summary`, and optional `rollout_slug`.[^stage-one-json] Successful outputs are stored in the memories DB as stage-1 outputs.[^memory-readme]
2. **Consolidated handbook (`MEMORY.md`)**: Phase 2 materializes selected stage-1 outputs into `raw_memories.md` and `rollout_summaries/`, then runs an internal consolidation agent that updates `MEMORY.md` and `memory_summary.md`.[^memory-readme]
3. **Prompt summary (`memory_summary.md`)**: the memory read extension reads `~/.codex/memories/memory_summary.md`, truncates it if necessary, and embeds it into developer instructions for new threads.[^memory-prompts] The read-path prompt says the summary is already provided and should not be reopened during the quick memory pass.[^read-path-layout]

`raw_memories.md` is not the durable user-facing index. The Phase 2 prompt calls it a mechanical merge of selected Phase 1 raw memories and warns not to treat file order as recency or importance.[^consolidation-inputs] The storage code writes it in stable ascending thread-id order and includes a placeholder when no raw memories are selected.[^storage-raw]

`MEMORY.md` is the durable, retrieval-oriented handbook. Its strict schema begins with `# Task Group`, followed by `scope:` and `applies_to:` lines, then task sections with `rollout_summary_files` and keywords, followed by optional consolidated preference/knowledge/failure sections.[^consolidation-memory-schema]

`memory_summary.md` is a compact navigational summary. The Phase 2 prompt requires the first line to be exactly `v1`; if the file is missing, empty, or not `v1`, the consolidation agent must regenerate it rather than patching it in place.[^consolidation-inputs] The user-facing injected memory prompt then includes that summary and tells the agent to search `MEMORY.md` and open rollout summaries only when relevant.[^read-path-layout]

## rollout_summaries/ and the data flow

In Codex memory terminology, a rollout is the persisted trace of a Codex thread/session. The source refers to “rollout traces” and stores rollout paths with thread records; Phase 1 selects recent eligible rollouts from the state DB and extracts memory from them.[^memory-readme] A rollout summary is the compact but evidence-preserving Markdown artifact produced by Phase 1 and later written under `rollout_summaries/` by Phase 2.[^stage-one-json]

The data flow is:

1. A root, non-ephemeral Codex session starts with memories enabled and state DB available.[^memory-readme]
2. Phase 1 finds eligible, idle, recent rollouts from allowed interactive sources, excluding the current thread and already-owned jobs.[^runtime-claim]
3. Each claimed rollout is filtered to memory-relevant response items and sent to a model in parallel, with a concurrency cap.[^memory-readme]
4. The model returns `raw_memory`, `rollout_summary`, and `rollout_slug`; secrets are redacted before successful outputs are stored as stage-1 DB rows.[^memory-readme]
5. Phase 2 selects a bounded top-N set of stage-1 outputs, materializes `raw_memories.md` and `rollout_summaries/`, and runs the consolidation agent if the memory workspace changed.[^memory-readme]
6. `MEMORY.md` points back to rollout summary files in each task’s `### rollout_summary_files` section.[^consolidation-memory-schema]

The storage code writes each rollout summary file with header lines for `thread_id`, `updated_at`, `rollout_path`, `cwd`, and optional `git_branch`, followed by the generated summary body.[^storage-summary] File names are generated from a timestamp fragment, a short hash, and an optional sanitized slug; the slug is lower-case, filesystem-safe, and length-limited by the Phase 1 prompt.[^storage-stems][^stage-one-json]

## AGENTS.md vs. memory tiers

`AGENTS.md` is not one of the memory tiers. It is a separate prompt-customization layer analogous to project instructions. OpenAI’s AGENTS guide says Codex looks for `AGENTS.md` files in the user’s Codex home and along the path from the repository root to the current working directory, merges applicable instructions, and rebuilds that instruction context for each run.[^agents-md] The customization overview likewise separates AGENTS files, skills, MCP, subagents, and memories: AGENTS shapes how Codex behaves, while memories carry useful local context forward across sessions.[^customization]

The practical distinction is: `AGENTS.md` is user-authored durable guidance, while memory artifacts are generated local state. The AGENTS guide says global instructions can live at `~/.codex/AGENTS.md`; if both `AGENTS.override.md` and `AGENTS.md` exist in the same directory, the override file wins.[^agents-md] The memory docs instead frame memory files as generated local artifacts under the Codex home and point users to settings or `/memories` for controlling whether Codex uses or generates them.[^memories-docs]

## Per-thread block schema (raw_memories.md)

The full `raw_memories.md` file is generated by storage code, not hand-authored by the model. Its top-level structure is:

```text
# Raw Memories

Merged stage-1 raw memories (stable ascending thread-id order):

## Thread `<thread_id>`
updated_at: <source timestamp>
cwd: <cwd>
rollout_path: <path>
rollout_summary_file: <file>.md

<raw_memory body from Phase 1>
```

This comes directly from the storage implementation that rebuilds `raw_memories.md` from DB-backed stage-1 outputs.[^storage-raw] The file order is stable ascending thread-id order, not recency or usefulness.[^consolidation-inputs]

The `<raw_memory body>` is specified by the Phase 1 prompt template. It begins with YAML-style frontmatter:

```yaml
---
description: concise but information-dense description of the primary task(s), outcome, and highest-value takeaway
task: <primary_task_signature>
task_group: <cwd_or_workflow_bucket>
task_outcome: <success|partial|fail|uncertain>
cwd: <single best primary working directory for this raw memory; use `unknown` only when none is identifiable>
keywords: k1, k2, k3, ... <searchable handles>
---
```

The same template then requires task-grouped body content, with each `### Task <n>` section containing `task:`, `task_group:`, `task_outcome:`, `Preference signals:`, `Reusable knowledge:`, `Failures and how to do differently:`, and `References:` subsections when applicable.[^stage-one-raw-schema]

The legal `task_outcome` values are specified in the Phase 1 prompt: `success` for a completed/correct result, `partial` for meaningful but incomplete or unverified progress, `uncertain` for no clear signal, and `fail` for an incomplete, wrong, stuck, tool-misused, or user-dissatisfying result.[^stage-one-outcomes] I found no separate public JSON Schema for arbitrary additional frontmatter keys beyond this official prompt template and the storage wrapper.

## Task-group / task structure (MEMORY.md)

The Phase 2 consolidation prompt is the official source for `MEMORY.md` shape. It calls `MEMORY.md` the durable retrieval-oriented handbook and requires every memory block to start with:

```text
# Task Group: <cwd / project / workflow / detail-task family; broad but distinguishable>

scope: <what this block covers, when to use it, and notable boundaries>
applies_to: cwd=<primary working directory, cwd family, or workflow scope>; reuse_rule=<when this memory is safe to reuse vs when to treat it as checkout-specific or time specific>
```

The template explains that `Task Group` is for retrieval, `scope:` is for scanning, and `applies_to:` is mandatory to preserve working-directory or workflow boundaries.[^consolidation-memory-schema] The required task body then starts with `## Task 1: ...`, includes a `### rollout_summary_files` subsection whose bullets carry `cwd`, `rollout_path`, `updated_at`, and `thread_id`, and a `### keywords` subsection containing task-local retrieval handles.[^consolidation-memory-schema]

After task sections, the prompt allows block-level `## User preferences`, `## Reusable knowledge`, and `## Failures and how to do differently` sections. These are consolidated from represented tasks and should remain auditable rather than generic.[^consolidation-memory-schema]

Consolidation from raw to `MEMORY.md` is triggered by the Phase 2 pipeline after selected stage-1 outputs have been synced to the filesystem and the memory workspace diff is non-empty. The Phase 2 prompt tells the internal agent to read `phase2_workspace_diff.md`, use changed `raw_memories.md` sections and corresponding rollout summaries as evidence, and update `MEMORY.md` plus `memory_summary.md` accordingly.[^consolidation-inputs]

## Write triggers

There are three distinct write paths relevant to default users.

First, normal memory generation is startup-triggered, not immediate end-of-thread writing. The official write-path README says the pipeline is triggered when a root session starts, only if the session is not ephemeral, the memory feature is enabled, the session is not a sub-agent session, and the state DB is available.[^memory-readme] It runs asynchronously in the background. The public docs similarly say Codex waits until a thread is idle before creating memory, updates in the background, and may skip work when remaining rate limit is below the configured threshold.[^memories-docs]

Second, a thread only becomes a future memory source if its persisted metadata has memory mode enabled. The session source sets `memory_mode` to enabled or disabled based on `config.memories.generate_memories` for new and resumed non-ephemeral threads.[^session-memory-mode] The runtime claim query later excludes threads whose `memory_mode` is not enabled, excludes the current thread, applies age and idle windows, and bounds the scan/claim work.[^runtime-claim]

Third, there is an explicit ad hoc update path. The injected memory prompt says updates to memories should occur only when the user explicitly asks, and those updates should be written as small Markdown note files under `extensions/ad_hoc/notes/` rather than directly editing core memory files.[^read-path-updates] The source also exposes dedicated memory tools only when the memories feature is enabled, memories are being used, and `memories.dedicated_tools` is true; the default for `dedicated_tools` is false.[^extension-tools][^config-types]

The config docs and source also include a guard for external context. The public config reference documents `memories.disable_on_external_context`, described as keeping memories from being recorded when web search or MCP tools were used, with the legacy alias `no_memories_if_mcp_or_web_search`.[^config-reference] The source carries the same alias in `MemoriesToml`.[^config-types]

## User-visible commands

The main user-visible memory control is the `/memories` slash command. Official slash-command docs say `/memories` lets users choose whether Codex uses existing memories, generates new memories, or disables both for future sessions.[^slash-commands] The TUI source shows the menu items as “Use memories,” “Generate memories,” and “Reset all memories,” with reset clearing local memory files and rollout summaries for the current Codex home.[^tui-memories]

Feature enablement is also user-visible. The config docs say the feature can be enabled via `[features] memories = true` in config, and the CLI reference documents `codex features` as the command group for managing feature flags stored in `~/.codex/config.toml` or a profile.[^memories-docs][^cli-reference]

I found no documented `codex memory` subcommand and no documented `codex export` memory command in the official CLI reference.[^cli-reference] The source does include `debug clear-memories`, which clears memory state from the memories DB and memory directories under the current Codex home, but this appears in source as a debug command rather than in the user-facing CLI reference.[^debug-clear]

For inspection/editing, the public memories docs say the files under `~/.codex/memories/` are generated state and are useful for troubleshooting, but users should review the files before sharing and should not store secrets.[^memories-docs] The Chronicle memory-extension docs, which are separate from default CLI memory, explicitly say generated Markdown memories can be read and modified, and users can delete or edit them to forget information, while warning not to manually add new information.[^chronicle]

## Lifecycle: eviction, deduplication, summarization

The lifecycle begins with opt-in feature enablement. In current source, the feature flag key is `memories`, default-enabled is `false`, and the feature description is “startup memory extraction and file-backed memory consolidation.”[^features-source] Once the feature is enabled, `generate_memories` and `use_memories` default to true, while `dedicated_tools` defaults to false.[^config-types]

Phase 1 is bounded and leased. The write-path README says Phase 1 claims a bounded set of jobs, runs extraction in parallel with a fixed cap, stores successful outputs, and gives failed jobs retry backoff rather than hot-looping.[^memory-readme] The source defaults include Phase 1 model `gpt-5.4-mini`, low effort, concurrency 8, and one-hour job lease/retry windows.[^write-lib-defaults]

Phase 2 is globally serialized. The write-path README says Phase 2 claims a single global lock before touching the memory root, so only one consolidation inspects or mutates the workspace at a time.[^memory-readme] The runtime source describes a singleton `memory_consolidate_global` job row, active running leases, retry backoff, and a success cooldown; a heartbeat extends the lease while the internal agent runs.[^runtime-phase2-lock][^runtime-heartbeat]

Retention and selection are usage-weighted. The runtime selection comments say eligible rows are those whose `last_usage` is within `max_unused_days`, or never-used rows whose source timestamp is still inside the same window; eligible rows are ranked by usage count, then recency, then source timestamp and thread id, and the selected top-N rows are returned in stable `thread_id ASC` order.[^runtime-selection] The source defaults include `max_raw_memories_for_consolidation = 256`, `max_unused_days = 30`, `max_rollout_age_days = 10`, `max_rollouts_per_startup = 2`, `min_rollout_idle_hours = 6`, and `min_rate_limit_remaining_percent = 25`.[^config-defaults]

File-level pruning follows Phase 2 selection. The write-path README says Phase 2 syncs `rollout_summaries/` directly to the selected inputs, prunes stale summaries that are no longer selected, prunes old extension resource files, writes `phase2_workspace_diff.md`, and exits without spawning the consolidation agent if the memory workspace has no changes.[^memory-readme] The storage code prunes rollout summary files whose stems are no longer retained.[^storage-summary]

Deduplication is primarily semantic, not a simple deterministic duplicate-removal pass. The DB has one `stage1_outputs` row per `thread_id`, so a thread’s extracted memory is updated rather than duplicated at that level.[^sqlite-migration] Above that, the Phase 2 prompt asks the consolidation agent to merge, split, deprecate, and remove stale guidance based on the workspace diff and current evidence.[^consolidation-inputs]

## Export / backup / portability

I found no official memory export format and no documented `codex export` or `codex memory export` command in the CLI reference.[^cli-reference] The official docs describe local generated files under the Codex home and warn users to review those files before sharing them, but do not define a portable archive schema.[^memories-docs]

A complete local memory state has at least two layers: visible Markdown artifacts under `~/.codex/memories/`, and internal state in `memories_1.sqlite` containing stage-1 outputs, usage counts, selected-for-Phase-2 flags, and job rows.[^sqlite-migration] The generated Markdown files are the default user-visible artifacts; the SQLite DB is an implementation detail exposed by official source, not by public user docs.

## Concurrent-write semantics

Concurrent writes are coordinated through the SQLite jobs table and a git-baseline workspace.

Phase 1 uses per-thread job claiming. The write-path README says each Phase 1 job is leased/claimed in the state DB before processing, preventing duplicate work across concurrent workers or startups.[^memory-readme] The runtime claim comments describe filtering active threads and then calling `try_claim_stage1_job` until the bounded number of claims is reached.[^runtime-claim]

Phase 2 uses a singleton global job lock. The runtime source describes the singleton global row, active running leases, retry backoff, cooldown, and owned lease heartbeat; only the owner can mark the job succeeded and rewrite the selected stage-1 snapshot set.[^runtime-phase2-lock][^runtime-heartbeat] This is the most important answer for two simultaneous Codex sessions: they can each claim different Phase 1 jobs, but only one Phase 2 consolidation should mutate the memory workspace at a time.

The memory root is also managed as a git-baseline directory. The write-path README says `~/.codex/memories/.git` is initialized by `codex-git-utils` and Phase 2 writes `phase2_workspace_diff.md` from the previous successful baseline to the current worktree.[^memory-readme] Workspace source confirms that stale diff files are removed, the git baseline is kept, and successful consolidation resets the baseline after deleting the generated diff file.[^workspace-source]

I found no public guarantee for concurrent manual edits to generated memory files. The Phase 2 prompt treats changes in `phase2_workspace_diff.md` as authoritative and warns not to drop apparent user changes, but the public docs still frame the files as generated state rather than a supported editing API.[^consolidation-inputs][^memories-docs]

## Recent history and changelog

The memory system changed substantially in early 2026. Public changelog pages are high-level, so the clearest chronology comes from official GitHub PRs, commits, and release notes.

- **February 2026: memory v2 global root and Phase 1/2 artifacts.** PRs around `mem-v2` moved memories toward a global `~/.codex/memories/` root, removed per-cwd buckets, introduced `raw_memories.md`, `rollout_summaries/`, and stage-1 output shape.[^pr-11364][^pr-11365][^pr-11369]
- **2026-02-20: richer raw schema and summary file linkage.** PR #12221 added `rollout_summary_file` to raw memory headers and introduced the `task`, `task_group`, and `task_outcome` schema shape used in raw memories.[^pr-12221]
- **2026-02-24: stricter consolidation prompt.** PR #12653 tightened the Phase 2 consolidation prompt, including `MEMORY.md` task structure and ordering expectations.[^pr-12653]
- **2026-04-27: git-backed workspace diffs.** Commit `01ab25d` added git-backed memory workspace diffs, which is the current `phase2_workspace_diff.md` flow.[^commit-git-diff]
- **2026-04-28: stable raw order.** Commit `fa127be` changed `raw_memories.md` ordering to stable ascending thread id to avoid churn and misleading recency signals.[^commit-stable-order]
- **2026-05-01: ad hoc note path.** Commit `70fc55b` moved explicit ad hoc memory note guidance to `extensions/ad_hoc/notes`.[^commit-ad-hoc]
- **2026-05-20: versioned memory summaries.** Release `rust-v0.132.0` notes “version memory summaries; rebuild when stale,” matching the current `v1` summary rule.[^release-132]
- **2026-05-26: memory toggles via app server.** Release `rust-v0.134.0` includes an app-server memory toggle change, relevant to UI/settings propagation rather than the core schema.[^release-134]

## Open questions

- **No public schema document found.** The exact raw block and `MEMORY.md` schemas are specified in official source prompt templates and storage code, not in a standalone public docs page.
- **No official export format found.** Official docs and CLI reference do not document a memory export or backup command.
- **Public docs vs. source defaults may lag.** The current source at commit `c57dee98b7e70f306d2981f9075dde1d1b9a90e7` sets `max_rollouts_per_startup = 2` and `max_rollout_age_days = 10`; the public config reference documents memory-related knobs but may not always match source defaults at HEAD.[^config-defaults][^config-reference]
- **Manual editing is not a stable API.** Source prompts try to preserve apparent user changes in the memory workspace diff, and Chronicle docs discuss editing generated Markdown, but I found no official default-memory API contract for concurrent manual edits.[^consolidation-inputs][^chronicle]
- **Legal extra frontmatter keys are not enumerated.** The Phase 1 prompt gives the strict keys listed above; I found no separate list of allowed extension keys.

## Sources

[^memories-docs]: https://developers.openai.com/codex/memories — Official user-facing docs for enabling Codex memories, default-off status, storage under `~/.codex/memories/`, background generation, privacy, `/memories`, and memory settings.
[^config-advanced]: https://developers.openai.com/codex/config-advanced — Official advanced configuration docs for `CODEX_HOME` and local state layout.
[^quickstart]: https://developers.openai.com/codex/quickstart — Official quickstart identifying supported platforms and install path context.
[^config-basic]: https://developers.openai.com/codex/config-basic — Official basic config docs for `~/.codex/config.toml`, project config, precedence, and feature flags.
[^config-reference]: https://developers.openai.com/codex/config-reference — Official reference for memory settings including `generate_memories`, `use_memories`, external-context guard, age/idle limits, and models.
[^slash-commands]: https://developers.openai.com/codex/cli/slash-commands — Official slash-command reference for `/memories` and other CLI slash commands.
[^cli-reference]: https://developers.openai.com/codex/cli/reference — Official CLI reference; used to verify absence of documented `codex memory` / `codex export` memory commands and presence of `codex features`.
[^agents-md]: https://developers.openai.com/codex/guides/agents-md — Official AGENTS.md guide for global/project instruction discovery, overrides, and rebuild behavior.
[^customization]: https://developers.openai.com/codex/concepts/customization — Official customization overview distinguishing AGENTS.md, memories, skills, MCP, and subagents.
[^chronicle]: https://developers.openai.com/codex/memories/chronicle — Official Chronicle memory-extension docs; used only for related generated-Markdown edit/delete behavior and extension storage context.
[^memory-readme]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/memories/README.md#L28-L135 — Official source README for memory write-path triggers, Phase 1/Phase 2 data flow, concurrency, selection, and workspace diff behavior.
[^stage-one-json]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/memories/write/templates/memories/stage_one_system.md#L222-L230 — Phase 1 prompt JSON keys and rollout slug constraints.
[^stage-one-outcomes]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/memories/write/templates/memories/stage_one_system.md#L150-L165 — Phase 1 task outcome labels and meanings.
[^stage-one-raw-schema]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/memories/write/templates/memories/stage_one_system.md#L401-L450 — Phase 1 strict `raw_memory` frontmatter/body schema.
[^consolidation-layout]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/memories/write/templates/memories/consolidation.md#L20-L35 — Phase 2 memory folder structure.
[^consolidation-inputs]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/memories/write/templates/memories/consolidation.md#L121-L195 — Phase 2 input files, incremental-update, diff, forgetting, and `memory_summary.md` `v1` rules.
[^consolidation-memory-schema]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/memories/write/templates/memories/consolidation.md#L201-L270 — Strict `MEMORY.md` task-group/task schema.
[^read-path-layout]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/ext/memories/templates/memories/read_path.md#L19-L46 — Injected memory read-path prompt layout and quick-pass rules.
[^read-path-updates]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/ext/memories/templates/memories/read_path.md#L117-L123 — Injected prompt rules for user-explicit memory updates under `extensions/ad_hoc/notes/`.
[^storage-raw]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/memories/write/src/storage.rs#L44-L77 — Source for `raw_memories.md` rendering and per-thread headers.
[^storage-summary]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/memories/write/src/storage.rs#L110-L135 — Source for rollout summary file headers and body writing.
[^storage-stems]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/memories/write/src/storage.rs#L153-L237 — Source for rollout summary filename stem generation and slug sanitization.
[^write-lib-paths]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/memories/write/src/lib.rs#L35-L136 — Source constants/functions for memory root, `rollout_summaries/`, `raw_memories.md`, and extension paths.
[^write-lib-defaults]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/memories/write/src/lib.rs#L78-L118 — Source defaults for Phase 1/Phase 2 models, effort, concurrency, leases, heartbeat, and diff file name.
[^config-defaults]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/config/src/types.rs#L46-L51 — Current source constants for memory retention, startup, idle, and rate-limit defaults.
[^config-types]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/config/src/types.rs#L258-L370 — Current source config keys, defaults, aliases, and clamping behavior for `MemoriesToml`.
[^features-source]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/features/src/lib.rs#L823-L830 — Feature flag metadata for `memories`, including default-disabled status and menu description.
[^state-db-paths]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/state/src/lib.rs#L78-L84 and https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/state/src/runtime.rs#L155-L230 — Source for `CODEX_SQLITE_HOME`, `memories_1.sqlite`, and runtime DB initialization under the configured Codex home.
[^sqlite-migration]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/state/memory_migrations/0001_memories.sql#L1-L35 — Initial memory DB migration for `stage1_outputs` and `jobs`.
[^runtime-usage]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/state/src/runtime/memories.rs#L49-L69 — Runtime usage accounting (`usage_count`, `last_usage`) for cited memory outputs.
[^runtime-claim]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/state/src/runtime/memories.rs#L130-L143 — Runtime comments for Phase 1 startup claim filtering and bounds.
[^runtime-selection]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/state/src/runtime/memories.rs#L411-L452 — Runtime comments/source for Phase 2 input selection and ranking.
[^runtime-phase2-lock]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/state/src/runtime/memories.rs#L1022-L1037 — Runtime comments for singleton global Phase 2 lock claim semantics.
[^runtime-heartbeat]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/state/src/runtime/memories.rs#L1167-L1207 — Runtime comments for Phase 2 heartbeat and successful completion behavior.
[^tui-memories]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/tui/src/bottom_pane/memories_settings_view.rs#L72-L128 — Source for `/memories` TUI menu items and reset text.
[^debug-clear]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/cli/src/main.rs#L1787-L1815 — Source implementation of undocumented `debug clear-memories`.
[^session-memory-mode]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/core/src/session/session.rs#L540-L589 — Source for thread persistence and `memory_mode` derived from `config.memories.generate_memories`.
[^extension-tools]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/ext/memories/src/extension.rs#L40-L110 — Source for memory prompt injection and dedicated memory tool gating.
[^memory-prompts]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/ext/memories/src/prompts.rs#L23-L49 — Source for reading/truncating `memory_summary.md` into developer instructions.
[^workspace-source]: https://github.com/openai/codex/blob/c57dee98b7e70f306d2981f9075dde1d1b9a90e7/codex-rs/memories/write/src/workspace.rs#L8-L45 — Source for git-baseline workspace preparation, diff writing, and baseline reset.
[^changelog]: https://developers.openai.com/codex/changelog — Official Codex changelog landing page; high-level release/change source checked for memory-related history.
[^config-sample]: https://developers.openai.com/codex/config-sample — Official sample configuration page fetched during research; it did not add memory-specific details beyond the config reference/basic docs cited above.
[^pr-11364]: https://github.com/openai/codex/pull/11364 — Official PR in the memory-v2 series, used for recent-history context.
[^pr-11365]: https://github.com/openai/codex/pull/11365 — Official PR in the memory-v2 series, used for recent-history context.
[^pr-11369]: https://github.com/openai/codex/pull/11369 — Official PR adding the Phase 1 stage schema and global memory-root direction in the memory-v2 series.
[^pr-12221]: https://github.com/openai/codex/pull/12221 — Official PR adding `rollout_summary_file` header and task/task_group/task_outcome schema refinements.
[^pr-12653]: https://github.com/openai/codex/pull/12653 — Official PR tightening the consolidation prompt and `MEMORY.md` task structure.
[^commit-git-diff]: https://github.com/openai/codex/commit/01ab25dbb5ffa5868266df0a7b870a601e19a2cd — Official commit introducing git-backed memory workspace diffs.
[^commit-stable-order]: https://github.com/openai/codex/commit/fa127be25ff547c950240c4bfe6c100c394880b2 — Official commit making `raw_memories.md` ordering stable by thread id.
[^commit-ad-hoc]: https://github.com/openai/codex/commit/70fc55b8f3ec7e1b6c49cf93b8cf5065a3435a31 — Official commit moving ad hoc note guidance under `extensions/ad_hoc/notes`.
[^release-132]: https://github.com/openai/codex/releases/tag/rust-v0.132.0 — Official release notes mentioning versioned memory summaries and stale-summary rebuild.
[^release-134]: https://github.com/openai/codex/releases/tag/rust-v0.134.0 — Official release notes mentioning app-server memory toggle updates.
