//! Synced and local config loading.

mod privacy;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{EmbeddingTriple, Roots};

pub use privacy::PrivacyEnforcement;

/// Dream prompt template generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PromptVersion {
    /// Original Stream F dogfood prompts.
    V1,
    /// Schema-heavy prompts with examples and stricter refusal guidance.
    V2,
}

/// Synced Stream A config.
///
/// `active_embedding` is NOT optional and has no silent fallback.  A missing
/// or absent `config.yaml` returns `Ok(None)` from
/// [`load_active_embedding`]; callers must surface
/// `OpenError::InvalidRoots` rather than substituting a default triple.
/// Spec §10.2.2 #5: "No silent fallback."
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncedConfig {
    /// Schema version.
    pub schema_version: u32,
    /// Active embedding triple.  Required; no silent default.
    pub active_embedding: Option<EmbeddingTriple>,
    /// Portable default paths. Local config and env overrides win over these.
    #[serde(default)]
    pub paths: Option<PathsConfig>,
    /// Stream F dreaming configuration.
    #[serde(default)]
    pub dreams: DreamsConfig,
    /// Event-log maintenance configuration.
    #[serde(default)]
    pub events: EventsConfig,
}

impl Eq for SyncedConfig {}

/// Path config shared by synced and local config files.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PathsConfig {
    /// Memory repo root.
    pub memory_root: Option<PathBuf>,
    /// Local runtime root.
    pub runtime_root: Option<PathBuf>,
}

/// Local per-device config.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalDeviceConfig {
    /// Schema version.
    pub schema_version: u32,
    /// Local device identity.
    pub device: LocalDevice,
    /// Local paths.
    #[serde(default)]
    pub paths: PathsConfig,
    /// Per-device runtime privacy enforcement switches. Never synced.
    #[serde(default)]
    pub privacy: PrivacyEnforcement,
}

/// Local device identity; never read from synced config.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalDevice {
    /// Device id.
    pub id: String,
    /// Human-readable device name.
    pub name: Option<String>,
    /// Device shard.
    pub shard: Option<String>,
}

/// Resolved config with precedence applied.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedConfig {
    /// Synced portable config.
    pub synced: SyncedConfig,
    /// Local per-device config, if adopted.
    pub local: Option<LocalDeviceConfig>,
    /// Resolved roots after explicit/env/local/synced precedence.
    pub roots: Roots,
}

impl LoadedConfig {
    /// Effective local runtime privacy enforcement.
    pub fn privacy_enforcement(&self) -> PrivacyEnforcement {
        self.local.as_ref().map(|local| local.privacy).unwrap_or_default()
    }
}

/// Stream F dreaming configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DreamsConfig {
    /// Enable scheduled dreaming unless disabled by local sentinel.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Prompt template version for all dream passes.
    #[serde(default = "default_prompt_version")]
    pub prompt_version: PromptVersion,
    /// Harness priority used when a scope has no override.
    #[serde(default = "default_cli_priority")]
    pub default_cli_priority: Vec<String>,
    /// Per-scope harness priority overrides.
    #[serde(default)]
    pub scope_overrides: BTreeMap<String, Vec<String>>,
    /// Per-pass harness timeout.
    #[serde(default = "default_per_pass_timeout_seconds")]
    pub per_pass_timeout_seconds: u32,
    /// Pass 1 substrate lookback.
    #[serde(default = "default_pass_1_window_days")]
    pub pass_1_window_days: u32,
    /// Pass 2 candidate cap.
    #[serde(default = "default_pass_2_max_candidates")]
    pub pass_2_max_candidates: u32,
    /// Pass 2 drift threshold.
    #[serde(default = "default_pass_2_drift_threshold")]
    pub pass_2_drift_threshold: f64,
    /// Pass 3 question cap.
    #[serde(default = "default_pass_3_max_questions")]
    pub pass_3_max_questions: u32,
    /// Pending-attention cap per scope.
    #[serde(default = "default_pending_attention_per_scope_cap")]
    pub pending_attention_per_scope_cap: u32,
    /// Pending-attention total cap.
    #[serde(default = "default_pending_attention_total_cap")]
    pub pending_attention_total_cap: u32,
    /// Pending-attention novelty window.
    #[serde(default = "default_pending_attention_recent_window_days")]
    pub pending_attention_recent_window_days: u32,
    /// Plaintext substrate lifetime before archival.
    #[serde(default = "default_fragment_lifetime_days")]
    pub fragment_lifetime_days: u32,
    /// Candidate stale threshold.
    #[serde(default = "default_candidate_stale_days")]
    pub candidate_stale_days: u32,
    /// Cleanup/dream anchor hour in UTC.
    #[serde(default = "default_cleanup_run_hour_utc")]
    pub cleanup_run_hour_utc: u32,
    /// Journal lease window.
    #[serde(default = "default_lease_window_seconds")]
    pub lease_window_seconds: u32,
    /// Scheduled dream retry window.
    #[serde(default = "default_dream_retry_window_minutes")]
    pub dream_retry_window_minutes: u32,
}

