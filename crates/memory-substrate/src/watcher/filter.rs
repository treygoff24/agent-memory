//! Watch path filters per spec §11.2.

use std::path::Path;

/// Return true for memory Markdown paths.
pub fn is_memory_path(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "md") && !path.components().any(|part| part.as_os_str() == ".git")
}

/// Return true when the path should be forwarded to subscribers.
///
/// Excludes `.git/` internals, `.DS_Store`, editor backup files, and
/// atomic-write temp files that Stream A creates during writes (spec §11.2).
pub fn should_watch(path: &Path) -> bool {
    !is_git_internal(path) && !is_ds_store(path) && !is_editor_backup(path) && !is_atomic_temp(path)
}

fn is_git_internal(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == ".git")
}

fn is_ds_store(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == ".DS_Store")
}

fn is_editor_backup(path: &Path) -> bool {
    path.file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|n| n.ends_with('~') || n.starts_with('#') || n.ends_with(".swp"))
}

fn is_atomic_temp(path: &Path) -> bool {
    // Matches the pattern from markdown/atomic.rs: `.<basename>.<op_id>.tmp`
    path.file_name().and_then(|s| s.to_str()).is_some_and(|n| n.starts_with('.') && n.ends_with(".tmp"))
}
