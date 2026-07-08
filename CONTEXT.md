# Memorum — Ambient Recall

Language for the passive-recall redesign (the successor to stream-e-ambient-recall-v3.0): human-like automatic associative memory for agents — relevant memories pushed into context at the moment of relevance, with zero tool calls and silence as the default.

## Language

**Associative channel**:
The push path — memory surfaced to the agent unbidden, cue-triggered, mid-session.
_Avoid_: passive recall (legacy), ambient injection

**Deliberate recall**:
The pull path — the agent consciously searching memory via the CLI.
_Avoid_: active recall

**Cue**:
The perceptual input the system matches memories against at a given moment — what the agent is seeing or doing right now.

**Prompt cue**:
A cue derived from a submitted user message (or a subagent's task brief).

**Work-stream cue**:
A cue derived from tool activity — files touched, commands run, errors emitted — perceived via post-tool-use lifecycle events.

**Rising edge**:
The discipline that a memory fires when its cue _becomes_ active, not continuously while the cue stays active; one ping, never a nag.

**Silence**:
The valid, default output of the associative channel when nothing clears the relevance bar. Token efficiency is a consequence of the bar, not the target.

**Trigger index**:
Dream-compiled activation conditions per memory — file paths/globs, error signatures, command patterns, key terms — consulted cheaply at cue time; the precomputed half of association.
_Avoid_: rules engine, watch list

**Standout gate**:
The relevance gate — a candidate surfaces only when it stands out sharply from the background score distribution for this cue, never by clearing an absolute score threshold. Exact trigger-index hits bypass it.

**Habituation**:
Per-memory, cross-session damping of a memory that keeps surfacing without evidence of use. Conservative, reversible, ranking-only — never a deletion.

**Use signal**:
Inferred evidence that a surfaced memory mattered: same-session **echo** (the memory's distinctive content reappearing in later cues — cheap, deterministic) and dream-time transcript adjudication (model-judged, offline, definitive).

**Surfaced set**:
The per-session record of what the channel has already injected — the state behind rising-edge dedup, echo detection, and habituation telemetry.

**Retina**:
The fast-model role (Gemma 4 31B on Cerebras today; a config value, not an identity): sensing, extraction, classification, and ranking over _presented_ items. The retina proposes, never disposes — its output is always a candidate, flag, or ranking with machine-verifiable provenance, never a canonical write or free conclusion.
_Avoid_: gemma (the model is swappable), fast judge

**Cortex**:
The smart-model role (the harness model dreaming already shells to): synthesis, prose, calibration, and disposition over retina output.

**Judge stage**:
The retina step on a prompt cue: one fast call over deterministic candidates answering "which of these, if any, are relevant now" — it may reorder and veto candidates, never author content. Fails open to the standout gate.

**Capture proposal**:
A retina-nominated candidate memory mined from a session transcript out-of-band, carrying a verbatim machine-checked supporting quote; lands as a governance Candidate (attention, not truth), disposed by cortex dreaming.

**Focus memory**:
One pinned, project-scoped, sitrep-shaped memory (status, open loops, worries, pointers) — dream-maintained (retina digests recent transcripts, cortex synthesizes) through the ordinary governed write path; hand-editable; its age always rendered. Supersedes hand-maintained STATE.md per-project once Memorum is live.
_Avoid_: continuity-state object, ContinuityState, sitrep file

**Desk**:
Live repo state — branch, uncommitted files, recent commits — read fail-open at session boundaries; the orientation cue. Context, not memory.

**Orientation**:
Session start (and post-compaction restart) rendered as the associative channel firing on the desk cue plus pinned memories (identity, invariants, the focus memory). Not a separate subsystem.

**Egress gate**:
The Stream D classification check deciding which content may leave the machine for cloud retina calls; sensitive content rides deterministic-only lanes.

## Relationships

- The **associative channel** consumes **prompt cues** and **work-stream cues**; **deliberate recall** consumes neither (it is agent-initiated).
- **Rising edge** applies per **cue**, per session.
- **Work-stream cues** are a per-harness progressive enhancement; **prompt cues** are the portable floor.

## Example dialogue

> **Dev:** "The agent opened `bench/baseline.json` — should the bench-corpus memory surface again on the next file read?"
> **Domain expert:** "No. The cue is already active; **rising edge** means it fired once when the file was first touched. If nothing new clears the bar, the channel stays **silent**."

## Flagged ambiguities

- "passive recall" historically named both the whole Stream E surface and the per-turn delta path — in the redesign, use **associative channel** for the push path and **deliberate recall** for the pull path.
