//! Governance write / supersede / forget pipeline, split into cohesive submodules.
//!
//! - [`pipeline`] — the serialized write/supersede/forget handlers, the write
//!   executor, the privacy-mediated write primitive, the supersede claim-lock
//!   machinery, and the request DTOs.
//! - [`policy`] — policy-set and tombstone-index loading, the bounded
//!   active-memory fan-out, and the governance-engine adapters.
//! - [`meta`] — the `GovernanceMeta` deserialization model and the parsed
//!   `GovernanceWriteInput` plus its `Memory`-building logic.
//! - [`privacy`] — classification glue between the parsed input and the
//!   deterministic privacy classifier.
//!
//! The re-exports below keep the crate-facing surface (`governance::*`)
//! unchanged from the pre-split single-file module so external call sites in
//! `handlers::mod`, `handlers::memory_ops`, and `handlers::review` stay the same.

pub(crate) mod meta;
mod pipeline;
mod policy;
pub(super) mod privacy;

pub(crate) use meta::GovernanceMeta;
pub(crate) use pipeline::{
    governance_forget_response, governance_supersede_response, governance_write_response, write_privacy_memory,
    GovernanceSupersedeRequest, GovernanceWriteRequest,
};
pub(crate) use policy::load_policy_set;
pub(crate) use privacy::classify_privacy;
