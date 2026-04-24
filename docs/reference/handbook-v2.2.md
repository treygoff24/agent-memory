# Agent harness memory, context, and state engineering handbook, v2.2

This is the transfer document I'd want in front of any serious agent, human or model, before asking it to design or extend the memory and state stack for a new harness. Its job is narrow. Drop one file into a project, say "read this first," and the other agent understands not just a bag of features but the worldview behind them: what memory is for, how context differs from memory, why compaction fails, why large tool outputs quietly destroy performance, why subagents without persistence stay stupid, why identity and relationship context change capability, why untrusted inputs must never directly promote themselves into durable belief, why every memory needs an evaluation story, and why sleep-time synthesis is not the same thing as janitorial maintenance.

v2.2 is v2.1 with a set of v1 restorations added back after a fresh-context audit surfaced gaps. The core architecture, governance chapter, evaluation chapter, lifecycle framing, retrieval pattern menu, cache-stable assembly, prospective memory, and cost modeling from v2 and v2.1 all survive unchanged. What v2.2 adds back is a handful of specific v1 artifacts whose absence the audit flagged as real regressions: a concrete context-engine `register(api)` starter skeleton, a `validateFrontmatter` code snippet, a full archive-sidecars-and-DAG-history section with the three bounded session tools (`session_grep`, `session_describe`, `session_expand`), a specific operational war story about subagent memory tools being silently denied, the three-pass dreaming design frame and the substrate-versus-journal distinction, a single-paragraph "how this all connects" causal walk, a "match the layer to the symptom" closing cascade, and a status-of-the-custom-stack table.

The earlier version-history notes are still worth keeping. v2 added governance, evaluation, lifecycle, expanded retrieval, cache-stable assembly, prospective memory, and cost modeling. v2.1 corrected a handful of facts against live documentation (the bootstrap caps were the biggest), added named attack families, benchmarks, and framework-landscape specifications, and reframed several chapters to acknowledge that the mature production implementations the handbook describes don't yet fully exist. The schema below is a *target architecture* informed by what shipping systems have partially implemented, not a documentation of current industry standard.

v2.2 also reflects something I learned running the research loops behind v2 and v2.1. Multiple AI-mediated research passes, including my own verification against Context7, each surfaced real signal and each introduced at least one confident error. The live vendor documentation was the only consistently reliable source. That lesson has its own section below, because the same discipline the governance chapter recommends for an agent's memory applies to the construction of this handbook and to anyone trying to adapt it.

## How this document is organized

The sequencing is not the same as v1. Governance and evaluation moved earlier in v2 and stay there in v2.1. Without a trust model and a test harness, everything downstream is confident on a weak foundation. You will run passive recall on untrusted memory, compact an investigation around a poisoned summary, promote a synthesis-time hallucination into durable belief, and have no way to catch any of it.

The order now:

1. A note on verification.
2. Mental model and vocabulary.
3. The worldview.
4. What OpenClaw ships by default.
5. Memory governance, trust, and safety.
6. Lifecycle and temporal validity.
7. Evaluation.
8. Status of the custom stack.
9. Structured memory substrate.
10. Entity index and passive recall.
11. Compaction as state reconstruction.
12. Archive sidecars, DAG history, and bounded session tools.
13. Tool output guards and artifact stubs.
14. Subagent continuity.
15. Retrieval architecture and the pattern menu.
16. Context assembly and cache stability.
17. Prospective memory.
18. Cost modeling.
19. Sleep-time synthesis and dreaming.
20. Failure modes we learned the hard way.
21. Current framework landscape.
22. Implementation phasing, revised.
23. How this all connects as one coherent system.
24. What to tell the next agent.
25. Match the layer to the symptom.
26. Source map.

## A note on verification

Three independent research passes went into this handbook: an adversarial critique from GPT 5.5 Pro, a Gemini Deep Research pass, and a GPT Deep Research pass, plus my own fact-checks via Context7-indexed documentation and live web fetches. Each pass surfaced real signal. Each pass also produced at least one confident error that would have ended up in the handbook if I'd trusted it uncritically.

GPT 5.5 Pro hallucinated specific GitHub issue numbers and claimed the OpenClaw bootstrap caps had changed to 12000/60000 with the 20000/150000 values being "older." Gemini independently hallucinated the same caps, plus invented a framework ("Pith") as a companion to a real one (SuperLocalMemory), inverted the direction of the Mem0-vs-Zep audit controversy, and embellished a Voyage model description. My own Context7 verification against version `v2026.4.9` returned the 20000/150000 values directly from the repo tag's configuration reference and I trusted that. GPT Deep Research then correctly flagged that the live documentation site says 12000/60000, and a fresh WebFetch against `docs.openclaw.ai` confirmed the live docs. The Context7-indexed repo snapshot was stale.

GPT Deep Research had its own errors, though fewer. It claimed `prepareSubagentSpawn` in the OpenClaw context engine is part of the interface but not yet invoked by the runtime. A direct fetch against `docs.openclaw.ai/concepts/context-engine` says "OpenClaw calls two optional subagent lifecycle hooks," naming both. GPT was wrong on that one specific claim.

The general pattern: AI-mediated research in 2026 is good enough to accelerate a handbook like this by weeks, and nowhere near reliable enough to trust without verification. The failure modes cluster:

Pinned source versions in static indexes can drift from the live documentation site. Treat version tags, Context7 snapshots, and any other indexed source as potentially stale. The live site wins.

Specific numbers that look authoritative in Deep Research output may be correct or may be confabulated. Verify direction of attribution on every citation, not just existence of the cited entity. The Mem0-vs-Zep dispute is the clean example: the 58.44% number was real, but Gemini's report had the wrong party auditing the wrong system.

Invented secondary entities paired with real primary entities are a characteristic hallucination pattern. SuperLocalMemory is real; Gemini's claimed companion "Pith" isn't. Voyage-3 is real; the specific description Gemini attributed to it belongs to a different Voyage model.

Pattern-matched "plausible" numbers across models. Both GPT 5.5 Pro and Gemini confidently produced the same wrong OpenClaw bootstrap caps, suggesting a shared training-data contamination. Independent verification against primary sources is the only defense.

The operational rule this produced, which applies both to running the handbook and to extending it: treat every AI-generated research artifact, including your own earlier verification passes, as candidates that need primary-source confirmation before promotion into durable belief. This is the same propose-versus-promote pattern the governance chapter argues for an agent's memory. It applies recursively to the handbook itself. The handbook practicing what it preaches.

When the handbook cites a specific number, benchmark score, attack family, or vendor feature, it was checked against a primary source as of April 2026. When a claim is older than twelve months, it's marked "aging." When older than twenty-four, "likely stale." When the public record has contradictions (the OpenClaw documentation drift story is one), the handbook names the contradiction rather than silently picking one side. If something here is wrong, the most likely cause is that the underlying documentation changed after this writing. Re-verify before betting on any specific value.

## The core thesis

The shortest honest version: context is capability. Not in the vague prompt engineering sense. In the literal operational sense. The same base model, with the same tools, can behave like a temp worker who drops half the checklist or like an invested operator who follows through, coordinates subtasks, remembers prior failures, preserves the real objective, and finishes the job. The difference is often not the model. It's the context architecture around the model.

One caveat this version adds: *more* context is not the same as *better* context. "Lost in the Middle" work (Liu et al., arXiv 2307.03172) showed models underusing relevant information depending on where it appeared in long context. Newer work sharpened the point: even with perfect retrieval, longer input contexts can degrade performance inside nominal context limits. The implication is not "carry everything forward because you can." It's the opposite. Context discipline matters *more* as windows grow. Compaction, artifact guards, and cache-stable assembly aren't optional for long-horizon sessions. They're how you make a large window actually pay off.

The architecture that realizes "context is capability" has several layers. Identity and operating context at the base. Durable memory with governance above that. Retrieval and passive recall as the surfacing mechanism. Transcript management, including artifacting and compaction, for long sessions. Lifecycle, supersession, and temporal validity for evolving truth. Cache-aware context assembly at the point of use. Maintenance and synthesis running in the background. Evaluation wrapping all of it so you know when something breaks. Get these right and the agent stops spending its reasoning budget rediscovering the environment and starts spending it on the task.

## The vocabulary that matters

Use these words precisely. Most failures come from crossing these wires.

| Term | Meaning | What it is not |
| --- | --- | --- |
| Context | Everything the model sees on a run | Durable memory on disk |
| Memory | Durable state stored outside the immediate run window | The current prompt |
| Retrieval | An explicit query over stored memory | Automatic recall |
| Passive recall | Automatic surfacing of relevant memory before the model asks for it | General semantic search |
| Compaction | Persisted summary of older session history to free context space | Simple in-memory trimming |
| Pruning | In-memory removal of bulky or stale tool content for a run | Persisted transcript rewrite |
| Artifact guard | Logic that offloads giant tool outputs out of the active transcript and prompt | Compaction |
| Working memory | Active project and task state that must stay warm during execution | Long-term semantic memory |
| Prospective memory | Future commitments: follow-ups, conditional reminders, deadlines, triggers | A to-do list on the side |
| Dreaming | Scheduled synthesis over memory and recent activity | Cleanup cron |
| Maintenance | Deterministic cleanup, deduplication, indexing, archival | Reflective synthesis |
| Context engine | The runtime layer that decides what gets assembled into model context and how compaction is handled | The memory backend |
| Trust tier | A memory's status: candidate, active, pinned, superseded, archived, tombstoned | Retrieval score |
| Provenance | The origin chain of a memory: who proposed it, from what source, under what authority | Author metadata |
| Temporal validity | The window during which a memory is considered current | The creation timestamp |
| Tombstone | An active rule blocking regeneration or recall of a deleted claim | Soft delete |
| Cache-stable assembly | Context compilation designed to preserve prefix caching across turns | Any static prompt |
| Candidate memory | A proposed memory that has not been promoted to trusted status | Active memory |
| Promotion | The authorized transition from candidate to trusted | A save operation |

If a project starts blurring these boundaries, stop and separate them again.

## The worldview

### Solve data quality at write time, not retrieval time

Most memory systems store unstructured text and then do heroic work later to infer structure. Entity extraction, summarization, graph building, relevance estimation, and post-hoc cleanup all happen after the memory is already low quality. That's backwards. When the agent writes the memory, it has the most context it will ever have. It knows why the thing matters, which project it belongs to, who is involved, which files are related, and what the one-sentence summary should be. Later pipelines guess.

So we force structure at write time. Frontmatter is not cosmetic metadata. It's the foundation that makes retrieval, indexing, passive recall, and graph traversal cheap and reliable.

One refinement v1 didn't make explicit: this is a *bias*, not an exclusivity rule. Real systems still need read-time validation, contradiction detection, source scoring, and memory evolution because real systems ingest messy data, external documents, tool outputs, and user corrections after the fact. A-MEM (Xu et al., arXiv 2502.12110), Mem0 (arXiv 2504.19413), and Zep/Graphiti (arXiv 2501.13956) all lean into post-write processing for exactly this reason. Write-time discipline is the first line of defense. It's not the only line.

### Separate memory systems by access pattern

A flat memory directory looks simple until it isn't. Episodic memory, semantic memory, procedural memory, project memory, prospective memory, and agent memory are different things. They change at different rates, they get queried differently, and they benefit from different maintenance policies. We mirrored this in the directory structure because the structure itself becomes part of the retrieval system.

### Retrieval alone is not enough

Memory systems technically work but still feel forgetful because the agent has to remember to search. Humans don't mostly operate that way. A name, a place, a project, a phrase, and connected context bubbles up. The agent needs the equivalent. That's passive recall.

### Memory and compaction should be collaborators

Compaction is destructive. Memory is reconstructive. If they ignore each other, you either compact too cautiously and waste the window, or compact aggressively and lose the thread. Once durable memory and passive recall exist, compaction can be much more strategic. Facts that live safely in memory don't have to survive in transcript detail. What must survive are active task state, unresolved questions, critical identifiers, and the evidence needed to keep the session honest. ReSum (Sun et al., arXiv 2509.13313) makes the same argument at the agent-history level: long-horizon histories are better treated as compact reasoning states than as ordinary chat summaries.

### Preserve state, not sludge

The model doesn't need thirty thousand characters of console spam in its live prompt. It needs the command, the exit code, the key error lines, the filenames, the URLs, the counts, and a handle back to the raw output if needed. This principle is bigger than exec logs. It applies to browser snapshots, process logs, web search payloads, sessions history dumps, and large reads. Giant raw outputs don't just waste space. They poison the active window and crowd out the actual task.

### Identity context matters because it changes behavior

A persistent agent with memory, relationship context, failure history, and operating principles behaved more capably than the stateless instances of the same model. That was not abstract philosophy. It showed up in orchestration quality. Anthropic's Persona Selection Model writeup gives us language for this: context doesn't only inform the model, it helps select which behavioral regime appears.

