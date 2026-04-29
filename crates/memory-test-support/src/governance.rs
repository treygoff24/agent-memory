//! Shared Stream C governance matrix fixtures.

/// Author path exercised by governance matrix tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GovernanceActor {
    /// Human-authored memory with explicit turn context.
    User,
    /// Primary agent write with a resolvable local source.
    GroundedAgent,
    /// Primary agent write without acceptable source grounding.
    UngroundedAgent,
    /// Spawned subagent write with a session-spawn source reference.
    Subagent,
}

/// Governance scope selected for a candidate write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GovernanceScope {
    /// Human memory policy.
    Me,
    /// Project memory policy.
    Project,
    /// Agent memory policy.
    Agent,
    /// Dreaming/synthesis policy.
    Dreaming,
}

/// Existing-memory relationship exercised by contradiction/tombstone checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GovernanceRelation {
    /// Fresh claim with no conflict.
    Fresh,
    /// Exact duplicate of an existing active memory.
    Duplicate,
    /// Similar claim that refines an existing active memory.
    Refinement,
    /// Similar claim that contradicts an existing active memory.
    Contradiction,
    /// Claim matching a tombstone rule.
    TombstoneHit,
}

/// Table fixture for actor/source grounding behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActorFixture {
    /// Stable case name.
    pub name: &'static str,
    /// Actor/source path.
    pub actor: GovernanceActor,
    /// Candidate scope.
    pub scope: GovernanceScope,
    /// Candidate claim body.
    pub claim: &'static str,
}

/// Table fixture for scope policy behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScopePolicyFixture {
    /// Stable case name.
    pub name: &'static str,
    /// Candidate scope.
    pub scope: GovernanceScope,
    /// Confidence supplied by the candidate.
    pub confidence: f32,
    /// Expected policy marker.
    pub policy_applied: &'static str,
}

/// Table fixture for relation behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RelationFixture {
    /// Stable case name.
    pub name: &'static str,
    /// Existing-memory relationship.
    pub relation: GovernanceRelation,
    /// Candidate claim body.
    pub claim: &'static str,
    /// Entity identifier used for canonical duplicate/tombstone matching.
    pub entity: &'static str,
}

/// Spawn id treated as registered by deterministic tests.
pub const SPAWNED_SUBAGENT_ID: &str = "spawned-subagent-1";
/// Claim that matches the Stream C tombstone fixture.
pub const TOMBSTONED_CLAIM: &str = "Claim: Keep The Red Door";
/// Entity that matches the Stream C tombstone fixture.
pub const TOMBSTONED_ENTITY: &str = "Home";

/// Actor/source fixtures required by Task 11.
pub const ACTOR_FIXTURES: &[ActorFixture] = &[
    ActorFixture {
        name: "user_write",
        actor: GovernanceActor::User,
        scope: GovernanceScope::Me,
        claim: "User-authored governance memories carry explicit context.",
    },
    ActorFixture {
        name: "grounded_agent_write",
        actor: GovernanceActor::GroundedAgent,
        scope: GovernanceScope::Agent,
        claim: "Grounded agent memories cite a local source file.",
    },
    ActorFixture {
        name: "ungrounded_agent_write",
        actor: GovernanceActor::UngroundedAgent,
        scope: GovernanceScope::Agent,
        claim: "Ungrounded agent memories fail closed.",
    },
    ActorFixture {
        name: "subagent_write",
        actor: GovernanceActor::Subagent,
        scope: GovernanceScope::Agent,
        claim: "Subagent memories are visible for parent review.",
    },
];

/// Scope policy fixtures required by Task 11.
pub const SCOPE_POLICY_FIXTURES: &[ScopePolicyFixture] = &[
    ScopePolicyFixture {
        name: "me_policy",
        scope: GovernanceScope::Me,
        confidence: 0.96,
        policy_applied: "me-strict@v1",
    },
    ScopePolicyFixture {
        name: "project_policy",
        scope: GovernanceScope::Project,
        confidence: 0.96,
        policy_applied: "project-standard@v2",
    },
    ScopePolicyFixture {
        name: "agent_policy",
        scope: GovernanceScope::Agent,
        confidence: 0.96,
        policy_applied: "agent-strict@v3",
    },
    ScopePolicyFixture {
        name: "dreaming_policy",
        scope: GovernanceScope::Dreaming,
        confidence: 0.96,
        policy_applied: "dreaming-strict@v1",
    },
];

/// Duplicate/refinement/contradiction/tombstone fixtures required by Task 11.
pub const RELATION_FIXTURES: &[RelationFixture] = &[
    RelationFixture {
        name: "fresh",
        relation: GovernanceRelation::Fresh,
        claim: "Fresh governance claims can be promoted when policy allows.",
        entity: "project:agent-memory",
    },
    RelationFixture {
        name: "duplicate",
        relation: GovernanceRelation::Duplicate,
        claim: "Duplicate governance claims return the existing memory id.",
        entity: "project:agent-memory",
    },
    RelationFixture {
        name: "refinement",
        relation: GovernanceRelation::Refinement,
        claim: "Governance claims may add narrower evidence to an active memory.",
        entity: "project:agent-memory",
    },
    RelationFixture {
        name: "contradiction",
        relation: GovernanceRelation::Contradiction,
        claim: "Governance claims may replace stale active memory facts.",
        entity: "project:agent-memory",
    },
    RelationFixture {
        name: "tombstone_hit",
        relation: GovernanceRelation::TombstoneHit,
        claim: TOMBSTONED_CLAIM,
        entity: TOMBSTONED_ENTITY,
    },
];
