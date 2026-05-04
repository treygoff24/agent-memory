# Codex Agent + Skill Inventory — 2026-05-03

Ground-truth enumeration of every agent and skill available to a Codex orchestrator. Produced by direct file inspection of all paths listed in the task brief.

---

## Path Existence Summary

| Path                                                            | Exists?                           | Notes                                                   |
| --------------------------------------------------------------- | --------------------------------- | ------------------------------------------------------- |
| `~/.codex-shared/agents/`                                       | NO                                | Directory does not exist                                |
| `~/.codex-shared/skill-library/`                                | NO                                | Directory does not exist                                |
| `~/.codex-shared/skills/`                                       | NO                                | Directory does not exist                                |
| `~/.codex/agents/`                                              | YES                               | 34 `.toml` agent definitions                            |
| `~/.codex/skill-library/`                                       | YES                               | ~70 skill directories (library, most inactive)          |
| `~/.codex/skills/`                                              | YES                               | 10 symlinks = **globally active** skills                |
| `~/.codex/skills/.system/`                                      | YES                               | Codex runtime internals                                 |
| `/Users/treygoff/Code/llm-council/.codex/skills/`               | YES                               | 2 project skills (council-smoke, verify)                |
| `/Users/treygoff/Code/llm-council/.codex/hooks.json`            | YES                               | autonomous-loop hooks                                   |
| `/Users/treygoff/Code/llm-council/.codex/autoloop.project.json` | YES                               | Gate config (`pnpm check`)                              |
| `/Users/treygoff/Code/llm-council/.agents/skills/`              | YES                               | ~40 marketing/growth skills (loaded via skills list)    |
| `/Users/treygoff/Code/llm-council/.claude/agents/`              | NO                                | No project-scoped Claude agents                         |
| `/Users/treygoff/Code/llm-council/.claude/skills/verify/`       | YES                               | Claude mirror of verify skill                           |
| `~/.claude-shared/agents/`                                      | YES                               | 11 Claude subagent `.md` files                          |
| `~/.claude-shared/skills/`                                      | NO                                | Does not exist (skills live in `.claude/skill-library`) |
| `~/.claude/skill-library/`                                      | YES                               | Large library (~70+ skills)                             |
| `~/.agents/skill-library/`                                      | SAME as `~/.codex/skill-library/` | Symlink-equivalent content                              |

---

## 1. Agent Summary Table — Codex Scope (`~/.codex/agents/`)

All models are `gpt-5.5` unless noted.

