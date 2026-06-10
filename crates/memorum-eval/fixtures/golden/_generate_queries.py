#!/usr/bin/env python3
"""Author queries.yaml for the golden corpus (Task 4.1).

Cases are written by *path-key* (human-readable) and resolved to real MemoryIds
via _id_registry.json, so the labels are correct-by-construction and survive any
re-mint of the corpus. Re-run after _generate.py.

Labeling rubric (see README.md for the full version):
  essential       — a correct answer to this query is incomplete without this memory.
  useful          — relevant supporting context; helps but isn't load-bearing.
  irrelevant_traps— LOOKS relevant (lexical/entity overlap) but MUST NOT surface:
                    superseded tails, wrong-project collisions, tombstoned, or
                    stale-competing facts. A ranker that surfaces a trap is wrong.

Disjointness invariant: per case, essential / useful / irrelevant_traps are
pairwise disjoint. The lint test enforces this.
"""
import json, os

HERE = os.path.dirname(os.path.abspath(__file__))
REG = json.load(open(os.path.join(HERE, "_id_registry.json")))

def i(key):
    return REG[key]

def ids(*keys):
    return [i(k) for k in keys]

# Each case: (id, query, namespace_scope, essential[], useful[], traps[])
CASES = []
def case(cid, query, scope, essential=None, useful=None, traps=None):
    CASES.append({
        "id": cid,
        "query": query,
        "namespace_scope": scope,
        "graded": {
            "essential": [i(k) for k in (essential or [])],
            "useful": [i(k) for k in (useful or [])],
            "irrelevant_traps": [i(k) for k in (traps or [])],
        },
    })

# --- EXACT-IDENTIFIER RECALL (query is a literal MemoryId) -------------------
case("q01-exact-id-ledger-head",
     i("projects/atlas/decisions/2025-12-15-ledger-migration-plan-v3.md"),
     ["project"],
     essential=["projects/atlas/decisions/2025-12-15-ledger-migration-plan-v3.md"])
case("q02-exact-id-jwt-decision",
     i("projects/orbit/decisions/2025-11-20-stateless-jwt.md"),
     ["project"],
     essential=["projects/orbit/decisions/2025-11-20-stateless-jwt.md"])
case("q03-exact-id-money-invariant",
     i("projects/atlas/invariants.md"),
     ["project"],
     essential=["projects/atlas/invariants.md"])

# --- ENTITY QUERIES (incl. cross-project collisions) -------------------------
case("q04-atlas-gateway",
     "What is the gateway in the Atlas billing project and how is it rate-limited?",
     ["project:atlas"],
     essential=["projects/atlas/entities/gateway.md"],
     useful=["projects/atlas/decisions/2025-12-10-processor-retry.md"],
     traps=["projects/orbit/entities/gateway.md"])
case("q05-orbit-gateway",
     "Describe the Orbit identity service gateway and what it validates.",
     ["project:orbit"],
     essential=["projects/orbit/entities/gateway.md"],
     useful=["projects/orbit/decisions/2025-11-20-stateless-jwt.md"],
     traps=["projects/atlas/entities/gateway.md"])
case("q06-quill-pipeline",
     "What is the Pipeline in the Quill docs project?",
     ["project:quill"],
     essential=["projects/quill/entities/pipeline.md"],
     traps=["projects/atlas/entities/pipeline.md"])
case("q07-atlas-pipeline",
     "What does the Atlas billing pipeline do?",
     ["project:atlas"],
     essential=["projects/atlas/entities/pipeline.md"],
     traps=["projects/quill/entities/pipeline.md"])
case("q08-who-is-dana-okafor",
     "Who is Dana Okafor and what is her current role?",
     ["me"],
     essential=["me/relationship/facts/title.md", "me/identity/role.md"],
     traps=["me/relationship/facts/dana-wu.md",
            "me/relationship/facts/title-2024.md",
            "me/relationship/facts/title-early-2025.md"])
