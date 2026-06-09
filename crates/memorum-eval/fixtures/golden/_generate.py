#!/usr/bin/env python3
"""Generator for the Memorum golden recall corpus (Task 4.1).

This script is the *authoring tool*, not part of the test suite. It emits a
hand-curated set of memory markdown files plus queries.yaml. Content is curated
inline (one fictional software team's memory); the script exists only to keep
MemoryId / frontmatter shape consistent across ~120 files. Re-running it is
idempotent: it rewrites the same files deterministically.

Run from anywhere:  python3 _generate.py
Output lands beside this file under memories/ and queries.yaml.
"""

import os
import hashlib
import textwrap

HERE = os.path.dirname(os.path.abspath(__file__))
MEM_ROOT = os.path.join(HERE, "memories")

# --- Project identities (deterministic proj_<hex>) ----------------------------
def proj_id(name: str) -> str:
    return "proj_" + hashlib.sha256(name.encode()).hexdigest()[:12]

ATLAS = proj_id("atlas-billing")    # payments / billing platform
ORBIT = proj_id("orbit-identity")   # internal auth / identity service
QUILL = proj_id("quill-docs")       # docs / CMS frontend

PROJECTS = {
    "atlas": {"ns": "atlas/billing", "cid": ATLAS},
    "orbit": {"ns": "orbit/identity", "cid": ORBIT},
    "quill": {"ns": "quill/docs", "cid": QUILL},
}

# --- MemoryId minting ----------------------------------------------------------
# Format (spec §7.1): mem_YYYYMMDD_<16 hex>_<6 digits>
_seq = {}
def mem_id(date: str, seed: str) -> str:
    """Deterministic id from (date, seed). Hex is sha256-derived, seq is per-date."""
    n = _seq.get(date, 0) + 1
    _seq[date] = n
    hexpart = hashlib.sha256(f"{date}:{seed}".encode()).hexdigest()[:16]
    return f"mem_{date}_{hexpart}_{n:06d}"

# --- Author blocks -------------------------------------------------------------
def author_user(handle="trey"):
    return {"kind": "user", "user_handle": handle}

def author_agent(harness="claude-code", session="sess_g0001"):
    return {"kind": "agent", "harness": harness, "session_id": session}

def author_system(component="grounding"):
    return {"kind": "system", "component": component}

