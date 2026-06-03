use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Tunable Stream I cross-session coordination configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CoordinationConfig {
    #[serde(default = "default_level")]
    pub level: u8,
    #[serde(default)]
    pub relevance_gate: RelevanceGateConfig,
    #[serde(default)]
    pub presence: PresenceConfig,
    #[serde(default)]
    pub claim_lock: ClaimLockConfig,
}

impl Default for CoordinationConfig {
    fn default() -> Self {
        Self {
            level: 2,
            relevance_gate: RelevanceGateConfig::default(),
            presence: PresenceConfig::default(),
            claim_lock: ClaimLockConfig::default(),
        }
    }
}

/// Validate that an integer setting falls within an inclusive `[min, max]`
/// range, returning the spec-standard `must be in [min, max], got <value>`
/// diagnostic on failure. `label` is the fully-qualified config key.
fn validate_inclusive_range<T: PartialOrd + std::fmt::Display>(
    label: &str,
    value: T,
    min: T,
    max: T,
) -> Result<(), String> {
    if value < min || value > max {
        return Err(format!("{label} must be in [{min}, {max}], got {value}"));
    }
    Ok(())
}

/// Validate that a probability/threshold setting falls within the half-open
/// `(0.0, 1.0]` range, returning the spec-standard diagnostic on failure.
fn validate_unit_threshold(label: &str, value: f64) -> Result<(), String> {
    if !(value > 0.0 && value <= 1.0) {
        return Err(format!("{label} must be in (0.0, 1.0], got {value}"));
    }
    Ok(())
}

impl CoordinationConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=3).contains(&self.level) {
            return Err(format!("coordination.level must be 1, 2, or 3, got {}", self.level));
        }
        self.relevance_gate.validate()?;
        self.presence.validate()?;
        self.claim_lock.validate()?;
        if self.presence.stale_after_seconds < self.presence.heartbeat_seconds.saturating_mul(2) {
            return Err("coordination.presence.stale_after_seconds must be at least 2 * heartbeat_seconds".to_owned());
        }
        Ok(())
    }
}

/// Relevance gate thresholds and caps.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RelevanceGateConfig {
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    #[serde(default = "default_recency_window_seconds")]
    pub recency_window_seconds: u64,
    #[serde(default = "default_per_turn_cap")]
    pub per_turn_cap: usize,
    #[serde(default = "default_cross_device_startup_window_seconds")]
    pub cross_device_startup_window_seconds: u64,
    #[serde(default = "default_cross_device_startup_threshold")]
    pub cross_device_startup_threshold: f64,
}

impl Default for RelevanceGateConfig {
    fn default() -> Self {
        Self {
            threshold: 0.6,
            recency_window_seconds: 1_800,
            per_turn_cap: 2,
            cross_device_startup_window_seconds: 86_400,
            cross_device_startup_threshold: 0.7,
        }
    }
}

impl RelevanceGateConfig {
    pub fn recency_window(&self) -> Duration {
        Duration::from_secs(self.recency_window_seconds)
    }

    fn validate(&self) -> Result<(), String> {
        validate_unit_threshold("coordination.relevance_gate.threshold", self.threshold)?;
        validate_inclusive_range(
            "coordination.relevance_gate.recency_window_seconds",
            self.recency_window_seconds,
            60,
            3_600,
        )?;
        validate_inclusive_range("coordination.relevance_gate.per_turn_cap", self.per_turn_cap, 1, 5)?;
        if self.cross_device_startup_window_seconds < self.recency_window_seconds {
            return Err(
                "coordination.relevance_gate.cross_device_startup_window_seconds must be >= recency_window_seconds"
                    .to_owned(),
            );
        }
        validate_unit_threshold(
            "coordination.relevance_gate.cross_device_startup_threshold",
            self.cross_device_startup_threshold,
        )?;
        Ok(())
    }
}

/// Peer-presence heartbeat timing.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PresenceConfig {
    #[serde(default = "default_heartbeat_seconds")]
    pub heartbeat_seconds: u64,
    #[serde(default = "default_stale_after_seconds")]
    pub stale_after_seconds: u64,
}

impl Default for PresenceConfig {
    fn default() -> Self {
        Self { heartbeat_seconds: 60, stale_after_seconds: 300 }
    }
}

impl PresenceConfig {
    pub fn stale_after(&self) -> Duration {
        Duration::from_secs(self.stale_after_seconds)
    }

    fn validate(&self) -> Result<(), String> {
        validate_inclusive_range("coordination.presence.heartbeat_seconds", self.heartbeat_seconds, 10, 300)
    }
}

/// Advisory claim-lock timing.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ClaimLockConfig {
    #[serde(default = "default_claim_lock_ttl_seconds")]
    pub ttl_seconds: u64,
}

impl Default for ClaimLockConfig {
    fn default() -> Self {
        Self { ttl_seconds: 300 }
    }
}

impl ClaimLockConfig {
    pub fn ttl(&self) -> Duration {
        Duration::from_secs(self.ttl_seconds)
    }

    fn validate(&self) -> Result<(), String> {
        validate_inclusive_range("coordination.claim_lock.ttl_seconds", self.ttl_seconds, 60, 3_600)
    }
}

fn default_level() -> u8 {
    2
}

fn default_threshold() -> f64 {
    0.6
}

fn default_recency_window_seconds() -> u64 {
    1_800
}

fn default_per_turn_cap() -> usize {
    2
}

fn default_cross_device_startup_window_seconds() -> u64 {
    86_400
}

fn default_cross_device_startup_threshold() -> f64 {
    0.7
}

fn default_heartbeat_seconds() -> u64 {
    60
}

fn default_stale_after_seconds() -> u64 {
    300
}

fn default_claim_lock_ttl_seconds() -> u64 {
    300
}
