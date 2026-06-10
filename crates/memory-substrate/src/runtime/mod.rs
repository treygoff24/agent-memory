//! Runtime support: reconciliation and the post-commit repair cascade.
//!
//! `runtime::blocking` has been deleted per Q12. Callers that wrap blocking
//! work should use `tokio::task::spawn_blocking` directly (Decision A / Q2).

pub mod reconcile;
pub mod repair_cascade;
