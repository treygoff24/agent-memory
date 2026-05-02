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
        if !(self.threshold > 0.0 && self.threshold <= 1.0) {
            return Err(format!("coordination.relevance_gate.threshold must be in (0.0, 1.0], got {}", self.threshold));
        }
        if !(60..=3_600).contains(&self.recency_window_seconds) {
            return Err(format!(
                "coordination.relevance_gate.recency_window_seconds must be in [60, 3600], got {}",
                self.recency_window_seconds
            ));
        }
        if !(1..=5).contains(&self.per_turn_cap) {
            return Err(format!(
                "coordination.relevance_gate.per_turn_cap must be in [1, 5], got {}",
                self.per_turn_cap
            ));
        }
        if self.cross_device_startup_window_seconds < self.recency_window_seconds {
            return Err(
                "coordination.relevance_gate.cross_device_startup_window_seconds must be >= recency_window_seconds"
                    .to_owned(),
            );
        }
        if !(self.cross_device_startup_threshold > 0.0 && self.cross_device_startup_threshold <= 1.0) {
            return Err(format!(
                "coordination.relevance_gate.cross_device_startup_threshold must be in (0.0, 1.0], got {}",
                self.cross_device_startup_threshold
            ));
        }
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
        if !(10..=300).contains(&self.heartbeat_seconds) {
            return Err(format!(
                "coordination.presence.heartbeat_seconds must be in [10, 300], got {}",
                self.heartbeat_seconds
            ));
        }
        Ok(())
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
        if !(60..=3_600).contains(&self.ttl_seconds) {
            return Err(format!("coordination.claim_lock.ttl_seconds must be in [60, 3600], got {}", self.ttl_seconds));
        }
        Ok(())
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
