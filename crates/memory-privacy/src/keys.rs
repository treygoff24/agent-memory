use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRotation {
    pub previous_recipient: Option<String>,
    pub active_recipient: String,
    pub active_key_path: PathBuf,
    pub archived_key_path: Option<PathBuf>,
    pub active_manifest_path: PathBuf,
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

    /// Load keys that may decrypt existing ciphertexts.
    fn load_decryption_keys(&self) -> PrivacyResult<Vec<KeyMaterial>> {
        self.load_key().map(|key| vec![key])
    }
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
        write_key_record(&self.path, &key)?;
        self.write_active_manifest(&key)?;
        Ok(key)
    }

    /// Rotate active key material and keep prior local identities for decrypting old records.
    pub fn rotate_local_file(&self) -> PrivacyResult<KeyRotation> {
        fs::create_dir_all(self.decommissioned_dir()).map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?;
        harden_private_directory(&self.decommissioned_dir())?;
        let previous = match self.load_key() {
            Ok(key) => Some(key),
            Err(error) if is_missing_key_error(&error) => None,
            Err(error) => return Err(error),
        };
        let archived_key_path = previous.as_ref().map(|key| self.archive_key(key)).transpose()?;
        let active = KeyMaterial::generate();
        write_key_record(&self.path, &active)?;
        let active_manifest_path = self.write_active_manifest(&active)?;

        Ok(KeyRotation {
            previous_recipient: previous.map(|key| key.recipient),
            active_recipient: active.recipient,
            active_key_path: self.path.clone(),
            archived_key_path,
            active_manifest_path,
        })
    }

    /// Key storage path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn active_manifest_path(&self) -> PathBuf {
        self.key_store_dir().join("active.json")
    }

    pub fn decommissioned_dir(&self) -> PathBuf {
        self.key_store_dir().join("decommissioned")
    }

    fn key_store_dir(&self) -> PathBuf {
        self.path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
    }

    fn write_active_manifest(&self, key: &KeyMaterial) -> PrivacyResult<PathBuf> {
        let path = self.active_manifest_path();
        let manifest = serde_json::json!({
            "schema_version": 1,
            "active_key_path": self.path.file_name().and_then(|name| name.to_str()).unwrap_or("age-key.json"),
            "recipient": key.recipient,
            "rotated_at_unix_nanos": unix_nanos()?,
        });
        let json = serde_json::to_vec_pretty(&manifest).map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?;
        write_private_file(&path, &json)?;
        Ok(path)
    }

    fn archive_key(&self, key: &KeyMaterial) -> PrivacyResult<PathBuf> {
        let dir = self.decommissioned_dir();
        fs::create_dir_all(&dir).map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?;
        harden_private_directory(&dir)?;
        let path = dir.join(format!("{}-age-key.json", unix_nanos()?));
        write_key_record(&path, key)?;
        Ok(path)
    }

    fn archived_keys(&self) -> PrivacyResult<Vec<KeyMaterial>> {
        let dir = self.decommissioned_dir();
        let Ok(entries) = fs::read_dir(&dir) else {
            return Ok(Vec::new());
        };
        let mut keys = Vec::new();
        for entry in entries {
            let path = entry.map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            reject_symlink(&path)?;
            validate_private_file(&path)?;
            let text = fs::read_to_string(&path)
                .map_err(|err| PrivacyError::KeyUnavailable(format!("{} ({err})", path.display())))?;
            let record = serde_json::from_str::<KeyRecord>(&text)
                .map_err(|err| PrivacyError::KeyUnavailable(format!("{} ({err})", path.display())))?;
            keys.push(KeyMaterial::from(record));
        }
        Ok(keys)
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

    fn load_decryption_keys(&self) -> PrivacyResult<Vec<KeyMaterial>> {
        let mut keys = vec![self.load_key()?];
        keys.extend(self.archived_keys()?);
        Ok(keys)
    }
}

fn write_key_record(path: &Path, key: &KeyMaterial) -> PrivacyResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?;
        harden_private_directory(parent)?;
    }
    let json = serde_json::to_string_pretty(&KeyRecord::from(key.clone()))
        .map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))?;
    write_private_file(path, json.as_bytes())
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
    // Best-effort parent-directory fsync for crash durability on Unix
    // filesystems that support it. POSIX rename is atomic and visible to
    // other processes once it returns; this fsync exists so the new dirent
    // survives an unclean shutdown, not for cross-process visibility.
    // Filesystems that don't support directory fsync (some non-Unix targets,
    // tmpfs variants) are tolerated — they would otherwise report write-failed
    // after the rename has already succeeded, leaving callers with a
    // half-rotated state on disk that the error message wouldn't predict.
    sync_parent_dir_best_effort(path);
    validate_private_file(path)
}

fn sync_parent_dir_best_effort(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    if parent.as_os_str().is_empty() {
        return;
    }
    if let Ok(dir) = fs::File::open(parent) {
        // Ignore errors: some filesystems return Unsupported/InvalidInput on
        // directory fsync, and we'd rather under-fsync than report a spurious
        // failure on a write that already succeeded.
        let _ = dir.sync_all();
    }
}

fn unix_nanos() -> PrivacyResult<u128> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|err| PrivacyError::KeyUnavailable(err.to_string()))
}

fn is_missing_key_error(error: &PrivacyError) -> bool {
    matches!(error, PrivacyError::KeyUnavailable(message) if message.contains("No such file") || message.contains("os error 2"))
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