| Agent Name (toml)                    | Model   | Reasoning | Sandbox            | Description                                                                                        | Key Skills                                         |
| ------------------------------------ | ------- | --------- | ------------------ | -------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| `worker`                             | gpt-5.5 | low       | workspace-write    | Main go-to implementation subagent for general coding tasks                                        | —                                                  |
| `heavy_worker`                       | gpt-5.5 | high      | danger-full-access | Moderately to highly complex impl, maximum autonomy                                                | —                                                  |
| `continuousclaudev4_7_worker`        | gpt-5.5 | high      | workspace-write    | Generic bounded step worker (ported from Claude); reads/updates task state                         | —                                                  |
| `continuousclaudev4_7_oracle`        | gpt-5.5 | high      | read-only          | External research — web search, docs, package analysis                                             | —                                                  |
| `reviewer`                           | gpt-5.5 | medium    | read-only          | Rigorous owner-minded code reviewer; 8-lens review; structured verdict/findings output             | —                                                  |
| `plan_reviewer`                      | gpt-5.5 | high      | read-only          | Reviews implementation plans before execution; validates DAG, file ownership, gate coverage        | spec-quality-checklist, writing-plans, write-human |
| `refactor_pilot`                     | gpt-5.5 | high      | workspace-write    | Behavior-preserving refactors in small dependency-ordered commits; refuses schema work             | refactor, clean-code                               |
| `ui_qa_driver`                       | gpt-5.5 | medium    | workspace-write    | Agent Browser browser QA; coverage matrix; issue packet creation                                   | —                                                  |
| `ui_fix_worker`                      | gpt-5.5 | medium    | workspace-write    | UI/UX fix from browser QA issue packets; loads clean-code, TDD, a11y skills                        | —                                                  |
| `ui_review_guard`                    | gpt-5.5 | medium    | read-only          | Read-only reviewer for UI fix batches; regressions/UX/a11y/design-system                           | —                                                  |
| `desloppify_comments`                | gpt-5.5 | high      | workspace-write    | Axis 8: remove AI slop / unhelpful / narrating comments                                            | clean-code                                         |
| `desloppify_cycles`                  | gpt-5.5 | high      | workspace-write    | Axis 4: circular module dependency breaking (madge/pycycle)                                        | refactor                                           |
| `desloppify_dead_code`               | gpt-5.5 | high      | workspace-write    | Axis 3: unused code removal (knip/ts-prune/vulture)                                                | refactor                                           |
| `desloppify_dedup`                   | gpt-5.5 | high      | workspace-write    | Axis 1: duplicate-logic consolidation                                                              | clean-code, refactor                               |
| `desloppify_defensive`               | gpt-5.5 | high      | workspace-write    | Axis 6: remove error-hiding try/catch and fake fallbacks                                           | clean-code                                         |
| `desloppify_legacy`                  | gpt-5.5 | high      | workspace-write    | Axis 7: dead flag branches, deprecated paths, "old way" removal                                    | refactor                                           |
| `desloppify_types`                   | gpt-5.5 | high      | workspace-write    | Axis 2: type definition consolidation, single source of truth                                      | clean-code                                         |
| `desloppify_weak_types`              | gpt-5.5 | high      | workspace-write    | Axis 5: `any`/`unknown`/untyped → strong types                                                     | clean-code                                         |
| `bug_diagnoser`                      | gpt-5.5 | xhigh     | workspace-write    | Rare-use deep bug diagnoser for nastiest failures; methodical reproduce/fix                        | —                                                  |
| `backend_arch`                       | gpt-5.5 | high      | workspace-write    | Designs backend module boundaries, contracts, and data flow                                        | —                                                  |
| `frontend_arch`                      | gpt-5.5 | medium    | workspace-write    | Scalable frontend architecture and UX consistency                                                  | —                                                  |
| `cli_developer`                      | gpt-5.5 | high      | workspace-write    | CLI ergonomics, flag/env precedence, automation contracts                                          | —                                                  |
| `mcp_developer`                      | gpt-5.5 | high      | workspace-write    | MCP servers, clients, tool schemas, protocol integrations                                          | —                                                  |
| `performance_engineer`               | gpt-5.5 | xhigh     | workspace-write    | Latency, throughput, memory, runtime cost improvements                                             | —                                                  |
| `security_auditor`                   | gpt-5.5 | xhigh     | read-only          | Auth, injection, secret, permission risk finding                                                   | —                                                  |
| `test_hardener`                      | gpt-5.5 | high      | workspace-write    | Closes test gaps, hardens edge-case coverage                                                       | —                                                  |
| `code_mapper`                        | gpt-5.5 | medium    | read-only          | High-confidence maps of owning paths, execution flow, branch points                                | —                                                  |
| `docs_researcher`                    | gpt-5.5 | medium    | read-only          | Verifies library/SDK/CLI behavior against current primary docs                                     | —                                                  |
| `prompt_engineer`                    | gpt-5.5 | medium    | read-only          | Prompt/instruction contract improvement for reliable model behavior                                | —                                                  |
| `postgres_pro`                       | gpt-5.5 | high      | read-only          | PostgreSQL schema, planner, locking, indexing, migration behavior                                  | —                                                  |
| `product_analyst`                    | gpt-5.5 | medium    | read-only          | Converts product goals into execution-ready requirements                                           | —                                                  |
| `atlasos_assistant_contract_checker` | gpt-5.5 | high      | read-only          | **atlasos-scoped**: validates TS types, Zod contracts, Postgres enums, tool registry sync          | —                                                  |
| `atlasos_migration_shepherd`         | gpt-5.5 | high      | workspace-write    | **atlasos-scoped**: SQL migration + Drizzle TS + Zod + service types sync                          | —                                                  |
| `atlasos_error_detective`            | gpt-5.5 | high      | read-only          | **atlasos-scoped**: correlates failure signals across run ledger, provider adapters, Supabase logs | —                                                  |

