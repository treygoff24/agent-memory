use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use thiserror::Error;

const DEFAULT_BIND_ADDRESS: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const DEFAULT_PORT: u16 = 7137;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebConfig {
    pub enabled: bool,
    pub bind_address: IpAddr,
    pub port: u16,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self { enabled: false, bind_address: DEFAULT_BIND_ADDRESS, port: DEFAULT_PORT }
    }
}

impl WebConfig {
    pub fn from_config_yaml(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let file: ConfigFile = serde_yaml::from_str(&contents).with_context(|| format!("parse {}", path.display()))?;
        let config = file.web.map(WebSection::into_config).unwrap_or_default();
        config.validate_localhost()?;
        Ok(config)
    }

    pub fn validate_localhost(&self) -> Result<()> {
        if self.bind_address == DEFAULT_BIND_ADDRESS {
            Ok(())
        } else {
            tracing::error!(bind_address = %self.bind_address, "memoryd web bind_address must be 127.0.0.1");
            Err(WebConfigError::NonLocalBindAddress { bind_address: self.bind_address }.into())
        }
    }
}

#[derive(Debug, Error)]
pub enum WebConfigError {
    #[error("web.bind_address must be 127.0.0.1, got {bind_address}")]
    NonLocalBindAddress { bind_address: IpAddr },
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    web: Option<WebSection>,
}

#[derive(Debug, Deserialize)]
struct WebSection {
    enabled: Option<bool>,
    bind_address: Option<IpAddr>,
    port: Option<u16>,
}

impl WebSection {
    fn into_config(self) -> WebConfig {
        let defaults = WebConfig::default();
        WebConfig {
            enabled: self.enabled.unwrap_or(defaults.enabled),
            bind_address: self.bind_address.unwrap_or(defaults.bind_address),
            port: self.port.unwrap_or(defaults.port),
        }
    }
}
