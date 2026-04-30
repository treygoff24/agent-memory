use serde::{Deserialize, Serialize};

use crate::decision::{PrivacyNamespace, PrivacySpan, PrivacyTier};
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

impl PrivacyPolicy {
    /// Default tier for a namespace.
    pub fn default_tier(namespace: PrivacyNamespace) -> PrivacyTier {
        match namespace {
            PrivacyNamespace::Me => PrivacyTier::Personal,
            PrivacyNamespace::Project | PrivacyNamespace::Agent => PrivacyTier::Internal,
        }
    }

    /// Resolve the final tier. Caller metadata and classifier spans may raise, never lower, the namespace default.
    pub fn resolve_tier(
        namespace: PrivacyNamespace,
        caller: Option<CallerSensitivity>,
        spans: &[PrivacySpan],
    ) -> PrivacyResult<PrivacyTier> {
        let mut tier = Self::default_tier(namespace);
        if let Some(caller) = caller {
            tier = tier.max(caller.tier());
        }
        for span in spans {
            if span.end < span.start {
                return Err(PrivacyError::Policy("privacy span end precedes start".to_string()));
            }
            tier = tier.max(span.label.implied_tier());
        }
        Ok(tier)
    }
}