---

## 2. Agent Summary Table — Claude Scope (`~/.claude-shared/agents/`)

These are invoked from Claude Code, not from Codex directly, but available in parallel review workflows.

| Agent Name              | Model   | Tools                               | Description                                                                                     | Preloaded Skills                                   |
| ----------------------- | ------- | ----------------------------------- | ----------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| `plan-reviewer`         | opus    | Read, Grep, Glob, Bash              | Adversarial plan reviewer before execution; validates DAG, ownership, invariant-to-test mapping | spec-quality-checklist, writing-plans, write-human |
| `refactor-pilot`        | inherit | Read, Edit, Write, Grep, Glob, Bash | Behavior-preserving refactors; never touches schema/migrations                                  | refactor, clean-code                               |
| `worker`                | —       | Read, Edit, Write, Bash, Grep, Glob | Generic pipeline step worker; reads task state, does work, updates state                        | —                                                  |
| `desloppify-comments`   | inherit | Read, Edit, Grep, Glob, Bash        | Axis 8: slop/unhelpful comment removal                                                          | clean-code                                         |
| `desloppify-cycles`     | inherit | Read, Edit, Grep, Glob, Bash        | Axis 4: circular dependency breaking                                                            | refactor                                           |
| `desloppify-dead-code`  | inherit | Read, Edit, Grep, Glob, Bash        | Axis 3: unused code elimination                                                                 | refactor                                           |
| `desloppify-dedup`      | inherit | Read, Edit, Grep, Glob, Bash        | Axis 1: duplicate logic consolidation                                                           | clean-code, refactor                               |
| `desloppify-defensive`  | inherit | Read, Edit, Grep, Glob, Bash        | Axis 6: error-hiding defensive code removal                                                     | clean-code                                         |
| `desloppify-legacy`     | inherit | Read, Edit, Grep, Glob, Bash        | Axis 7: deprecated/dead-flag-branch removal                                                     | refactor                                           |
| `desloppify-types`      | inherit | Read, Edit, Grep, Glob, Bash        | Axis 2: type consolidation                                                                      | clean-code                                         |
| `desloppify-weak-types` | inherit | Read, Edit, Grep, Glob, Bash        | Axis 5: `any`/`unknown` → strong types                                                          | clean-code                                         |

---

## 3. Skill Inventory

### 3a. Globally Active Codex Skills (`~/.codex/skills/` symlinks)

These are always preloaded in every Codex session.

| Skill Name               | Library Path                           | What It Does                                                                                              |
| ------------------------ | -------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `autonomous-loop`        | `skill-library/autonomous-loop`        | Prevents premature exit; enforces plan completion via stop hooks; integrates with `autonomous-loop` CLI   |
| `brainstorming`          | `skill-library/brainstorming`          | Structured ideation and creative problem exploration                                                      |
| `claude-review`          | `skill-library/claude-review`          | Delegates a read-only review pass to Claude via `claudish` CLI                                            |
| `claude-second-opinion`  | `skill-library/claude-second-opinion`  | Second-opinion analysis pass routed to Claude from Codex                                                  |
| `claude-worker`          | `skill-library/claude-worker`          | Delegates bounded implementation to Claude via `claudish` CLI                                             |
| `exa-search`             | `skill-library/exa-search`             | Web search via Exa API for grounded research                                                              |
| `parallel-cli`           | `skill-library/parallel-cli`           | Multi-agent parallel task execution patterns                                                              |
| `spec-quality-checklist` | `skill-library/spec-quality-checklist` | Validates specs for completeness, precision, testability before planning                                  |
| `write-human`            | `skill-library/write-human`            | Human-readable writing/documentation generation                                                           |
| `writing-plans`          | `skill-library/writing-plans`          | Implementation plan authoring with exact file paths, parallel/blocked-by structure, owned-file validation |

