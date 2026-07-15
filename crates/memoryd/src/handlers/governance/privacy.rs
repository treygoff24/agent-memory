//! Privacy-classification glue between the parsed governance write input and the
//! deterministic privacy classifier.
//!
//! `classify_input_privacy` projects the scannable text out of a
//! `GovernanceWriteInput`; `classify_privacy` is the shared classifier entry
//! point consumed by `memory_ops` and `review` as well; `attach_privacy_scan`
//! records the scan result onto a `Memory` before it is persisted.

use memory_privacy::{DeterministicPrivacyClassifier, PrivacyClassifier, PrivacyDecision, PrivacyNamespace};
use memory_substrate::{ClassificationOutcome, Memory, Scope};

use super::meta::GovernanceWriteInput;
use crate::handlers::HandlerError;
use memory_privacy::CallerSensitivity;

pub(super) fn classify_input_privacy(input: &GovernanceWriteInput) -> Result<PrivacyDecision, HandlerError> {
    classify_privacy(&input.privacy_scan_text(), input.privacy_namespace(), input.caller_sensitivity())
}

pub(crate) fn classify_privacy(
    text: &str,
    namespace: PrivacyNamespace,
    caller: Option<CallerSensitivity>,
) -> Result<PrivacyDecision, HandlerError> {
    DeterministicPrivacyClassifier::new().classify(text, namespace, caller).map_err(HandlerError::privacy)
}

pub(super) fn attach_privacy_scan(memory: &mut Memory, privacy: &PrivacyDecision) {
    memory.frontmatter.extras.insert(
        "privacy_scan".to_string(),
        serde_json::to_value(&privacy.scan).expect("privacy scan always serializes"),
    );
}

/// Reclassify the complete canonical payload before a plaintext lifecycle rewrite.
pub(crate) fn classify_plaintext_memory(memory: &Memory) -> Result<ClassificationOutcome, HandlerError> {
    let decision = classify_plaintext_memory_decision(memory, true)?;
    if decision.storage_action.refuses_storage() {
        return Err(HandlerError::invalid_request("privacy refused secret before disk effects"));
    }
    if decision.storage_action.requires_encryption() {
        return Err(HandlerError::invalid_request(
            "plaintext lifecycle rewrite requires encryption after combined-payload classification",
        ));
    }
    Ok(decision.tier.classification())
}

/// Classify stored plaintext metadata plus the memory body when it is available.
/// Scan set matches the pre-B3 lifecycle baseline (summary, body, abstraction,
/// cues — no tags); only the metadata-amend classifier below scans tags.
pub(crate) fn classify_plaintext_memory_decision(
    memory: &Memory,
    body_available: bool,
) -> Result<PrivacyDecision, HandlerError> {
    let text = std::iter::once(memory.frontmatter.summary.as_str())
        .chain(body_available.then_some(memory.body.as_str()))
        .chain(memory.frontmatter.abstraction.as_deref())
        .chain(memory.frontmatter.cues.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    classify_memory_text(memory, text)
}

/// Classify the complete mutable payload before a metadata amendment.
pub(crate) fn classify_metadata_amendment_decision(
    memory: &Memory,
    body_available: bool,
) -> Result<PrivacyDecision, HandlerError> {
    let text = std::iter::once(memory.frontmatter.summary.as_str())
        .chain(memory.frontmatter.tags.iter().map(String::as_str))
        .chain(body_available.then_some(memory.body.as_str()))
        .chain(memory.frontmatter.abstraction.as_deref())
        .chain(memory.frontmatter.cues.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    classify_memory_text(memory, text)
}

fn classify_memory_text(memory: &Memory, text: String) -> Result<PrivacyDecision, HandlerError> {
    let namespace = match memory.frontmatter.scope {
        Scope::User => PrivacyNamespace::Me,
        Scope::Project | Scope::Org => PrivacyNamespace::Project,
        Scope::Agent | Scope::Subagent => PrivacyNamespace::Agent,
    };
    classify_privacy(&text, namespace, None)
}
