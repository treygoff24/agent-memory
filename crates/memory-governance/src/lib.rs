#![deny(unsafe_op_in_unsafe_fn)]
//! Deterministic governance decisions for memory promotion.

pub mod contradiction;
pub mod decision;
pub mod engine;
pub mod error;
pub mod grounding;
pub mod policy;
pub mod review;
pub mod supersession;
pub mod tombstone;

pub use contradiction::{
    CandidateMemory, ContradictionDecision, ContradictionDetector, ContradictionTiebreaker, ExistingMemorySummary,
    SimilaritySearch, TiebreakOutcome,
};
pub use decision::{GovernanceDecision, GovernanceRefusalReason, GovernanceStatus, NextAction};
pub use engine::{GovernanceEngine, GovernanceProviders, GovernanceWriteDecision, NextWriteAction};
pub use error::{GovernanceError, GovernanceResult};
pub use grounding::{
    FileSourceResolver, GroundingContext, GroundingVerifier, SessionSpawnResolver, Source, SourceKind,
    SourceRefResolver, SourceResolution,
};
pub use policy::{
    CandidateContext, ContradictionPolicy, Policy, PolicyError, PolicyPreview, PolicyResult, PolicySet, PolicySource,
    Scope, TombstoneEnforcementMode,
};
pub use review::{ReviewMemoryEnvelope, ReviewQueue, ReviewQueueItem, ReviewStatus};
pub use supersession::{
    SupersessionFrontmatterMutations, SupersessionPlan, SupersessionPlanError, SupersessionStatusTransition,
};
pub use tombstone::{
    CandidateTombstoneKey, CanonicalEntities, MemoryId, TombstoneIndex, TombstoneKind, TombstoneLoadError,
    TombstoneMatch, TombstoneRef, TombstoneRule,
};