case("q09-who-is-dana-wu",
     "Who is Dana Wu on the Quill team?",
     ["me"],
     essential=["me/relationship/facts/dana-wu.md"],
     traps=["me/relationship/facts/title.md", "me/identity/role.md"])
case("q10-who-owns-payments-observability",
     "Who is the escalation contact for Atlas payment incidents and owns observability?",
     ["me"],
     essential=["me/relationship/facts/priya.md"],
     useful=["me/knowledge/oncall-rotation.md"])
case("q11-security-reviewer-auth",
     "Who must review changes to Orbit token issuance and signing keys?",
     ["me"],
     essential=["me/relationship/facts/marco.md"],
     useful=["projects/orbit/invariants.md"])

# --- TOPICAL QUERIES ---------------------------------------------------------
case("q12-ledger-migration-approach",
     "How did we migrate the Atlas ledger table, and how do we roll it back?",
     ["project:atlas"],
     essential=["projects/atlas/decisions/2025-12-15-ledger-migration-plan-v3.md"],
     useful=["projects/atlas/regressions/backfill-double-count.md"],
     traps=["projects/atlas/decisions/2025-12-01-ledger-migration-plan.md",
            "projects/atlas/decisions/2025-12-08-ledger-migration-plan-v2.md"])
case("q13-auth-model",
     "What is the current authentication model for Orbit?",
     ["project:orbit"],
     essential=["projects/orbit/decisions/2025-11-20-stateless-jwt.md"],
     useful=["projects/orbit/playbooks/key-rotation.md", "projects/orbit/invariants.md"],
     traps=["projects/orbit/decisions/2025-11-10-session-cookies.md"])
case("q14-flaky-ci-root-cause",
     "What was the root cause of the flaky CI in Quill and how was it fixed?",
     ["project:quill"],
     essential=["projects/quill/decisions/2026-01-20-flaky-root-cause.md"],
     useful=["projects/quill/invariants.md", "agent/postmortems/ci-flake-erosion.md"],
     traps=["projects/quill/decisions/2026-01-02-flaky-retry.md"])
case("q15-how-to-rollback-payments",
     "How do we roll back a bad payments deploy?",
     ["project:atlas"],
     essential=["projects/atlas/playbooks/payment-rollback.md"],
     useful=["projects/atlas/playbooks/oncall-triage.md"])
case("q16-rotate-signing-key",
     "How do we rotate the JWT signing key without rejecting valid tokens?",
     ["project:orbit"],
     essential=["projects/orbit/playbooks/key-rotation.md"],
     useful=["projects/orbit/invariants.md"])
case("q17-money-representation",
     "How are monetary amounts represented in Atlas?",
     ["project:atlas", "agent"],
     essential=["projects/atlas/invariants.md"],
     useful=["agent/anti-patterns/float-for-money.md"])
case("q18-idempotency-payments",
     "How does Atlas prevent double-charging on retried payment requests?",
     ["project:atlas"],
     essential=["projects/atlas/invariants-idempotency.md"],
     useful=["projects/atlas/playbooks/idempotency-keys.md",
             "agent/patterns/idempotent-writes.md"])
case("q19-clock-skew-auth-failures",
     "We're seeing random intermittent auth failures in Orbit — what's the likely cause?",
     ["project:orbit"],
     essential=["projects/orbit/regressions/clock-skew-rejection.md"],
     useful=["projects/orbit/heuristics-noop.md"])
case("q20-debug-broken-preview",
     "A Quill PR preview deploy is broken — where do I start?",
     ["project:quill"],
     essential=["projects/quill/playbooks/preview-deploy.md"],
     useful=["projects/quill/invariants.md"])
case("q21-user-language-preference",
     "What language does the user prefer for new backend services?",
     ["me"],
     essential=["me/relationship/preferences/language.md"],
     traps=["me/relationship/preferences/language-2025.md"])
case("q22-user-editor-preference",
     "What are the user's editor and formatting preferences?",
     ["me"],
     essential=["me/relationship/preferences/editor.md"])
case("q23-user-comm-style",
     "How does the user like updates communicated?",
     ["me"],
     essential=["me/relationship/preferences/communication.md"])

