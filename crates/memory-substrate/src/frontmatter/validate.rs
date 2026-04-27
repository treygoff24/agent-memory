//! Frontmatter validation.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::error::ValidationError;
use crate::frontmatter::schema::SUPPORTED_SCHEMA_VERSION;
use crate::model::{AuthorKind, Frontmatter, MemoryStatus, MemoryType, Scope, Sensitivity, TrustLevel};

#[allow(clippy::expect_used)]
static MEMORY_ID_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^mem_\d{8}_[0-9a-f]{16}_\d{6}$").expect("valid regex") // expect-justified: static regex is tested at startup
});
#[allow(clippy::expect_used)]
static SLUG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[a-z0-9][a-z0-9._-]{0,62}$").expect("valid regex") // expect-justified: static regex is tested at startup
});

/// Validate a memory id.
pub fn validate_memory_id(id: &str) -> Result<(), ValidationError> {
    if MEMORY_ID_RE.is_match(id) {
        Ok(())
    } else {
        Err(ValidationError::InvalidMemoryId(id.to_string()))
    }
}

/// Validate typed frontmatter.
pub fn validate_frontmatter(frontmatter: &Frontmatter) -> Result<(), ValidationError> {
    if frontmatter.schema_version > SUPPORTED_SCHEMA_VERSION {
        return Err(ValidationError::UnsupportedSchemaVersion {
            found: frontmatter.schema_version,
            supported: SUPPORTED_SCHEMA_VERSION,
        });
    }
    validate_memory_id(frontmatter.id.as_str())?;
    if frontmatter.summary.is_empty() || frontmatter.summary.chars().count() > 280 {
        return Err(ValidationError::BadShape("summary".to_string()));
    }
    if !(0.0..=1.0).contains(&frontmatter.confidence) {
        return Err(ValidationError::BadShape("confidence".to_string()));
    }
    if frontmatter.updated_at < frontmatter.created_at {
        return Err(ValidationError::BadShape("updated_at".to_string()));
    }
    validate_lifecycle(frontmatter.status, frontmatter.trust_level)?;
    validate_scope_namespace(frontmatter)?;
    validate_author(frontmatter)?;
    validate_cross_fields(frontmatter)
}

fn validate_lifecycle(status: MemoryStatus, trust: TrustLevel) -> Result<(), ValidationError> {
    let allowed = match status {
        MemoryStatus::Candidate => {
            matches!(trust, TrustLevel::Candidate | TrustLevel::Untrusted | TrustLevel::Quarantined)
        }
        MemoryStatus::Active => matches!(trust, TrustLevel::Trusted | TrustLevel::Untrusted),
        MemoryStatus::Pinned => matches!(trust, TrustLevel::Pinned | TrustLevel::Trusted),
        MemoryStatus::Superseded | MemoryStatus::Archived => {
            matches!(trust, TrustLevel::Trusted | TrustLevel::Untrusted | TrustLevel::Candidate)
        }
        MemoryStatus::Tombstoned => !matches!(trust, TrustLevel::Pinned),
        MemoryStatus::Quarantined => matches!(trust, TrustLevel::Quarantined),
    };
    if allowed {
        Ok(())
    } else {
        Err(ValidationError::InvalidLifecyclePair)
    }
}

fn validate_scope_namespace(frontmatter: &Frontmatter) -> Result<(), ValidationError> {
    let requires_namespace = matches!(frontmatter.scope, Scope::Project | Scope::Org);
    if requires_namespace && (frontmatter.namespace.is_none() || frontmatter.canonical_namespace_id.is_none()) {
        return Err(ValidationError::BadShape("namespace".to_string()));
    }
    if !requires_namespace && (frontmatter.namespace.is_some() || frontmatter.canonical_namespace_id.is_some()) {
        return Err(ValidationError::BadShape("namespace".to_string()));
    }
    Ok(())
}

