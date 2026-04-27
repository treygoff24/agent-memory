//! Runtime support: reconciliation and fault injection.
//!
//! `runtime::blocking` has been deleted per Q12. Callers that wrap blocking
//! work should use `tokio::task::spawn_blocking` directly (Decision A / Q2).

pub mod faults;
pub mod reconcile;