# --- SUPERSESSION-HEAD SELECTION (head essential, tails are traps) -----------
case("q24-ledger-current-plan",
     "What is the ledger migration plan we actually executed (not the abandoned ones)?",
     ["project:atlas"],
     essential=["projects/atlas/decisions/2025-12-15-ledger-migration-plan-v3.md"],
     traps=["projects/atlas/decisions/2025-12-01-ledger-migration-plan.md",
            "projects/atlas/decisions/2025-12-08-ledger-migration-plan-v2.md"])
case("q25-orbit-rate-limit-current",
     "What is Orbit's current rate-limiting policy for auth endpoints?",
     ["project:orbit"],
     essential=["projects/orbit/decisions/2025-12-05-rate-limit-tiered.md"],
     traps=["projects/orbit/decisions/2025-11-22-rate-limit-fixed.md"])
case("q26-current-migration-heuristic",
     "What's our default approach for migrating a large database table?",
     ["agent"],
     essential=["agent/heuristics/migration-window.md"],
     traps=["agent/heuristics/migration-window-v1.md"])
case("q27-dana-current-title",
     "What is Dana Okafor's current job title?",
     ["me"],
     essential=["me/relationship/facts/title.md"],
     traps=["me/relationship/facts/title-2024.md",
            "me/relationship/facts/title-early-2025.md"])
case("q28-orbit-auth-history-context",
     "Give the history of how Orbit's session handling evolved.",
     ["project:orbit"],
     essential=["projects/orbit/decisions/2025-11-20-stateless-jwt.md"],
     useful=["projects/orbit/decisions/2025-11-10-session-cookies.md",
             "agent/postmortems/auth-rolling-deploy.md"])

# --- CROSS-PROJECT ISOLATION (scope must keep other projects out) ------------
case("q29-gateway-atlas-only",
     "Within Atlas only: what is the gateway and its retry policy?",
     ["project:atlas"],
     essential=["projects/atlas/entities/gateway.md"],
     useful=["projects/atlas/decisions/2025-12-10-processor-retry.md",
             "projects/atlas/decisions/2025-12-11-gateway-retry-policy.md"],
     traps=["projects/orbit/entities/gateway.md"])
case("q30-pipeline-quill-only",
     "Scoped to Quill: explain the Pipeline and how previews deploy.",
     ["project:quill"],
     essential=["projects/quill/entities/pipeline.md"],
     useful=["projects/quill/playbooks/preview-deploy.md"],
     traps=["projects/atlas/entities/pipeline.md"])
case("q31-invariants-orbit-only",
     "What are the security invariants for the Orbit identity service?",
     ["project:orbit"],
     essential=["projects/orbit/invariants.md"],
     traps=["projects/atlas/invariants.md", "projects/atlas/invariants-idempotency.md"])

# --- AGENT-SCOPE PATTERN/POSTMORTEM RECALL -----------------------------------
case("q32-anti-pattern-flaky-retry",
     "Is auto-retrying flaky tests a good idea?",
     ["agent"],
     essential=["agent/anti-patterns/retry-to-hide-flakes.md"],
     useful=["agent/postmortems/ci-flake-erosion.md", "agent/heuristics/flaky-debt.md"])
case("q33-pattern-need-repro",
     "Should I patch a reported bug before I can reproduce it?",
     ["agent"],
     essential=["agent/patterns/reproduce-before-fix.md"])
case("q34-rollback-pattern",
     "What's the rule about being able to revert a risky change?",
     ["agent"],
     essential=["agent/patterns/one-step-rollback.md"],
     useful=["agent/patterns/instant-revert.md", "agent/patterns/flag-gated-rollout.md"])
case("q35-postmortem-auth-deploy",
     "Has a rolling deploy ever broken authentication for us?",
     ["agent", "project:orbit"],
     essential=["agent/postmortems/auth-rolling-deploy.md"],
     useful=["agent/anti-patterns/sticky-sessions.md",
             "projects/orbit/decisions/2025-11-20-stateless-jwt.md"])
