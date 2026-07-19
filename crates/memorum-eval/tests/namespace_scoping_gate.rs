//! Namespace-scoping leak gate.
//!
//! Asserts that the scoped retrieval seams never return a memory outside the
//! query case's visible namespace set, over the multi-project golden corpus.
//! A cross-namespace leak is invisible to score-based metrics on any corpus
//! where scoped and global retrieval coincide; this gate checks membership
//! directly, so a scoping regression fails loudly with the leaked ids named.

#![cfg(feature = "quality")]

use memorum_eval::quality::run_namespace_scoping_gate;

#[test]
fn scoped_seams_never_serve_out_of_scope_memories() {
    let violations =
        memorum_eval::block_on(run_namespace_scoping_gate()).expect("scoping gate runs against the golden corpus");
    assert!(violations.is_empty(), "cross-namespace leak(s) detected:\n{}", violations.join("\n"));
}