### 3b. Project-Active Skills — llm-council (`.codex/skills/` + `.agents/skills/`)

Active via `.codex/skills/.claude-sync-allow`:

| Skill Name        | Location                                                           | What It Does                                                                                           |
| ----------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `verify`          | `.codex/skills/verify/` and `.claude/skills/verify/`               | Runs full verification suite (`pnpm check`): format, lint, typecheck, test, build, agentlint, specgate |
| `council-smoke`   | `.codex/skills/council-smoke/` and `.agents/skills/council-smoke/` | Paid Tier 1 council smoke; intake-dialogue benchmark; preserves dogfood artifacts                      |
| `agent-browser`   | `.agents/skills/` via allow-list                                   | Vercel agent browser QA default path                                                                   |
| `clean-code`      | `.agents/skills/` via allow-list                                   | Code quality enforcement                                                                               |
| `desloppify-deep` | `.agents/skills/` via allow-list                                   | 8-subagent parallel codebase cleanup coordinator (gate-free subagents + one coordinator gate)          |

### 3c. Skill Library (Inactive — Available On-Demand) (`~/.codex/skill-library/`)

Load with `claude-skill add <name>` or reference directly.

| Skill Name                | Description Summary                                                                                                                                                                                                                | Relevant to Hunt?                     |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------- |
| `game-sprites`            | End-to-end sprite/sprite-sheet pipeline: lock contract → generate identity ref → generate animation canvas → per-frame background removal → normalize/pack via `sprite_tools.py` → verify. Includes `gpt-image-2` constraints doc. | YES — **PRIMARY sprite skill**        |
| `image-prompting`         | Subject-Context-Style prompt structure for any image model. Covers lighting, composition, text-in-images, editing, character consistency.                                                                                          | YES — image gen wrapper               |
| `canvas-design`           | Museum-quality design philosophy → PNG/PDF canvas creation using code. Two-step: manifesto then rendered artifact.                                                                                                                 | Partial — visual design               |
| `algorithmic-art`         | Generative/algorithmic art via p5.js: philosophy → seeded-random implementation → self-contained interactive HTML artifact with controls                                                                                           | Partial — generative art, not sprites |
| `react-three-fiber`       | Production R3F apps: Top 20 power moves, useFrame, Drei, Zustand, Rapier physics, postprocessing, performance checklist                                                                                                            | YES — 3D/animation                    |
| `glsl-shaders`            | GLSL shader creation/debugging/optimization for Three.js. ShaderMaterial patterns, procedural techniques, visual effects                                                                                                           | YES — visual effects                  |
| `desloppify-deep`         | 8-axis coordinator skill. Spawns all 8 desloppify-\* subagents gate-free; coordinator runs gate once. CPU discipline built in.                                                                                                     | YES — code review                     |
| `vercel-ai-sdk`           | AI SDK v6 best practices: `generateText`, `streamText`, `useChat`, tool calling with `inputSchema`, `stopWhen` agents, `Output.object` (not deprecated `generateObject`), provider switching                                       | YES — AI SDK expertise                |
| `webapp-testing`          | Playwright Python scripts for local web app testing; `with_server.py` helper; reconnaisance-then-action pattern                                                                                                                    | YES — visual QA (Playwright-based)    |
| `ui-qa-swarm`             | Multi-agent UI QA loop: lead in Agent Browser → fixer → reviewer → verify. Coverage matrix, issue packets, autonomous-loop gating                                                                                                  | YES — visual QA coordination          |
| `vercel-agent-browser`    | Vercel agent browser as default QA tool (NOT Playwright). Visual verification, screenshot capture, UX bug repro                                                                                                                    | YES — visual QA                       |
| `spec-quality-checklist`  | Already globally active. Precision/completeness/testability validator                                                                                                                                                              | YES — spec/plan                       |
| `writing-plans`           | Already globally active. Structured impl plan authoring                                                                                                                                                                            | YES — spec/plan                       |
| `autonomous-loop`         | Already globally active                                                                                                                                                                                                            | YES — loop control                    |
| `slop-cleaner`            | Single-pass code quality cleanup                                                                                                                                                                                                   | YES — code review                     |
| `refactor`                | Behavior-preserving refactor patterns                                                                                                                                                                                              | YES — code quality                    |
| `accessibility-checklist` | WCAG, keyboard, focus, ARIA, semantic HTML checklist                                                                                                                                                                               | UI QA                                 |
| `frontend-delight`        | Visual hierarchy, polish, rendered signoff                                                                                                                                                                                         | UI QA                                 |
| `debugging-systematic`    | Systematic root-cause debugging workflow                                                                                                                                                                                           | debug                                 |
| `checkpoint`              | State checkpoint / handoff creation                                                                                                                                                                                                | workflow                              |
| `create-handoff`          | Session handoff document creation                                                                                                                                                                                                  | workflow                              |
| `orchestrator`            | Orchestrator coordination pattern                                                                                                                                                                                                  | workflow                              |
| `claude` / `codex`        | Interaction skills for cross-model delegation                                                                                                                                                                                      | workflow                              |
| `threejs`                 | Three.js scene building patterns                                                                                                                                                                                                   | 3D                                    |
| `blender-3d`              | Blender 3D operations                                                                                                                                                                                                              | 3D                                    |
| `vanilla-web-dev`         | Plain HTML/CSS/JS dev without frameworks                                                                                                                                                                                           | web                                   |
| `exa-search`              | Already globally active                                                                                                                                                                                                            | research                              |
| `autoresearch`            | Autonomous deep research workflow                                                                                                                                                                                                  | research                              |
| `brainstorming`           | Already globally active                                                                                                                                                                                                            | ideation                              |
| `premortem`               | Adversarial risk analysis before execution                                                                                                                                                                                         | planning                              |
| `triage-issue`            | GitHub issue triage workflow                                                                                                                                                                                                       | project mgmt                          |
| `github-triage`           | GitHub issue and PR triage                                                                                                                                                                                                         | project mgmt                          |
| `to-prd`                  | Convert brief → product requirements doc                                                                                                                                                                                           | spec                                  |
| `domain-model`            | Domain model / ubiquitous language extraction                                                                                                                                                                                      | spec                                  |
| `skill-creator`           | Create new skills                                                                                                                                                                                                                  | meta                                  |