case("q36-sticky-sessions-warning",
     "Are sticky sessions safe to rely on for correctness?",
     ["agent"],
     essential=["agent/anti-patterns/sticky-sessions.md"],
     useful=["agent/postmortems/auth-rolling-deploy.md"])
case("q37-pr-size-guidance",
     "How big should a pull request be?",
     ["agent", "me"],
     essential=["agent/heuristics/small-prs.md"],
     useful=["me/identity/principles.md", "agent/anti-patterns/big-pr.md"])
case("q38-caching-guidance",
     "What's our current guidance on caching to improve latency?",
     ["agent"],
     essential=["agent/heuristics/measure-first.md"],
     traps=["agent/heuristics/cache-everything.md"])
case("q39-incident-comms",
     "How should I run communications during an active incident?",
     ["agent"],
     essential=["agent/playbooks/incident-comms.md"],
     useful=["me/knowledge/doc-habit.md"])
case("q40-hermetic-build-flaky",
     "How should I approach a build before chasing flaky tests?",
     ["agent"],
     essential=["agent/patterns/hermetic-builds.md"],
     useful=["projects/quill/invariants.md"])

# --- NEAR-DUPLICATE COLLAPSE (both relevant; either acceptable, prefer one) --
case("q41-standup-time",
     "When does the user want the daily standup, and how long?",
     ["me"],
     essential=["me/relationship/preferences/standup-time.md"],
     useful=["me/relationship/preferences/morning-sync.md"])
case("q42-gateway-retry-policy-dup",
     "What is the processor retry/backoff policy in Atlas?",
     ["project:atlas"],
     essential=["projects/atlas/decisions/2025-12-10-processor-retry.md"],
     useful=["projects/atlas/decisions/2025-12-11-gateway-retry-policy.md"])

# --- CORRECTIONS & TOMBSTONE TRAPS -------------------------------------------
case("q43-user-timezone",
     "What timezone is the user in?",
     ["me"],
     essential=["me/relationship/corrections/timezone.md"],
     useful=["agent/regressions/timezone-assumption.md"])
case("q44-company-name",
     "What is the exact legal name of the company?",
     ["me"],
     essential=["me/relationship/corrections/company-name.md"])
case("q45-user-current-machine",
     "What is the user's current primary work machine?",
     ["me"],
     essential=[],
     useful=[],
     traps=["me/relationship/facts/old-laptop.md"])
case("q46-third-payment-processor",
     "Which third payment processor did Atlas adopt?",
     ["project:atlas"],
     essential=[],
     useful=[],
     traps=["projects/atlas/decisions/2025-12-05-vendor-pick.md"])

# --- ABSTENTION CASES (zero relevant memories: empty essential + useful) -----
case("q47-abstain-kubernetes",
     "What is our Kubernetes cluster autoscaling configuration?",
     ["me", "project:atlas", "project:orbit", "project:quill", "agent"],
     essential=[], useful=[])
case("q48-abstain-marketing",
     "What did the marketing team decide about the Q3 ad campaign?",
     ["me", "project:atlas", "project:orbit", "project:quill", "agent"],
     essential=[], useful=[])
case("q49-abstain-mobile-app",
     "How is the iOS mobile app's offline sync implemented?",
     ["project:atlas", "project:orbit", "project:quill"],
     essential=[], useful=[])
case("q50-abstain-out-of-scope-secret",
     "What is the user's home address and personal phone number?",
     ["me"],
     essential=[], useful=[])

# --- KEYWORD / ENTITY SEARCH-SEAM PROBES ------------------------------------
case("q51-keyword-atlas-gateway",
     "payment gateway Stripe Adyen",
     ["project:atlas"],
     essential=["projects/atlas/entities/gateway.md"],
     useful=["projects/atlas/decisions/2025-12-10-processor-retry.md",
             "projects/atlas/decisions/2025-12-11-gateway-retry-policy.md"],
     traps=["projects/orbit/entities/gateway.md"])