impl PartialEq for DreamsConfig {
    fn eq(&self, other: &Self) -> bool {
        self.enabled == other.enabled
            && self.prompt_version == other.prompt_version
            && self.default_cli_priority == other.default_cli_priority
            && self.scope_overrides == other.scope_overrides
            && self.per_pass_timeout_seconds == other.per_pass_timeout_seconds
            && self.pass_1_window_days == other.pass_1_window_days
            && self.pass_2_max_candidates == other.pass_2_max_candidates
            && self.pass_2_drift_threshold.to_bits() == other.pass_2_drift_threshold.to_bits()
            && self.pass_3_max_questions == other.pass_3_max_questions
            && self.pending_attention_per_scope_cap == other.pending_attention_per_scope_cap
            && self.pending_attention_total_cap == other.pending_attention_total_cap
            && self.pending_attention_recent_window_days == other.pending_attention_recent_window_days
            && self.fragment_lifetime_days == other.fragment_lifetime_days
            && self.candidate_stale_days == other.candidate_stale_days
            && self.cleanup_run_hour_utc == other.cleanup_run_hour_utc
            && self.lease_window_seconds == other.lease_window_seconds
            && self.dream_retry_window_minutes == other.dream_retry_window_minutes
    }
}

impl Eq for DreamsConfig {}

impl Default for DreamsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            prompt_version: default_prompt_version(),
            default_cli_priority: default_cli_priority(),
            scope_overrides: BTreeMap::new(),
            per_pass_timeout_seconds: default_per_pass_timeout_seconds(),
            pass_1_window_days: default_pass_1_window_days(),
            pass_2_max_candidates: default_pass_2_max_candidates(),
            pass_2_drift_threshold: default_pass_2_drift_threshold(),
            pass_3_max_questions: default_pass_3_max_questions(),
            pending_attention_per_scope_cap: default_pending_attention_per_scope_cap(),
            pending_attention_total_cap: default_pending_attention_total_cap(),
            pending_attention_recent_window_days: default_pending_attention_recent_window_days(),
            fragment_lifetime_days: default_fragment_lifetime_days(),
            candidate_stale_days: default_candidate_stale_days(),
            cleanup_run_hour_utc: default_cleanup_run_hour_utc(),
            lease_window_seconds: default_lease_window_seconds(),
            dream_retry_window_minutes: default_dream_retry_window_minutes(),
        }
    }
}

/// Event-log maintenance config.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventsConfig {
    /// Live event-log compaction age.
    #[serde(default = "default_events_compaction_days")]
    pub compaction_days: u32,
}

impl Default for EventsConfig {
    fn default() -> Self {
        Self { compaction_days: default_events_compaction_days() }
    }
}

/// Load synced config.  Returns `Ok(None)` when `config.yaml` does not exist.
pub fn load_synced_config(repo: &Path) -> Result<Option<SyncedConfig>, String> {
    let path = repo.join("config.yaml");
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let config: SyncedConfig = serde_yaml::from_str(&text).map_err(|err| err.to_string())?;
    validate_synced_config(&config)?;
    Ok(Some(config))
}