### 3d. Marketing/Growth Skills (`.agents/skills/` in llm-council)

~40 skills covering: `ab-test-setup`, `ad-creative`, `ai-seo`, `analytics-tracking`, `aso-audit`, `brand-storytelling`, `churn-prevention`, `cold-email`, `community-marketing`, `competitor-alternatives`, `competitor-profiling`, `content-marketing`, `content-strategy`, `copy-editing`, `copywriting`, `customer-persona`, `customer-research`, `designing-growth-loops`, `directory-submissions`, `email-design`, `email-sequence`, `find-skills`, `form-cro`, `free-tool-strategy`, `image` (marketing image gen), `landing-page-design`, `launch-strategy`, `lead-magnets`, `marketing-ideas`, `marketing-psychology`, `og-image-design`, `onboarding-cro`, `page-cro`, `paid-ads`, `paywall-upgrade-cro`, `popup-cro`, `positioning-messaging`, `pricing-strategy`, `product-hunt-launch`, `product-marketing-context`, `programmatic-seo`, `referral-program`, `revops`, `sales-enablement`, `schema-markup`, `seo-audit`, `signup-flow-cro`, `social-content`, `video`.

These are marketing-facing; not relevant to the technical implementation multi-wave plan except as context.

---

## 4. Capability Matrix

