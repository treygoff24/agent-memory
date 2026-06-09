//! Privacy-classification glue between the parsed governance write input and the
//! deterministic privacy classifier.
//!
//! `classify_input_privacy` projects the scannable text out of a
//! `GovernanceWriteInput`; `classify_privacy` is the shared classifier entry
//! point consumed by `memory_ops` and `review` as well; `attach_privacy_scan`
//! records the scan result onto a `Memory` before it is persisted.

use memory_privacy::{DeterministicPrivacyClassifier, PrivacyClassifier, PrivacyDecision, PrivacyNamespace};
use memory_substrate::Memory;

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
