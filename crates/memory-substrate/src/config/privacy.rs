//! Per-device runtime privacy enforcement switches.

use serde::{Deserialize, Serialize};

/// Runtime privacy enforcement flags.
///
/// These are local-device settings, not synced repository policy. The dogfood
/// default keeps the full classifier/encryption/masking stack off while the
/// secret-refusal invariant remains enforced by `memory-privacy`'s always-on
/// secret-only scanner. Use [`PrivacyEnforcement::paranoid`] for the eventual
/// ship default.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PrivacyEnforcement {
    /// Run the full PII/privacy classifier in addition to the always-on secret scan.
    #[serde(default)]
    pub classifier: bool,
    /// Route encrypt-at-rest decisions through encrypted substrate writes.
    #[serde(default)]
    pub encryption: bool,
    /// Enable masking sessions for display/redaction surfaces.
    #[serde(default)]
    pub masking: bool,
}

impl PrivacyEnforcement {
    /// Dogfood default: exercise the stack without full privacy friction.
    pub const fn dogfood() -> Self {
        Self { classifier: false, encryption: false, masking: false }
    }

    /// Safe production fallback used before runtime config is installed.
    pub const fn paranoid() -> Self {
        Self { classifier: true, encryption: true, masking: true }
    }

    /// Parse from a YAML snippet.
    pub fn from_yaml(yaml: &str) -> Result<Self, String> {
        let enforcement: Self = serde_yaml::from_str(yaml).map_err(|err| err.to_string())?;
        enforcement.validate()?;
        Ok(enforcement)
    }

    /// Parse supported environment overrides on top of the dogfood default.
    pub fn from_env() -> Result<Self, String> {
        let mut enforcement = Self::default();
        apply_bool_env("MEMORUM_PRIVACY_CLASSIFIER", &mut enforcement.classifier)?;
        apply_bool_env("MEMORUM_PRIVACY_ENCRYPTION", &mut enforcement.encryption)?;
        apply_bool_env("MEMORUM_PRIVACY_MASKING", &mut enforcement.masking)?;
        enforcement.validate()?;
        Ok(enforcement)
    }

    /// Validate this config. Kept explicit so future switch coupling fails at
    /// config-load time instead of inside a write path.
    pub fn validate(self) -> Result<(), String> {
        Ok(())
    }
}

impl Default for PrivacyEnforcement {
    fn default() -> Self {
        Self::dogfood()
    }
}

fn apply_bool_env(name: &str, target: &mut bool) -> Result<(), String> {
    let Some(value) = std::env::var_os(name) else {
        return Ok(());
    };
    *target = parse_bool(name, &value.to_string_lossy())?;
    Ok(())
}

fn parse_bool(name: &str, value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Ok(true),
        "0" | "false" | "off" | "no" => Ok(false),
        _ => Err(format!("{name} must be one of on/off/true/false/1/0/yes/no")),
    }
}
