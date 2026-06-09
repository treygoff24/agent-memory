use memorum_coordination::{
    ClaimLockConfig, ConfigValidationError, CoordinationConfig, PresenceConfig, RelevanceGateConfig,
};

#[test]
fn validate_typed_accepts_default_config() {
    let config = CoordinationConfig::default();

    assert_eq!(config.validate_typed(), Ok(()));
    assert_eq!(config.validate(), Ok(()));
}

#[test]
fn validate_typed_reports_invalid_level() {
    let config = CoordinationConfig { level: 0, ..CoordinationConfig::default() };

    assert_config_error(
        config,
        ConfigValidationError::InvalidLevel { level: 0 },
        "coordination.level must be 1, 2, or 3, got 0",
    );
}

#[test]
fn validate_typed_reports_inclusive_range_with_stringified_bounds() {
    let config = CoordinationConfig {
        relevance_gate: RelevanceGateConfig { recency_window_seconds: 59, ..RelevanceGateConfig::default() },
        ..CoordinationConfig::default()
    };

    assert_config_error(
        config,
        ConfigValidationError::InclusiveRange {
            label: "coordination.relevance_gate.recency_window_seconds",
            min: "60".to_string(),
            max: "3600".to_string(),
            value: "59".to_string(),
        },
        "coordination.relevance_gate.recency_window_seconds must be in [60, 3600], got 59",
    );
}

#[test]
fn validate_typed_reports_usize_inclusive_range_with_stringified_value() {
    let config = CoordinationConfig {
        relevance_gate: RelevanceGateConfig { per_turn_cap: 0, ..RelevanceGateConfig::default() },
        ..CoordinationConfig::default()
    };

    assert_config_error(
        config,
        ConfigValidationError::InclusiveRange {
            label: "coordination.relevance_gate.per_turn_cap",
            min: "1".to_string(),
            max: "5".to_string(),
            value: "0".to_string(),
        },
        "coordination.relevance_gate.per_turn_cap must be in [1, 5], got 0",
    );
}

#[test]
fn validate_typed_reports_claim_lock_inclusive_range_upper_boundary() {
    let config =
        CoordinationConfig { claim_lock: ClaimLockConfig { ttl_seconds: 3_601 }, ..CoordinationConfig::default() };

    assert_config_error(
        config,
        ConfigValidationError::InclusiveRange {
            label: "coordination.claim_lock.ttl_seconds",
            min: "60".to_string(),
            max: "3600".to_string(),
            value: "3601".to_string(),
        },
        "coordination.claim_lock.ttl_seconds must be in [60, 3600], got 3601",
    );
}

#[test]
fn validate_typed_reports_unit_threshold_lower_boundary() {
    let config = CoordinationConfig {
        relevance_gate: RelevanceGateConfig { threshold: 0.0, ..RelevanceGateConfig::default() },
        ..CoordinationConfig::default()
    };

    assert_config_error(
        config,
        ConfigValidationError::UnitThreshold { label: "coordination.relevance_gate.threshold", value: "0".to_string() },
        "coordination.relevance_gate.threshold must be in (0.0, 1.0], got 0",
    );
}

#[test]
fn validate_typed_reports_unit_threshold_upper_boundary() {
    let config = CoordinationConfig {
        relevance_gate: RelevanceGateConfig { cross_device_startup_threshold: 1.1, ..RelevanceGateConfig::default() },
        ..CoordinationConfig::default()
    };

    assert_config_error(
        config,
        ConfigValidationError::UnitThreshold {
            label: "coordination.relevance_gate.cross_device_startup_threshold",
            value: "1.1".to_string(),
        },
        "coordination.relevance_gate.cross_device_startup_threshold must be in (0.0, 1.0], got 1.1",
    );
}

#[test]
fn validate_typed_reports_presence_stale_below_double_heartbeat() {
    let config = CoordinationConfig {
        presence: PresenceConfig { heartbeat_seconds: 60, stale_after_seconds: 119 },
        ..CoordinationConfig::default()
    };

    assert_config_error(
        config,
        ConfigValidationError::PresenceStaleBelowDoubleHeartbeat,
        "coordination.presence.stale_after_seconds must be at least 2 * heartbeat_seconds",
    );
}

#[test]
fn validate_typed_reports_cross_device_startup_window_below_recency() {
    let config = CoordinationConfig {
        relevance_gate: RelevanceGateConfig {
            recency_window_seconds: 1_800,
            cross_device_startup_window_seconds: 1_799,
            ..RelevanceGateConfig::default()
        },
        ..CoordinationConfig::default()
    };

    assert_config_error(
        config,
        ConfigValidationError::CrossDeviceStartupWindowBelowRecency,
        "coordination.relevance_gate.cross_device_startup_window_seconds must be >= recency_window_seconds",
    );
}

#[test]
fn validate_typed_keeps_valid_boundary_values_inclusive() {
    let config = CoordinationConfig {
        level: 3,
        relevance_gate: RelevanceGateConfig {
            threshold: 1.0,
            recency_window_seconds: 60,
            per_turn_cap: 5,
            cross_device_startup_window_seconds: 60,
            cross_device_startup_threshold: 1.0,
        },
        presence: PresenceConfig { heartbeat_seconds: 10, stale_after_seconds: 20 },
        claim_lock: ClaimLockConfig { ttl_seconds: 3_600 },
    };

    assert_eq!(config.validate_typed(), Ok(()));
}

fn assert_config_error(config: CoordinationConfig, expected: ConfigValidationError, display: &str) {
    let typed_error = config.validate_typed().expect_err("config should fail typed validation");

    assert_eq!(typed_error, expected);
    assert_eq!(typed_error.to_string(), display);
    assert_eq!(config.validate().expect_err("config should fail compatibility validation"), display);
}
