use chrono::{DateTime, Utc};
use memory_substrate::{ClassificationOutcome, Sensitivity};
use serde::{Deserialize, Serialize};

/// Namespace used for privacy defaults.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyNamespace {
    /// User-owned memory defaults to personal handling.
    Me,
    /// Project memory defaults to internal handling.
    Project,
    /// Agent memory defaults to internal handling.
    Agent,
}

/// Stream D storage/privacy tier. `Secret` is runtime-only and is never persisted as frontmatter sensitivity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyTier {
    /// Public plaintext is allowed.
    Public,
    /// Internal plaintext is allowed.
    Internal,
    /// Confidential content requires encryption.
    Confidential,
    /// Personal content requires encryption.
    Personal,
    /// Secret material is refused before disk effects.
    Secret,
}

impl PrivacyTier {
    /// Convert the privacy tier into Stream A's required classification contract.
    pub fn classification(self) -> ClassificationOutcome {
        match self {
            Self::Public | Self::Internal => ClassificationOutcome::Trusted,
            Self::Confidential | Self::Personal => ClassificationOutcome::RequiresEncryption,
            Self::Secret => ClassificationOutcome::Secret,
        }
    }

    /// Convert to a persisted frontmatter sensitivity when the tier can be stored.
    pub fn persisted_sensitivity(self) -> Option<Sensitivity> {
        match self {
            Self::Public => Some(Sensitivity::Public),
            Self::Internal => Some(Sensitivity::Internal),
            Self::Confidential => Some(Sensitivity::Confidential),
            Self::Personal => Some(Sensitivity::Personal),
            Self::Secret => None,
        }
    }

    /// Whether this tier requires encrypted Stream A writes.
    pub fn requires_encryption(self) -> bool {
        matches!(self, Self::Confidential | Self::Personal)
    }
}

/// Detected privacy span label.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyLabel {
    /// Account or customer number.
    AccountNumber,
    /// Physical address.
    PrivateAddress,
    /// Email address.
    PrivateEmail,
    /// Person-like name.
    PrivatePerson,
    /// Phone number.
    PrivatePhone,
    /// Private URL.
    PrivateUrl,
    /// Private date.
    PrivateDate,
    /// Credential, token, key, or other secret.
    Secret,
}

impl PrivacyLabel {
    /// Conservative tier implied by a detected label.
    pub fn implied_tier(self) -> PrivacyTier {
        match self {
            Self::Secret => PrivacyTier::Secret,
            Self::AccountNumber
            | Self::PrivateAddress
            | Self::PrivateEmail
            | Self::PrivatePerson
            | Self::PrivatePhone
            | Self::PrivateUrl
            | Self::PrivateDate => PrivacyTier::Personal,
        }
    }

    /// Stable masking token prefix.
    pub fn token_prefix(self) -> &'static str {
        match self {
            Self::AccountNumber => "Account",
            Self::PrivateAddress => "Address",
            Self::PrivateEmail => "Email",
            Self::PrivatePerson => "Person",
            Self::PrivatePhone => "Phone",
            Self::PrivateUrl => "Url",
            Self::PrivateDate => "Date",
            Self::Secret => "Secret",
        }
    }
}

/// Byte-offset span detected by a privacy classifier.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PrivacySpan {
    /// Span label.
    pub label: PrivacyLabel,
    /// Inclusive byte offset.
    pub start: usize,
    /// Exclusive byte offset.
    pub end: usize,
    /// Confidence in `[0.0, 1.0]`.
    pub confidence: f32,
}

impl PrivacySpan {
    /// Build a privacy span.
    pub fn new(label: PrivacyLabel, start: usize, end: usize, confidence: f32) -> Self {
        Self { label, start, end, confidence }
    }
}

/// Audit metadata for a completed privacy scan.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PrivacyScanMetadata {
    /// Scanner/model name.
    pub model: String,
    /// Scan timestamp.
    pub ran_at: DateTime<Utc>,
    /// Number of spans detected.
    pub spans_detected: usize,
    /// Labels detected.
    pub labels: Vec<PrivacyLabel>,
}

/// Final Stream D decision for a candidate write.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PrivacyDecision {
    /// Final tier after defaults, caller metadata, deterministic scanner, and optional model spans.
    pub tier: PrivacyTier,
    /// All spans contributing to the decision.
    pub spans: Vec<PrivacySpan>,
    /// Audit metadata.
    pub scan: PrivacyScanMetadata,
}

impl PrivacyDecision {
    /// Build a decision.
    pub fn new(tier: PrivacyTier, spans: Vec<PrivacySpan>, model: impl Into<String>) -> Self {
        let labels = spans.iter().map(|span| span.label).collect::<Vec<_>>();
        Self {
            tier,
            spans,
            scan: PrivacyScanMetadata { model: model.into(), ran_at: Utc::now(), spans_detected: labels.len(), labels },
        }
    }
}
