use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;

const DEFAULT_TICK_MS: u64 = 16;
const DEFAULT_DAEMON_POLL_MS: u64 = 250;
const DEFAULT_SOCKET_PATH: &str = "/run/user/1000/memoryd.sock";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiConfig {
    pub socket_path: PathBuf,
    pub tick_interval: Duration,
    pub daemon_poll_interval: Duration,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            socket_path: PathBuf::from(DEFAULT_SOCKET_PATH),
            tick_interval: Duration::from_millis(DEFAULT_TICK_MS),
            daemon_poll_interval: Duration::from_millis(DEFAULT_DAEMON_POLL_MS),
        }
    }
}

impl UiConfig {
    pub fn from_config_yaml(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let file: ConfigFile = serde_yaml::from_str(&contents).with_context(|| format!("parse {}", path.display()))?;
        Ok(file.ui.map(UiSection::into_config).unwrap_or_default())
    }
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    ui: Option<UiSection>,
}

#[derive(Debug, Deserialize)]
struct UiSection {
    socket_path: Option<PathBuf>,
    tick_ms: Option<u64>,
    daemon_poll_ms: Option<u64>,
}

impl UiSection {
    fn into_config(self) -> UiConfig {
        let defaults = UiConfig::default();
        UiConfig {
            socket_path: self.socket_path.unwrap_or(defaults.socket_path),
            tick_interval: Duration::from_millis(self.tick_ms.unwrap_or(DEFAULT_TICK_MS)),
            daemon_poll_interval: Duration::from_millis(self.daemon_poll_ms.unwrap_or(DEFAULT_DAEMON_POLL_MS)),
        }
    }
}
