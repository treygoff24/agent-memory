use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use memorum_theme::{Charset, ColorCapability};
use serde::Deserialize;

const DEFAULT_TICK_MS: u64 = 16;
const DEFAULT_DAEMON_POLL_MS: u64 = 250;
const DEFAULT_SOCKET_PATH: &str = "/run/user/1000/memoryd.sock";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiConfig {
    pub socket_path: PathBuf,
    pub tick_interval: Duration,
    pub daemon_poll_interval: Duration,
    pub theme: String,
    pub theme_config: Option<PathBuf>,
    pub charset: Charset,
    pub no_motion: bool,
    pub color_capability: Option<ColorCapability>,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            socket_path: PathBuf::from(DEFAULT_SOCKET_PATH),
            tick_interval: Duration::from_millis(DEFAULT_TICK_MS),
            daemon_poll_interval: Duration::from_millis(DEFAULT_DAEMON_POLL_MS),
            theme: "default-warm-dark".to_string(),
            theme_config: default_theme_config_path(),
            charset: Charset::detect(),
            no_motion: false,
            color_capability: None,
        }
    }
}

impl UiConfig {
    pub fn from_config_yaml(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let file: ConfigFile = yaml_serde::from_str(&contents).with_context(|| format!("parse {}", path.display()))?;
        match file.ui {
            Some(ui) => ui.into_config(),
            None => Ok(Self::default()),
        }
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
    theme: Option<String>,
    theme_config: Option<PathBuf>,
    charset: Option<Charset>,
    no_motion: Option<bool>,
    color_capability: Option<ColorCapability>,
}

impl UiSection {
    fn into_config(self) -> Result<UiConfig> {
        let defaults = UiConfig::default();
        Ok(UiConfig {
            socket_path: self.socket_path.unwrap_or(defaults.socket_path),
            tick_interval: non_zero_duration(self.tick_ms, DEFAULT_TICK_MS, "ui.tick_ms")?,
            daemon_poll_interval: non_zero_duration(self.daemon_poll_ms, DEFAULT_DAEMON_POLL_MS, "ui.daemon_poll_ms")?,
            theme: self.theme.unwrap_or(defaults.theme),
            theme_config: self.theme_config.or(defaults.theme_config),
            charset: self.charset.unwrap_or(defaults.charset),
            no_motion: self.no_motion.unwrap_or(defaults.no_motion),
            color_capability: self.color_capability.or(defaults.color_capability),
        })
    }
}

fn non_zero_duration(value: Option<u64>, default_ms: u64, field: &str) -> Result<Duration> {
    let millis = value.unwrap_or(default_ms);
    if millis == 0 {
        anyhow::bail!("{field} must be greater than 0");
    }
    Ok(Duration::from_millis(millis))
}

fn default_theme_config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join(".config/memorum/theme.toml");
    path.exists().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_rejects_zero_tick_interval() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.yaml");
        std::fs::write(&path, "ui:\n  tick_ms: 0\n").expect("write config");

        let error = UiConfig::from_config_yaml(&path).expect_err("zero tick rejected");

        assert!(error.to_string().contains("ui.tick_ms must be greater than 0"));
    }

    #[test]
    fn config_rejects_zero_daemon_poll_interval() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.yaml");
        std::fs::write(&path, "ui:\n  daemon_poll_ms: 0\n").expect("write config");

        let error = UiConfig::from_config_yaml(&path).expect_err("zero poll rejected");

        assert!(error.to_string().contains("ui.daemon_poll_ms must be greater than 0"));
    }

    #[test]
    fn config_accepts_positive_intervals() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.yaml");
        std::fs::write(&path, "ui:\n  tick_ms: 1\n  daemon_poll_ms: 2\n").expect("write config");

        let config = UiConfig::from_config_yaml(&path).expect("config loads");

        assert_eq!(config.tick_interval, Duration::from_millis(1));
        assert_eq!(config.daemon_poll_interval, Duration::from_millis(2));
    }
}