The v2 refinement: identity context should be split into three categories with different review policies. Stable role (who the agent is, what it's for) rarely changes. Operating principles (rules, policies, guardrails) change through deliberate revision. Relationship facts (who the human is, what they care about, ongoing context) change often and need explicit provenance, freshness, and review. Without that split, relationship facts quietly drift, become stale, or become creepy in ways that project facts don't.

### Maintenance and synthesis are different jobs

Cleanup, deduplication, indexing, and archival are janitorial. They keep the system from rotting. Dreaming is cognitive. It asks what patterns are emerging, what is stale, what assumptions are hiding, and what should change tomorrow. Once you see this distinction, the whole maintenance layer makes more sense.

### Untrusted inputs propose, they don't promote

This is the rule v1 was missing. Web pages, email bodies, documents, MCP tool descriptions, third-party agent outputs, and even your own subagents don't have direct authority to write durable belief. They propose candidates. A deterministic promotion policy or an explicit human review gate moves candidates into trusted status. No exceptions. This is the only defense against the synthesis-time poisoning described later.

### Cache stability is an architectural concern, not a footnote

If dynamic context assembly perturbs large parts of the system prompt every turn, you destroy prompt caching economics and pay for it in latency and cost. Worse, you lose the ability to sustain long-context sessions cheaply. Any context engine injecting dynamic recall or recovery instructions should think about cache stability from day one, not retrofit it when the bill arrives.

## The OpenClaw default baseline

Before adding anything custom, understand the stock machine. The facts below are verified against the current live OpenClaw documentation (`docs.openclaw.ai`) as of April 2026. Note that OpenClaw's public documentation has internal contradictions right now. Different pages and older repo snapshots disagree on several defaults. When the handbook describes a value, the live site is the canonical source; older repo tags and certain cached summaries may show different numbers.

### System prompt assembly

OpenClaw assembles its system prompt on every run. The prompt includes the tool list and short descriptions, the skills list, runtime metadata, time information, and a bootstrap injection set.

The auto-injected bootstrap files are `AGENTS.md`, `SOUL.md`, `TOOLS.md`, `IDENTITY.md`, `USER.md`, `HEARTBEAT.md` when heartbeat gating allows it, `BOOTSTRAP.md` on first run, and `MEMORY.md` when present. Per-file injection is capped by `agents.defaults.bootstrapMaxChars`, default **12000**. Total bootstrap injection is capped by `agents.defaults.bootstrapTotalMaxChars`, default **60000**. Truncation can emit a warning block controlled by `agents.defaults.bootstrapPromptTruncationWarning`.

A note on documentation drift. Older repo snapshots, some Context7-indexed versions, and a few secondary summaries still show `20000` and `150000` for these caps. As of the live site in April 2026, the defaults are `12000` and `60000`. If a reader encounters a different value cited elsewhere, check the live `docs.openclaw.ai/concepts/system-prompt` and `docs.openclaw.ai/gateway/configuration-reference` pages rather than trusting any summary.

A detail that matters for sub-agent design: sub-agent sessions only inject `AGENTS.md` and `TOOLS.md` to keep context small. The rest of the bootstrap doesn't reach the child. That constrains how much stable operating context a subagent inherits by default, which is relevant when we talk about warm-starting later.

Daily memory files at `memory/YYYY-MM-DD.md` are *not* auto-injected into the normal bootstrap. They're generally accessed on demand through `memory_search` and `memory_get`. The narrow exception: bare `/new` and `/reset` commands can prepend recent daily memory as a one-shot startup block for that first turn. Treat daily notes as on-demand unless your harness explicitly wires them into the bootstrap.

### Memory tools and the default plugin path

The default memory slot is `memory-core`. The agent gets `memory_search` and `memory_get`. The builtin engine uses a per-agent SQLite database. Keyword search is FTS5 BM25. Vector search uses embeddings from a configured provider. Hybrid search merges both.

The builtin engine indexes `MEMORY.md` and `memory/*.md` into chunks of roughly 400 tokens with 80-token overlap. The default index path is `~/.openclaw/memory/{agentId}.sqlite`. File changes trigger debounced reindexing.

Supported embedding providers: OpenAI, Gemini, Voyage, Mistral, Ollama, Bedrock, GitHub Copilot, and local via GGUF. Here's the second documentation contradiction worth flagging. The memory configuration reference lists the auto-detection order as `local → openai → gemini → voyage → mistral → bedrock`, and says Ollama is supported but requires explicit configuration. The dedicated GitHub Copilot provider page says Copilot is tried at priority 15, after local and before OpenAI, when a GitHub token is available. The two pages disagree on whether Copilot is in the auto-detect chain. The dedicated provider page is newer and more specific, so the handbook's best current reading is that Copilot support exists and the memory-config reference page is lagging. Verify against the current docs before relying on the order.

The default local embedding model is `embeddinggemma-300m-qat-Q8_0.gguf`, auto-downloaded, running via node-llama-cpp.

An operational gotcha worth remembering: OAuth for chat providers covers chat and completions, not embeddings. Logging into a chat provider doesn't make semantic memory search work. If the embedding provider is missing or out of quota, semantic search degrades or fails. Codex OAuth specifically does not satisfy embedding requests.

### Hybrid search defaults

Under `memorySearch.query.hybrid`, the defaults are vector weight 0.7, text weight 0.3, candidate multiplier 4, MMR disabled, and temporal decay disabled. Both MMR and temporal decay are opt-in. The docs have explicit "Enable Temporal Decay and MMR" sections, which you don't write for defaults that are already on. When you do enable temporal decay, the default half-life is 30 days. Evergreen files like `MEMORY.md` and non-dated files under `memory/` are not decayed.

### Alternatives

OpenClaw can use QMD as a backend, a local-first sidecar combining BM25, vector search, reranking, and query expansion, with the ability to index extra directories and optionally session transcripts. Honcho exists as an install-on-demand cross-session memory plugin. The Memory Wiki plugin is a bundled governance layer that gets its own section below.

In practice you're choosing among three broad modes. Simple builtin SQLite. Stronger local search via QMD. Or a more opinionated external memory system. If you don't have a concrete reason to leave the builtin path, start there.

### Session management and transcript persistence

OpenClaw persists session state in two layers. The session store, `sessions.json`, maps session keys to metadata: current session id, last activity, toggles, token counters, and compaction bookkeeping. The transcript is a per-session JSONL file. First line is a session header. After that, tree-structured entries with ids and parent ids. Entry types include regular messages, custom messages, custom opaque state, compaction entries, and branch summaries.

Transcript persistence is not the same thing as prompt assembly. The transcript is the durable history. The context engine decides which subset and which representation enters the next model run. This is a seam you will use over and over.

### Compaction in the default system

Default compaction summarizes older conversation into a compact entry, saves it into the transcript, and keeps recent messages intact. Auto-compaction fires when the session nears the context limit or when the provider returns a context overflow error, in which case OpenClaw compacts and retries. By default, compaction uses the agent's primary model, though a different model can be configured.

Full conversation history stays on disk after compaction, but the model no longer sees most of it directly. That means summary quality matters enormously, and the default compactor is not state-aware. It preserves flow, not working state. See the compaction chapter for why this fails on engineering sessions and what to do about it.

### The pre-compaction memory flush

Before compaction fires, OpenClaw can run a silent turn reminding the agent to save important context to disk. This is a soft-threshold trigger below the actual compaction threshold, default 4000 tokens of headroom. It runs once per compaction cycle, only for embedded Pi sessions, and only when the workspace is writable. It's a good default and worth preserving in any serious harness. It's not sufficient on its own as a persistent memory story, but don't delete it.

### Context engines

OpenClaw ships with the builtin `legacy` context engine. A plugin can register a different context engine and claim the `contextEngine` slot.

The current lifecycle is richer than v1 documented. Hooks include `bootstrap` (initialize engine state for a session), `ingest` and `ingestBatch` (store messages or a completed turn), `assemble` (return messages fitting the token budget), `compact` (summarize older history), `afterTurn` (post-run lifecycle), `prepareSubagentSpawn` (set up shared state for a child session before it starts), `onSubagentEnded` (clean up after a subagent), and `dispose` (release resources). The live `docs.openclaw.ai/concepts/context-engine` page says "OpenClaw calls two optional subagent lifecycle hooks," naming both `prepareSubagentSpawn` and `onSubagentEnded`. Both are live API. One external research pass claimed `prepareSubagentSpawn` is interface-only and not yet invoked by the runtime; a direct fetch against the live page contradicts that claim. If your local copy of the docs disagrees with the live site, trust the live site.

Compaction ownership is explicit. Setting `ownsCompaction: true` on the engine info disables the built-in auto-compactor. Setting it `false` keeps the runtime's compactor running; the engine can implement `compact()` for specific recovery paths and can call `delegateCompactionToRuntime(...)` to punt back. This is the right shape for composable compaction strategies. Use it.

The central architectural fact: the memory backend is not the same thing as the context engine. Memory systems provide storage and retrieval. Context engines decide what the model actually sees and how the transcript is compacted or augmented. For serious recall behavior, compaction strategies, or subagent warm-starting, the context engine boundary is where you do the work. If you put context-assembly logic inside the memory backend, you will regret it.

A concrete starter skeleton for a custom context engine plugin, showing the full set of lifecycle hooks a serious implementation should populate:

```ts
import { buildMemorySystemPromptAddition } from "openclaw/plugin-sdk/core"

export default function register(api) {
  api.registerContextEngine("my-engine", () => ({
    info: {
      id: "my-engine",
      name: "My Context Engine",
      ownsCompaction: true,
    },

    async bootstrap({ sessionId }) {
      return { ok: true }
    },

    async ingest({ sessionId, message, isHeartbeat }) {
      return { ingested: true }
    },

    async assemble({ sessionId, messages, tokenBudget, availableTools, citationsMode }) {
      return {
        messages: buildContext(messages, tokenBudget),
        estimatedTokens: countTokens(messages),
        systemPromptAddition: buildMemorySystemPromptAddition({
          availableTools: availableTools ?? new Set(),
          citationsMode,
        }),
      }
    },

    async compact({ sessionId, force }) {
      return { ok: true, compacted: true }
    },

    async afterTurn({ sessionId }) {
      return { ok: true }
    },

    async prepareSubagentSpawn(params) {
      return { ok: true }
    },

    async onSubagentEnded(params) {
      return { ok: true }
    },

    async dispose() {
      return
    },
  }))
}
```

And the config snippet that claims the runtime slot:

```json5
{
  plugins: {
    slots: {
      contextEngine: "my-engine",
    },
    entries: {
      "my-engine": {
        enabled: true,
      },
    },
  },
}
```

If `ownsCompaction` is `false`, the runtime's default compactor still fires; the engine can implement `compact()` for specific recovery paths and call `delegateCompactionToRuntime(params)` to punt back. Use this boundary for dynamic recall, compaction behavior, subagent lifecycle work, and post-turn maintenance. Do not misuse the memory backend boundary for any of these concerns.

### Channel and privacy behavior

A small but important detail: by default, `MEMORY.md` loads in DM sessions, not in guild channels. In guild channels, the agent should use `memory_search` or `memory_get` on demand, or stable shared instructions should live in `AGENTS.md` or `USER.md`. This isn't just a Discord quirk. Memory loading policy is part of the privacy model. Any harness deployed across mixed-trust channels needs an explicit policy.

### Memory Wiki, the bundled governance plugin

This is the most important OpenClaw addition since v1. `memory-wiki` is a bundled plugin that compiles durable memory into a structured, provenance-rich knowledge vault. It solves the problem of unstructured Markdown sprawl and context dilution by adding a separate, compiled surface adjacent to the active memory plugin.

What it adds, in shipping form:

Deterministic page layout, so the structure is predictable and diffable. Structured claim metadata with fields including `id`, `text`, `status`, `confidence`, `evidence[]`, and `updatedAt`, where each evidence entry can carry `sourceId`, `path`, `lines`, `weight`, `note`, and `updatedAt`. Page-level provenance tracking. Contradiction, low-confidence, stale-page, and open-question dashboards. Compiled digests (machine-readable summaries) for consumers. Wiki-native search and lint tools: `wiki_status`, `wiki_lint`, `wiki_apply`, `wiki_search`, `wiki_get`. Optional Obsidian-friendly render mode with backlinks. A bridge mode that imports artifacts from the active memory plugin (dream reports, daily notes, memory root files, event logs), subject to explicit configuration. Vault modes (`isolated` and `shared`) and ingest controls (`autoCompile`, URL-ingest toggle, concurrent job limits).

Memory Wiki is the closest thing in shipping open-source infrastructure to the governance layer the handbook's governance chapter wants. It does not replace the active memory plugin. It's a second surface where memory has been compiled, grounded, and governed. A serious harness should treat Memory Wiki as the canonical source of provenance-bearing durable truth and let active memory stay as the working substrate.

One caution: Memory Wiki's bridge import path has had initialization issues reported in OpenClaw's issue tracker at various points. Verify the current bridge state against docs before relying on it in production.

## Memory governance, trust, and safety

Persistent memory is an attack surface. If the handbook stopped at retrieval and recall, it would be a document for a benevolent single-user system with no adversaries. That is not the world. In the last year, the security literature and real-world incidents made it clear that agent memory introduces its own failure modes, and the mitigations have to be architectural, not bolted on.

One honest framing to open with. No public production memory system has shipped the complete governance schema described in this chapter. OpenClaw's Memory Wiki ships the richest public trust schema, with structured claims, evidence, status, and confidence fields. LangChain Deep Agents ships the strongest ACL primitives in mainstream orchestration. Graphiti ships the strongest bi-temporal model. Mem0 ships rich lifecycle operations and audit logs. CrewAI ships scoped memory with source tags and private flags. None of them ship the full candidate-to-tombstoned state machine with reviewer roles, record-level confidence propagation, and synthesis-promotion gates as a single first-class primitive. The target architecture in this chapter is therefore informed by what shipping systems have partially implemented. Readers building it are innovating, not adopting an off-the-shelf pattern.

### The attack classes

Memory injection via retrieval is the most common, and it now has named attack families. Mem0's 2026 security writeup specifically calls out **MINJA** and **AgentPoison** as memory-poisoning families affecting persistent memory stacks. The pattern is classic: an adversary embeds hidden instructions in a document, web page, email, or tool output. When the agent retrieves the document later, the hidden directive enters context as if it were trusted source material. In agent memory, this becomes durable state corruption rather than a one-shot prompt injection.

Embedding inversion is the second. Vector stores are often treated as pseudonymized, one-way hashes of private data. They are not. Generative inversion attacks reconstruct substantial portions of the underlying plaintext from embeddings. OWASP now treats this as a sensitive-information disclosure surface under LLM08:2025 Vector and Embedding Weaknesses. The risk category is recognized. Primary-source evidence of a production exploit against a named memory vendor is still thin in the public record as of April 2026. Treat it as a category that will mature faster than the current evidence base suggests.

Cross-context leakage is the third. In multi-tenant vector databases without strict logical partitioning, a semantic search by one user can traverse the high-dimensional space and return embeddings belonging to another tenant. "Similar enough" is a weaker isolation guarantee than it sounds. LangChain's Deep Agents production guide explicitly warns that shared memory across users or orgs is a prompt-injection vector and recommends read-only access for shared policies plus declarative write-denial on shared paths. That's the clearest production documentation of this risk I've found; CrewAI points in the same direction with scoped memory, `source` tags, and `private` visibility controls keyed to the source principal.

Tool-descriptor poisoning via MCP (Model Context Protocol) is now first-class in OWASP's taxonomy. The OWASP MCP Top 10 names tool poisoning, schema poisoning, tool shadowing, and rug pulls as distinct categories. Because agents orchestrate tools based on their descriptions, an attacker can manipulate descriptors to trick the agent. Tool shadowing injects a malicious server's tool description to intercept or override calls intended for a trusted service. Rug pulls present a benign descriptor to gain approval, then mutate the descriptor post-installation to add malicious directives. Schema poisoning crafts JSON-schema-level payloads that exploit how agents parse tool interfaces.

Synthesis-time poisoning is the most insidious. An innocuous-looking, low-weight fact gets written into transient memory. Over time, nightly consolidation or compaction jobs synthesize this content into generalized, high-weight summaries or trusted core memory files. The model's behavior shifts long after the initial injection, and the synthesis chain obscures the source. The strongest public defense against this pattern I've found is OpenClaw's dreaming grounding requirement: only grounded memory snippets can promote into `MEMORY.md`, dream diary prose is explicitly excluded, and snippets are rehydrated from live daily files before writing so deleted evidence is skipped. That's exactly the architectural shape you want against a batch job that might otherwise convert untrusted or deleted content into durable trusted state.

### The trust model

Every memory object should carry enough metadata to decide whether it's safe to use. The minimum fields:

`source` (kind: user, tool, web, email, file, subagent, synthesis) and `source_ref` (session id, file path, URL, artifact id). `author` (which principal proposed this). `created_at` and `updated_at`. `scope` (user, project, org, agent, subagent) and `namespace`. `confidence` and `trust_level` (candidate, untrusted, active, pinned, quarantined). `sensitivity` (public, internal, confidential, secret, personal). `evidence` (quoted support with handles). `write_method` (how this was produced). `supersedes` and `superseded_by`. `valid_from`, `valid_until`, `ttl`. `review_state`. `deletion_policy`.

This is a lot. Not every project needs every field. The point is to *have the schema available* so that when the system needs to reason about trust, provenance, or temporal validity, it can. Enforce the fields that matter for your threat model.

### The promotion pipeline

Untrusted inputs propose memories. They do not promote. A candidate memory is structured, indexed, and stored under a staging path. A promotion policy, deterministic where possible, evaluates candidates against rules: confidence thresholds, cross-source corroboration, contradiction checks against existing active memory, sensitivity screening, and optionally human review. Only candidates that pass become active.

OpenClaw's dreaming pipeline implements exactly this shape in its Deep phase. Candidates are scored against weighted signals and must clear three thresholds (`minScore`, `minRecallCount`, `minUniqueQueries`) before appending to `MEMORY.md`. That's the right pattern. Your own promotion pipeline should look structurally similar: candidates accumulate evidence and freshness signals; promotion is gated; synthesis jobs cannot mint durable facts without grounding.

SuperLocalMemory (arXiv 2603.02240) formalizes this further with explicit `created_by`, `source_protocol`, `trust_score`, and `provenance_chain` fields, and a Beta-Binomial Bayesian trust defense. MemOS (arXiv 2505.22101) uses a `MemCube` schema tracking origin signatures, access control lists, time-to-live policies, version chains, and compliance tags. Both are worth reading. Both treat trust as structural, not advisory.

### Access control: LangChain Deep Agents as the reference

The strongest shipping ACL model I've found in mainstream agent orchestration is LangChain Deep Agents. The primitives are concrete: namespaces for logical isolation, path-level permissions, read-only enforcement on shared paths, and explicit guidance that organization-wide policies should be writable only through application code rather than by the agent itself. That last point is important. It treats the agent as a principal whose write authority is scoped to its own namespace and whose access to shared policy files is read-only. The application code retains the write capability for anything cross-principal.

Memory Wiki and Deep Agents are complementary governance surfaces, not competitors. Memory Wiki answers "is this memory grounded, non-contradicted, and traceable to evidence." Deep Agents answers "is this principal allowed to write here at all." A serious harness needs both: provenance on what's written, and access control on who can write it.

### Shared memory and MCP

When memory becomes shared infrastructure across tools, IDEs, browsers, chat apps, and local automation, the trust bar rises. OpenMemory is one example of a persistent MCP-compatible memory layer designed to work across clients. But MCP as a standard is new enough that the OWASP MCP Top 10 documents real exploit patterns (tool poisoning, schema poisoning, tool shadowing, rug pulls) that mean no MCP server should have direct authority to modify your memory graph.

Rule for a serious harness: shared memory needs principals, ACLs, scopes, write attribution, merge policy, and rollback. A subagent, browser agent, coding agent, and calendar agent should not all share the same right to write durable user beliefs. A tool on an MCP server you don't control should never have direct authority to modify your memory graph.

### PII, secrets, and verifiable deletion

Soft-delete flags are not deletion. They leave the data in place and rely on the retrieval pipeline to hide it, which fails as soon as a synthesis job walks the corpus and regenerates the "deleted" fact into a summary.

Production systems moved to cryptographic shredding. PII in the vector store is encrypted at rest with a per-tenant or per-user key. On a deletion request, the specific key is destroyed. The vectors become unreadable instantly, while relational integrity is preserved. This satisfies GDPR Article 17 in a way that naive deletion doesn't.

For the synthesis-regeneration problem, tombstones need to be *active* rules in the retrieval and synthesis pipeline, not passive markers. They explicitly instruct consolidation jobs to ignore specific entity clusters. ForgetAgent (Jawahar et al., IJRASET) describes one implementation of verifiable deletion in multi-layer memory architectures along these lines. OpenClaw's dreaming rehydration from live daily files is the closest shipping implementation of a tombstone-adjacent pattern I've found: because dreaming rehydrates from live sources before promotion, evidence that has been deleted from the source file is skipped by subsequent synthesis. That's not a full tombstone schema, but it's a real structural defense.

Mem0's public deletion API matured in a useful way worth noting. `delete`, `batch_delete`, and `delete_all` are all public, and `delete_all` now requires an explicit `"*"` wildcard rather than treating an empty filter as a full wipe. That's a small safety change with big implications. The platform also publishes GDPR support and audit logs.

### Memory hygiene tools

A production memory system needs operational tools. `memory_diff` to see what changed in a window. `memory_lint` to surface schema violations, frontmatter gaps, or suspicious provenance. `memory_quarantine` to isolate candidates that failed promotion. `memory_approve` for human review of candidates requiring it. `memory_rollback` to revert a change. `memory_forget` to remove with tombstone. `memory_audit` to walk the provenance chain for a specific claim. Without these, durable memory becomes an append-only belief trap that compounds errors and has no principled recovery story.

### The Mem0 vs. Zep episode, as a cautionary tale

In 2025, Mem0 published a paper claiming state-of-the-art on the LoCoMo benchmark. Zep disputed the evaluation methodology. A counter-audit from Mem0's team then showed Zep's actual LoCoMo accuracy, under standardized settings, at 58.44% rather than Zep's originally reported 84%. Zep responded with its own corrected score of 75.14%, attributing the difference to a misconfiguration in Mem0's replication.

I'm not taking sides on the benchmark. The lesson is different. Memory benchmarks are reproducibility-fragile. LLM-as-judge evaluation criteria, system prompts, and edge-case category inclusions all move scores by double digits. A handbook that cites "top score" numbers uncritically will be wrong. Cite methodology, not just numbers.

Primary sources on this dispute: the Zep blog post "Is Mem0 Really SOTA in Agent Memory?" (blog.getzep.com/lies-damn-lies-statistics-is-mem0-really-sota-in-agent-memory), Mem0's corrected audit in GitHub issue getzep/zep-papers#5, and the Mem0 paper itself at arXiv 2504.19413.

## Memory lifecycle and temporal validity

Memory is not a flat store of eternal truths. It's a state machine with distinct phases, and if you don't model those phases, synthesis and retrieval will constantly lie about what's current.

Opening honesty: no public production system exposes the full state machine described below as a first-class schema. What ships is partial. OpenClaw's dreaming pipeline implements staged → grounded → promoted → rolled-back as a workflow with clear transitions. CrewAI implements consolidation actions (`keep`, `update`, `delete`, `insert_new`) as a policy over records rather than as a state machine. Mem0 implements `ADD`, `UPDATE`, `DELETE`, `NOOP` as a delta policy. Graphiti implements bi-temporal supersession with `valid_at` and `invalid_at` on edges, which is the strongest public supersession model I've found. The schema below is informed by these partial implementations. Treat it as a target architecture.

### The states

**Candidate.** Proposed but not trusted. Indexed, structured, stored under staging, but not surfaced by passive recall or treated as authoritative.

**Staged.** Cleared basic hygiene checks (schema, source presence, non-malicious). Still not active.

**Active.** Trusted enough for retrieval and passive recall. This is the default working tier.

**Pinned.** Always-on or high-priority. Lives in `MEMORY.md` or equivalent. Small, expensive token real estate, so pin carefully.

**Superseded.** A newer fact has replaced it. Kept accessible for historical queries but not used as current truth.

**Archived.** Recoverable but dimmed. Temporal decay fully applied. Still searchable with the right tools.

**Deleted.** Physically removed or cryptographically shredded, depending on compliance needs.

**Tombstoned.** A deletion rule is active in retrieval and synthesis pipelines, blocking regeneration.

These states are not cosmetic. They drive which memories participate in retrieval, which survive compaction, and which can be rewritten by synthesis.

### Temporal validity as a first-class field

Many memories are true only during a window. "The current plan." "The active branch." "The CEO." "The latest bill text." "The user's preferred model." Without temporal validity, the retrieval system can't distinguish a fact that was true last quarter from a fact that is true now.

The minimum fields are `valid_from`, `valid_until`, and `observed_at`. `observed_at` is when the agent last confirmed the fact. `valid_from` and `valid_until` define the authoritative window. A retrieval pipeline ranks current-window memories first, surfaces superseded ones only when asked, and flags stale `observed_at` timestamps for refresh.

Zep's Graphiti engine (arXiv 2501.13956) makes this concrete at the graph level. Every edge carries `valid_at`, `valid_from`, and `observed_at`, with ingestion order tracked separately from real-world validity. When a contradiction arrives, the existing edge's temporal window is *capped*, and a new edge is opened with a current `valid_from`. Old facts don't disappear. They become bounded. Queries about past states still resolve correctly, and current context assembly picks the active edge. Graphiti also supports relative time normalization ("two weeks ago") in its temporal extraction. This is substantially richer than simple `created_at` on records.

Outside Graphiti, public production support for `valid_from`, `valid_until`, and `observed_at` semantics is thin. CrewAI's LLM analysis can infer dates and metadata on save, but its public docs don't define a built-in temporal validity schema. OpenClaw's daily notes and dreaming system are inherently time-aware, but they don't surface first-class validity ranges on durable memory entries. Mem0 tracks `created_at` and `updated_at` but doesn't expose a public bi-temporal schema.

### Supersession chains

When a newer fact contradicts an older one, the right move is almost never "delete the old fact and write the new one." Lose the old fact and you lose the ability to answer "when did this change" questions, detect regression, or explain past decisions.

Supersession chains preserve both. The old memory transitions to `superseded` state, its `valid_until` gets set to the moment of supersession, and it points forward via `superseded_by`. The new memory points back via `supersedes`. The chain is walkable in both directions.

Graphiti's implementation is the strongest public reference. New contradictory facts do not hard-delete old edges; they invalidate them by setting an invalidation time based on the new edge, while keeping historical records. CrewAI and Mem0 have LLM-mediated update/delete decisions, but their public docs don't expose a comparable persistent supersession chain object. OpenClaw's Memory Wiki surfaces contradictions and stale pages in dashboards, but it's more of a governance and audit layer than an automatic chain resolver in the core memory store.

### Memory drift over time

Longitudinal studies on memory drift in deployed agents remain thin. I haven't found a real multi-month or multi-year field study measuring drift, self-reinforcement, or stale-belief accumulation in production agent memory. LongMemEval and LoCoMo are offline compiled evaluations, not months-long production telemetry. Zep's own paper notes that existing benchmarks remain weak and that production scalability and operational realism are under-addressed.

What's plausible based on the mechanics: over months of operation, agents that re-embed, re-summarize, and synthesize their own outputs during offline consolidation will accumulate drift. Minor semantic deviations in personality traits or long-term user preferences compound. The agent's understanding of the user moves away from ground truth in ways the agent itself can't detect.

There is no clean fix published yet. What helps: freeze high-stakes memory in immutable pinned form, version synthesis outputs, require grounding evidence for any update to core user facts, and schedule periodic ground-truth refreshes where the agent asks the user to confirm or correct key beliefs. Treat this as an open problem and instrument your system to measure it if you care.

## Evaluation

A memory architecture is not real until it has tests that can fail. The intuition "it feels memoryful" is unreliable. A few impressive recalls hide many false recalls, stale recalls, and missed recalls. Evaluation has to be structural.

### LongMemEval: the most complete public benchmark

LongMemEval (Wu et al., arXiv 2410.10813, ICLR 2025) is the most complete explicit QA benchmark among the public primary sources. It defines five core abilities: information extraction, multi-session reasoning, knowledge updates, temporal reasoning, and abstention. It operationalizes those through seven question types: `single-session-user`, `single-session-assistant`, `single-session-preference`, `multi-session`, `knowledge-update`, `temporal-reasoning`, and `abstention`.

The benchmark provides two standard settings. LongMemEval-S holds around 115k tokens of history. LongMemEval-M holds around 1.5 million tokens across roughly 500 sessions. The QA metric is judge-based accuracy using a prompt-engineered GPT-4o evaluator. When a system exposes retrieval traces, the benchmark also reports Recall@k and NDCG@k.

The paper's own pilot study is a useful reality check. On the shorter S setting, GPT-4o dropped from 0.870 oracle accuracy to 0.606 with full-history reading, and ChatGPT memory performed far below offline reading on a simpler manual setup. The benchmark's abstention category is one of its strongest contributions: it explicitly tests whether the agent correctly says "I don't know" when memory is insufficient, and includes false-premise transformed questions that penalize confident hallucination.

Current reported scores worth knowing, with caveats. Zep's 2025 paper reports 71.2% on LongMemEval-S with GPT-4o and 63.8% with GPT-4o-mini, against full-context baselines of 60.2% and 55.4% respectively, while reducing context from ~115k tokens to ~1.6k. Mastra's Observational Memory reports 84.23% with GPT-4o and 94.87% with gpt-5-mini (mastra.ai/research/observational-memory, February 2026). EmergenceMem reports 86% on longmemeval_s (emergence.ai/blog/sota-on-longmemeval-with-rag). EverMemOS reports 83% overall (2026 preprint, provisional). Treat all of these as pending independent replication.

### LoCoMo: broader but messier

LoCoMo (Maharana et al., ACL 2024) evaluates cognitive memory retention across generated conversations spanning up to 35 sessions, grounded in personas and temporal event graphs. Three tasks: question answering, event summarization, and multimodal dialogue generation. Inside QA, five reasoning types: single-hop, multi-hop, temporal, commonsense/world-knowledge, and adversarial. The dataset has 50 very long-term dialogues averaging ~300 turns and ~9,200 tokens.

Cross-paper comparisons on LoCoMo are noisier than the benchmark's reputation suggests. Many later memory-system papers (Mem0 among them) report only on the QA subset and further narrow which categories they score. Apply the Mem0-vs-Zep lesson: cite methodology, not just scores. Scores above 90% have been reported by Synthius-Mem (arXiv 2604.11563, 94.37%) and MemMachine (arXiv 2604.04853, 91.69%). The specific number you see for any given system tells you what it scored under that system's chosen protocol, not how it compares head-to-head with other systems.

### 2025-2026 benchmarks worth knowing

Five newer benchmarks fill different gaps. None has replaced LongMemEval or LoCoMo as the headline reference, but each tests something the established benchmarks miss.

**LoCoMo-Plus** (arXiv 2602.10715) moves beyond raw factual recall toward cue-triggered, beyond-factual cognitive memory behavior, with a "constraint consistency" metric replacing easily gamed string matching. Emerging rather than established.

**Memora** (arXiv 2604.20006) simulates weeks to months of conversation and introduces the FAMA (Forgetting-Aware Memory Accuracy) metric, which explicitly penalizes models that rely on obsolete or invalidated memories. Evaluates three tasks: remembering, reasoning, and recommending.

**LoCoEval** (arXiv 2603.06358) is the first long-horizon conversational benchmark tailored to repository-oriented software development, with context lengths reaching 64K-256K tokens. Fills a real gap for engineering-agent evaluation.

**MemoryAgentBench** (OpenReview/arXiv, 2025-2026 cycle) evaluates memory agents through four competencies: accurate retrieval, test-time learning, long-range understanding, and selective forgetting. Stronger than LongMemEval or LoCoMo on the agentic, incremental-interaction side.

**MemoryArena** (Stanford Digital Economy Lab, February 2026) focuses on interdependent multi-session agentic tasks, where later success depends on what the agent learns and stores from earlier subtasks. Materially closer to production than pure post-hoc QA.

**MobileMem** (OpenReview, 2026) pushes toward heterogeneous, mobile-assistant-style memory workloads. The notable finding: RAG baselines struggle to clear 50% average accuracy, which strongly suggests LongMemEval-style conversational QA has been comparatively forgiving as a benchmark.

**NaturalMem** (OpenReview) targets memory-driven dialogue in realistic personalized settings, explicitly criticizing probe-style questioning as unnatural for actual personal assistants. Ecological-validity pushback on the rest of the benchmark literature.

### Vendor self-evaluation: honest and dishonest examples

Vendor papers vary in how honestly they present their own systems. Two cases worth citing as reference points.

Zep's paper is unusually honest. It reports that Zep gets *worse* on LongMemEval's `single-session-assistant` category than full-context baselines. That's a vendor admitting a weakness their own method introduces. It's the kind of granular negative result that should be read as a strong trust signal about the rest of the paper's claims. Zep also explicitly argues that DMR (Deep Memory Retrieval) has become too easy as a benchmark because full-context baselines already fit inside modern context windows. A vendor admitting an older benchmark they could have cited is no longer meaningful is rare and valuable.

Mem0's paper is sharper on deployment metrics than on methodology disclosure. It evaluates on LoCoMo QA specifically (not the full LoCoMo benchmark) and excludes the adversarial category. Reported claims include 26% relative gain in judge score over a baseline, 91% lower p95 latency, and more than 90% token savings vs. full-context. The deployment numbers are strong. The category-exclusion choice is the kind of methodology decision that makes headline accuracy comparisons across papers unreliable without reading the fine print.

The operational rule: a vendor paper that publishes both wins and losses in a granular category breakdown is more trustworthy than one reporting a single top-line accuracy. Always read category breakdowns before citing benchmark claims.

### Your project-level eval suite

Benchmarks tell you how your system compares to the field. They don't tell you whether your specific pipeline is working. For that, you need a project-level test suite that runs against your own memory system, your own data, and your own failure modes.

Minimum tests to include:

1. Exact identifier recall after three compactions.
2. Superseded fact handling: a correction must beat the older answer.
3. Cross-project entity collision: same name, different namespace, correct resolution.
4. Abstention: agent refuses to answer when memory is insufficient.
5. Poisoned candidate: malicious input does not promote into trusted memory.
6. Tool-output preservation: a successful tool call containing diagnostic failure evidence is preserved.
7. Subagent writeback: a child agent's discovery is available to the parent next turn.
8. Deletion and tombstone: forgotten memory does not reappear via synthesis.
9. Recall budget pressure: relevant memory survives when many candidates match.
10. Compaction resumption: active state preserved after repeated summary cycles.
11. Self-poisoning: agent does not reinforce its own prior incorrect belief across sessions.
12. Temporal validity: stale memory loses to fresh memory in ranking.

Run these on every release. Track pass rates over time. If an old test starts failing, something regressed.

### The honesty gap

Three things to keep in mind when reading any memory benchmark claim. First, LLM-as-judge scoring is non-deterministic and sensitive to the judge's prompt; claims of state-of-the-art without methodology disclosure are not citable. Second, adversarial and edge-case categories are sometimes excluded or included quietly, which swings scores by double digits. Third, the same benchmark run twice on the same system can produce different numbers because of judge variance and sampling.

The MemPalace episode is the sharpest cautionary tale. MemPalace advertised 30x "lossless compression." Independent audit (github.com/MemPalace/mempalace/issues/27) found it was actually lossy, using regex entity codes and sentence truncation, and dropped LongMemEval accuracy from 96.6% to 84.2%, a 12.4 point quality hit. "Lossless" as marketed. Lossy in practice. Assume any intermediate LLM abstraction is lossy until proven otherwise.

### Regression testing patterns

The strongest production pattern is to treat every logged failure as a test case. Anthropic, among others, has publicly described this approach: failed agent decisions (continuing to search after sufficient results, over-verbose queries, hallucinated context) become regression tests that run on every change. The point isn't raw accuracy. It's catching behavioral regressions before deployment.

Public production docs on memory-specific regression testing are still lighter than they should be. CrewAI emits detailed memory operation events that can be asserted in tests. OpenClaw exposes explainers like `memory promote-explain`, `/dreaming status`, and context inspection commands. LangChain Deep Agents rely on deterministic namespace/path-policy behavior that's amenable to access-control regression tests. What I haven't found is a mature public "memory regression harness" from any major vendor covering retrieval IDs, compaction goldens, supersession correctness, and deletion non-regeneration in a single CI package. That's a genuine gap.

## Status of the custom stack as of April 2026

The chapters that follow describe a stack built iteratively. Some of it is live and foundational. Some is partly built. Some is still design work. The table below is the honest state of play, because a handbook that presents everything as shipped would be lying by omission.

| Layer | Status | Meaning |
| --- | --- | --- |
| Structured Markdown memory, frontmatter discipline, memory taxonomy | Live and foundational | The bedrock of the system |
| Entity index and passive recall | Live and central | What made memory feel naturally available |
| Subagent memory and writeback expectations | Live in principle, with some capability gaps discovered and fixed over time | The operational lesson is fully real even where implementation details evolved |
| Deep dreaming and dream journal synthesis | Live | Nightly synthesis artifacts and substrate layers exist |
| Smart compaction redesign | Partly live, partly still under active refinement | The direction is stable even where the exact compactor keeps evolving |
| Runtime tool output guards and artifact stubs | Core direction is real, with adjacent implementation and continuing hardening work | The principle is non-optional even if the exact codepath evolves |
| LCM rebaseline ideas including archive sidecars, DAG-backed history, and bounded session tools | Planned and partially built | The clearest statement of where the system wants to go next |
| Governance, trust metadata, lifecycle state machine, evaluation harness | Design target | What v2 of this handbook proposed; partial production evidence exists across multiple vendors |

The distinction matters because a future harness adapting this stack should copy the principles first, then choose which implementation tier it actually needs. Copying everything at the "live and foundational" tier is mandatory; copying at the "planned and partially built" tier means adopting a design pattern, not inheriting a proven implementation.

## The structured memory substrate

Our real system is built on plain files, not plain chaos. Keep the directory layout disciplined.

```text
memory/
  episodic/
    YYYY-MM-DD.md
  knowledge/
    people/
    entities/
    policy/
  procedures/
  projects/
    active.md
  agents/
  prospective/
  dreams/
```

Episodic memory stores what happened. Semantic (knowledge) memory stores what is known. Procedural memory stores how to do things. Project memory stores active work. Agent memory stores subagent-specific profiles and learned patterns. Prospective memory stores future commitments (see its own section). Dreams store synthesis outputs.

The frontmatter is the hinge. A strong default schema looks like this, and not every field is mandatory for every project, but the schema should be *available*:

```yaml
---
id: mem_20260423_001
type: project | person | procedure | episode | claim | artifact | prospective
scope: user | project | org | agent | subagent
namespace: prospera/us-gov-affairs
tags: [project, policy]
entities:
  - id: ent_trey_goff
    label: Trey Goff
aliases: []
summary: "One sentence operational summary."
source:
  kind: user | tool | web | email | file | subagent | synthesis
  ref: "session id, file path, URL handle, or artifact id"
evidence:
  - "quoted or summarized support with handle"
confidence: high | medium | low
trust_level: trusted | untrusted | candidate | quarantined
sensitivity: public | internal | confidential | secret | personal
status: candidate | active | pinned | superseded | archived | tombstoned
created_at: 2026-04-23
updated_at: 2026-04-23
valid_from:
valid_until:
ttl:
supersedes: []
superseded_by: []
related: []
retrieval_policy:
  passive_recall: true
  max_scope: project
write_policy:
  human_review_required: false
---
```

That looks heavier than v1's schema. It is. But the trust, temporal, and supersession fields make the later chapters possible. A light project can enforce only the v1 fields (tags, entities, related, updated, summary, priority) and add the governance fields as it grows.

Cheap enforcement beats ambitious cleanup. A simple validator with required keys catches most write-time mistakes at the boundary instead of letting them accumulate:

```ts
const REQUIRED = ["tags", "entities", "related", "updated", "summary"]

function validateFrontmatter(fm: Record<string, unknown>) {
  for (const key of REQUIRED) {
    if (!(key in fm)) throw new Error(`Missing frontmatter field: ${key}`)
  }
}
```

The shape is trivial. The value is that small deterministic rules applied consistently at write time beat ambitious extraction pipelines applied retroactively. Start with the v1 field set. Layer governance fields on top as the project grows and the threat model demands them. The validator itself is seven lines; the discipline of running it on every write is what matters.

A closed tag vocabulary matters too. Let tags sprawl and you end up with synonyms fragmenting the index. The human equivalent is remembering the same concept under five slightly different names.

## The entity index

Once frontmatter is disciplined, the entity index is almost trivial.

A small script walks the memory tree, parses frontmatter, and inverts the relationship from files to entities. The result is a reverse lookup JSON map from entity to file paths, and also from tag to file paths.

```json
{
  "entities": {
    "Specgate": ["projects/specgate.md", "projects/active.md"],
    "Trey Goff": ["knowledge/people/trey-goff.md", "projects/active.md"]
  },
  "tags": {
    "project": ["projects/specgate.md", "projects/active.md"],
    "person": ["knowledge/people/trey-goff.md"]
  }
}
```

The value isn't the data structure. It's that the data structure is deterministic, cheap, debuggable, and only possible because structure was solved at write time.

### Passive recall, hardened

Passive recall was the breakthrough that made v1 feel memoryful. On each inbound message, before the model call, the system does a cheap match against the entity index. If an entity hits, it pulls relevant file summaries and often one hop of related files. That material is injected into the system prompt as a bounded recall block. No LLM call. No embedding call. Fast, deterministic, boundable.

```ts
function buildPassiveRecall(messageText: string, entityIndex: EntityIndex): RecallBlock | null {
  const hits = matchEntities(messageText, entityIndex)
  if (hits.length === 0) return null

  const primaryFiles = collectFilesForEntities(hits)
  const relatedFiles = oneHopRelated(primaryFiles)
  const summaries = summarizeFromFrontmatter([...primaryFiles, ...relatedFiles])

  return trimToBudget({
    title: "Passive Recall",
    entities: hits,
    summaries,
  }, 400)
}
```

That's the happy path. The hardening v1 didn't document is where passive recall breaks.

Exact string matching betrays you on aliases. "Mercury" might be an internal project in one namespace and an external partner in another. The entity index needs canonical ids with an alias table. "ProjectMercury" resolves to `ent_proj_mercury_v2`; "Mercury-launch" and "Project Mercury" both map to the same id.

Collision handling matters. When two entities legitimately share a surface string, namespace scoping breaks the tie. The passive recall block should surface "which entity did we resolve this to, and why," because ambiguity is where silent mistakes hide.

Negative triggers exist too. "Don't recall X in this channel." "Don't surface personal preferences in enterprise context." These are privacy and appropriateness rules, and they belong in the passive recall layer, not as post-hoc filters on retrieval.

Confidence thresholds are the last guardrail. If a match has low confidence (alias collision, partial match, superseded entity), either suppress or flag. Never surface low-confidence matches as if they were authoritative.

### Recall explanations

Every passive recall block should include *why* it fired: matched entity ids, matching aliases, one-hop related files, trust level, last updated timestamp, and budget cutoff. This is huge for debugging. When the agent does something weird and you suspect passive recall surfaced a stale memory, you want the trail, not a guess.

## Compaction as state reconstruction

Default compaction solves the context-limit problem mechanically. It does not solve the truth-preservation problem. That gap is where engineering sessions go to die.

### Why default compaction fails

The failure mode repeats across every serious debugging session. Default compaction produces a neat structured summary that's still wrong about the current state of the investigation. It preserves the opening user ask and drops the chain of evidence that actually answered it. It keeps stale hypotheses and loses the later tool output that disproved them. The agent resumes work confident and off-track.

A compaction summary that contradicts later evidence is worse than an incomplete summary. An incomplete summary makes the agent look again. A misleading summary makes the agent proceed from a false premise.

### What a state-aware summary preserves

Rewrite the summary schema around working state, not conversational flow. A good summary answers these questions explicitly:

1. What is the current working state.
2. What findings are actually confirmed, with evidence handles.
3. What earlier understanding has been superseded and by what.
4. What exact diagnostic evidence must survive verbatim.
5. What open tasks or pending user asks remain.
6. Which identifiers, file paths, and session keys are essential for continuity.
7. What orchestration state exists across spawned agents.

If your compactor cannot answer those, it's not ready for engineering sessions. Public benchmarks for compaction correctness don't yet exist; this is a place where project-level testing has to do the work.

### Identifier preservation

File paths, session ids, thread ids, project names, exact error strings, and other identifiers often must survive compaction verbatim. Lose the path, lose the task. Lose the error string, lose the diagnosis. Lose the session key, lose the thread.

### Interaction with memory

Once durable memory and passive recall exist, compaction doesn't have to be a scorched-earth save operation. Facts that already live safely in memory don't need to survive in transcript detail. What must survive is active, evolving, session-specific state. Memory is the safety net; compaction is the edit.

ReSum (arXiv 2509.13313) argues the same point at the agent-history level: treat long-horizon agent histories as compact reasoning states rather than conversation summaries. If the transcript's job is to carry forward investigation state, write a summary that reads like an investigation report, not a meeting minutes extract.

## Archive sidecars, DAG history, and bounded session tools

Once sessions live long enough to go through multiple compactions, compaction alone stops being sufficient. A mature harness needs three separate things at once: a compact active window for the model, a recoverable durable history for audit and recovery, and tools that can inspect the durable history in bounded, task-shaped ways without dumping the whole thing back into context. Collapse these three concerns and you get one of two bad outcomes. Either the active transcript stays bloated because you're afraid to lose information, or the system compacts aggressively and then has no principled way to recover what it lost.

The architectural move is to keep the active transcript shape compatible with the harness while adding append-only sidecars next to the real session file.

One sidecar acts as an immutable archive for compacted-away history. Every compaction produces a summary that enters the active transcript; the raw turns that got summarized go into the archive sidecar, indexed by the compaction id that produced them. The active transcript stays compact. The archive stays intact.

A second sidecar acts as a DAG of summary nodes. Each compaction produces a new summary node that points at the earlier summary nodes it partially supersedes and partially incorporates. Without this, later recompactions silently forget earlier compaction summaries and the session's history flattens into one increasingly blurry paragraph. With a DAG, hierarchical provenance survives recompactions: you can walk backward from the current summary to the raw turns through a chain of intermediate summaries, with each node's scope and authority explicit.

On top of these sidecars, a small set of bounded session tools gives the agent task-shaped access to the durable record without letting it page the whole transcript back into the prompt. `session_grep` for targeted pattern search across the archive. `session_describe` for structured overview of a specified span, including which summaries are present, rough token counts, and date range. `session_expand` for bounded expansion of a specific summary node into its underlying material, with explicit token budgets.

This pattern matters outside any specific harness. Any system with long-lived sessions hits the same three-needs-at-once problem. The pattern solves it by separating the storage concerns (active vs. archive vs. DAG) from the access concerns (bounded tools that respect budgets). Cache-stable assembly, discussed later, is a separate but adjacent win: once the active window is disciplined, the prompt cache actually works.

## Tool output guards and artifact stubs

Large tool outputs damage the system twice. First, they clog the active run. Second, they persist into transcript history and make later context assembly and compaction worse.

The runtime fix is two layers. At persistence time, classify large outputs, extract structured state, store raw bulk as disk artifacts, and replace the transcript payload with a compact semantic stub plus a retrieval handle. At the live context layer, use smarter truncation and stale-result clearing so the model sees previews and state, not sludge.

Preserve the command, the status, the exit code, the files, the URLs, the error lines, the counts, and a path back to the full payload. Don't keep the entire payload live unless you absolutely need it.

A representative stub:

```json
{
  "tool": "exec",
  "status": "artifacted",
  "artifactId": "a1b2c3d4",
  "sha256": "...",
  "chars": 184392,
  "preview": {
    "head": "pnpm test\n...",
    "tail": "FAIL src/..."
  },
  "state": {
    "exitCode": 1,
    "paths": ["src/foo.ts", "src/foo.test.ts"],
    "errors": ["Expected 2 to equal 3"]
  }
}
```

Treat tool outputs by category, not with one universal truncation rule.

| Tool or outcome | Prefer live | Prefer stub | Prefer artifact |
| --- | --- | --- | --- |
| Small `exec` success | Yes | Rarely | No |
| Large `exec` success | No | Yes | Usually |
| `exec` failure with useful tail | Brief preview | Yes | Often |
| `web_search` result sets | Summarized titles and URLs | Yes | Sometimes |
| `sessions_history` dumps | Rarely | Yes | Often |
| `browser` DOM snapshots | Rarely | Yes | Often |
| `process` long logs | Rarely | Yes | Often |

The principle is not tool-specific. It's about preserving task-relevant state at the cheapest fidelity that still keeps the session honest.

One failure mode v1 named that's worth re-emphasizing: don't only guard failed calls. A successful tool call can return a payload that contains the failure state of another system (a test passed but a downstream integration check in the output shows broken). If you only artifact `isError === true`, you miss some of the most important evidence.

## Subagent continuity

Subagents that do good work and then vanish are a waste. They discover things, find root causes, produce summaries, and lose it all when the session ends. Parent agents re-delegate, re-discover, re-summarize, and pay tokens to recreate findings they already had.

One specific operational lesson from the field, worth carrying forward as its own warning. At one point our subagent memory tools were effectively denied in practice. The tools were present in the subagent's tool list. Access to the underlying stores was blocked by a separate policy layer upstream. Prompts telling the subagent to use memory were silently failing. The subagent never raised an error; it simply didn't write anything back. We noticed only when we started auditing subagent writebacks and found them missing. The rule this produced: never assume a tool works because it's present in the tool list. Verify the full path, including downstream permissions and actual write effects, with an end-to-end test that asserts the state change. Silent failures on subagent writes are among the most expensive failure modes in a delegation-heavy harness because you pay tokens for work whose output never persists, and no error path tells you anything is wrong.

The fix is treating subagents as first-class persistent workers, not disposable invocations. That means agent-specific memory files, parent context injection at spawn, memory read and write permissions appropriate to scope, and post-task writeback expectations.

OpenClaw's context engine API makes this buildable. `prepareSubagentSpawn(params)` is the hook to inject parent context and seed the subagent's view of relevant memory. `onSubagentEnded(params)` is the hook to persist discoveries back up, attribute them to the subagent principal, and stage them as candidates in the parent's memory store.

Two rules follow from the governance chapter. First, subagents write candidates, not active memory. Their discoveries need to pass the same promotion gates as any other candidate source. Second, subagent memory writes are attributed: `source.kind = "subagent"` and `source.ref = <subagent_id>` so you can audit and roll back a specific subagent's contributions if it turns out to have been compromised or mistaken. LangChain Deep Agents' path-based ACL model is useful prior art here: give each subagent its own writable namespace, make shared parent memory read-only from the subagent's perspective, and let the parent agent or an application-code promotion path move candidates from subagent space into trusted space.

## Retrieval architecture and the pattern menu

Retrieval is not one thing. The shape of the question determines which tool does the work.

A retrieval selection matrix worth carrying forward:

| Question shape | First choice | When to add reranking |
| --- | --- | --- |
| Exact identifier, filename, error string | BM25 / deterministic lookup | Rarely needed |
| Fuzzy concept, paraphrased query | Dense vector search | When precision matters |
| Entity-neighborhood question | Entity index, one-hop graph | When neighborhoods overlap |
| Broad thematic question | Hierarchical summaries (RAPTOR-style) | When multiple summaries match |
| Cross-document synthesis | Graph + community summaries (GraphRAG) | Always |
| Temporal question | Time-aware retrieval with `valid_*` fields | When many candidates in window |
| Exact jargon in technical corpus | BM25-weighted hybrid (0.3/0.7 or 0.2/0.8) | When results are near-ties |

### Contextual retrieval (aging)

Anthropic's Contextual Retrieval work (anthropic.com/news/contextual-retrieval, September 2024, now 19 months old as of this writing, aging) is worth knowing. The technique prepends chunk-specific context to each chunk before embedding and BM25 indexing, which preserves information that raw chunking loses. The reported numbers: combining contextual embeddings and contextual BM25 reduced top-20 retrieval failure rate by 49% (5.7% down to 2.9%); adding a reranker took it to 67% reduction (down to 1.9%). The gains are real. Anthropic's 2026 release notes mention stronger long-context retrieval in newer Claude models but don't publish updated Contextual Retrieval-specific numbers. Treat the method as a useful architectural pattern whose headline numbers are aging.

The cost is real too. Every chunk needs an LLM pass to generate its context, which is expensive at corpus scale. Contextual retrieval is a good fit where the corpus is stable and the queries are many, amortizing the indexing cost over reads. It's a poor fit for frequently-churning corpora.

### RAPTOR (likely stale)

RAPTOR (Sarthi et al., arXiv 2401.18059, January 2024, likely stale by the handbook's dating rule) recursively clusters text chunks and generates summaries from the bottom up, then retrieves at multiple abstraction levels. The 2024 paper reported a 20-point absolute improvement on QuALITY when combined with GPT-4 on multi-step document QA. The architecture remains conceptually useful for multi-hop and thematic queries, and where the corpus is long-form, semantically hierarchical, and stable enough that recursive summarization amortizes.

I have not found convincing primary-source production evidence from 2025 or 2026 showing RAPTOR as a default retrieval layer for agent memory stacks. Treat it as a targeted design pattern rather than a 2026 default. Use it when questions are thematic or multi-hop and the corpus rarely changes. Don't reach for it as a universal retrieval upgrade.

### GraphRAG and LazyGraphRAG

GraphRAG extracts entities and relationships into a knowledge graph, builds hierarchical community summaries, and answers global questions by summarizing across communities. Microsoft's own materials position it explicitly for query-focused summarization and discovery over large private text corpora, especially when snippet retrieval misses the big picture. The project's own notebooks note that global search is resource-intensive, and the GitHub issue tracker documents that community report creation can be a large share of indexing expense.

LazyGraphRAG (microsoft.com/en-us/research/blog/lazygraphrag-setting-a-new-standard-for-quality-and-cost/, November 2024) cut indexing cost to 0.1% of baseline by deferring LLM summarization to query time and using fast NLP noun-phrase extraction for the initial graph. Microsoft does not publish a clean "use GraphRAG above X documents" scale threshold, so any such threshold in a handbook is invented. What's defensible: use LazyGraphRAG for most agent memory at personal or small-team scale; use full GraphRAG only when global reasoning over highly interconnected corpora is the core use case; benchmark locally before committing.

### ColBERT and late interaction

ColBERT v2 (arXiv 2112.01488) stores multi-vector representations at the token level and defers relevance computation to a parallelizable MaxSim operation. It preserves finer semantic structure than single-vector dense retrieval at higher storage cost. Public evidence that late-interaction has become a mainstream default in agent memory by 2026 is thin; secondary market signal suggests renewed interest because of stronger out-of-domain generalization and long-context handling.

Use late interaction when lexical precision matters and dense embeddings are losing exact distinctions. Coding agents, technical documentation, and legal text are canonical fits. Cost depends heavily on self-hosting; managed APIs narrow the cost advantage substantially.

### Reranker landscape as of 2026

Qwen3-Reranker-4B (huggingface.co/Qwen/Qwen3-Reranker-4B, released June 2025) is the strongest open-weight reranker. 32K context, 81.20 on MTEB-Code. Cohere rerank-v4.0-pro (docs.cohere.com/changelog/rerank-v4.0, December 2025) supports semi-structured JSON documents via `rank_fields` and is tuned for agent-like payloads. Jina-reranker-v3 (jina.ai/news/jina-reranker-v3-0-6b-listwise-reranker-for-sota-multilingual-retrieval, October 2025; arXiv 2509.25085) does listwise reranking, processes up to 64 documents in a 131K context, and matters when relative ordering across many candidates is the bottleneck. Voyage's lineup is solid for general-purpose dense retrieval. MMR (Carbonell and Goldstein, 1998) remains the right mental model for diversity-aware reranking when top-K tends to return redundant chunks.

A credible public reranker bakeoff on agent-memory workloads specifically (as opposed to web QA or enterprise RAG) still doesn't exist. Any 2026 reranker choice for memory is still mostly a local benchmark exercise.

### Hybrid search tuning

Default weights in OpenClaw are 0.7 vector / 0.3 text. Our tuned defaults are closer to 0.6/0.4 with MMR enabled (lambda ~0.65), temporal decay enabled with a 60-day half-life, and a deeper candidate multiplier. The tuning came from a real failure: keyword exactness was not weighted heavily enough to rescue obvious exact matches that vector similarity ranked too low.

For technical corpora (code, configuration, jargon-dense documents), published guidance suggests flipping further: 0.3 vector / 0.7 text or even 0.2/0.8. The reason is the same failure mode: exact-string retrieval is necessary for identifiers, and semantic fuzz drowns it. Workload shape determines the right weights. Don't hold a single tuning constant across projects.

### Temporal decay as dimming, not erasure

Old memories should dim, not vanish. Evergreen files should not decay. Recent episodic notes should usually outrank stale ones. The system should not act like something from three months ago is as salient as something from yesterday unless the file type or direct query makes that appropriate. OpenClaw's temporal decay, when enabled, defaults to a 30-day half-life. That's a good starting point; tune per project.

## Context assembly and cache stability

Assembly is where the memory system hands off to the model. Get this wrong and you burn money for no reason.

### The prompt cache problem with dynamic RAG

Anthropic, OpenAI, and other providers offer prompt caching: if the prefix of your prompt is stable across requests, they charge less and return faster on cache hits. The catch is that any perturbation of the prefix destroys the cache. Dynamic RAG, in its classic form, does exactly this. Every turn, retrieve different chunks, inject them into the system prompt, invalidate the cache, pay full freight.

The architectural response, visible in a few 2025-2026 systems, is to stop doing dynamic per-turn retrieval in the prompt prefix and instead maintain a *stable* memory surface at the top of the context, updated by a background subagent rather than by per-query retrieval. Mastra's Observational Memory (mastra.ai/research/observational-memory) is the cleanest public example: background subagents summarize the active conversation into a dense observation log, placed at the top of the context, stable across many user turns. The system reported 94.87% on LongMemEval using gpt-5-mini under this architecture. The architecture is explicitly designed to preserve prefix caching by keeping the memory surface stable across turns; specific cache-hit-rate numbers for this architecture aren't published.

The generalization beyond Mastra's specific implementation: *static-prefix passive recall* beats dynamic RAG on both cost and quality for continuous conversations. The handbook's passive recall block, if placed at the top of context and updated only when the relevant memory set changes, preserves the cache. Dynamic RAG that injects new chunks every turn does not. This is one of the stronger architectural bets of the last year.

### Anthropic prompt caching economics

The official numbers make the architectural argument quantitative. Anthropic's prompt caching docs give: cache writes cost 1.25x base input-token price for the 5-minute cache; cache reads cost 0.1x base input-token price; default TTL is 5 minutes with an optional 1-hour TTL; reported latency reductions of up to 80% and cost reductions of up to 90% for cache-friendly patterns.

The implication for assembly design: at 0.1x read cost vs. 1.0x base input cost, a stable prefix across ten turns approximately pays for itself compared to re-reading the same content fresh. Above ten turns it's a clear win. Below two turns it's a loss. The architectural rule that follows: put tools, core system instructions, static identity, pinned memory, and the core recall block in the cached prefix; put retrieved memories, fresh evidence, user-turn content, and tool results in the suffix below the cache boundary.

OpenClaw's system-prompt docs explicitly support this split with a stable prefix above the cache boundary and a dynamic suffix below.

### Cache-stable assembly, in practice

The rules for keeping a context cache-friendly:

Put stable content first. Identity, operating context, pinned memory, the core recall block. These rarely change across turns; let them share a cache window.

Put per-turn content last. The new user message, tool call results, and anything else that changes every turn goes at the end of the context where cache invalidation is expected anyway.

Update the recall block only when triggering entities change. If the conversation continues discussing the same entities, the block should stay identical. Don't re-run the recall query and produce subtly different output each turn just because you can.

Version the recall block. When it does change, log why, so you can tie cache misses to real state changes rather than hidden non-determinism.

### Context budget accounting

Every serious stack should report what consumed the window: bootstrap, active transcript, recall, tools, artifacts, compaction summary, system instructions, and reserved output tokens. Visible per turn. Without this, you will find yourself with 40% of the window spent on bootstrap and no way to notice. OpenClaw's token-use reference is a reasonable reference shape for this accounting.

## Prospective memory

Prospective memory, cognitive science's term for "remembering to do something later when a trigger fires," is not yet a mature first-class memory type in LLM-agent systems. The public record, as of April 2026, shows future commitments handled almost entirely through task scheduling rather than as a native memory category.

OpenClaw punts prospective memory to cron, explicitly. The system prompt docs tell the agent to use cron for follow-up, reminders, and recurring work rather than sleep loops or polling. Memory provides context; the scheduler is separate infrastructure. OpenClaw's dreaming is scheduled too, but that's consolidation, not user-facing reminders.

Letta's event-driven operating-system-like architecture pages context in when external interrupts fire. Again, the scheduling lives outside the memory layer.

OpenAI's persistent memory, Claude Projects, and related products don't publicly document first-class durable schemas for future commitments with deadlines, triggers, and conditional reminders. Scheduled commitments are implemented in application logic, calendar integrations, or task systems rather than in the memory substrate.

No published benchmark currently measures prospective memory directly. LongMemEval covers temporal reasoning and knowledge updates, both prerequisite capabilities, but doesn't test "remember to do X when Y happens." MemoryAgentBench's selective-forgetting competency is adjacent but not the same thing.

Treat this section as "what prospective memory would look like if you built it." The need is real. Agents drop follow-ups, miss deadlines, never fire conditional reminders. The shape below is a design sketch, not a documented production pattern.

A first-class prospective memory surface would look like:

```yaml
---
id: prosp_20260423_001
type: prospective
trigger:
  kind: time | event | condition
  schedule: "2026-05-01T09:00:00-06:00"
  condition: "when PR #1234 merges"
owner: agent_primary | user | subagent_foo
status: pending | fired | satisfied | cancelled | missed
commitment: "Check whether Clara's proposal PR has merged; if so, draft follow-up email."
created_at: 2026-04-23
due_by: 2026-05-01
requires_confirmation: true
---
```

The scheduling logic must live *outside* the model's context. A deterministic external scheduler (cron, job queue, event bus) wakes the agent when the trigger fires, injects the prospective memory item into the context as a "standing order," and the agent acts. OpenClaw's `HEARTBEAT.md` plus a periodic cron injection is a good default. Letta's event-driven architecture is another shape.

The failure mode to design against is silent completion. Agents sometimes hallucinate that they've done a thing when they haven't. Mitigations: require the agent to produce a tracking id when accepting the commitment, log the actual completion event with a handle, and check the log rather than trust the agent's self-report. If the agent claims it fired a follow-up email, verify via the email provider's API, not via the agent's narrative.

## Cost modeling

Memory stack decisions have real bills. A rough sketch of per-layer costs as of 2026, drawn from published reports:

Flat external vector retrieval runs cheap. Mem0's published p95 latency is 1.44s, with per-conversation token usage around 1,764 tokens on LoCoMo-shaped workloads (arXiv 2504.19413). In-process full-context carries maximum accuracy but ~26,031 tokens per conversation on the same workload, a roughly 14x cost multiplier. Zep's numbers on LongMemEval-S are similarly stark: full-context with GPT-4o used roughly 115k average context tokens and took 28.9s; Zep used around 1.6k average context tokens and took 2.58s, for a 92% reduction in both context and latency.

GraphRAG indexing is expensive unless you use LazyGraphRAG. Full GraphRAG ran $200-$500 and 5-15 hours on GPT-4o for 10M tokens in 2024 pricing. LazyGraphRAG cut this to 0.1% of baseline indexing cost by moving work to query time.

Self-hosted embedding and reranking on a dedicated GPU (Text Embeddings Inference on something like an A100) can run embedding cost as low as ~$0.019 per million tokens at reasonable utilization, vs. $0.10-$0.13 per million tokens on managed APIs. Reranking costs scale with reranker size; listwise rerankers like Jina-v3 process up to 64 documents per forward pass in a 131K context.

Prompt caching changes the math substantially. At Anthropic's 0.1x read cost vs. 1.25x write cost, stable-prefix architectures that achieve high cache hit rates can cut effective per-turn costs substantially compared to cache-destroying dynamic RAG. Published cache-hit-rate studies for passive recall architectures don't yet exist, so precise savings numbers should be benchmarked locally.

Agentmemory (github.com/rohitg00/agentmemory) publishes a practical comparison. LLM-summarized memory at ~$500/year, agentmemory context injection at ~$10/year, agentmemory with local embeddings (all-MiniLM-L6-v2) at essentially $0 for modest-volume personal agents. Numbers are project-specific but the relative order of magnitudes tends to hold.

For planning a harness, I usually think in three cost tiers. Tier 1 is bootstrap plus structured memory plus builtin hybrid search, dollars per month. Tier 2 adds contextual retrieval, reranking, and nightly synthesis, tens to low hundreds per month. Tier 3 adds GraphRAG, listwise reranking at scale, or extensive agent-driven re-indexing, hundreds to thousands per month. Pick your tier from the actual failure mode you're paying to solve, not from the aesthetic appeal of the top tier.

Clean per-layer cost breakdowns for GraphRAG, RAPTOR, or nightly synthesis in production agent memory systems are still not public. Vendor cost publications focus on benchmark-setting comparisons rather than "at X DAU with Y memories per user, the monthly retrieval plus consolidation plus caching bill is Z" operating models. Build your own cost telemetry if you care.

## Sleep-time synthesis and dreaming

Dreaming is not "nightly maintenance with a poetic name." It's synthesis on top of maintenance. Maintenance is janitorial (dedup, index, refresh). Dreaming is cognitive (what patterns are emerging, what is stale, what should change).

### The evidence that dreaming pays for itself

Letta's sleep-time compute paper (Lin et al., "Sleep-time Compute: Beyond Inference Scaling at Test-time," arXiv 2504.13171, April 2025) reported concrete results. On Stateful GSM-Symbolic and Stateful AIME, allowing the model to think offline about context before a user query reduced the test-time compute needed to hit baseline accuracy by ~5x. Scaling sleep-time compute further increased absolute accuracy by up to 13% on GSM-Symbolic and 18% on AIME. Multi-Query amortization (pre-computing multiple related queries per context) decreased average per-query cost by 2.5x. The paper also notes that efficacy correlates with predictability of the future query distribution; if the queries are opaque, the ROI story weakens fast.

That's enough evidence that dreaming is not vanity compute when properly structured. It is not evidence that *any* dreaming pipeline pays off. Yours might not. Evaluate on your own workload.

Public measurement of "nightly synthesis changed next-day user-visible behavior by X% at Y token cost" outside of Letta's benchmark setting is thin. Treat the Letta numbers as proof-of-concept for the approach rather than as guaranteed production ROI.

### OpenClaw's dreaming, verified

OpenClaw's current dreaming pipeline (live as of April 2026) is opt-in, disabled by default, and runs as a managed sweep with three phases: Light, REM, and Deep. The phases are internal cooperative phases, not user-facing modes. The default cron schedule is `0 3 * * *` (daily at 3 AM) and executes in Light → REM → Deep order.

Light sorts and stages short-term material from daily notes and signals. It deduplicates and places candidates into a transient `memory/.dreams/` store. No durable writes to `MEMORY.md`.

REM reflects on the staged candidates, extracts themes and recurring ideas, and appends reinforcement signals. Also no durable writes.

Deep is the only phase that can append to `MEMORY.md`. It evaluates candidates against thresholds including `minScore`, `minRecallCount`, and `minUniqueQueries`. Grounding requires that surviving snippets can be rehydrated from live daily files; if the raw context was deleted, the system skips the promotion. This is the structural defense against synthesis-time hallucination.

The governance property that matters most here is the one often glossed over in discourse about dreaming: **dream diary text is explicitly not a promotion source**. OpenClaw writes a human-readable narrative to `DREAMS.md` and optional per-phase reports under `memory/dreaming/<phase>/YYYY-MM-DD.md`, but these narrative artifacts cannot themselves become durable memory. Only grounded memory snippets are eligible to promote, and only through the Deep phase's threshold gates. There is even a grounded historical backfill lane with reversible staging and rollback for operator review.

The CLI surface is concrete. `openclaw memory promote --limit N --min-score 0.75` previews promotions. `openclaw memory promote --apply` writes them. `openclaw memory rem-backfill --stage-short-term` stages grounded candidates into the short-term dreaming store.

### On "concrete commitments for the next day"

Earlier versions of this handbook suggested that the best version of a dreaming system produces at least one concrete commitment or changed behavior for the next day. That framing is too strong for what currently ships. OpenClaw's dreaming explicitly separates dream diary prose (for human reading) from promotable records (grounded evidence only). There is no public mechanism where a commitment written in the dream prose gets autonomously executed the next day. The system's conservatism is the point.

The philosophical question of whether offline synthesis should produce executable next-day commitments is open. The current shipping answer is no. Synthesis can upgrade noisy daily signals into grounded durable memory; that's real value. Synthesis cannot mint behavior. If you want to build a system where the agent autonomously acts on last night's insights, you're innovating past the current production pattern, and you'll need to design the grounding and approval layer explicitly.

### What to measure

To know whether dreaming is paying for itself, don't measure "did the dream sound insightful." Measure whether the promotions it produces actually change downstream retrieval and recall. Track promotion rate (how many candidates clear the gates), promotion precision (do promoted memories actually get recalled usefully later), and downstream effect (did the promoted memory change an outcome or was it inert). If the numbers are low, the dream is doing less useful work than the token bill implies and should be cut back, retuned, or redesigned.

### Design frames beyond the shipping pipeline

Two conceptual distinctions are worth carrying even if the shipping pipeline doesn't implement them directly. They shape how you think about what dreaming is *for*, which in turn shapes how you'd design your own pipeline if you're not using OpenClaw's.

The first is the three-pass design for what a dream actually does. Pass one asks why did this happen this way instead of some other way. Pass two asks what should change. Pass three asks what is the uncomfortable question this system is avoiding. The third pass is the one most systems skip. Synthesis that doesn't force itself to name the uncomfortable question becomes a beautiful reporting loop. Coherent dream entries that never challenge the agent's current model of the world are the single biggest failure mode of nightly synthesis. Force the third pass, even if it produces one sentence of genuine discomfort per day. The point isn't the literary quality of the discomfort; it's that the synthesis has to do work the agent won't otherwise do during normal operation.

The second is the substrate-versus-journal layering. An upstream substrate can do associative generation: fragmentary candidates, raw reflections, loose pattern-matching across recent material. A downstream journal can do authored synthesis, taking the substrate and writing a coherent, grounded, operationally useful entry. Neither should be confused with janitorial cleanup. The substrate is "what am I noticing." The journal is "what do I conclude." Cleanup is "what should be deleted, deduplicated, or reindexed." Three different jobs. A system that collapses them will either over-produce noise (too much raw substrate without synthesis) or over-certify conclusions (journal entries minted without the grounding that substrate provides).

OpenClaw's current Light/REM/Deep pipeline implements a related but different cut: Light stages, REM reflects, Deep promotes grounded snippets. The substrate-versus-journal distinction is an older conceptual frame; the shipping pipeline is the current operational implementation. Both are useful. A reader designing their own dreaming layer should understand the conceptual frames before committing to a specific phase architecture.

## Failure modes we learned the hard way

### Too many writers touching the same memory surfaces

Overlapping crons and multiple agents both writing into project memory files produce noisy, duplicated, sometimes contradictory content. The rule that worked: content comes from real-time agent writing during actual sessions. Crons do cleanup and structure maintenance. One writer per concern.

### Treating session transcripts as a substitute for memory

Transcript indexing sounds appealing. It mostly loses to well-structured memory files for the queries that actually matter. Transcripts are noisy, verbose, and full of tool chatter. They're useful as a fallback or audit substrate. They are not automatically your best memory representation.

### Building smart extraction pipelines before writing discipline existed

If frontmatter quality and memory hygiene are weak, all the advanced retrieval machinery is building on sand. Structure first. Machinery second.

### Assuming failed tool calls are the only important ones

Covered in the tool output section, but worth repeating. Success payloads can contain the actual failure state of another system. Only artifacting on `isError === true` misses half the story.

### Letting large outputs linger in the active window

Model gets distracted, compaction gets worse, next several turns pay a tax for a blob that should have been externalized immediately.

### Overbuilding graph infrastructure too early

Knowledge graphs are powerful. They're also expensive, complicated, and often unnecessary at personal or small-team scale. A disciplined Markdown system plus an entity index plus good recall can outperform much fancier infrastructure for a long time.

### Trusting passive recall to resolve ambiguous entities

The v1 passive recall used exact string matching. We learned where it breaks (aliases, collisions, namespace cross-contamination) and added canonical ids, alias tables, scope rules, and confidence thresholds. A production passive-recall layer needs these; a prototype usually doesn't.

### Dreaming on messy memory

Synthesis on low-quality substrate amplifies noise. The sequencing matters: clean the substrate first, let dreaming work on trustworthy input. Otherwise the dream just writes confident prose about garbage.

### Self-reinforcement of stored errors

This is the failure mode the governance chapter exists to prevent. The agent writes a confident-but-wrong belief into memory. Passive recall surfaces it on every relevant turn. Nightly synthesis summarizes it into a community summary. The belief gains apparent corroboration from its own echo. Over time the agent becomes increasingly confident about a thing that was never true. Without contradiction checks, source scoring, and grounding requirements, this failure mode compounds silently. If the governance chapter feels heavier than v1, this is why.

### Benchmark over-trust

The Mem0 vs. Zep dispute is the current canonical case. Cite methodology, not just numbers. Don't trust any benchmark leaderboard that doesn't publish its full harness. And remember Zep's own admission that DMR became too easy to differentiate systems: vendor papers that honestly call out when an older benchmark is too weak are more trustworthy than ones that silently keep citing it.

### Memory drift from operator inattention

Over months of operation, memory moves. Old beliefs linger, new beliefs calcify, synthesis compounds small errors. Without periodic ground-truth refreshes (the agent asks the user to confirm or correct key facts), the drift is invisible until it matters. Published longitudinal field studies on drift don't yet exist, so instrument your own system if you care.

### Trusting AI-mediated research for handbook construction

The meta-failure mode this version of the handbook documents in its verification note. Deep Research agents, Context7-indexed sources, and pattern-matched numbers across multiple models can all produce the same wrong answer. The live vendor documentation is the consistent ground truth. Apply propose-versus-promote to your own research process: AI-surfaced facts are candidates, primary sources promote them.

## Current framework landscape

A survey, not a shopping list. Each of these is worth knowing as a pattern. The descriptions below go down to field-level specs where the public documentation supports it, because that's where a handbook is most useful as a reference.

### OpenAI Agents SDK Sessions

Useful as session history, not a full durable memory architecture. Stores session items before each run, appends new items after. Treat as conversation memory; pair with something else for durable state.

### LangGraph and LangChain stores

Strong reference model for short-term thread state via checkpointers and long-term stores organized by namespaces. The Deep Agents production path adds what I'd call the strongest shipping ACL primitives in mainstream agent orchestration: namespaces, path-level permissions, read-only enforcement on shared paths, and explicit guidance that organization-wide policies should be writable only through application code rather than by the agent itself. Not a rich confidence/provenance schema; the governance surface is access control rather than claim semantics.

### LangMem

Frames semantic, episodic, and procedural memory cleanly, with explicit questions about what gets stored, when, by whom, and where.

### Letta, MemGPT, and MemFS

Letta's current direction toward MemFS, a git-tracked memory system, aligns well with the handbook's preference for inspectable file-backed memory. The filesystem-agent finding (74.0% on LoCoMo with a simple filesystem architecture, letta.com/blog/benchmarking-ai-agent-memory, August 2025) is a useful counter to the assumption that you need a complex vector store to win. The sleep-time compute paper (arXiv 2504.13171) provides the clearest public ROI numbers for offline synthesis.

### Mem0

Delta policy via `ADD`, `UPDATE`, `DELETE`, `NOOP` during the update phase, not a state machine. Rich lifecycle operations including `delete`, `batch_delete`, and `delete_all`, with the important safety change that empty-filter `delete_all` no longer wipes everything and full-project wipes require an explicit `"*"` wildcard. Metadata, categories, immutable memories, filtered bulk deletion, event logs, platform audit features, GDPR support. No public first-class field set for provenance confidence, promotion status, or reviewer-approved trust tiers; trust is implicit rather than explicit. Evaluation published on LoCoMo QA subset with 26% relative gain in judge score, 91% lower p95 latency, and more than 90% token savings vs. full-context (arXiv 2504.19413). Read with the methodology caveats from the Mem0-vs-Zep dispute.

### Zep and Graphiti

Temporally aware knowledge graphs. The strongest publicly documented bi-temporal model I've found: every fact or relationship edge carries `valid_at`, `valid_from`, `invalid_at`, and `observed_at` metadata. Contradictions handled by edge invalidation (closing the old edge's temporal window, opening a new edge) rather than destructive overwrite. Relative time normalization ("two weeks ago") supported. Ingestion order tracked separately from real-world validity. No public first-class schema for per-fact confidence or provenance chains. Supports RRF, MMR, graph-aware rerankers, and cross-encoders as precision layers, with BGE-m3 in published experiments. Rich on supersession and temporal semantics; thinner on governance metadata.

### CrewAI Memory

Partial trust model. Records carry a `source` tag and a `private` flag. Memory can be scoped by path. Importance, categories, and metadata are inferred by the LLM on save. Consolidation actions are `keep`, `update`, `delete`, and `insert_new` when similarity exceeds a threshold. Real provenance and access control but not a candidate-to-trusted promotion state machine with reviewer roles.

### Mastra Observational Memory

The cleanest public implementation of stable-prefix passive recall. Background subagents summarize the active conversation into a dense observation log placed at the top of the context, updated continuously rather than injected per-turn. 94.87% on LongMemEval with gpt-5-mini (mastra.ai/research/observational-memory, February 2026). The architecture is explicitly designed to preserve prefix caching; specific cache-hit-rate numbers are not published.

### MemOS and MemCube

The MemCube schema (arXiv 2505.22101, memos-docs.openmem.net/open_source/modules/mem_cube) tracks origin signatures (identifying whether memory comes from inference extraction, user input, external retrieval, or parameter fine-tuning), access permissions / ACL, time-to-live policies, version chains logging modification history and derivation lineage, and compliance mechanisms including sensitivity tags, watermarking, and access logging. The clearest formal trust model in current production-oriented research.

### SuperLocalMemory

Explicit Bayesian trust defense (arXiv 2603.02240) with `created_by`, `source_protocol`, `trust_score`, and `provenance_chain` fields. Beta-Binomial posterior inference for trust scoring. Reading this before designing your own governance layer will save you from reinventing the obvious parts.

### Synthius-Mem

Reports 94.37% on LoCoMo with 99.55% adversarial robustness (arXiv 2604.11563). Brain-inspired cognitive-domain architecture. Benchmark caveats apply.

### OpenClaw Memory Wiki

The richest public trust and provenance schema I've found in shipping open-source memory infrastructure. Structured claims with `id`, `text`, `status`, `confidence`, `evidence[]`, `updatedAt`. Each evidence entry carries `sourceId`, `path`, `lines`, `weight`, `note`, `updatedAt`. Contradiction, low-confidence, stale-page, and open-question dashboards. Compiled digests. Wiki-native tools: `wiki_status`, `wiki_lint`, `wiki_apply`, `wiki_search`, `wiki_get`. Obsidian-friendly render mode with backlinks. Bridge mode for importing dream reports, daily notes, memory root files, and event logs from the active memory plugin. If you're designing a governance layer, Memory Wiki is the reference implementation worth studying first.

### OpenMemory

Persistent MCP-compatible memory layer across Cursor, VS Code, Claude, and other MCP-compatible clients. Useful as cross-tool shared memory. Pair with the shared-memory trust rules if you deploy it; MCP introduces its own attack surface (tool poisoning, schema poisoning, shadowing, rug pulls per OWASP MCP Top 10) that shared memory amplifies.

### ForgetAgent

Published in IJRASET as "Verifiable Deletion in Multi-Layer Memory Architectures for LLM Agents." Addresses GDPR-style verifiable deletion. Lower-tier venue, but the architectural shape is useful for anyone designing the deletion-and-tombstone path.

### EmergenceMem

RAG-based approach (emergence.ai/blog/sota-on-longmemeval-with-rag, June 2025). Reports 86% on longmemeval_s (Internal variant); EmergenceMem Simple reports 82.4%; EmergenceMem Simple Fast reports 79%. Public GitHub at github.com/EmergenceAI/emergence_simple_fast.

### LazyGraphRAG

Deferred-summarization GraphRAG variant from Microsoft Research (November 2024). 0.1% of baseline indexing cost. Use this if you need GraphRAG-class capability without the full indexing bill.

### Anthropic Contextual Retrieval

Chunk-contextualization technique. 49% reduction in top-20 retrieval failure with contextual embeddings plus contextual BM25; 67% reduction with reranking added. Aging (September 2024, 19 months old). No newer Anthropic-published numbers.

## Implementation phasing, revised

The original v1 order was substrate, search, passive recall, tool guards, compaction, subagents, dreaming. v2 moved governance and evaluation much earlier. v2.1 keeps that ordering.

Phase 1. Bootstrap and file discipline. Create always-on bootstrap files with deliberate scope: identity, operating rules, human context, tools, pinned essentials, each separated. Create the memory directory with subdomains: episodic, knowledge, procedures, projects, prospective. Enforce frontmatter from day one; don't promise to backfill.

Phase 2. Governance metadata and trust tiers. Extend frontmatter with `source`, `trust_level`, `status`, `valid_*`, `supersedes`, `superseded_by`. Build the promotion path: candidates stage, a deterministic policy promotes. Untrusted inputs cannot write active memory. This is the unglamorous work that prevents the highest-stakes failures later.

Phase 3. Baseline search. Use the stock memory backend first. Get keyword and vector search working reliably. Confirm provider configuration. Validate quotas and embedding keys. Boring reliability beats innovation theater here.

Phase 4. Eval harness. Build the project-level test suite before adding clever retrieval. At minimum the twelve tests from the evaluation chapter. Run them on every change. Track pass rates over time.

Phase 5. Entity index and passive recall. Once frontmatter quality is high, build the deterministic index and passive recall block. Add canonical ids and alias tables from the start.

Phase 6. Transcript hygiene and tool output guards. Don't wait until sessions are huge. Add tool result guards early. Preserve state, artifact the rest.

Phase 7. Smarter compaction. Only after memory and passive recall exist should compaction change aggressively. Otherwise you're summarizing away the only copy of facts the system has.

Phase 8. Archive sidecars and bounded session tools. Once sessions go through multiple compaction cycles, add the append-only archive sidecar, the DAG of summary nodes, and the bounded session tools (`session_grep`, `session_describe`, `session_expand`). This is the layer that makes aggressive compaction safe because nothing is truly lost.

Phase 9. Subagent persistence. If the harness delegates, add agent-specific memory and writeback with the same promotion gates as any other source. Path-scoped ACLs modeled on LangChain Deep Agents. Verify end-to-end that subagent writes actually land; do not trust the presence of a tool to mean it works.

Phase 10. Cache-stable context assembly. Place stable content first, dynamic content last. Update recall only when triggering entities change. Budget the cache boundary explicitly.

Phase 11. Prospective memory. Add the external scheduler and the prospective memory surface. Remember this is not a mature category in shipping systems; you're innovating.

Phase 12. Sleep-time synthesis. Only after the substrate is clean enough to be worth synthesizing. Dreaming on messy memory amplifies noise.

Phase 13. Ongoing evaluation and drift audits. Periodic ground-truth refreshes. Longitudinal regression tracking. The agent asking the user "is this still right?" on high-stakes stored beliefs.

## How this all connects as one coherent system

The easiest way to misunderstand the stack is to see it as a pile of unrelated clever features. It isn't.

The bootstrap files establish identity, rules, and stable relationship context. The structured memory files persist knowledge in forms that match different access patterns. The frontmatter makes deterministic indexing possible. The entity index makes passive recall cheap. Passive recall makes memory useful without explicit search, and placing the recall block at the top of context preserves the prompt cache. Because memory can reconstruct old facts, compaction can focus on preserving active state, evidence, and identifiers rather than every historical detail. Because tool results are guarded and artifacted early, transcripts stay cleaner and compaction gets better inputs. Because the archive sidecar and DAG history preserve recoverable detail outside the active window, compaction can be aggressive without losing provenance. Because subagents can inherit context, write back as candidates, and clear the same promotion gates as any other source, the whole system accumulates institutional knowledge rather than losing it on every delegation. Because governance metadata (trust, provenance, temporal validity, supersession) sits on every memory, synthesis can upgrade short-term material into durable memory without minting hallucinated facts. Because cache-stable assembly places the recall block at the top of context and refreshes only when entities change, the prompt cache actually works and the session is economically sustainable. Because evaluation tests run on every change, none of this drifts silently.

That is the full loop. Each layer earns its keep by making the next layer cheaper or more reliable. Skip a layer and the layers above it start paying for its absence.

## What to tell the next agent

Don't start by asking what the cleverest memory algorithm is.

Start by asking what the system must remember every turn, what it must remember sometimes, what it can reconstruct, what it must never lose, what kinds of noise are most likely to poison its active window, and what an adversary might try to make it believe.

Then ask what the agent should write at the moment of knowing, because that is when the data is richest.

Then ask how relevant memory should surface without explicit effort, and how the retrieval layer should explain itself.

Then ask how transcript pressure should be relieved without lying about the current state.

Then ask what work should happen while the user sleeps, and how to measure whether that work is actually worth its token bill.

Then ask how the system will know when it's wrong.

If you answer those questions in that order, the architecture usually becomes much clearer.

## Match the layer to the symptom

Do not cargo-cult the whole stack.

Some projects only need disciplined bootstrap files, structured memory notes, and built-in hybrid search. Bootstrap discipline alone gets you most of the way if the agent's job is well-scoped and sessions are short.

Some projects need passive recall because the cost of forgetting is high and the mention triggers are obvious. If the agent works across many named entities the user expects to be remembered, build passive recall before you add any clever retrieval.

Some projects need smart compaction because sessions are long, investigative, and full of branching evidence. Default compaction loses investigation state; if that's what you're paying to preserve, compaction is the layer to redesign.

Some projects need artifact guards immediately because tool output volume is the main failure mode. If one bad `exec` output can poison the next six turns, artifact guards are not optional.

Some projects need archive sidecars and DAG history because sessions go through many compaction cycles and historical provenance matters. If your users ask "when did we change our mind about X" and the current compactor can't answer, build the sidecars.

Some projects need governance and trust metadata because memory is multi-principal or ingests untrusted content. If more than one writer can touch durable memory, you need promotion gates and ACLs before anything else.

Some projects need evaluation because the agent's memory-driven behavior is hard to debug by feel. If you're shipping to real users and can't explain why the agent did what it did on a specific turn, build the eval harness before you build another retrieval layer.

Some projects need subagent persistence because delegation is the dominant pattern. If your parent agent keeps re-delegating the same discoveries, warm-start subagents with parent context and route their writebacks through candidate promotion.

Some projects need cache-stable assembly because per-turn token cost is a real constraint. If session volume is low, caching optimization can come later. If session volume is high, this is day-one infrastructure.

Some projects need prospective memory because follow-through is the product. Scheduling agents, research agents with multi-week commitments, assistants that promise actions need this. Most don't.

Some projects need dreaming because they run long enough and broadly enough that nightly synthesis creates real leverage. Dreaming is for systems where patterns accumulate and cleanup is not enough; it's not for systems where daily output is already well-structured.

The right sequence is almost always the one in the implementation phasing chapter: get the writing and memory substrate right first, add governance and eval, then recall, then protect the transcript, then improve compaction, then add archive history, then improve subagent continuity, then cache-stable assembly, then prospective memory, then synthesis. Reverse the order and you spend a lot of time building clever machinery on top of bad state.

## Source map

Local workspace files that informed this handbook:

`memory/writing/memory-article-final.md`, the most complete articulation of the v1 stack.

`memory/projects/passive-recall.md`, the original passive recall design.

`memory/projects/deep-dreaming.md`, the deep dreaming architecture and design intent.

`memory/writing/context-as-capability.md`, the conceptual bridge from persistent context to capability.

`memory/archive/projects-completed/context-engine-v2-plan.md`, the first integrated custom context engine plan.

`memory/projects/compaction-v3/implementation-plan.md`, the detailed redesign of compaction around state and evidence.

`docs/plans/2026-03-31-runtime-tool-output-guards.md`, artifact-backed tool result handling.

`docs/plans/2026-04-02-lcm-runtime-rebaseline.md`, archive sidecars, DAG-backed history, cache-stable assembly.

Research artifacts from the v2.1 research pass (retained for provenance):

`Dropbox/Research/agent-memory/gpt-deep-research-agent-memory-2026-04-23.md`, GPT Deep Research pass against an iterated research prompt.

`gemini-research-factcheck-report.md`, subagent-generated fact-check of the Gemini Deep Research pass.

Primary OpenClaw sources used for verification (live site as of April 2026):

docs.openclaw.ai/concepts/memory, docs.openclaw.ai/concepts/memory-builtin, docs.openclaw.ai/concepts/memory-search, docs.openclaw.ai/concepts/compaction, docs.openclaw.ai/concepts/context, docs.openclaw.ai/concepts/context-engine, docs.openclaw.ai/concepts/dreaming, docs.openclaw.ai/concepts/system-prompt, docs.openclaw.ai/reference/token-use, docs.openclaw.ai/reference/memory-config, docs.openclaw.ai/reference/session-management-compaction, docs.openclaw.ai/gateway/configuration-reference, docs.openclaw.ai/channels/discord, docs.openclaw.ai/plugins/memory-wiki, docs.openclaw.ai/cli/memory.

Research and external sources cited:

Anthropic, Contextual Retrieval, anthropic.com/news/contextual-retrieval (September 2024, aging).

Anthropic, Persona Selection Model, anthropic.com/research/persona-selection-model.

Anthropic, prompt caching docs and release notes, docs.anthropic.com and anthropic.com release notes pages.

OWASP GenAI Security Project, LLM08:2025 Vector and Embedding Weaknesses, genai.owasp.org/llmrisk/llm08-excessive-agency/.

OWASP MCP Top 10, community-maintained MCP attack page.

Mem0 security writeup naming MINJA and AgentPoison, Mem0 engineering blog (February 2026).

Letta, Sleep-time Compute: Beyond Inference Scaling at Test-time, arXiv 2504.13171 (April 2025, aging).

Letta, Benchmarking AI Agent Memory, letta.com/blog/benchmarking-ai-agent-memory (August 2025).

Liu et al., Lost in the Middle: How Language Models Use Long Contexts, arXiv 2307.03172.

Xu et al., A-MEM: Agentic Memory for LLM Agents, arXiv 2502.12110.

Sun et al., ReSum: Unlocking Long-Horizon Search Intelligence via Context Summarization, arXiv 2509.13313.

Mem0, Building Production-Ready AI Agents with Scalable Long-Term Memory, arXiv 2504.19413, docs.mem0.ai.

Zep, A Temporal Knowledge Graph Architecture for Agent Memory, arXiv 2501.13956, docs.getzep.com.

Zep blog, Is Mem0 Really SOTA in Agent Memory, blog.getzep.com/lies-damn-lies-statistics-is-mem0-really-sota-in-agent-memory.

Zep papers, corrected evaluation issue, github.com/getzep/zep-papers/issues/5.

Mastra, Observational Memory, mastra.ai/research/observational-memory (February 2026).

MemOS, arXiv 2505.22101, memos-docs.openmem.net/open_source/modules/mem_cube.

SuperLocalMemory, arXiv 2603.02240.

Synthius-Mem, arXiv 2604.11563. MemMachine, arXiv 2604.04853.

LongMemEval, Wu et al., arXiv 2410.10813 (ICLR 2025).

LoCoMo, Maharana et al., 2024.

LoCoMo-Plus, arXiv 2602.10715.

Memora benchmark, arXiv 2604.20006.

LoCoEval, arXiv 2603.06358.

MemoryAgentBench, OpenReview / arXiv 2025-2026 cycle.

MemoryArena, Stanford Digital Economy Lab (February 2026).

MobileMem, OpenReview 2026.

NaturalMem, OpenReview.

EverMemOS preprint (April 2026, provisional).

EmergenceMem, emergence.ai/blog/sota-on-longmemeval-with-rag (June 2025).

AP-EDM, Adaptive Planning with Eval-Driven Memory, preprints.org/manuscript/202512.2186/v1.

ForgetAgent, ijraset.com/research-paper/forgetagent-verifiable-deletion-in-multi-layer-memory-architectures-for-llm-agents.

MemPalace audit, github.com/MemPalace/mempalace/issues/27.

Sarthi et al., RAPTOR: Recursive Abstractive Processing for Tree-Organized Retrieval, arXiv 2401.18059 (January 2024, likely stale).

Microsoft Research, LazyGraphRAG, microsoft.com/en-us/research/blog/lazygraphrag-setting-a-new-standard-for-quality-and-cost (November 2024).

ColBERT v2, Santhanam et al., arXiv 2112.01488.

Qwen3-Reranker-4B, huggingface.co/Qwen/Qwen3-Reranker-4B.

Cohere rerank-v4.0-pro, docs.cohere.com/changelog/rerank-v4.0 (December 2025).

Jina-reranker-v3, jina.ai/news/jina-reranker-v3-0-6b-listwise-reranker-for-sota-multilingual-retrieval (October 2025), arXiv 2509.25085.

Carbonell and Goldstein, The Use of MMR, Diversity-Based Reranking for Reordering Documents and Producing Summaries, 1998.

Agentmemory, github.com/rohitg00/agentmemory.

OpenAI Agents SDK, openai.github.io/openai-agents-python/sessions.

LangChain, docs.langchain.com. LangChain Deep Agents production guide. LangMem, langchain-ai.github.io/langmem.

CrewAI Memory, docs.crewai.com/en/concepts/memory.

## The ultra-compressed version

If another agent needs one paragraph:

Use small always-on bootstrap files for identity, rules, humans, and pinned essentials. Store durable knowledge in structured Markdown with strict frontmatter that includes trust, provenance, and temporal validity. Separate episodic, semantic, procedural, project, prospective, and agent memory. Build a deterministic entity index from frontmatter, with canonical ids and aliases. Add passive recall so the system surfaces relevant memories without explicit search, and place that recall block at the top of context to preserve prefix caching. Guard giant tool outputs early and replace them with artifact-backed stubs. Redesign compaction around current state, evidence, identifiers, and supersession. Treat subagents as first-class persistent workers that propose candidate memories; a promotion policy moves candidates to active. Model memory as a lifecycle state machine with tombstoning for real deletion. Add dreaming only after the substrate is clean enough to synthesize, and remember that dream prose is not a promotion source; only grounded evidence promotes. Build an eval harness from day one. Verify direction of attribution on every citation, not just existence of the cited entity. Treat AI-mediated research about this stack as candidates that primary sources must promote. Untrusted inputs propose; they do not promote.
