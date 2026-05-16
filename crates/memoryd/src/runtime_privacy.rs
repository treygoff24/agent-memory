use std::path::Path;

use memory_substrate::config::PrivacyEnforcement;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimePrivacyInstallStatus {
    Installed(PrivacyEnforcement),
    AlreadyInstalled(PrivacyEnforcement),
}

impl RuntimePrivacyInstallStatus {
    pub fn enforcement(self) -> PrivacyEnforcement {
        match self {
            Self::Installed(enforcement) | Self::AlreadyInstalled(enforcement) => enforcement,
        }
    }
}

pub fn install_privacy_runtime_from_roots(repo: &Path, runtime: &Path) -> Result<RuntimePrivacyInstallStatus, String> {
    let loaded_config = memory_substrate::config::load_config(repo, runtime, None)?;
    Ok(install_privacy_runtime(loaded_config.privacy_enforcement()))
}

pub fn install_privacy_runtime(enforcement: PrivacyEnforcement) -> RuntimePrivacyInstallStatus {
    match memory_privacy::install_runtime_enforcement(enforcement) {
        Ok(()) => RuntimePrivacyInstallStatus::Installed(enforcement),
        Err(_) => RuntimePrivacyInstallStatus::AlreadyInstalled(enforcement),
    }
}