| Hunt Target                                                        | Covered?             | Agents                                                                 | Skills                                                                                                                       | Notes                                                                                                                                                                                                                                                                                     |
| ------------------------------------------------------------------ | -------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Sprite / pixel-art / character / animation generation**          | YES                  | `heavy_worker`, `worker`                                               | `game-sprites` (primary), `image-prompting`                                                                                  | `game-sprites` has full production pipeline + `sprite_tools.py` + `gpt-image-2` constraints doc. Load `image-prompting` first for strong prompt structure.                                                                                                                                |
| **Image-gen workflow wrappers (gpt-image-2, mcp-image, composio)** | PARTIAL              | `heavy_worker`                                                         | `game-sprites` (references `imagegen` native tool), `image-prompting`, `og-image-design`                                     | Codex has a native `imagegen` tool (wraps `gpt-image-2`). `game-sprites` documents its constraints. `mcp-image` is available as a Claude-side MCP tool. No dedicated Codex "image-gen wrapper" skill exists beyond `game-sprites`.                                                        |
| **Visual QA / screenshot diff**                                    | YES                  | `ui_qa_driver`, `ui_fix_worker`, `ui_review_guard`                     | `vercel-agent-browser`, `ui-qa-swarm`, `webapp-testing`                                                                      | `ui-qa-swarm` coordinates the full loop. `vercel-agent-browser` is the default path over Playwright. `webapp-testing` is Playwright Python (fallback). No Percy/visual-diff tool; the workflow is screenshot + human-in-loop via `ui_review_guard`.                                       |
| **Code review / clean-code passes**                                | YES (deep)           | `reviewer`, `desloppify_*` (×8), `refactor_pilot`, `ui_review_guard`   | `desloppify-deep`, `slop-cleaner`, `refactor`, `clean-code`                                                                  | `desloppify-deep` coordinator + 8 specialist subagents is the full-pass option. `reviewer` for single-diff review. All available in both Codex and Claude scopes.                                                                                                                         |
| **Spec / plan writing**                                            | YES                  | `plan_reviewer` (Codex + Claude), `product_analyst`, `prompt_engineer` | `writing-plans`, `spec-quality-checklist`, `premortem`, `to-prd`                                                             | `writing-plans` produces task-DAG plans. `spec-quality-checklist` validates. `plan_reviewer` agent does adversarial review. `premortem` for risk analysis.                                                                                                                                |
| **Schema / migration shepherding**                                 | PARTIAL (wrong repo) | `atlasos_migration_shepherd`, `postgres_pro`                           | —                                                                                                                            | `atlasos_migration_shepherd` exists but is scoped to the `atlasos` project (Drizzle/Supabase). For `llm-council` there is NO project-scoped migration agent. `postgres_pro` covers review. A new `llm-council-migration-shepherd` agent needs to be defined or the task briefed manually. |
| **AI SDK v6 / streaming UI**                                       | YES                  | `heavy_worker`, `frontend_arch`, `worker`                              | `vercel-ai-sdk` (full v6 coverage: `generateText`, `streamText`, `useChat`, tool `inputSchema`, `stopWhen`, `Output.object`) | `vercel-ai-sdk` skill is in the library (not globally active). Load it explicitly. It documents the v6 API including the breaking `generateObject` → `output` migration.                                                                                                                  |
| **Test authoring / test runner agents**                            | YES                  | `test_hardener`, `bug_diagnoser`, `worker`                             | `verify` (project-active), `webapp-testing`, `tdd` (in `.agents/skills/`)                                                    | `test_hardener` for gap closure. `verify` runs `pnpm check`. `webapp-testing` for Playwright-based functional tests.                                                                                                                                                                      |

---

## 5. Codex CLI Capabilities

### How Codex loads skills

- **Global skills** (`~/.codex/skills/`): symlinks to entries in `~/.codex/skill-library/`. Every SKILL.md in this folder is injected into the session context. Currently 10 skills are globally active.
- **Project skills** (`.codex/skills/` in the repo): SKILL.md files are injected when Codex opens that project. llm-council has `verify` and `council-smoke` here.
- **`.claude-sync-allow` file**: lists which Claude skills may be made prompt-visible in Codex. For llm-council: `agent-browser`, `clean-code`, `verify`, `council-smoke`, `desloppify-deep`.
- **Library** (`~/.codex/skill-library/`): available but not injected. An orchestrator can instruct subagents to load a skill by name, or the user can activate with the Codex skills UI or `claude-skill add`.
- **No `codex --version`**: `codex` is a shell function (`_ai_run_codex`), not a bare CLI with `--help`/`--version`. Codex is the GUI application running on this machine.
- **Manifest**: no separate manifest file; the skills system uses folder-presence + `SKILL.md` as the convention.

