//! Synced and local config loading.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{EmbeddingTriple, Roots};

/// Synced Stream A config.
///
/// `active_embedding` is NOT optional and has no silent fallback.  A missing
/// or absent `config.yaml` returns `Ok(None)` from
/// [`load_active_embedding`]; callers must surface
/// `OpenError::InvalidRoots` rather than substituting a default triple.
/// Spec §10.2.2 #5: "No silent fallback."
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncedConfig {
    /// Schema version.
    pub schema_version: u32,
    /// Active embedding triple.  Required; no silent default.
    pub active_embedding: Option<EmbeddingTriple>,
    /// Portable default paths. Local config and env overrides win over these.
    #[serde(default)]
    pub paths: Option<PathsConfig>,
}

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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedConfig {
    /// Synced portable config.
    pub synced: SyncedConfig,
    /// Local per-device config, if adopted.
    pub local: Option<LocalDeviceConfig>,
    /// Resolved roots after explicit/env/local/synced precedence.
    pub roots: Roots,
}

/// Load synced config.  Returns `Ok(None)` when `config.yaml` does not exist.
pub fn load_synced_config(repo: &Path) -> Result<Option<SyncedConfig>, String> {
    let path = repo.join("config.yaml");
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    yaml_serde::from_str(&text).map(Some).map_err(|err| err.to_string())
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
    yaml_serde::from_str(&text).map(Some).map_err(|err| err.to_string())
}

/// Load config and apply roots precedence: explicit, environment, local, synced defaults.
pub fn load_config(repo: &Path, runtime: &Path, explicit_roots: Option<Roots>) -> Result<LoadedConfig, String> {
    let synced =
        load_synced_config(repo)?.unwrap_or(SyncedConfig { schema_version: 1, active_embedding: None, paths: None });
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

fn first_path(
    env_value: Option<PathBuf>,
    local_value: Option<PathBuf>,
    synced_value: Option<PathBuf>,
    fallback: PathBuf,
) -> PathBuf {
    env_value.or(local_value).or(synced_value).unwrap_or(fallback)
}