/// Load and return the active embedding triple.
///
/// Returns `Err` when `config.yaml` is missing or has no `active_embedding`
/// field.  Callers must not substitute a default — spec §10.2.2 #5.
pub fn load_active_embedding(repo: &Path) -> Result<EmbeddingTriple, String> {
    let synced = load_synced_config(repo)?
        .ok_or_else(|| "config.yaml missing; cannot determine active_embedding".to_string())?;
    synced.active_embedding.ok_or_else(|| "active_embedding not set in config.yaml".to_string())
}

/// Load local device config if present.
pub fn load_local_device_config(runtime: &Path) -> Result<Option<LocalDeviceConfig>, String> {
    let path = runtime.join("local-device.yaml");
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_yaml::from_str(&text).map(Some).map_err(|err| err.to_string())
}

/// Load config and apply roots precedence: explicit, environment, local, synced defaults.
pub fn load_config(repo: &Path, runtime: &Path, explicit_roots: Option<Roots>) -> Result<LoadedConfig, String> {
    let synced = load_synced_config(repo)?.unwrap_or(SyncedConfig {
        schema_version: 1,
        active_embedding: None,
        paths: None,
        dreams: DreamsConfig::default(),
        events: EventsConfig::default(),
    });
    let local = load_local_device_config(runtime)?;
    let roots = explicit_roots.unwrap_or_else(|| {
        let env_memory = std::env::var_os("STREAM_A_MEMORY_ROOT").map(PathBuf::from);
        let env_runtime = std::env::var_os("STREAM_A_RUNTIME_ROOT").map(PathBuf::from);
        Roots::new(
            first_path(
                env_memory,
                local.as_ref().and_then(|cfg| cfg.paths.memory_root.clone()),
                synced.paths.as_ref().and_then(|paths| paths.memory_root.clone()),
                repo.to_path_buf(),
            ),
            first_path(
                env_runtime,
                local.as_ref().and_then(|cfg| cfg.paths.runtime_root.clone()),
                synced.paths.as_ref().and_then(|paths| paths.runtime_root.clone()),
                runtime.to_path_buf(),
            ),
        )
    });
    Ok(LoadedConfig { synced, local, roots })
}

fn validate_synced_config(config: &SyncedConfig) -> Result<(), String> {
    validate_cli_priority("dreams.default_cli_priority", &config.dreams.default_cli_priority)?;
    for (scope, priority) in &config.dreams.scope_overrides {
        validate_scope_key(scope)?;
        validate_cli_priority(&format!("dreams.scope_overrides.{scope}"), priority)?;
    }
    validate_range("dreams.per_pass_timeout_seconds", config.dreams.per_pass_timeout_seconds, 30, 1800)?;
    validate_range("dreams.pass_1_window_days", config.dreams.pass_1_window_days, 1, 90)?;
    validate_range("dreams.pass_2_max_candidates", config.dreams.pass_2_max_candidates, 1, 64)?;
    validate_float_range("dreams.pass_2_drift_threshold", config.dreams.pass_2_drift_threshold, 0.05, 0.90)?;
    validate_range("dreams.pass_3_max_questions", config.dreams.pass_3_max_questions, 1, 64)?;
    validate_range("dreams.pending_attention_per_scope_cap", config.dreams.pending_attention_per_scope_cap, 1, 8)?;
    validate_range("dreams.pending_attention_total_cap", config.dreams.pending_attention_total_cap, 1, 24)?;
    validate_range(
        "dreams.pending_attention_recent_window_days",
        config.dreams.pending_attention_recent_window_days,
        1,
        30,
    )?;
    validate_range("dreams.fragment_lifetime_days", config.dreams.fragment_lifetime_days, 1, 365)?;
    validate_range("dreams.candidate_stale_days", config.dreams.candidate_stale_days, 1, 365)?;
    validate_range("dreams.cleanup_run_hour_utc", config.dreams.cleanup_run_hour_utc, 0, 23)?;
    validate_range("dreams.lease_window_seconds", config.dreams.lease_window_seconds, 60, 14400)?;
    validate_range("dreams.dream_retry_window_minutes", config.dreams.dream_retry_window_minutes, 0, 720)?;
    validate_range("events.compaction_days", config.events.compaction_days, 7, 730)?;
    if config.dreams.pending_attention_per_scope_cap > config.dreams.pending_attention_total_cap {
        return Err(format!(
            "dreams.pending_attention_per_scope_cap ({}) cannot exceed dreams.pending_attention_total_cap ({})",
            config.dreams.pending_attention_per_scope_cap, config.dreams.pending_attention_total_cap
        ));
    }
    Ok(())
}

