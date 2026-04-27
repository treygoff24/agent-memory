//! Typed defaults for permissive frontmatter parsing.

use crate::model::{RetrievalPolicy, Scope, Sensitivity, Source, SourceKind, WritePolicy};

/// Default source object.
pub fn default_source() -> Source {
    Source {
        kind: SourceKind::Import,
        reference: None,
        harness: None,
        harness_version: None,
        session_id: None,
        subagent_id: None,
        device: None,
    }
}

/// Default retrieval policy generated from scope and sensitivity.
pub fn default_retrieval_policy(scope: Scope, sensitivity: Sensitivity) -> RetrievalPolicy {
    let indexable = matches!(sensitivity, Sensitivity::Public | Sensitivity::Internal);
    let max_scope = match scope {
        Scope::Subagent => Scope::Agent,
        other => other,
    };
    RetrievalPolicy {
        passive_recall: true,
        max_scope,
        mask_personal_for_synthesis: matches!(sensitivity, Sensitivity::Confidential | Sensitivity::Personal),
        index_body: indexable,
        index_embeddings: indexable,
    }
}

/// Default write policy.
pub fn default_write_policy() -> WritePolicy {
    WritePolicy { human_review_required: false, policy_applied: "default-v1".to_string(), expected_base_hash: None }
}
