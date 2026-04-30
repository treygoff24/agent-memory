#![deny(unsafe_op_in_unsafe_fn)]
//! Stream D privacy classification and encryption boundaries.

pub mod classifier;
pub mod crypto;
pub mod decision;
pub mod entropy;
pub mod error;
pub mod keys;
pub mod masking;
pub mod policy;
pub mod privacy_filter;
pub mod regex;

pub use classifier::{DeterministicPrivacyClassifier, PrivacyClassifier};
pub use crypto::{EncryptedPayload, PrivacyEncryptor};
pub use decision::{
    safe_plaintext_fragment, PrivacyDecision, PrivacyLabel, PrivacyNamespace, PrivacyScanMetadata, PrivacySpan,
    PrivacyStorageAction, PrivacyTier, SafeFragmentDecision,
};
pub use error::{PrivacyError, PrivacyResult};
pub use keys::{FileKeyProvider, KeyMaterial, KeyProvider};
pub use masking::{MaskingSession, MaskingSessionId};
pub use policy::{CallerSensitivity, PrivacyPolicy, ResolvedPrivacyPolicy};
pub use privacy_filter::{DisabledPrivacyFilter, FixturePrivacyFilter, PrivacyFilterProvider};