case("q52-keyword-orbit-jwt",
     "stateless JWT KMS",
     ["project:orbit"],
     essential=["projects/orbit/decisions/2025-11-20-stateless-jwt.md"],
     useful=["projects/orbit/playbooks/key-rotation.md",
             "projects/orbit/invariants.md"],
     traps=["projects/orbit/decisions/2025-11-10-session-cookies.md"])
case("q53-keyword-atlas-pipeline",
     "billing aggregation pipeline",
     ["project:atlas"],
     essential=["projects/atlas/entities/pipeline.md"],
     traps=["projects/quill/entities/pipeline.md"])
case("q54-keyword-quill-preview",
     "GitHub Actions preview",
     ["project:quill"],
     essential=["projects/quill/entities/pipeline.md"],
     useful=["projects/quill/playbooks/preview-deploy.md"])
case("q55-keyword-priya-observability",
     "Priya payments observability",
     ["me"],
     essential=["me/relationship/facts/priya.md"],
     useful=["me/knowledge/oncall-rotation.md"])
case("q56-keyword-idempotency",
     "client supplied idempotency key dedupe",
     ["agent", "project:atlas"],
     essential=["agent/patterns/idempotent-writes.md"],
     useful=["projects/atlas/invariants-idempotency.md",
             "projects/atlas/playbooks/idempotency-keys.md"])


HEADER = """\
# Golden recall corpus — labeled query cases (Task 4.1).
#
# Schema (per case):
#   id               unique case id (kebab, qNN-...)
#   query            search text OR session-context description the recall path receives
#   namespace_scope  list of in-scope namespaces; "project:<alias>" narrows to one project.
#                    The quality runner maps aliases to canonical_namespace_id at load time.
#   graded:
#     essential          memory ids an answer is incomplete without (recall must surface)
#     useful             relevant supporting context (helps; not load-bearing)
#     irrelevant_traps   ids that LOOK relevant but MUST NOT surface — superseded tails,
#                        wrong-project entity collisions, tombstoned, stale-competing facts.
#
# Invariant (enforced by tests/golden_fixtures_lint.rs):
#   essential / useful / irrelevant_traps are pairwise disjoint within a case,
#   and every referenced id exists in memories/.
#
# Abstention cases (qNN-abstain-*) have empty essential AND useful: the correct
# behavior is to surface nothing (or only ignore traps). They measure precision /
# false-positive resistance, the counterpart to recall.
#
# GENERATED by _generate_queries.py from _id_registry.json — edit cases there, not here.
"""

def emit_list(ids_list, indent):
    if not ids_list:
        return " []"
    out = "\n"
    for x in ids_list:
        out += " " * indent + "- " + x + "\n"
    return out.rstrip("\n")

def render():
    lines = [HEADER, "cases:"]
    for c in CASES:
        lines.append(f"  - id: {c['id']}")
        # quote query (always; contains punctuation)
        q = c["query"].replace("\\", "\\\\").replace('"', '\\"')
        lines.append(f'    query: "{q}"')
        scope = ", ".join(c["namespace_scope"])
        lines.append(f"    namespace_scope: [{scope}]")
        lines.append("    graded:")
        for grade in ("essential", "useful", "irrelevant_traps"):
            vals = c["graded"][grade]
            if not vals:
                lines.append(f"      {grade}: []")
            else:
                lines.append(f"      {grade}:")
                for v in vals:
                    lines.append(f"        - {v}")
    return "\n".join(lines) + "\n"

if __name__ == "__main__":
    # disjointness self-check before writing
    for c in CASES:
        e = set(c["graded"]["essential"]); u = set(c["graded"]["useful"]); t = set(c["graded"]["irrelevant_traps"])
        assert e.isdisjoint(u), f"{c['id']}: essential/useful overlap"
        assert e.isdisjoint(t), f"{c['id']}: essential/trap overlap"
        assert u.isdisjoint(t), f"{c['id']}: useful/trap overlap"
    with open(os.path.join(HERE, "queries.yaml"), "w") as fh:
        fh.write(render())
    print(f"wrote queries.yaml with {len(CASES)} cases")