fn validate_author(frontmatter: &Frontmatter) -> Result<(), ValidationError> {
    let author = &frontmatter.author;
    match author.kind {
        AuthorKind::User => {
            let handle = author
                .user_handle
                .as_deref()
                .ok_or_else(|| ValidationError::BadShape("author.user_handle".to_string()))?;
            if handle.contains('@') || !SLUG_RE.is_match(handle.trim_start_matches("sha256:")) {
                return Err(ValidationError::BadShape("author.user_handle".to_string()));
            }
        }
        AuthorKind::Agent => {
            require(&author.harness, "author.harness").and_then(|_| require(&author.session_id, "author.session_id"))?
        }
        AuthorKind::Subagent => {
            require(&author.harness, "author.harness")?;
            require(&author.session_id, "author.session_id")?;
            require(&author.subagent_id, "author.subagent_id")?;
        }
        AuthorKind::Dreaming => require(&author.phase, "author.phase")?,
        AuthorKind::System => require(&author.component, "author.component")?,
    }
    Ok(())
}

fn require(value: &Option<String>, field: &str) -> Result<(), ValidationError> {
    if value.as_deref().is_some_and(|text| !text.is_empty()) {
        Ok(())
    } else {
        Err(ValidationError::BadShape(field.to_string()))
    }
}

fn validate_cross_fields(frontmatter: &Frontmatter) -> Result<(), ValidationError> {
    if frontmatter.supersedes.contains(&frontmatter.id)
        || frontmatter.superseded_by.contains(&frontmatter.id)
        || frontmatter.related.contains(&frontmatter.id)
    {
        return Err(ValidationError::BadShape("self-reference".to_string()));
    }
    if !frontmatter.superseded_by.is_empty()
        && !matches!(frontmatter.status, MemoryStatus::Superseded | MemoryStatus::Quarantined)
    {
        return Err(ValidationError::BadShape("superseded_by/status".to_string()));
    }
    if matches!(frontmatter.status, MemoryStatus::Superseded) && frontmatter.superseded_by.is_empty() {
        return Err(ValidationError::BadShape("superseded_by/status".to_string()));
    }
    if matches!(frontmatter.status, MemoryStatus::Quarantined) && frontmatter.merge_diagnostics.is_none() {
        return Err(ValidationError::BadShape("_merge_diagnostics".to_string()));
    }
    if has_intersection(&frontmatter.supersedes, &frontmatter.superseded_by) {
        return Err(ValidationError::BadShape("supersession overlap".to_string()));
    }
    if matches!(frontmatter.status, MemoryStatus::Tombstoned) && frontmatter.tombstone_events.is_empty() {
        return Err(ValidationError::BadShape("tombstone_events".to_string()));
    }
    if matches!(frontmatter.memory_type, MemoryType::Prospective) && !frontmatter.extras.contains_key("prospective") {
        return Err(ValidationError::BadShape("prospective".to_string()));
    }
    if matches!(frontmatter.sensitivity, Sensitivity::Confidential | Sensitivity::Personal)
        && (frontmatter.retrieval_policy.index_body || frontmatter.retrieval_policy.index_embeddings)
    {
        return Err(ValidationError::BadShape("retrieval_policy".to_string()));
    }
    if privacy_scan_has_private_credential(frontmatter) && !matches!(frontmatter.status, MemoryStatus::Quarantined) {
        return Err(ValidationError::BadShape("privacy_scan.private_credential".to_string()));
    }
    Ok(())
}

fn has_intersection(left: &[crate::model::MemoryId], right: &[crate::model::MemoryId]) -> bool {
    left.iter().any(|id| right.contains(id))
}

fn privacy_scan_has_private_credential(frontmatter: &Frontmatter) -> bool {
    frontmatter
        .extras
        .get("privacy_scan")
        .and_then(|scan| scan.get("labels"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|labels| {
            labels.iter().filter_map(serde_json::Value::as_str).any(|label| label == "private_credential")
        })
}
