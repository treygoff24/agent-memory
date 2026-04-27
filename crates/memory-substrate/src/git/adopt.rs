//! Clone adoption.

use std::path::Path;

use crate::error::GitError;
use crate::git::init::configure_merge_driver;
use crate::tree::bootstrap_repo_tree;

/// Error specific to `adopt_clone`.
#[derive(Debug, thiserror::Error)]
pub enum AdoptError {
    /// Merge driver path is required but was not supplied.
    #[error("adopt_clone requires an explicit merge driver binary path; ambient PATH is not acceptable")]
    MergeDriverPathRequired,
    /// IO error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Git error.
    #[error(transparent)]
    Git(#[from] GitError),
}

impl From<AdoptError> for std::io::Error {
    fn from(err: AdoptError) -> Self {
        std::io::Error::other(err.to_string())
    }
}

/// Adopt a clone: recreate local-only directories, configure the merge driver,
/// and mint a device identity if one does not exist.
///
/// Per Q4 `adopt_clone` is the **sole** authority for minting `local-device.yaml`.
/// `Substrate::open` fails with `OpenError::DeviceIdentityMissing` when absent.
/// Deferred: expose `adopt_clone_with_driver` taking an explicit path (deferred).
pub fn adopt_clone(repo: &Path, runtime: &Path) -> Result<(), GitError> {
    let merge_driver_binary = resolve_merge_driver_binary();
    adopt_clone_impl(repo, runtime, &merge_driver_binary, None, false).map_err(|err| GitError::CommandFailed {
        program: "adopt_clone".to_string(),
        args: Vec::new(),
        stderr: err.to_string(),
    })
}

/// Adopt a clone with explicit merge driver path and device control.
///
/// Takes the merge driver binary as an explicit parameter (spec §13.1 footnote:
/// ambient PATH is not acceptable for unattended merges). Surface
/// `AdoptError::MergeDriverPathRequired` if the path is empty.
#[allow(clippy::too_many_arguments)]
pub fn adopt_clone_explicit(
    repo: &Path,
    runtime: &Path,
    merge_driver_binary: &Path,
    device_id: Option<String>,
    force_new_device: bool,
) -> Result<(), AdoptError> {
    if merge_driver_binary.as_os_str().is_empty() {
        return Err(AdoptError::MergeDriverPathRequired);
    }
    adopt_clone_impl(repo, runtime, merge_driver_binary, device_id, force_new_device)
}

#[allow(clippy::too_many_arguments)]
fn adopt_clone_impl(
    repo: &Path,
    runtime: &Path,
    merge_driver_binary: &Path,
    device_id: Option<String>,
    force_new_device: bool,
) -> Result<(), AdoptError> {
    bootstrap_repo_tree(repo)?;

    if repo.join(".git").exists() {
        configure_merge_driver(repo, merge_driver_binary)?;
    }

    std::fs::create_dir_all(runtime.join("pending"))?;
    mint_device_identity(runtime, device_id, force_new_device)?;

    Ok(())
}

/// Resolve the merge driver binary path.
///
/// Deferred: replace with explicit caller-supplied path; `current_exe()` is a
/// placeholder to keep existing `adopt_clone` call sites compiling.
fn resolve_merge_driver_binary() -> std::path::PathBuf {
    #[allow(clippy::disallowed_methods)]
    std::env::current_exe().unwrap_or_else(|_| which::which("memory-merge-driver").unwrap_or_default())
}

/// Write `local-device.yaml` atomically under `runtime`.
///
/// Skips when the file already exists and `force_new_device` is false.
fn mint_device_identity(runtime: &Path, device_id: Option<String>, force_new_device: bool) -> std::io::Result<()> {
    let device_file = runtime.join("local-device.yaml");
    if device_file.exists() && !force_new_device {
        return Ok(());
    }

    let id = device_id.unwrap_or_else(|| {
        let raw = uuid::Uuid::new_v4().simple().to_string();
        format!("dev_{raw}")
    });

    let shard = id.get(4..12).unwrap_or("00000000");
    let yaml = format!(
        "schema_version: 1\ndevice:\n  id: {id}\n  name: {id}\n  shard: {shard}\npaths:\n  memory_root: {}\n  runtime_root: {}\n",
        runtime.parent().map(|p| p.to_string_lossy()).unwrap_or_default(),
        runtime.to_string_lossy()
    );

    let dir = device_file.parent().ok_or_else(|| std::io::Error::other("local-device.yaml has no parent"))?;
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut tmp, yaml.as_bytes())?;
    std::io::Write::flush(&mut tmp)?;
    tmp.persist(&device_file).map_err(|err| err.error)?;
    Ok(())
}