fn validate_cli_priority(field: &str, priority: &[String]) -> Result<(), String> {
    if priority.is_empty() {
        return Err(format!("{field} must contain at least one harness"));
    }
    for name in priority {
        if !is_known_harness_name(name) {
            return Err(format!("{field} contains unknown harness name: {name}"));
        }
    }
    Ok(())
}

fn is_known_harness_name(name: &str) -> bool {
    matches!(name, "claude" | "codex")
}

fn validate_scope_key(scope: &str) -> Result<(), String> {
    if matches!(scope, "me" | "agent") {
        return Ok(());
    }
    if let Some(id) = scope.strip_prefix("project:") {
        return validate_scope_id(scope, id);
    }
    if let Some(id) = scope.strip_prefix("org:") {
        return validate_scope_id(scope, id);
    }
    Err(format!("dreams.scope_overrides has invalid scope key: {scope}"))
}

fn validate_scope_id(scope: &str, id: &str) -> Result<(), String> {
    if !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
        Ok(())
    } else {
        Err(format!("dreams.scope_overrides has invalid scope key: {scope}"))
    }
}

fn validate_range(field: &str, value: u32, min: u32, max: u32) -> Result<(), String> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(format!("{field} out of range [{min}, {max}]: {value}"))
    }
}

fn validate_float_range(field: &str, value: f64, min: f64, max: f64) -> Result<(), String> {
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(format!("{field} out of range [{min}, {max}]: {value}"))
    }
}

fn first_path(
    env_value: Option<PathBuf>,
    local_value: Option<PathBuf>,
    synced_value: Option<PathBuf>,
    fallback: PathBuf,
) -> PathBuf {
    env_value.or(local_value).or(synced_value).unwrap_or(fallback)
}

fn default_true() -> bool {
    true
}

fn default_prompt_version() -> PromptVersion {
    PromptVersion::V2
}

fn default_cli_priority() -> Vec<String> {
    // Keep this in sync with `is_known_harness_name`: only shipped v0.2
    // adapters are valid config names. Deferred adapters, such as Gemini, fail
    // config validation until their prompt transport and auth probe ship.
    vec!["claude".to_string(), "codex".to_string()]
}

fn default_per_pass_timeout_seconds() -> u32 {
    300
}

fn default_pass_1_window_days() -> u32 {
    7
}

fn default_pass_2_max_candidates() -> u32 {
    8
}

fn default_pass_2_drift_threshold() -> f64 {
    0.30
}

fn default_pass_3_max_questions() -> u32 {
    12
}

fn default_pending_attention_per_scope_cap() -> u32 {
    2
}

fn default_pending_attention_total_cap() -> u32 {
    6
}

fn default_pending_attention_recent_window_days() -> u32 {
    7
}

fn default_fragment_lifetime_days() -> u32 {
    14
}

fn default_candidate_stale_days() -> u32 {
    30
}

fn default_cleanup_run_hour_utc() -> u32 {
    3
}

fn default_lease_window_seconds() -> u32 {
    3600
}

fn default_dream_retry_window_minutes() -> u32 {
    180
}

fn default_events_compaction_days() -> u32 {
    90
}
