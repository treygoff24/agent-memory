use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use thiserror::Error;

const DEFAULT_BIND_ADDRESS: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const DEFAULT_PORT: u16 = 7137;

/// The set of bind addresses accepted by the web server.
/// Spec §4.4 / §8 config notes: `bind_address` must be `127.0.0.1` or `::1`.
/// `0.0.0.0` and any routable address are rejected at config load.
fn is_localhost(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => v4 == Ipv4Addr::LOCALHOST,
        IpAddr::V6(v6) => v6 == Ipv6Addr::LOCALHOST,
    }
}

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
        if is_localhost(self.bind_address) {
            Ok(())
        } else {
            tracing::error!(
                bind_address = %self.bind_address,
                "memoryd web bind_address must be 127.0.0.1 or ::1"
            );
            Err(WebConfigError::NonLocalBindAddress { bind_address: self.bind_address }.into())
        }
    }
}

#[derive(Debug, Error)]
pub enum WebConfigError {
    #[error("web.bind_address must be 127.0.0.1 or ::1, got {bind_address}")]
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

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::WebConfig;

    fn config_with_addr(addr: IpAddr) -> WebConfig {
        WebConfig { enabled: false, bind_address: addr, port: 7137 }
    }

    #[test]
    fn test_ipv4_loopback_accepted() {
        let config = config_with_addr(IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert!(config.validate_localhost().is_ok());
    }

    #[test]
    fn test_ipv6_loopback_accepted() {
        // Spec §4.4 / §8: `bind_address` must be `127.0.0.1` OR `::1`.
        // Users on IPv6-only or dual-stack systems that configure `::1` must not
        // receive a confusing rejection.
        let config = config_with_addr(IpAddr::V6(Ipv6Addr::LOCALHOST));
        assert!(config.validate_localhost().is_ok());
    }

    #[test]
    fn test_non_loopback_ipv6_rejected() {
        // A routable IPv6 address (documentation range 2001:db8::/32) must be
        // rejected — the dashboard must never bind outside localhost.
        let routable: IpAddr = "2001:db8::1".parse().expect("valid IPv6");
        let config = config_with_addr(routable);
        let err = config.validate_localhost().unwrap_err();
        assert!(err.to_string().contains("2001:db8::1"));
    }

    #[test]
    fn test_ipv4_non_loopback_rejected() {
        let config = config_with_addr(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));
        assert!(config.validate_localhost().is_err());
    }
}
