use serde::{Deserialize, Serialize};

use crate::decision::{PrivacyNamespace, PrivacySpan, PrivacyStorageAction, PrivacyTier};
use crate::error::{PrivacyError, PrivacyResult};

/// Caller-supplied sensitivity metadata accepted at daemon boundaries.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallerSensitivity {
    /// Public content.
    Public,
    /// Internal content.
    Internal,
    /// Confidential content.
    Confidential,
    /// Personal content.
    Personal,
    /// Compatibility alias for confidential handling.
    Sensitive,
    /// Secret material.
    Secret,
}

impl CallerSensitivity {
    /// Convert caller metadata into a privacy tier.
    pub fn tier(self) -> PrivacyTier {
        match self {
            Self::Public => PrivacyTier::Public,
            Self::Internal => PrivacyTier::Internal,
            Self::Confidential | Self::Sensitive => PrivacyTier::Confidential,
            Self::Personal => PrivacyTier::Personal,
            Self::Secret => PrivacyTier::Secret,
        }
    }
}

/// Deterministic policy that combines namespace defaults, caller metadata, and scanner spans.
#[derive(Clone, Copy, Debug, Default)]
pub struct PrivacyPolicy;

/// Resolved tier plus storage routing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedPrivacyPolicy {
    /// User-facing sensitivity tier.
    pub tier: PrivacyTier,
    /// Storage/refusal routing.
    pub storage_action: PrivacyStorageAction,
}

impl PrivacyPolicy {
    /// Default tier for a namespace.
    pub fn default_tier(namespace: PrivacyNamespace) -> PrivacyTier {
        match namespace {
            PrivacyNamespace::Me => PrivacyTier::Personal,
            PrivacyNamespace::Project | PrivacyNamespace::Agent => PrivacyTier::Internal,
        }
    }

    /// Resolve final tier and storage action.
    ///
    /// Caller metadata can raise the tier. Classifier spans affect storage
    /// routing, not the caller-visible tier, except that secret-like labels
    /// refuse storage before any disk effect.
    pub fn resolve(
        namespace: PrivacyNamespace,
        caller: Option<CallerSensitivity>,
        spans: &[PrivacySpan],
    ) -> PrivacyResult<ResolvedPrivacyPolicy> {
        let mut tier = Self::default_tier(namespace);
        if let Some(caller) = caller {
            tier = tier.max(caller.tier());
        }
        let mut storage_action = storage_action_for_tier(tier);
        for span in spans {
            if span.end < span.start {
                return Err(PrivacyError::Policy("privacy span end precedes start".to_string()));
            }
            storage_action = storage_action.max(span.label.storage_action());
        }
        Ok(ResolvedPrivacyPolicy { tier, storage_action })
    }

    /// Resolve only the final tier for legacy callers.
    pub fn resolve_tier(
        namespace: PrivacyNamespace,
        caller: Option<CallerSensitivity>,
        spans: &[PrivacySpan],
    ) -> PrivacyResult<PrivacyTier> {
        Self::resolve(namespace, caller, spans).map(|decision| decision.tier)
    }
}

fn storage_action_for_tier(tier: PrivacyTier) -> PrivacyStorageAction {
    match tier {
        PrivacyTier::Public | PrivacyTier::Internal => PrivacyStorageAction::Plaintext,
        PrivacyTier::Confidential | PrivacyTier::Personal => PrivacyStorageAction::EncryptAtRest,
        PrivacyTier::Secret => PrivacyStorageAction::Refuse,
    }
}