### Image generation in Codex

- Codex has a native **`imagegen` tool** built in (wraps `gpt-image-2`). This is a first-class tool available in Codex sessions — not a plugin or skill.
- `game-sprites` skill documents `gpt-image-2` constraints for this tool.
- Key constraint: `gpt-image-2` does NOT support `background: "transparent"`. Alpha requires post-processing.
- The `mcp-image` server (available on the Claude side) uses Gemini for image generation and editing.
- No standalone "image-gen wrapper skill" exists; the canonical workflow is: load `image-prompting` + `game-sprites`, then call the native `imagegen` tool directly.

### Autonomous-loop / headless invocation

- `autonomous-loop` CLI is installed at `~/.venv/bin/autonomous-loop`.
- llm-council project has it configured: `SessionStart` and `Stop` hooks run the autonomous-loop daemon.
- Gate command: `pnpm check`. Fast/final/default profiles all map to the same `pnpm check`.
- Max stop iterations: 12. Max repeated failure signature: 3.

### Agent invocation (Codex)

- Agents are invoked as subagents via the multi_agent feature (all agents have `multi_agent = true`).
- Codex spawns them by name; the orchestrator waits via `wait_agent`.
- AGENTS.md guidance: be patient with `gpt-5.5` high subagents (5–10 min wait); xhigh (10–15 min).

---

## 6. Gaps and Install Actions

### Confirmed Gaps

| Gap                                                 | Severity | Fix                                                                                                                                                                                             |
| --------------------------------------------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **No llm-council migration shepherd**               | Medium   | Port `atlasos_migration_shepherd` to a `llm-council` variant targeting `packages/persistence/`, `supabase/migrations/`, and `packages/contracts/`. Or brief manually per task.                  |
| **No visual-diff / Percy-style tool**               | Low      | No automated pixel-diff workflow exists. Current approach is screenshot + `ui_review_guard` human-in-loop. Acceptable for now; could add visual-regression Playwright plugin if needed.         |
| **`vercel-ai-sdk` skill not globally active**       | Low      | Run `claude-skill add vercel-ai-sdk` to activate globally, or load per-task.                                                                                                                    |
| **`game-sprites` not globally active**              | Low      | Load per-task via `claude-skill add game-sprites`. No need to global-activate unless sprite work is ongoing.                                                                                    |
| **`ui-qa-swarm` not globally active**               | Low      | Load per-task.                                                                                                                                                                                  |
| **No Lottie/animation-frames skill**                | Gap      | No Lottie, animation-frames, or CSS animation skill exists. Closest: `react-three-fiber` (3D animation), `glsl-shaders` (WebGL effects), `algorithmic-art` (p5.js generative).                  |
| **`atlasos_*` agents are scoped to atlasos**        | Info     | The three atlasos-ported agents (`migration_shepherd`, `assistant_contract_checker`, `error_detective`) will read atlasos file paths. Do not use them for llm-council work without re-briefing. |
| **No dedicated "API integration / Supabase" skill** | Low      | `postgres_pro` covers schema review. Supabase-specific patterns live in `atlasos_migration_shepherd` instructions.                                                                              |

### Install Actions

```bash
# Activate skills that will be needed for planned work
claude-skill add game-sprites        # sprite generation pipeline
claude-skill add vercel-ai-sdk       # AI SDK v6 expertise
claude-skill add ui-qa-swarm         # UI QA swarm coordination
claude-skill add image-prompting     # strong prompts before calling imagegen

# These are already project-active in llm-council:
# - verify, council-smoke (via .codex/skills/)
# - agent-browser, clean-code, desloppify-deep (via .claude-sync-allow)
```

---

## 7. Recommendations by Wave

### Wave: Sprite Generation

