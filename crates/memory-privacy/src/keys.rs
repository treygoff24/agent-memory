use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use age::secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

use crate::error::{PrivacyError, PrivacyResult};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Minimal key material boundary for the encrypted tier.
#[derive(Clone)]
pub struct KeyMaterial {
    /// Public recipient identifier.
    pub recipient: String,
    /// Local private key material or test secret.
    pub identity: String,
}

impl KeyMaterial {
    /// Generate fresh X25519 key material.
    pub fn generate() -> Self {
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public().to_string();
        let identity = identity.to_string().expose_secret().to_string();
        Self { recipient, identity }
    }

    /// Parse the age recipient.
    pub fn recipient(&self) -> PrivacyResult<age::x25519::Recipient> {
        self.recipient.parse().map_err(|err| PrivacyError::KeyUnavailable(format!("invalid recipient: {err}")))
    }

    /// Parse the age identity.
    pub fn identity(&self) -> PrivacyResult<age::x25519::Identity> {
        self.identity.parse().map_err(|err| PrivacyError::KeyUnavailable(format!("invalid identity: {err}")))
    }
}

impl std::fmt::Debug for KeyMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KeyMaterial")
            .field("recipient", &self.recipient)
            .field("identity", &"[redacted]")
            .finish()
    }
}

#[derive(Deserialize, Serialize)]
struct KeyRecord {
    recipient: String,
    identity: String,
}

impl From<KeyMaterial> for KeyRecord {
    fn from(key: KeyMaterial) -> Self {
        Self { recipient: key.recipient, identity: key.identity }
    }
}

impl From<KeyRecord> for KeyMaterial {
    fn from(record: KeyRecord) -> Self {
        Self { recipient: record.recipient, identity: record.identity }
    }
}

/// Source of encryption key material.
pub trait KeyProvider: Send + Sync {
    /// Load the active encryption key.
    fn load_key(&self) -> PrivacyResult<KeyMaterial>;
}

/// File-backed key provider used by local daemon tests and CLI onboarding.
#[derive(Clone, Debug)]
pub struct FileKeyProvider {
    path: PathBuf,
}

impl FileKeyProvider {
    /// Create a file key provider.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Default runtime-local path.
    pub fn runtime_default(runtime: &Path) -> Self {
        Self::new(runtime.join("privacy").join("age-key.json"))
    }

    /// Write fresh key material for local onboarding/tests.
    pub fn onboard_local_file(&self) -> PrivacyResult<KeyMaterial> {
        let key = KeyMaterial::generate();
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?;
            harden_private_directory(parent)?;
        }
        let json = serde_json::to_string_pretty(&KeyRecord::from(key.clone()))
            .map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?;
        write_private_file(&self.path, json.as_bytes())?;
        Ok(key)
    }

    /// Key storage path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl KeyProvider for FileKeyProvider {
    fn load_key(&self) -> PrivacyResult<KeyMaterial> {
        reject_symlink(&self.path)?;
        validate_private_file(&self.path)?;
        let text = fs::read_to_string(&self.path)
            .map_err(|err| PrivacyError::KeyUnavailable(format!("{} ({err})", self.path.display())))?;
        serde_json::from_str::<KeyRecord>(&text)
            .map(KeyMaterial::from)
            .map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))
    }
}

fn write_private_file(path: &Path, contents: &[u8]) -> PrivacyResult<()> {
    reject_symlink(path)?;
    let temp_path = path.with_extension(format!(
        "tmp.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?
            .as_nanos()
    ));
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?;
        #[cfg(unix)]
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?;
        file.write_all(contents).map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?;
        file.sync_all().map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?;
    }
    fs::rename(&temp_path, path).map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?;
    validate_private_file(path)
}

fn reject_symlink(path: &Path) -> PrivacyResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(PrivacyError::KeyUnavailable(format!("refusing symlinked key path: {}", path.display())))
        }
        Ok(_) | Err(_) => Ok(()),
    }
}

#[cfg(unix)]
fn harden_private_directory(path: &Path) -> PrivacyResult<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))
}

#[cfg(not(unix))]
fn harden_private_directory(_path: &Path) -> PrivacyResult<()> {
    Ok(())
}

#[cfg(unix)]
fn validate_private_file(path: &Path) -> PrivacyResult<()> {
    let metadata = fs::metadata(path).map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?;
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(PrivacyError::KeyUnavailable(format!(
            "key file must not be group/world accessible: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_private_file(_path: &Path) -> PrivacyResult<()> {
    Ok(())
}
