use std::path::{Path, PathBuf};

/// Canonical default repo: `$MEMORUM_REPO` → `~/memorum` → `./memorum`.
///
/// Shared by `init`, `doctor`, and `status` default resolution.
pub(crate) fn default_repo_root() -> PathBuf {
    std::env::var("MEMORUM_REPO")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join("memorum")))
        .unwrap_or_else(|| PathBuf::from("./memorum"))
}

/// Resolve repo and runtime using the same rules as `memoryd init`.
pub(crate) fn resolve_repo_runtime_paths(repo: Option<PathBuf>, runtime: Option<PathBuf>) -> (PathBuf, PathBuf) {
    let repo = repo.unwrap_or_else(default_repo_root);
    let runtime = runtime.unwrap_or_else(|| repo.join(".memoryd"));
    (repo, runtime)
}

/// Default daemon socket for bare client commands.
///
/// `$MEMORUM_RUNTIME` remains an escape hatch for non-standard daemon
/// placements; otherwise clients follow the same repo-aligned runtime default
/// used by standard daemon setup.
pub(crate) fn default_socket() -> PathBuf {
    if let Some(runtime) = std::env::var_os("MEMORUM_RUNTIME") {
        return crate::socket::resolve_socket_path(&PathBuf::from(runtime));
    }
    let (_, runtime) = resolve_repo_runtime_paths(None, None);
    crate::socket::resolve_socket_path(&runtime)
}

pub(crate) fn resolve_socket_arg(socket: &Option<PathBuf>) -> PathBuf {
    socket.clone().unwrap_or_else(default_socket)
}

pub(crate) fn resolve_socket_with_runtime(socket: &Option<PathBuf>, runtime: &std::path::Path) -> PathBuf {
    socket.clone().unwrap_or_else(|| crate::socket::resolve_socket_path(runtime))
}

/// Device-local Gemini API key path. This lives under runtime state, never the
/// synced memory repo config.
pub(crate) fn gemini_api_key_path(runtime_root: &Path) -> PathBuf {
    runtime_root.join("gemini_api_key")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn cli_default_repo_root_honors_memorum_repo_env() {
        let previous = std::env::var_os("MEMORUM_REPO");
        std::env::set_var("MEMORUM_REPO", "/tmp/env-repo");

        assert_eq!(default_repo_root(), PathBuf::from("/tmp/env-repo"));

        match previous {
            Some(value) => std::env::set_var("MEMORUM_REPO", value),
            None => std::env::remove_var("MEMORUM_REPO"),
        }
    }

    #[test]
    #[serial]
    fn cli_resolve_repo_runtime_paths_defaults_runtime_under_repo() {
        let previous = std::env::var_os("MEMORUM_REPO");
        std::env::set_var("MEMORUM_REPO", "/tmp/env-repo");

        let (repo, runtime) = resolve_repo_runtime_paths(None, None);
        assert_eq!(repo, PathBuf::from("/tmp/env-repo"));
        assert_eq!(runtime, PathBuf::from("/tmp/env-repo/.memoryd"));

        match previous {
            Some(value) => std::env::set_var("MEMORUM_REPO", value),
            None => std::env::remove_var("MEMORUM_REPO"),
        }
    }

    #[test]
    #[serial]
    fn cli_default_socket_memorum_runtime_wins() {
        let previous_runtime = std::env::var_os("MEMORUM_RUNTIME");
        let previous_repo = std::env::var_os("MEMORUM_REPO");
        std::env::set_var("MEMORUM_RUNTIME", "/tmp/env-runtime");
        std::env::set_var("MEMORUM_REPO", "/tmp/env-repo");

        assert_eq!(default_socket(), PathBuf::from("/tmp/env-runtime/memoryd.sock"));

        match previous_runtime {
            Some(value) => std::env::set_var("MEMORUM_RUNTIME", value),
            None => std::env::remove_var("MEMORUM_RUNTIME"),
        }
        match previous_repo {
            Some(value) => std::env::set_var("MEMORUM_REPO", value),
            None => std::env::remove_var("MEMORUM_REPO"),
        }
    }

    #[test]
    #[serial]
    fn cli_default_socket_uses_init_aligned_runtime() {
        let previous_runtime = std::env::var_os("MEMORUM_RUNTIME");
        let previous_repo = std::env::var_os("MEMORUM_REPO");
        std::env::remove_var("MEMORUM_RUNTIME");
        std::env::set_var("MEMORUM_REPO", "/tmp/env-repo");

        assert_eq!(default_socket(), PathBuf::from("/tmp/env-repo/.memoryd/memoryd.sock"));

        match previous_runtime {
            Some(value) => std::env::set_var("MEMORUM_RUNTIME", value),
            None => std::env::remove_var("MEMORUM_RUNTIME"),
        }
        match previous_repo {
            Some(value) => std::env::set_var("MEMORUM_REPO", value),
            None => std::env::remove_var("MEMORUM_REPO"),
        }
    }

    #[test]
    #[serial]
    fn cli_resolve_socket_arg_defaults_to_repo_aligned_socket() {
        let previous_runtime = std::env::var_os("MEMORUM_RUNTIME");
        let previous_repo = std::env::var_os("MEMORUM_REPO");
        std::env::remove_var("MEMORUM_RUNTIME");
        std::env::set_var("MEMORUM_REPO", "/tmp/env-repo");

        assert_eq!(resolve_socket_arg(&None), PathBuf::from("/tmp/env-repo/.memoryd/memoryd.sock"));

        match previous_runtime {
            Some(value) => std::env::set_var("MEMORUM_RUNTIME", value),
            None => std::env::remove_var("MEMORUM_RUNTIME"),
        }
        match previous_repo {
            Some(value) => std::env::set_var("MEMORUM_REPO", value),
            None => std::env::remove_var("MEMORUM_REPO"),
        }
    }

    #[test]
    #[serial]
    fn cli_resolve_socket_arg_honors_explicit_override() {
        let explicit = PathBuf::from("/tmp/explicit.sock");
        assert_eq!(resolve_socket_arg(&Some(explicit.clone())), explicit);
    }
}