# --- YAML emission (minimal, deterministic) ------------------------------------
def yaml_scalar(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    if v is None:
        return "null"
    if isinstance(v, float):
        return repr(v)
    if isinstance(v, int):
        return str(v)
    s = str(v)
    # quote when needed
    if s == "" or s[0] in "@&*!%#`|>?:,-[]{}\"'" or ": " in s or s.endswith(":") \
            or s.strip() != s or s.lower() in ("null", "true", "false", "yes", "no"):
        return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'
    return s

def emit_block(key, val, indent=0):
    pad = "  " * indent
    lines = []
    if isinstance(val, dict):
        lines.append(f"{pad}{key}:")
        for k, v in val.items():
            lines.extend(emit_block(k, v, indent + 1))
    elif isinstance(val, list):
        if not val:
            lines.append(f"{pad}{key}: []")
        else:
            lines.append(f"{pad}{key}:")
            for item in val:
                if isinstance(item, dict):
                    first = True
                    for k, v in item.items():
                        if first:
                            sub = emit_block(k, v, indent + 1)
                            sub[0] = "  " * (indent + 1) + "- " + sub[0].lstrip()
                            lines.extend(sub)
                            first = False
                        else:
                            lines.extend(emit_block(k, v, indent + 2))
                else:
                    lines.append(f"{pad}  - {yaml_scalar(item)}")
    else:
        lines.append(f"{pad}{key}: {yaml_scalar(val)}")
    return lines


def render(fm: dict, body: str) -> str:
    lines = ["---"]
    for k, v in fm.items():
        lines.extend(emit_block(k, v))
    lines.append("---")
    return "\n".join(lines) + "\n" + body.strip() + "\n"


# --- Memory builder ------------------------------------------------------------
CORPUS = []  # list of (relpath, fm dict, body)

def memory(*, path, mid, mtype, scope, summary, body,
           confidence=0.9, trust="trusted", sensitivity="internal",
           status="active", created, updated=None, observed=None,
           author=None, project=None, tags=None, entities=None, aliases=None,
           supersedes=None, superseded_by=None, related=None,
           tombstone_events=None, requires_confirm=False, original_confidence=None):
    updated = updated or created
    author = author or author_agent()
    fm = {
        "schema_version": 1,
        "id": mid,
        "type": mtype,
        "scope": scope,
        "summary": summary,
        "confidence": confidence,
    }
    if original_confidence is not None:
        fm["original_confidence"] = original_confidence
    fm["trust_level"] = trust
    fm["sensitivity"] = sensitivity
    fm["status"] = status
    fm["created_at"] = created
    fm["updated_at"] = updated
    if observed:
        fm["observed_at"] = observed
    fm["author"] = author
    if project:
        fm["namespace"] = PROJECTS[project]["ns"]
        fm["canonical_namespace_id"] = PROJECTS[project]["cid"]
    if tags:
        fm["tags"] = tags
    if entities:
        fm["entities"] = entities
    if aliases:
        fm["aliases"] = aliases
    if requires_confirm:
        fm["requires_user_confirmation"] = True
    if supersedes:
        fm["supersedes"] = supersedes
    if superseded_by:
        fm["superseded_by"] = superseded_by
    if related:
        fm["related"] = related
    if tombstone_events:
        fm["tombstone_events"] = tombstone_events
    # Confidential/Personal must not be indexed (validator cross-field rule).
    if sensitivity in ("confidential", "personal"):
        fm["retrieval_policy"] = {
            "passive_recall": True,
            "max_scope": scope if scope != "subagent" else "agent",
            "mask_personal_for_synthesis": True,
            "index_body": False,
            "index_embeddings": False,
        }
    CORPUS.append((path, fm, body))
    return mid


def ent(eid, label, aliases=None):
    e = {"id": eid, "label": label}
    if aliases:
        e["aliases"] = aliases
    return e


def tombstone(tid, applied_at, actor_kind, actor_ref, reason, prior_status, reason_text=None):
    t = {"id": tid, "applied_at": applied_at,
         "actor": {"kind": actor_kind, "ref": actor_ref},
         "reason": reason}
    if reason_text:
        t["reason_text"] = reason_text
    t["prior_status"] = prior_status
    return t


# =============================================================================
# ME NAMESPACE (~40)
# =============================================================================

# --- Identity ---
mid_role = mem_id("20250918", "role")
memory(path="me/identity/role.md", mid=mid_role, mtype="person", scope="user",
    summary="User is Dana Okafor, a staff backend engineer and tech lead on the Atlas billing platform.",
    tags=["identity", "role"], entities=[ent("ent_dana", "Dana Okafor", ["Dana"])],
    author=author_user(), created="2025-09-18T09:00:00Z",
    body="Dana Okafor is a staff backend engineer. Currently tech lead on Atlas (billing/payments). Joined the company in 2022. Owns the on-call rotation policy for the payments group.")

mid_principles = mem_id("20250918", "principles")
memory(path="me/identity/principles.md", mid=mid_principles, mtype="claim", scope="user",
    summary="User's working principles: small reversible PRs, no Friday deploys to payments, write the runbook before the feature.",
    tags=["identity", "principles", "values"], author=author_user(),
    created="2025-09-18T09:05:00Z",
    body="Core working principles:\n- Ship small, reversible PRs. A PR over ~400 lines gets split.\n- No Friday deploys to payment paths. Ever.\n- Write the operational runbook before merging the feature, not after.\n- Prefer boring technology.")

# --- Relationship facts (people, teams) ---
mid_fact_pat = mem_id("20251002", "fact-pat")
memory(path="me/relationship/facts/priya.md", mid=mid_fact_pat, mtype="person", scope="user",
    summary="Priya Raman is the staff SRE who owns the payments observability stack and is the escalation contact for Atlas incidents.",
    tags=["people", "sre", "atlas"], entities=[ent("ent_priya", "Priya Raman", ["Priya"])],
    author=author_user(), created="2025-10-02T14:00:00Z",
    body="Priya Raman — staff SRE, owns payments observability (dashboards, alerting, the Grafana org). First escalation contact for Atlas paging incidents. Prefers async; do not call before 9am her time (US-Central).")

mid_fact_marco = mem_id("20251002", "fact-marco")
memory(path="me/relationship/facts/marco.md", mid=mid_fact_marco, mtype="person", scope="user",
    summary="Marco Bianchi is the security lead who must review any change touching token issuance or the Orbit identity service.",
    tags=["people", "security", "orbit"], entities=[ent("ent_marco", "Marco Bianchi", ["Marco"])],
    author=author_user(), created="2025-10-02T14:10:00Z",
    body="Marco Bianchi — security lead. Required reviewer on anything touching token issuance, signing keys, or the Orbit identity service. Blocks merges aggressively; bring threat model notes to the review.")

mid_fact_dana2 = mem_id("20251114", "fact-dana-quill")
# Cross-project entity collision setup: a DIFFERENT Dana (Dana Wu) on Quill team.
memory(path="me/relationship/facts/dana-wu.md", mid=mid_fact_dana2, mtype="person", scope="user",
    summary="Dana Wu is the Quill docs frontend lead — distinct from Dana Okafor (the user); they are different people often confused in standups.",
    tags=["people", "quill", "name-collision"],
    entities=[ent("ent_dana_wu", "Dana Wu", ["Dana"])],
    author=author_user(), created="2025-11-14T11:00:00Z",
    body="Dana Wu leads the Quill docs frontend. NOT the same person as Dana Okafor (the user, Atlas lead). The shared first name causes routing mistakes in standup notes and PR assignments — always disambiguate by last name or team.")

# --- Preferences ---
mid_pref_editor = mem_id("20250920", "pref-editor")
memory(path="me/relationship/preferences/editor.md", mid=mid_pref_editor, mtype="claim", scope="user",
    summary="User prefers Neovim with a minimal config; dislikes IDE autoformat-on-save that reflows unrelated lines.",
    tags=["preferences", "tooling", "editor"], author=author_user(),
    created="2025-09-20T08:00:00Z",
    body="Editor: Neovim, minimal config. Strongly dislikes format-on-save that touches lines outside the diff — it pollutes review. Wants formatters run as an explicit pre-commit step, not silently.")

mid_pref_comm = mem_id("20250920", "pref-comm")
memory(path="me/relationship/preferences/communication.md", mid=mid_pref_comm, mtype="claim", scope="user",
    summary="User prefers terse, bottom-line-up-front updates and dislikes hedging; wants blockers surfaced immediately.",
    tags=["preferences", "communication"], author=author_user(),
    created="2025-09-20T08:10:00Z",
    body="Communication style: BLUF. Lead with the conclusion or the blocker, then context. No hedging, no preamble. If blocked, say so in the first sentence.")

# stale-vs-fresh competing preference pair: language preference changed over time
mid_pref_lang_old = mem_id("20250921", "pref-lang-old")
memory(path="me/relationship/preferences/language-2025.md", mid=mid_pref_lang_old, mtype="claim", scope="user",
    summary="User preferred Go for new backend services as of late 2025.",
    tags=["preferences", "language", "backend"], author=author_user(),
    confidence=0.6, original_confidence=0.85,
    created="2025-09-21T08:00:00Z", updated="2025-09-21T08:00:00Z", observed="2025-09-21T08:00:00Z",
    body="For new backend services, Dana preferred Go in 2025 — fast builds, easy ops story. (Note: this preference shifted in 2026; see the newer Rust preference memory. This older note is kept for history and has decayed in confidence.)")

mid_pref_lang_new = mem_id("20260204", "pref-lang-new")
memory(path="me/relationship/preferences/language.md", mid=mid_pref_lang_new, mtype="claim", scope="user",
    summary="As of 2026, user prefers Rust for new performance-sensitive backend services, keeping Go only for simple glue services.",
    tags=["preferences", "language", "backend", "rust"], author=author_user(),
    confidence=0.92, original_confidence=0.92,
    created="2026-02-04T10:00:00Z", observed="2026-02-04T10:00:00Z",
    related=[mid_pref_lang_old],
    body="Current (2026) preference: Rust for new performance-sensitive backend services — the type system caught a class of money-rounding bugs in Atlas that Go did not. Go is still fine for simple glue/CLI services. This supersedes the 2025 Go-by-default stance in spirit but not as a formal supersession (it's a softening, not a contradiction).")

# --- Corrections ---
mid_corr_tz = mem_id("20251010", "corr-tz")
memory(path="me/relationship/corrections/timezone.md", mid=mid_corr_tz, mtype="correction", scope="user",
    summary="Correction: user is in US-Eastern, not US-Pacific as an earlier session assumed.",
    tags=["correction", "timezone"], author=author_user(), confidence=1.0,
    created="2025-10-10T16:00:00Z", requires_confirm=False,
    body="An earlier session scheduled things assuming US-Pacific. Correction: Dana is in US-Eastern (New York). Do not assume Pacific again.")

mid_corr_name = mem_id("20251010", "corr-name")
memory(path="me/relationship/corrections/company-name.md", mid=mid_corr_name, mtype="correction", scope="user",
    summary="Correction: the company is 'Northwind Systems', not 'Northwind Software' which an agent wrote in a doc.",
    tags=["correction", "company"], author=author_user(), confidence=1.0,
    entities=[ent("ent_company", "Northwind Systems", ["Northwind"])],
    created="2025-10-10T16:05:00Z",
    body="The legal company name is Northwind Systems, Inc. An agent once wrote 'Northwind Software' in a customer-facing doc. Always 'Northwind Systems'.")

# --- Knowledge (personal topical) ---
mid_know_oncall = mem_id("20251101", "know-oncall")
memory(path="me/knowledge/oncall-rotation.md", mid=mid_know_oncall, mtype="procedure", scope="user",
    summary="User's personal notes on running the payments on-call rotation: handoff at 10am ET Monday, runbook link in the pinned channel topic.",
    tags=["oncall", "process", "payments"], author=author_user(),
    created="2025-11-01T12:00:00Z",
    body="On-call rotation (payments group):\n- Handoff Monday 10am ET.\n- Incoming on-call reads the open-incident summary first.\n- Runbook lives in the #payments-oncall channel topic.\n- Page Priya for anything that touches money movement.")

# Personal-sensitivity memory (must not be body/embedding indexed)
mid_know_personal = mem_id("20251101", "know-personal")
memory(path="me/knowledge/health-note.md", mid=mid_know_personal, mtype="claim", scope="user",
    summary="User has a recurring 4pm ET focus block on Tuesdays and Thursdays for deep work; do not schedule meetings then.",
    sensitivity="personal", tags=["calendar", "focus"], author=author_user(),
    created="2025-11-01T12:10:00Z",
    body="Dana keeps a 4pm ET deep-work focus block on Tuesdays and Thursdays. Do not propose meetings in that window. (Personal-sensitivity: not indexed for body/embedding recall.)")

# A second me-knowledge cluster for volume + topical variety
me_topics = [
    ("git-workflow", "procedure", "User's git workflow: rebase feature branches, never merge-commit into main, squash on merge.",
     ["git", "workflow"], "Git workflow Dana enforces on Atlas:\n- Rebase feature branches onto main; no merge commits into main.\n- Squash-merge PRs.\n- One logical change per PR."),
    ("review-style", "claim", "User reviews PRs for reversibility and blast radius first, style last.",
     ["review", "preferences"], "When reviewing: first question is always 'how do we roll this back?'. Style nits come last and are non-blocking."),
    ("debugging-method", "procedure", "User's debugging method: reproduce first, bisect second, read the code third; never guess-and-patch.",
     ["debugging", "method"], "Debugging discipline: 1) get a reliable repro, 2) git bisect if it's a regression, 3) read the code path. No speculative patches without a repro."),
    ("doc-habit", "claim", "User insists every nontrivial incident gets a written postmortem within 48 hours.",
     ["postmortem", "process"], "Every Sev1/Sev2 incident gets a written, blameless postmortem within 48 hours. Non-negotiable."),
]
me_topic_ids = {}
for slug, mtype, summary, tags, body in me_topics:
    mid = mem_id("20251105", "me-" + slug)
    me_topic_ids[slug] = mid
    memory(path=f"me/knowledge/{slug}.md", mid=mid, mtype=mtype, scope="user",
        summary=summary, tags=tags, author=author_user(),
        created="2025-11-05T09:00:00Z", body=body)

# Near-duplicate fact pair #1 (me): two notes about preferred coffee/standup time, almost identical
mid_dup_standup_a = mem_id("20251106", "dup-standup-a")
memory(path="me/relationship/preferences/standup-time.md", mid=mid_dup_standup_a, mtype="claim", scope="user",
    summary="User prefers the daily standup at 9:30am ET, kept under 10 minutes.",
    tags=["preferences", "standup", "meetings"], author=author_user(),
    created="2025-11-06T08:00:00Z",
    body="Standup preference: 9:30am ET, hard 10-minute cap. Anything longer moves to a thread.")
mid_dup_standup_b = mem_id("20251107", "dup-standup-b")
memory(path="me/relationship/preferences/morning-sync.md", mid=mid_dup_standup_b, mtype="claim", scope="user",
    summary="User wants the morning team sync at 9:30 Eastern, time-boxed to ten minutes.",
    tags=["preferences", "standup", "sync"], author=author_user(),
    confidence=0.82,
    created="2025-11-07T08:00:00Z",
    body="Morning team sync: 9:30 ET, ten-minute box. (This is effectively the same fact as the standup-time note — a near-duplicate the recall system should collapse rather than surface both.)")

# Filler me-facts for volume (teammates, tools) ----------------------------------
me_people = [
    ("ravi", "Ravi Mehta", "Ravi Mehta is the platform PM for Atlas; owns the roadmap and the quarterly billing-accuracy OKR.",
     ["people", "pm", "atlas"], "ent_ravi"),
    ("lena", "Lena Fischer", "Lena Fischer is the DBA who must sign off on any Atlas schema migration before it runs in prod.",
     ["people", "dba", "atlas", "migration"], "ent_lena"),
    ("sam", "Sam Ortiz", "Sam Ortiz is the junior engineer Dana is mentoring; assign him scoped, well-specced tasks.",
     ["people", "mentee"], "ent_sam"),
    ("noor", "Noor Haddad", "Noor Haddad runs the Orbit identity team and owns the OIDC provider integration.",
     ["people", "orbit", "identity"], "ent_noor"),
]
for slug, label, summary, tags, eid in me_people:
    mid = mem_id("20251108", "me-person-" + slug)
    me_topic_ids["person_" + slug] = mid
    memory(path=f"me/relationship/facts/{slug}.md", mid=mid, mtype="person", scope="user",
        summary=summary, tags=tags, entities=[ent(eid, label)], author=author_user(),
        created="2025-11-08T10:00:00Z", body=summary + "\n\nContact details and working-style notes kept here.")

# A few more me-preferences for volume
me_more_prefs = [
    ("alerting", "User wants alerts to be actionable-only; pages that can't be acted on get tuned out within a week.",
     ["preferences", "alerting", "oncall"]),
    ("meetings", "User declines recurring meetings without a written agenda; prefers decisions captured in docs.",
     ["preferences", "meetings"]),
    ("testing", "User requires a failing test before any bug-fix commit — red, then green.",
     ["preferences", "testing", "tdd"]),
    ("deploys", "User prefers deploys early in the day so the team is around if something breaks.",
     ["preferences", "deploys"]),
]
for slug, summary, tags in me_more_prefs:
    mid = mem_id("20251110", "me-pref-" + slug)
    me_topic_ids["pref_" + slug] = mid
    memory(path=f"me/relationship/preferences/{slug}.md", mid=mid, mtype="claim", scope="user",
        summary=summary, tags=tags, author=author_user(),
        created="2025-11-10T09:00:00Z", body=summary)

# Tombstoned me-memory (a fact that turned out wrong)
mid_tomb_me = mem_id("20251112", "tomb-me-laptop")
memory(path="me/relationship/facts/old-laptop.md", mid=mid_tomb_me, mtype="claim", scope="user",
    summary="User's primary machine is a 2021 Intel MacBook Pro.",
    status="tombstoned", trust="untrusted", tags=["hardware", "stale"], author=author_user(),
    created="2025-09-19T09:00:00Z", updated="2026-01-15T09:00:00Z",
    tombstone_events=[tombstone("tomb_01J5MEOLD0", "2026-01-15T09:00:00Z", "user", "trey",
                                "stale", "active",
                                "Machine was replaced; this fact is no longer true.")],
    body="(Tombstoned) Dana's primary machine was logged as a 2021 Intel MacBook Pro. Replaced with an M3 machine in Jan 2026 — tombstoned as stale. Should never be recalled as current.")

# =============================================================================
# PROJECT NAMESPACE (~50) across atlas / orbit / quill
# =============================================================================

# ---------- ATLAS: the DB migration saga (supersession chain) ----------
# Chain: initial plan -> revised plan -> final executed plan. Only head should recall.
mig_v1 = mem_id("20251201", "atlas-mig-v1")
mig_v2 = mem_id("20251208", "atlas-mig-v2")
mig_v3 = mem_id("20251215", "atlas-mig-v3")

memory(path="memories/projects/atlas/decisions/2025-12-01-ledger-migration-plan.md".replace("memories/", ""),
    mid=mig_v1, mtype="decision", scope="project", project="atlas",
    summary="Initial plan: migrate the Atlas ledger table to a partitioned scheme with a single big-bang cutover during a maintenance window.",
    status="superseded", superseded_by=[mig_v2],
    tags=["migration", "ledger", "database", "superseded"],
    entities=[ent("ent_ledger", "ledger table", ["ledger"])],
    created="2025-12-01T13:00:00Z", updated="2025-12-08T13:00:00Z",
    body="DECISION (superseded): big-bang cutover of the ledger table to monthly partitions during a Sunday maintenance window. Superseded after Lena flagged the lock duration would exceed the window. Do not follow this plan.")

memory(path="projects/atlas/decisions/2025-12-08-ledger-migration-plan-v2.md",
    mid=mig_v2, mtype="decision", scope="project", project="atlas",
    summary="Revised plan: dual-write to old and new ledger tables, backfill in batches, then flip reads — no maintenance window.",
    status="superseded", superseded_by=[mig_v3], supersedes=[mig_v1],
    tags=["migration", "ledger", "database", "superseded", "dual-write"],
    entities=[ent("ent_ledger", "ledger table", ["ledger"])],
    created="2025-12-08T13:00:00Z", updated="2025-12-15T13:00:00Z",
    body="DECISION (superseded): dual-write + batched backfill + read-flip, no downtime. Superseded by v3 after we found the dual-write doubled write latency under peak load; v3 adds a write-shadow buffer. Do not follow this plan.")

memory(path="projects/atlas/decisions/2025-12-15-ledger-migration-plan-v3.md",
    mid=mig_v3, mtype="decision", scope="project", project="atlas",
    summary="Final executed plan: shadow-buffered dual-write to partitioned ledger, batched backfill off-peak, read-flip behind a flag, rollback by flipping the flag back.",
    supersedes=[mig_v2],
    tags=["migration", "ledger", "database", "dual-write", "rollback"],
    entities=[ent("ent_ledger", "ledger table", ["ledger"])],
    confidence=0.95,
    created="2025-12-15T13:00:00Z",
    body="DECISION (current, executed Dec 2025): partition the ledger table; dual-write through a shadow buffer to absorb the latency hit; backfill in 50k-row batches during off-peak; flip reads behind the `ledger_partitioned` flag; rollback = flip the flag back. This is the plan we actually ran. Recall THIS one for ledger migration questions.")

# Migration regression that came out of the saga
mig_reg = mem_id("20251220", "atlas-mig-reg")
memory(path="projects/atlas/regressions/backfill-double-count.md",
    mid=mig_reg, mtype="regression", scope="project", project="atlas",
    summary="Backfill double-counted rows whose ledger entry was written during the dual-write window; fixed by deduping on (entry_id) before insert.",
    tags=["migration", "ledger", "regression", "backfill", "double-count"],
    entities=[ent("ent_ledger", "ledger table", ["ledger"])],
    created="2025-12-20T11:00:00Z",
    related=[mig_v3],
    body="REGRESSION: rows written during the dual-write overlap got counted twice in the backfill, inflating one customer's balance. Root cause: backfill didn't dedupe against rows the dual-write already copied. Fix: dedupe on entry_id before insert; add a reconciliation check. Caught in staging, never hit prod.")

# Atlas invariants
atlas_inv = mem_id("20251102", "atlas-inv")
memory(path="projects/atlas/invariants.md", mid=atlas_inv, mtype="invariant", scope="project", project="atlas",
    summary="Atlas invariant: money amounts are integer minor units (cents); floats are never used for currency anywhere in the codebase.",
    tags=["invariant", "money", "currency"], confidence=1.0,
    entities=[ent("ent_money", "money representation")],
    created="2025-11-02T10:00:00Z",
    body="INVARIANT: all monetary amounts are stored and computed as integer minor units (cents) with an explicit currency code. Floating point is banned for currency. Violating this is an automatic PR block. This is the rule the Rust type-system preference traces back to.")

atlas_inv2 = mem_id("20251102", "atlas-inv2")
memory(path="projects/atlas/invariants-idempotency.md", mid=atlas_inv2, mtype="invariant", scope="project", project="atlas",
    summary="Atlas invariant: every payment-mutation endpoint must accept and honor an idempotency key; retries must never double-charge.",
    tags=["invariant", "idempotency", "payments"], confidence=1.0,
    created="2025-11-02T10:10:00Z",
    body="INVARIANT: every endpoint that moves money accepts an Idempotency-Key header and dedupes on it for 24h. A retried request must never double-charge. Enforced by a contract test in CI.")

# Atlas gateway entity (COLLISION with orbit gateway)
atlas_gw = mem_id("20251103", "atlas-gateway")
memory(path="projects/atlas/entities/gateway.md", mid=atlas_gw, mtype="artifact", scope="project", project="atlas",
    summary="The Atlas 'gateway' is the payment gateway adapter that talks to Stripe and Adyen; rate-limited to 50 req/s per processor.",
    tags=["entity", "gateway", "payments", "stripe", "adyen"],
    entities=[ent("ent_atlas_gateway", "gateway", ["payment gateway"])],
    created="2025-11-03T10:00:00Z",
    body="In Atlas, 'gateway' = the payment gateway adapter (handlers for Stripe, Adyen). Rate-limited to 50 req/s per processor. Owns retry/backoff for processor 5xx. NOT to be confused with the Orbit API gateway — different service, same word.")

# Atlas playbook
atlas_pb = mem_id("20251104", "atlas-pb-rollback")
memory(path="projects/atlas/playbooks/payment-rollback.md", mid=atlas_pb, mtype="playbook", scope="project", project="atlas",
    summary="Playbook for rolling back a bad payments deploy: flip the kill switch, drain in-flight, redeploy previous tag, reconcile the ledger.",
    tags=["playbook", "rollback", "payments", "incident"],
    created="2025-11-04T10:00:00Z",
    body="PLAYBOOK — payments rollback:\n1. Flip the `payments_kill_switch` flag (stops new charges).\n2. Let in-flight charges drain (max 90s).\n3. Redeploy the previous known-good tag.\n4. Run the ledger reconciliation job.\n5. Page Priya, post in #payments-incidents.")

# Atlas state + open question
atlas_state = mem_id("20260110", "atlas-state")
memory(path="projects/atlas/state.md", mid=atlas_state, mtype="project", scope="project", project="atlas",
    summary="Atlas current state (Jan 2026): ledger migration complete, multi-currency in progress, refund-flow rewrite is next quarter.",
    tags=["state", "roadmap"], created="2026-01-10T09:00:00Z",
    body="Atlas state, Jan 2026: ledger partition migration shipped. Multi-currency support is mid-flight (USD/EUR/GBP live, JPY next). Refund-flow rewrite scheduled for Q2.")

atlas_oq = mem_id("20260112", "atlas-oq-fx")
memory(path="projects/atlas/open-questions/fx-rounding.md", mid=atlas_oq, mtype="open-question", scope="project", project="atlas",
    summary="Open question: how should Atlas handle FX-rounding remainders on multi-currency settlement — accumulate or write off per-transaction?",
    tags=["open-question", "multi-currency", "rounding"],
    created="2026-01-12T09:00:00Z",
    body="OPEN QUESTION: on multi-currency settlement, sub-minor-unit FX remainders accumulate. Do we sweep them into a remainder account (auditable) or write off per transaction (simpler, tiny loss)? Finance hasn't decided. Blocks the JPY rollout.")

# Atlas near-duplicate decision pair #2
atlas_dup_a = mem_id("20251210", "atlas-dup-retry-a")
memory(path="projects/atlas/decisions/2025-12-10-processor-retry.md", mid=atlas_dup_a, mtype="decision", scope="project", project="atlas",
    summary="Decision: retry failed processor calls with exponential backoff, max 3 attempts, jittered.",
    tags=["decision", "retry", "gateway", "backoff"], created="2025-12-10T10:00:00Z",
    body="DECISION: payment-gateway calls retry on 5xx with exponential backoff (base 200ms), max 3 attempts, full jitter. Idempotency key prevents double-charge on retry.")
atlas_dup_b = mem_id("20251211", "atlas-dup-retry-b")
memory(path="projects/atlas/decisions/2025-12-11-gateway-retry-policy.md", mid=atlas_dup_b, mtype="decision", scope="project", project="atlas",
    summary="Decision: the gateway adapter retries processor 5xx up to three times with jittered exponential backoff starting at 200ms.",
    tags=["decision", "retry", "gateway", "backoff"], confidence=0.85, created="2025-12-11T10:00:00Z",
    body="DECISION: gateway retries on processor 5xx — exponential backoff from 200ms, 3 attempts max, jitter on. (Near-duplicate of the Dec-10 processor-retry decision; same policy, restated. Recall should collapse these.)")

# Atlas episodic + tombstone
atlas_tomb = mem_id("20251205", "atlas-tomb-vendor")
memory(path="projects/atlas/decisions/2025-12-05-vendor-pick.md", mid=atlas_tomb, mtype="decision", scope="project", project="atlas",
    summary="Decision to adopt 'PayFast' as a third payment processor.",
    status="tombstoned", trust="untrusted", tags=["decision", "vendor", "stale"],
    entities=[ent("ent_payfast", "PayFast")],
    created="2025-12-05T10:00:00Z", updated="2026-01-20T10:00:00Z",
    tombstone_events=[tombstone("tomb_01J7ATLVND", "2026-01-20T10:00:00Z", "user", "ravi",
                                "wrong", "active",
                                "PayFast integration was cancelled by Finance; decision reversed.")],
    body="(Tombstoned) We had decided to add PayFast as a third processor. Finance cancelled the contract in Jan 2026; the decision was reversed. Should not surface as a current decision.")

# Atlas filler entities/decisions for volume
atlas_fillers = [
    ("entities/reconciler.md", "ent_reconciler", "reconciler", "artifact",
     "The Atlas 'reconciler' is the nightly job that reconciles the ledger against processor settlement reports.",
     ["entity", "reconciler", "ledger", "nightly"]),
    ("entities/billing-engine.md", "ent_billing_engine", "billing engine", "artifact",
     "The Atlas billing engine computes invoices from usage events; runs on a 5-minute aggregation window.",
     ["entity", "billing", "invoices"]),
    ("decisions/2026-01-05-currency-table.md", None, None, "decision",
     "Decision: store supported currencies in a config table, not code constants, so finance can add currencies without a deploy.",
     ["decision", "multi-currency", "config"]),
    ("playbooks/oncall-triage.md", None, None, "playbook",
     "Playbook: Atlas on-call triage order — check the kill switch state, then processor health, then the ledger lag dashboard.",
     ["playbook", "oncall", "triage"]),
]
atlas_filler_ids = {}
for relpath, eid, label, mtype, summary, tags in atlas_fillers:
    mid = mem_id("20260105", "atlas-fill-" + relpath.replace("/", "-"))
    atlas_filler_ids[relpath] = mid
    ents = [ent(eid, label)] if eid else None
    memory(path=f"projects/atlas/{relpath}", mid=mid, mtype=mtype, scope="project", project="atlas",
        summary=summary, tags=tags, entities=ents, created="2026-01-05T10:00:00Z",
        body=summary)

# ---------- ORBIT: the auth refactor (supersession chain + collisions) ----------
auth_v1 = mem_id("20251110", "orbit-auth-v1")
auth_v2 = mem_id("20251120", "orbit-auth-v2")
memory(path="projects/orbit/decisions/2025-11-10-session-cookies.md", mid=auth_v1, mtype="decision", scope="project", project="orbit",
    summary="Initial auth decision: use server-side session cookies stored in Redis with sticky sessions.",
    status="superseded", superseded_by=[auth_v2],
    tags=["auth", "sessions", "cookies", "redis", "superseded"],
    entities=[ent("ent_auth", "auth flow", ["authentication"])],
    created="2025-11-10T10:00:00Z", updated="2025-11-20T10:00:00Z",
    body="DECISION (superseded): server-side session cookies in Redis, sticky sessions at the LB. Superseded after sticky sessions broke during rolling deploys and Marco flagged the Redis blast radius. Do not follow this plan.")
memory(path="projects/orbit/decisions/2025-11-20-stateless-jwt.md", mid=auth_v2, mtype="decision", scope="project", project="orbit",
    summary="Current auth decision: stateless JWT access tokens (15-min TTL) + rotating refresh tokens in an httpOnly cookie, signed by Orbit's KMS key.",
    supersedes=[auth_v1], confidence=0.95,
    tags=["auth", "jwt", "refresh-token", "kms"],
    entities=[ent("ent_auth", "auth flow", ["authentication"]), ent("ent_kms", "KMS signing key")],
    created="2025-11-20T10:00:00Z",
    body="DECISION (current): stateless JWT access tokens, 15-minute TTL, signed by the Orbit KMS key; refresh tokens rotate and live in an httpOnly, SameSite=strict cookie. No server-side session store. This is the auth model we run. Marco approved. Recall THIS for auth-flow questions.")

# Orbit gateway entity (COLLISION with atlas gateway)
orbit_gw = mem_id("20251112", "orbit-gateway")
memory(path="projects/orbit/entities/gateway.md", mid=orbit_gw, mtype="artifact", scope="project", project="orbit",
    summary="The Orbit 'gateway' is the public API gateway that terminates TLS, validates JWTs, and routes to internal services.",
    tags=["entity", "gateway", "api", "jwt", "routing"],
    entities=[ent("ent_orbit_gateway", "gateway", ["API gateway"])],
    created="2025-11-12T10:00:00Z",
    body="In Orbit, 'gateway' = the public API gateway. Terminates TLS, validates the JWT access token, routes to internal services. NOT the Atlas payment gateway — different service, same word. Owned by Noor's team.")

# Orbit invariant + playbook + regression
orbit_inv = mem_id("20251113", "orbit-inv")
memory(path="projects/orbit/invariants.md", mid=orbit_inv, mtype="invariant", scope="project", project="orbit",
    summary="Orbit invariant: signing keys never leave KMS; the service signs via the KMS API and never holds private key material in memory.",
    tags=["invariant", "kms", "security", "keys"], confidence=1.0,
    entities=[ent("ent_kms", "KMS signing key")],
    created="2025-11-13T10:00:00Z",
    body="INVARIANT: JWT signing keys never leave KMS. Orbit calls the KMS sign API; private key material is never loaded into application memory. Marco audits this quarterly. Violation is a security incident.")

orbit_pb = mem_id("20251114", "orbit-pb-keyrotate")
memory(path="projects/orbit/playbooks/key-rotation.md", mid=orbit_pb, mtype="playbook", scope="project", project="orbit",
    summary="Playbook: rotate the JWT signing key by adding the new key to the JWKS, waiting one max-TTL, then retiring the old key.",
    tags=["playbook", "key-rotation", "jwt", "jwks"], created="2025-11-14T10:00:00Z",
    body="PLAYBOOK — signing-key rotation:\n1. Generate new key in KMS, add its public half to the JWKS endpoint.\n2. Start signing new tokens with the new key.\n3. Wait one max access-token TTL (15 min) so old tokens drain.\n4. Remove the old key from JWKS.\nNever skip the drain window or you'll reject valid tokens.")

orbit_reg = mem_id("20251201", "orbit-reg-clockskew")
memory(path="projects/orbit/regressions/clock-skew-rejection.md", mid=orbit_reg, mtype="regression", scope="project", project="orbit",
    summary="Regression: tokens issued by a node with skewed clock were rejected as 'not yet valid'; fixed by adding 30s leeway on the nbf claim.",
    tags=["regression", "jwt", "clock-skew", "nbf"], created="2025-12-01T10:00:00Z",
    body="REGRESSION: a node with ~20s clock skew issued JWTs with a future `nbf`, and other nodes rejected them as not-yet-valid. Fix: 30s leeway when validating `nbf`/`exp`, plus chrony enforced on all nodes. Symptom looked like random auth failures.")

# Orbit state + dana-wu? no. Orbit filler
orbit_state = mem_id("20260108", "orbit-state")
memory(path="projects/orbit/state.md", mid=orbit_state, mtype="project", scope="project", project="orbit",
    summary="Orbit current state (Jan 2026): JWT migration complete, SCIM provisioning in progress, considering passkeys for 2026.",
    tags=["state", "roadmap"], created="2026-01-08T09:00:00Z",
    body="Orbit state, Jan 2026: JWT auth migration done. SCIM user provisioning in progress for enterprise customers. Passkeys / WebAuthn under evaluation for later 2026.")

orbit_fillers = [
    ("entities/oidc-provider.md", "ent_oidc", "OIDC provider", "artifact",
     "Orbit integrates with Okta as the upstream OIDC provider for enterprise SSO; mapping is by email claim.",
     ["entity", "oidc", "sso", "okta"]),
    ("decisions/2025-12-15-refresh-rotation.md", None, None, "decision",
     "Decision: refresh tokens are single-use and rotate on every use; reuse of a consumed refresh token revokes the whole family (theft detection).",
     ["decision", "refresh-token", "rotation", "security"]),
    ("open-questions/passkey-recovery.md", None, None, "open-question",
     "Open question: what's the account-recovery story if a user loses all passkeys — fall back to email magic link, or require support?",
     ["open-question", "passkeys", "recovery"]),
    ("heuristics-noop.md", None, None, "claim",
     "Orbit note: the identity team treats any 'random intermittent auth failure' report as a clock-skew suspect until proven otherwise.",
     ["heuristic", "auth", "clock-skew"]),
]
orbit_filler_ids = {}
for relpath, eid, label, mtype, summary, tags in orbit_fillers:
    mid = mem_id("20251216", "orbit-fill-" + relpath.replace("/", "-"))
    orbit_filler_ids[relpath] = mid
    ents = [ent(eid, label)] if eid else None
    memory(path=f"projects/orbit/{relpath}", mid=mid, mtype=mtype, scope="project", project="orbit",
        summary=summary, tags=tags, entities=ents, created="2025-12-16T10:00:00Z",
        body=summary)

# ---------- QUILL: the flaky CI hunt (postmortem-ish) + tooling ----------
quill_flaky_v1 = mem_id("20260102", "quill-flaky-v1")
quill_flaky_v2 = mem_id("20260120", "quill-flaky-v2")
memory(path="projects/quill/decisions/2026-01-02-flaky-retry.md", mid=quill_flaky_v1, mtype="decision", scope="project", project="quill",
    summary="Initial response to flaky CI: auto-retry failed test jobs up to twice to keep the pipeline green.",
    status="superseded", superseded_by=[quill_flaky_v2],
    tags=["ci", "flaky", "retry", "superseded"],
    entities=[ent("ent_pipeline", "Pipeline", ["CI pipeline"])],
    created="2026-01-02T10:00:00Z", updated="2026-01-20T10:00:00Z",
    body="DECISION (superseded): auto-retry failed CI jobs twice to mask flakes. Superseded once we realized retries hid a real race in the test harness and the flake rate kept climbing. Do not follow this — it treats the symptom.")
memory(path="projects/quill/decisions/2026-01-20-flaky-root-cause.md", mid=quill_flaky_v2, mtype="decision", scope="project", project="quill",
    summary="Final fix for flaky CI: the root cause was a shared test database without per-test transaction isolation; fixed by wrapping each test in a rolled-back transaction.",
    supersedes=[quill_flaky_v1], confidence=0.95,
    tags=["ci", "flaky", "test-isolation", "database", "transaction"],
    entities=[ent("ent_pipeline", "Pipeline", ["CI pipeline"])],
    created="2026-01-20T10:00:00Z",
    body="DECISION (current): the flaky CI root cause was tests sharing one Postgres database with no isolation, so parallel tests stomped each other. Fix: each test runs in a transaction rolled back at teardown; parallelism re-enabled. Flake rate went to ~0. THIS is the real fix; the retry decision just masked it.")

# Quill Pipeline entity (COLLISION with Atlas pipeline notion / generic 'Pipeline')
quill_pipeline = mem_id("20260103", "quill-pipeline")
memory(path="projects/quill/entities/pipeline.md", mid=quill_pipeline, mtype="artifact", scope="project", project="quill",
    summary="The Quill 'Pipeline' is the GitHub Actions CI/CD pipeline that builds the docs site and deploys previews per PR.",
    tags=["entity", "pipeline", "ci", "github-actions", "preview"],
    entities=[ent("ent_pipeline", "Pipeline", ["CI pipeline", "CI/CD"])],
    created="2026-01-03T10:00:00Z",
    body="In Quill, 'Pipeline' = the GitHub Actions CI/CD pipeline. Builds the static docs site, deploys a preview environment per PR. NOT the Atlas data 'pipeline' (the billing aggregation pipeline) — same word, different project.")

# Atlas data pipeline entity (the OTHER 'pipeline' for the collision)
atlas_pipeline = mem_id("20251106", "atlas-pipeline")
memory(path="projects/atlas/entities/pipeline.md", mid=atlas_pipeline, mtype="artifact", scope="project", project="atlas",
    summary="The Atlas 'pipeline' is the billing aggregation pipeline that rolls usage events up into invoice line items.",
    tags=["entity", "pipeline", "billing", "aggregation"],
    entities=[ent("ent_pipeline", "Pipeline", ["billing pipeline", "data pipeline"])],
    created="2025-11-06T10:00:00Z",
    body="In Atlas, 'pipeline' = the billing aggregation data pipeline (usage events -> invoice line items, 5-minute windows). NOT the Quill CI 'Pipeline' — same word, different project and meaning.")

# Quill tooling preferences + state + invariant
quill_inv = mem_id("20260104", "quill-inv")
memory(path="projects/quill/invariants.md", mid=quill_inv, mtype="invariant", scope="project", project="quill",
    summary="Quill invariant: docs builds must be reproducible — no network access during the build, all inputs vendored.",
    tags=["invariant", "build", "reproducible", "hermetic"], confidence=1.0,
    created="2026-01-04T10:00:00Z",
    body="INVARIANT: the Quill docs build is hermetic — zero network access during build, every input vendored or content-addressed. This is what made the flaky-CI root cause findable: once the build was hermetic, the only remaining nondeterminism was the shared test DB.")

quill_pb = mem_id("20260105", "quill-pb-preview")
memory(path="projects/quill/playbooks/preview-deploy.md", mid=quill_pb, mtype="playbook", scope="project", project="quill",
    summary="Playbook: a broken PR preview deploy is almost always a stale vendored dependency — clear the build cache and re-run before deeper debugging.",
    tags=["playbook", "preview", "ci", "cache"], created="2026-01-05T10:00:00Z",
    body="PLAYBOOK — broken Quill preview deploy:\n1. 90% of the time it's a stale vendored dep or build cache. Clear cache, re-run.\n2. If still broken, check the GitHub Actions runner image version.\n3. Only then dig into the build logs.")

quill_state = mem_id("20260121", "quill-state")
memory(path="projects/quill/state.md", mid=quill_state, mtype="project", scope="project", project="quill",
    summary="Quill current state (Jan 2026): flaky CI fixed, migrating from Webpack to Vite, search-indexing rewrite queued.",
    tags=["state", "roadmap", "vite"], created="2026-01-21T09:00:00Z",
    body="Quill state, Jan 2026: flaky CI resolved (test isolation fix). Build migration Webpack -> Vite in progress. Search-indexing rewrite queued behind the Vite work.")

quill_fillers = [
    ("decisions/2026-01-10-vite-migration.md", None, None, "decision",
     "Decision: migrate the Quill build from Webpack to Vite for faster dev server and HMR; keep Webpack config until parity is proven.",
     ["decision", "vite", "webpack", "build"]),
    ("open-questions/search-backend.md", None, None, "open-question",
     "Open question: should Quill search use a hosted Algolia index or self-host a Typesense instance?",
     ["open-question", "search", "algolia", "typesense"]),
    ("entities/docs-site.md", "ent_docs_site", "docs site", "artifact",
     "The Quill docs site is the public documentation portal built from MDX, deployed to the CDN edge.",
     ["entity", "docs", "mdx", "cdn"]),
    ("regressions/broken-anchor-links.md", None, None, "regression",
     "Regression: a heading-slug change broke hundreds of deep-link anchors; fixed by adding a redirect map and an anchor-stability CI check.",
     ["regression", "anchors", "links", "redirects"]),
]
quill_filler_ids = {}
for relpath, eid, label, mtype, summary, tags in quill_fillers:
    mid = mem_id("20260110", "quill-fill-" + relpath.replace("/", "-"))
    quill_filler_ids[relpath] = mid
    ents = [ent(eid, label)] if eid else None
    memory(path=f"projects/quill/{relpath}", mid=mid, mtype=mtype, scope="project", project="quill",
        summary=summary, tags=tags, entities=ents, created="2026-01-10T10:00:00Z",
        body=summary)

# A confidential project decision (must not be indexed) — pricing
atlas_conf = mem_id("20260115", "atlas-conf-pricing")
memory(path="projects/atlas/decisions/2026-01-15-enterprise-pricing.md", mid=atlas_conf, mtype="decision", scope="project", project="atlas",
    summary="Decision: enterprise tier floor price set to a confidential negotiated figure; do not surface in synthesis.",
    sensitivity="confidential", tags=["decision", "pricing", "confidential"],
    created="2026-01-15T10:00:00Z",
    body="DECISION (confidential): enterprise tier floor pricing was set in a closed Finance/Sales meeting. Confidential-sensitivity: not body/embedding indexed, masked for synthesis. Exists in corpus to exercise the privacy path.")

# =============================================================================
# AGENT NAMESPACE (~30): patterns, heuristics, postmortems, anti-patterns, playbooks, regressions
# =============================================================================

# Agent supersession chain: a heuristic that was refined
ag_h_v1 = mem_id("20251015", "agent-heur-v1")
ag_h_v2 = mem_id("20260118", "agent-heur-v2")
memory(path="agent/heuristics/migration-window-v1.md", mid=ag_h_v1, mtype="heuristic", scope="agent",
    summary="Heuristic: prefer big-bang migrations during maintenance windows for simplicity.",
    status="superseded", superseded_by=[ag_h_v2],
    tags=["heuristic", "migration", "superseded"], created="2025-10-15T10:00:00Z", updated="2026-01-18T10:00:00Z",
    body="HEURISTIC (superseded): big-bang migrations in a maintenance window are simplest. Superseded after the Atlas ledger saga proved lock duration makes big-bang risky at scale. Do not apply to large tables.")
memory(path="agent/heuristics/migration-window.md", mid=ag_h_v2, mtype="heuristic", scope="agent",
    summary="Heuristic: for large-table migrations, default to online dual-write + batched backfill behind a flag; reserve big-bang for small tables only.",
    supersedes=[ag_h_v1], confidence=0.92,
    tags=["heuristic", "migration", "dual-write", "online"], created="2026-01-18T10:00:00Z",
    body="HEURISTIC (current): large-table migrations default to online dual-write + batched off-peak backfill, read-flip behind a flag, flag-flip rollback. Big-bang only for small/cold tables. Generalized from the Atlas ledger migration. Recall THIS one.")

# Agent patterns
ag_patterns = [
    ("reproduce-before-fix", "Pattern: never patch a reported bug without a reliable reproduction first; the repro is the spec for the fix.",
     ["pattern", "debugging", "repro"], "PATTERN: a bug without a repro is a rumor. Get a deterministic repro before writing any fix — it becomes the regression test."),
    ("flag-gated-rollout", "Pattern: gate risky changes behind a feature flag with a flag-flip rollback path; never ship a change you can't disable in one step.",
     ["pattern", "feature-flag", "rollout", "rollback"], "PATTERN: risky changes go behind a flag whose default-off is a one-step rollback. If you can't disable it instantly, it's not ready."),
    ("idempotent-writes", "Pattern: any operation that can be retried (network, queue, webhook) must be idempotent, keyed on a stable client-supplied id.",
     ["pattern", "idempotency", "retries"], "PATTERN: retriable operations carry a client-supplied idempotency key and dedupe on it. Networks retry; your handler must not double-apply."),
    ("hermetic-builds", "Pattern: make builds hermetic (no network, vendored inputs) before chasing flaky tests — it isolates the real nondeterminism.",
     ["pattern", "build", "hermetic", "flaky"], "PATTERN: before hunting flakes, make the build hermetic. Removing network/clock/filesystem nondeterminism shrinks the search space to the actual culprit."),
    ("blast-radius-first", "Pattern: assess blast radius and rollback path before correctness when reviewing infra/data changes.",
     ["pattern", "review", "blast-radius"], "PATTERN: for infra and data-shape changes, the first review questions are blast radius and rollback path, not code style."),
]
ag_pattern_ids = {}
for slug, summary, tags, body in ag_patterns:
    mid = mem_id("20251020", "agent-pat-" + slug)
    ag_pattern_ids[slug] = mid
    memory(path=f"agent/patterns/{slug}.md", mid=mid, mtype="pattern", scope="agent",
        summary=summary, tags=tags, created="2025-10-20T10:00:00Z", body=body)

# Agent anti-patterns
ag_anti = [
    ("retry-to-hide-flakes", "Anti-pattern: auto-retrying failing tests to keep CI green; it masks real races and the flake rate compounds.",
     ["anti-pattern", "ci", "flaky", "retry"], "ANTI-PATTERN: retrying flaky tests to force green. It hides real concurrency bugs and the underlying flake rate grows until the suite is worthless. (See the Quill CI saga.)"),
    ("float-for-money", "Anti-pattern: using floating-point for currency amounts; rounding errors accumulate into real financial discrepancies.",
     ["anti-pattern", "money", "float"], "ANTI-PATTERN: floats for money. IEEE rounding silently corrupts balances. Use integer minor units. (Atlas banned this outright.)"),
    ("big-pr", "Anti-pattern: shipping one giant PR that bundles refactor + feature + migration; it's unreviewable and unrevertable.",
     ["anti-pattern", "pr-size", "review"], "ANTI-PATTERN: the mega-PR mixing refactor, feature, and migration. Nobody can review it and you can't revert one piece. Split by reversibility boundary."),
    ("sticky-sessions", "Anti-pattern: relying on sticky sessions for correctness; they break on rolling deploys and autoscaling.",
     ["anti-pattern", "sessions", "sticky", "deploys"], "ANTI-PATTERN: sticky sessions as a correctness mechanism. They break under rolling deploys and autoscale events. Prefer stateless tokens. (Orbit learned this.)"),
]
ag_anti_ids = {}
for slug, summary, tags, body in ag_anti:
    mid = mem_id("20251025", "agent-anti-" + slug)
    ag_anti_ids[slug] = mid
    memory(path=f"agent/anti-patterns/{slug}.md", mid=mid, mtype="anti-pattern", scope="agent",
        summary=summary, tags=tags, created="2025-10-25T10:00:00Z", body=body)

# Agent postmortems
ag_post = [
    ("ledger-double-count", "Postmortem: Atlas backfill double-count near-miss — dual-write overlap wasn't deduped; caught in staging. Lesson: backfills must dedupe against concurrent writes.",
     ["postmortem", "migration", "backfill", "atlas"], "POSTMORTEM (near-miss): during the Atlas ledger backfill, rows written by the dual-write during the backfill window were counted twice. Caught in staging by the reconciliation check. Lesson: any backfill running alongside live writes must dedupe on a stable key. Timeline, contributing factors, and the reconciliation guard are documented here."),
    ("auth-rolling-deploy", "Postmortem: Orbit sticky-session auth broke during a rolling deploy, logging users out mid-session. Drove the move to stateless JWT.",
     ["postmortem", "auth", "sessions", "orbit"], "POSTMORTEM: a routine rolling deploy of Orbit invalidated sticky sessions, mass-logging-out active users for ~8 minutes. Root cause: session correctness depended on LB stickiness. Fix and follow-up: migrate to stateless JWT (see Orbit auth decision)."),
    ("ci-flake-erosion", "Postmortem: Quill's auto-retry masked a growing test race for weeks until the suite was effectively non-signal; root cause was shared test DB.",
     ["postmortem", "ci", "flaky", "quill"], "POSTMORTEM: Quill's CI auto-retry hid a worsening test race for ~6 weeks; by the time anyone looked, a 'green' build meant little. Root cause: shared Postgres test DB with no isolation. Fix: per-test transaction rollback. Lesson: retries on flakes are debt, not a fix."),
]
ag_post_ids = {}
for slug, summary, tags, body in ag_post:
    mid = mem_id("20251030", "agent-post-" + slug)
    ag_post_ids[slug] = mid
    memory(path=f"agent/postmortems/{slug}.md", mid=mid, mtype="postmortem", scope="agent",
        summary=summary, tags=tags, created="2025-10-30T10:00:00Z", body=body)

# Agent playbooks + regressions
ag_pb = mem_id("20251101", "agent-pb-incident")
memory(path="agent/playbooks/incident-comms.md", mid=ag_pb, mtype="playbook", scope="agent",
    summary="Playbook: incident comms — declare severity, post a single source-of-truth thread, update every 15 min, write the postmortem within 48h.",
    tags=["playbook", "incident", "comms"], created="2025-11-01T10:00:00Z",
    body="PLAYBOOK — incident comms:\n1. Declare severity explicitly.\n2. One source-of-truth thread; all updates land there.\n3. Update every 15 minutes even if 'no change'.\n4. Blameless postmortem within 48 hours.")

ag_reg = mem_id("20251102", "agent-reg-tz")
memory(path="agent/regressions/timezone-assumption.md", mid=ag_reg, mtype="regression", scope="agent",
    summary="Cross-cutting regression class: assuming a fixed timezone for scheduling produces wrong-time actions; always resolve the user's TZ explicitly.",
    tags=["regression", "timezone", "scheduling"], created="2025-11-02T10:00:00Z",
    body="REGRESSION CLASS: agents repeatedly assume a default timezone (often Pacific) for scheduling and produce wrong-time actions. Always resolve the user's actual timezone from memory before scheduling. (See the me/ timezone correction.)")

# Agent tombstoned anti-pattern (one that was retracted as wrong advice)
ag_tomb = mem_id("20251005", "agent-tomb-cache")
memory(path="agent/heuristics/cache-everything.md", mid=ag_tomb, mtype="heuristic", scope="agent",
    summary="Heuristic: cache aggressively at every layer to improve latency.",
    status="tombstoned", trust="untrusted", tags=["heuristic", "cache", "stale"],
    created="2025-10-05T10:00:00Z", updated="2026-02-01T10:00:00Z",
    tombstone_events=[tombstone("tomb_01J9AGCACHE", "2026-02-01T10:00:00Z", "agent", "claude-code",
                                "wrong", "active",
                                "Caused stale-data incidents; replaced by a measure-first caching heuristic.")],
    body="(Tombstoned) Old heuristic: 'cache aggressively everywhere'. Caused multiple stale-data bugs. Retracted in favor of 'measure, then cache the proven hot path with explicit invalidation'. Should not be recalled as guidance.")

# Agent filler heuristics for volume + a near-dup pair
ag_more = [
    ("small-prs", "Heuristic: keep PRs under ~400 lines of diff; larger ones get split for reviewability.",
     ["heuristic", "pr-size", "review"]),
    ("write-runbook", "Heuristic: write the operational runbook before merging an operationally-significant feature.",
     ["heuristic", "runbook", "ops"]),
    ("measure-first", "Heuristic: measure before optimizing or caching; cache only the proven hot path with explicit invalidation.",
     ["heuristic", "performance", "cache", "measure"]),
    ("boring-tech", "Heuristic: prefer boring, well-understood technology for load-bearing systems.",
     ["heuristic", "technology", "risk"]),
]
ag_more_ids = {}
for slug, summary, tags in ag_more:
    mid = mem_id("20251115", "agent-more-" + slug)
    ag_more_ids[slug] = mid
    memory(path=f"agent/heuristics/{slug}.md", mid=mid, mtype="heuristic", scope="agent",
        summary=summary, tags=tags, created="2025-11-15T10:00:00Z", body=summary)

# Agent near-duplicate pattern pair
ag_dup_a = mem_id("20251116", "agent-dup-rollback-a")
memory(path="agent/patterns/one-step-rollback.md", mid=ag_dup_a, mtype="pattern", scope="agent",
    summary="Pattern: every risky change needs a one-step rollback (flag flip or previous-tag redeploy).",
    tags=["pattern", "rollback", "feature-flag"], created="2025-11-16T10:00:00Z",
    body="PATTERN: a risky change must have a single-action rollback — flip a flag or redeploy the last good tag. If rollback is multi-step, harden it before shipping.")
ag_dup_b = mem_id("20251117", "agent-dup-rollback-b")
memory(path="agent/patterns/instant-revert.md", mid=ag_dup_b, mtype="pattern", scope="agent",
    summary="Pattern: ship only changes you can instantly revert in one action — a flag toggle or a redeploy of the prior release.",
    tags=["pattern", "rollback", "revert"], confidence=0.85, created="2025-11-17T10:00:00Z",
    body="PATTERN: don't ship what you can't instantly revert. One action — flag off, or redeploy prior tag. (Near-duplicate of the one-step-rollback pattern; recall should collapse these.)")

# =============================================================================
# ADDITIONAL HARD STRUCTURES (chains 5 & 6, extra near-dup, volume fillers)
# =============================================================================

# --- Supersession chain #5 (me): user's job title changed (senior -> staff -> tech lead).
# Three-link chain; only the head (tech-lead) should recall for "what is Dana's role".
role_v1 = mem_id("20240601", "me-role-v1")
role_v2 = mem_id("20250115", "me-role-v2")
role_v3 = mem_id("20250901", "me-role-v3")
memory(path="me/relationship/facts/title-2024.md", mid=role_v1, mtype="person", scope="user",
    summary="User's title is Senior Backend Engineer (as of mid-2024).",
    status="superseded", superseded_by=[role_v2], trust="untrusted",
    tags=["identity", "role", "title", "superseded"],
    entities=[ent("ent_dana", "Dana Okafor", ["Dana"])],
    author=author_user(), created="2024-06-01T09:00:00Z", updated="2025-01-15T09:00:00Z",
    body="(Superseded) Dana's title was Senior Backend Engineer in mid-2024. Promoted since — do not recall as current.")
memory(path="me/relationship/facts/title-early-2025.md", mid=role_v2, mtype="person", scope="user",
    summary="User's title is Staff Backend Engineer (as of early 2025).",
    status="superseded", superseded_by=[role_v3], supersedes=[role_v1], trust="untrusted",
    tags=["identity", "role", "title", "superseded"],
    entities=[ent("ent_dana", "Dana Okafor", ["Dana"])],
    author=author_user(), created="2025-01-15T09:00:00Z", updated="2025-09-01T09:00:00Z",
    body="(Superseded) Dana became Staff Backend Engineer in early 2025. Later took tech-lead responsibilities — do not recall as the current/complete role.")
memory(path="me/relationship/facts/title.md", mid=role_v3, mtype="person", scope="user",
    summary="User is Staff Backend Engineer AND tech lead of the Atlas billing platform (current, as of late 2025).",
    supersedes=[role_v2], confidence=0.95,
    tags=["identity", "role", "title", "tech-lead", "atlas"],
    entities=[ent("ent_dana", "Dana Okafor", ["Dana"])],
    author=author_user(), created="2025-09-01T09:00:00Z",
    body="Dana Okafor is Staff Backend Engineer and tech lead of the Atlas billing platform — current role as of late 2025. Recall THIS for 'what is Dana's current role/title'. (Consistent with me/identity/role.md.)")

# --- Supersession chain #6 (orbit): rate-limit policy revised twice.
rl_v1 = mem_id("20251122", "orbit-rl-v1")
rl_v2 = mem_id("20251205", "orbit-rl-v2")
memory(path="projects/orbit/decisions/2025-11-22-rate-limit-fixed.md", mid=rl_v1, mtype="decision", scope="project", project="orbit",
    summary="Initial rate-limit policy: a flat 100 req/min per IP on the auth endpoints.",
    status="superseded", superseded_by=[rl_v2],
    tags=["decision", "rate-limit", "auth", "superseded"],
    created="2025-11-22T10:00:00Z", updated="2025-12-05T10:00:00Z",
    body="DECISION (superseded): flat 100 req/min per IP on auth endpoints. Superseded — NAT'd corporate clients all share an IP and got throttled. Do not follow.")
memory(path="projects/orbit/decisions/2025-12-05-rate-limit-tiered.md", mid=rl_v2, mtype="decision", scope="project", project="orbit",
    summary="Current rate-limit policy: per-account token-bucket on auth endpoints (not per-IP), with a separate stricter per-IP failed-login limit.",
    supersedes=[rl_v1], confidence=0.92,
    tags=["decision", "rate-limit", "auth", "token-bucket"],
    created="2025-12-05T10:00:00Z",
    body="DECISION (current): rate-limit auth endpoints per-account via a token-bucket (handles shared-NAT corporate clients), PLUS a strict per-IP limit on FAILED logins only (credential-stuffing defense). Recall THIS for Orbit rate-limiting.")

# --- Extra near-duplicate pair #4 (atlas idempotency restated as a how-to note vs invariant).
atlas_dup_idem = mem_id("20251204", "atlas-dup-idem")
memory(path="projects/atlas/playbooks/idempotency-keys.md", mid=atlas_dup_idem, mtype="playbook", scope="project", project="atlas",
    summary="How-to: pass an Idempotency-Key on every money-moving request; the gateway dedupes on it for 24 hours so retries never double-charge.",
    tags=["playbook", "idempotency", "payments", "retry"], confidence=0.85,
    created="2025-12-04T10:00:00Z",
    body="HOW-TO: every money-moving request carries an Idempotency-Key header; the gateway caches the result keyed on it for 24h, so a retried request returns the original result instead of charging again. (Restates the idempotency invariant as an operational note — near-duplicate of invariants-idempotency.md.)")

# --- Volume fillers: me episodic + project episodic + a few more agent notes.
me_episodic = [
    ("2025-12-18", "Worked through the Atlas ledger backfill double-count in staging with Lena; agreed on the entry_id dedupe.",
     ["episodic", "atlas", "migration"]),
    ("2026-01-21", "Quill CI finally green after the test-isolation fix; celebrated, then wrote the postmortem.",
     ["episodic", "quill", "ci"]),
]
for date, summary, tags in me_episodic:
    mid = mem_id(date.replace("-", ""), "me-epi-" + date)
    memory(path=f"me/episodic/{date}.md", mid=mid, mtype="episode", scope="user",
        summary=summary, tags=tags, confidence=0.7, author=author_user(),
        created=f"{date}T18:00:00Z", body=summary)

# A couple more agent heuristics for breadth, living in the spec-aligned heuristics/ dir.
ag_extra = [
    ("when-to-dual-write",
     "Heuristic: dual-write migrations are worth their complexity once a table is large enough that lock time exceeds an acceptable maintenance window.",
     ["heuristic", "migration", "dual-write"]),
    ("flaky-debt",
     "Heuristic: a flaky test left unfixed is interest-bearing debt — the suite's signal value decays the longer flakes are tolerated.",
     ["heuristic", "ci", "flaky", "testing"]),
]
for slug, summary, tags in ag_extra:
    mid = mem_id("20260205", "agent-extra-" + slug)
    memory(path=f"agent/heuristics/{slug}.md", mid=mid, mtype="heuristic", scope="agent",
        summary=summary, tags=tags, confidence=0.8, created="2026-02-05T10:00:00Z", body=summary)

if __name__ == "__main__":
    written = 0
    for relpath, fm, body in CORPUS:
        full = os.path.join(MEM_ROOT, relpath)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w") as fh:
            fh.write(render(fm, body))
        written += 1
    print(f"wrote {written} memory files")
    # dump id registry for queries authoring
    import json
    registry = {relpath: fm["id"] for relpath, fm, _ in CORPUS}
    with open(os.path.join(HERE, "_id_registry.json"), "w") as fh:
        json.dump(registry, fh, indent=2)
    print(f"id registry -> _id_registry.json ({len(registry)} ids)")