**Orchestrator** (root Codex session):

- Load skills: `game-sprites`, `image-prompting`, `autonomous-loop`
- Assign to: `heavy_worker` (sandbox: danger-full-access, gives imagegen tool access)
- Workflow: `image-prompting` → lock contract → generate identity ref with `imagegen` → generate animation canvas → `sprite_tools.py normalize` → verify
- Key constraint to brief: `gpt-image-2` does NOT support transparent background — post-process alpha per frame

**Review pass**:

- `reviewer` agent: check pixel dimensions, frame count, alpha correctness, engine-export format
- `ui_review_guard`: visual spot-check of the contact sheet

### Wave: Visual QA Loop

**Lead/Orchestrator**: Load `ui-qa-swarm` + `autonomous-loop` + `vercel-agent-browser`

- Use `ui_qa_driver` for parallel exploration
- Use `ui_fix_worker` for fixes (load `clean-code`, `vercel-ai-sdk` if AI SDK UI changes)
- Use `ui_review_guard` for code review of each fix batch
- Root verifies each fix in agent browser before closing

### Wave: Multi-Wave Code Review

**Primary option**: `desloppify-deep` coordinator (load skill) + 8 `desloppify_*` Codex agents

- Brief all 8 subagents to SKIP heavy gates (no `pnpm check`, no `vitest`)
- Coordinator runs `pnpm check` once at end
- Per-axis light checks: `npx tsc --noEmit`, `npm run lint`

**Single-diff code review**: `reviewer` Codex agent (read-only, medium reasoning)

**Cross-system review**: `claude-review` skill → delegates to Claude via `claudish`

### Wave: Spec / Plan Writing

- Author plan: load `writing-plans` skill (globally active); save to `docs/plans/YYYY-MM-DD-<name>.md`
- Validate spec: load `spec-quality-checklist` skill (globally active)
- Adversarial review: spawn `plan_reviewer` Codex agent (read-only, high reasoning)
- Risk pre-mortem: load `premortem` skill from library
- **Claude parallel**: spawn `plan-reviewer` Claude agent (opus model) for higher-quality adversarial read

### Wave: AI SDK / Streaming UI Implementation

- Primary worker: `heavy_worker` or `worker` with `vercel-ai-sdk` skill loaded
- Key points from skill: use `inputSchema` (not `parameters`) for tools; use `Output.object` (not `generateObject`); always provide `onError` for `streamText`; set `stopWhen` with a limit
- For UI layer: `frontend_arch` agent + `vercel-ai-sdk` skill

### Wave: Schema / Migration Work (llm-council)

- No dedicated agent exists for llm-council. Options:
  1. Brief `heavy_worker` manually with the llm-council schema paths and the same workflow as `atlasos_migration_shepherd` (SQL → Drizzle → Zod → types → tests → `pnpm check`)
  2. Create a `llm-council-migration-shepherd` agent by porting the atlasos one (change project scope, file paths, gate command to `pnpm check`)
- `postgres_pro` (read-only) for schema review
- `verify` skill for gate

---

## Appendix: Codex Agent Loading Mechanism

Codex agents live in `~/.codex/agents/` as `.toml` files. Fields that matter for orchestration:

- `model`: always `gpt-5.5` here
- `model_reasoning_effort`: `low` / `medium` / `high` / `xhigh` — affects cost and depth
- `sandbox_mode`: `read-only` / `workspace-write` / `danger-full-access`
- `approval_policy`: `never` / `on-request` — whether Codex pauses for approval on shell commands
- `[features] multi_agent = true` — enables spawning subagents
- `[features] shell_tool = true` — enables Bash/shell execution
- Skills listed in `developer_instructions` frontmatter are the agent's documented preloads; in Codex these must be explicitly named in the task brief or the SKILL.md content included in the prompt — there is no automatic preload from toml fields alone

Skill loading in a subagent task: tell the subagent explicitly "load `game-sprites` skill" or paste the SKILL.md content into the brief. The `.codex/skills/` symlinks are globally available and auto-injected; library skills are not.
