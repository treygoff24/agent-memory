//! Memory dynamics layer (memory-dynamics-v0.1).
//!
//! Use changes *ranking*, never existence (spec §1.1). This module owns the
//! shared usage computation (`usage.rs`), the use-driven strength function
//! (`strength.rs`), and the one canonical `dynamics:` config surface for the
//! crate (`DynamicsConfig`).
//!
//! ## One config struct for the crate
//!
//! The `dynamics:` section in `config.yaml` is parsed here once. Both the strength
//! ranking term (spec §3) and the Stream F fragment-archival deferral (spec §4,
//! `dream/fragment_archival.rs`) read the same [`DynamicsConfig`] — never two
//! independent parsers. The deferral knobs (`enabled`, `citation_defer_threshold`,
//! `max_fragment_lifetime_days`) and the ranking knobs (`alpha_points`, `tau_days`,
//! `weights`) all live on one struct, all defaulted, all dogfood-tunable (spec §7).

pub mod strength;
pub mod usage;

use std::path::Path;

use serde::{Deserialize, Serialize};

pub use strength::{strength, strength_points, StrengthFacts, StrengthWeights};
pub use usage::{distinct_sources_for, recall_usage_for, UsageSummary};

/// Default integer-points ceiling for the strength term (spec §3).
///
/// `strength_points(m) = floor(strength(m) × alpha_points)`. The invariant: a
/// structural ranking gap `>= alpha_points` can never be flipped by strength
/// alone. Near-ties (`< alpha_points`) can flip — including across scopes — by
/// design.
pub const DEFAULT_ALPHA_POINTS: u32 = 12;

/// Default exponential recency time-constant in days (spec §2).
pub const DEFAULT_TAU_DAYS: f64 = 14.0;

/// The canonical `dynamics:` config (spec §7).
///
/// One struct for the whole crate: the strength ranking term and the fragment
/// archival deferral both read it. All fields default to the spec values, so an
/// absent `dynamics:` block, an absent file, or any unrecognized keys all resolve
/// to spec defaults (`enabled: true`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DynamicsConfig {
    /// Master switch (spec §7). `false` → `strength_points = 0` everywhere and
    /// deferral off. The calibration log is intentionally *not* gated by this
    /// flag (review-outcome data collection never stops).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Integer-points ceiling for the strength term (spec §3, default 12).
    #[serde(default = "default_alpha_points")]
    pub alpha_points: u32,
    /// Recency time-constant in days for `exp(-Δdays/τ)` (spec §2, default 14).
    #[serde(default = "default_tau_days")]
    pub tau_days: f64,
    /// Strength component weights (spec §2). Renormalized at use (not
    /// validate-or-discard).
    #[serde(default)]
    pub weights: StrengthWeights,
    /// A fragment cited at least this many times is eligible for archival
    /// deferral (spec §4, default 2).
    #[serde(default = "default_citation_defer_threshold")]
    pub citation_defer_threshold: u32,
    /// Immortality cap: total fragment lifetime in days, after which archival
    /// proceeds regardless of citations (spec §4, default 42 = 3× base).
    #[serde(default = "default_max_fragment_lifetime_days")]
    pub max_fragment_lifetime_days: u32,
}

impl Default for DynamicsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            alpha_points: default_alpha_points(),
            tau_days: default_tau_days(),
            weights: StrengthWeights::default(),
            citation_defer_threshold: default_citation_defer_threshold(),
            max_fragment_lifetime_days: default_max_fragment_lifetime_days(),
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_alpha_points() -> u32 {
    DEFAULT_ALPHA_POINTS
}

fn default_tau_days() -> f64 {
    DEFAULT_TAU_DAYS
}

fn default_citation_defer_threshold() -> u32 {
    2
}

fn default_max_fragment_lifetime_days() -> u32 {
    42
}

/// Outer shape used to pluck just the `dynamics:` subtree out of `config.yaml`.
#[derive(Debug, Default, Deserialize)]
struct ConfigDynamicsEnvelope {
    #[serde(default)]
    dynamics: Option<DynamicsConfig>,
}

/// Load the `dynamics:` section from `<repo>/config.yaml`.
///
/// An absent file, an absent `dynamics:` section, or any unrecognized extra keys
/// all resolve to spec defaults (`enabled: true`). A malformed `dynamics:` block
/// is the only error surfaced; the caller treats that as "config off" rather than
/// failing the whole run.
pub fn load_dynamics_config(repo: &Path) -> Result<DynamicsConfig, String> {
    let path = repo.join("config.yaml");
    if !path.exists() {
        return Ok(DynamicsConfig::default());
    }
    let text = std::fs::read_to_string(&path).map_err(|err| err.to_string())?;
    let envelope: ConfigDynamicsEnvelope = serde_yaml::from_str(&text).map_err(|err| err.to_string())?;
    Ok(envelope.dynamics.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamics_config_defaults_match_spec() {
        let config = DynamicsConfig::default();
        assert!(config.enabled);
        assert_eq!(config.alpha_points, 12);
        assert_eq!(config.tau_days, 14.0);
        assert_eq!(config.weights, StrengthWeights::default());
        assert_eq!(config.citation_defer_threshold, 2);
        assert_eq!(config.max_fragment_lifetime_days, 42);
    }

    #[test]
    fn dynamics_config_parses_partial_section_with_ranking_keys() {
        let yaml = "dynamics:\n  alpha_points: 20\n  tau_days: 7\n  weights:\n    frequency: 0.5\n    recency: 0.3\n    corroboration: 0.2\n";
        let envelope: ConfigDynamicsEnvelope = serde_yaml::from_str(yaml).expect("parse");
        let config = envelope.dynamics.expect("dynamics present");
        assert_eq!(config.alpha_points, 20);
        assert_eq!(config.tau_days, 7.0);
        assert_eq!(config.weights.frequency, 0.5);
        // Unspecified keys fall back to spec defaults.
        assert!(config.enabled);
        assert_eq!(config.citation_defer_threshold, 2);
        assert_eq!(config.max_fragment_lifetime_days, 42);
    }

    #[test]
    fn dynamics_config_keeps_fragment_deferral_keys_working() {
        // The keys the cleanup layer (dream/fragment_archival.rs) previously parsed
        // locally must keep resolving against the consolidated struct.
        let yaml = "dynamics:\n  enabled: false\n  citation_defer_threshold: 5\n  max_fragment_lifetime_days: 28\n";
        let envelope: ConfigDynamicsEnvelope = serde_yaml::from_str(yaml).expect("parse");
        let config = envelope.dynamics.expect("dynamics present");
        assert!(!config.enabled);
        assert_eq!(config.citation_defer_threshold, 5);
        assert_eq!(config.max_fragment_lifetime_days, 28);
        // Ranking knobs default.
        assert_eq!(config.alpha_points, 12);
    }

    #[test]
    fn load_dynamics_config_defaults_when_file_absent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = load_dynamics_config(temp.path()).expect("load");
        assert_eq!(config, DynamicsConfig::default());
    }

    #[test]
    fn load_dynamics_config_ignores_unrelated_keys() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("config.yaml"),
            "schema_version: 1\ndreams:\n  enabled: true\ndynamics:\n  enabled: false\n  alpha_points: 6\n",
        )
        .expect("write config");
        let config = load_dynamics_config(temp.path()).expect("load");
        assert!(!config.enabled);
        assert_eq!(config.alpha_points, 6);
    }
}
