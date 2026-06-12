use std::path::PathBuf;

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

/// Default daemon socket for bare `memoryd status`: `<resolved repo>/.memoryd/memoryd.sock`.
pub(crate) fn default_status_socket() -> PathBuf {
    let (_, runtime) = resolve_repo_runtime_paths(None, None);
    crate::socket::resolve_socket_path(&runtime)
}

pub(crate) fn resolve_socket_arg(socket: &Option<PathBuf>) -> PathBuf {
    socket.clone().unwrap_or_else(|| crate::socket::resolve_socket_path(&crate::socket::default_runtime_root()))
}

pub(crate) fn resolve_status_socket_arg(socket: &Option<PathBuf>) -> PathBuf {
    socket.clone().unwrap_or_else(default_status_socket)
}

pub(crate) fn resolve_socket_with_runtime(socket: &Option<PathBuf>, runtime: &std::path::Path) -> PathBuf {
    socket.clone().unwrap_or_else(|| crate::socket::resolve_socket_path(runtime))
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
    fn cli_default_status_socket_uses_init_aligned_runtime() {
        let previous = std::env::var_os("MEMORUM_REPO");
        std::env::set_var("MEMORUM_REPO", "/tmp/env-repo");

        assert_eq!(
            default_status_socket(),
            PathBuf::from("/tmp/env-repo/.memoryd/memoryd.sock")
        );

        match previous {
            Some(value) => std::env::set_var("MEMORUM_REPO", value),
            None => std::env::remove_var("MEMORUM_REPO"),
        }
    }

    #[test]
    #[serial]
    fn cli_resolve_status_socket_arg_honors_explicit_override() {
        let explicit = PathBuf::from("/tmp/explicit.sock");
        assert_eq!(
            resolve_status_socket_arg(&Some(explicit.clone())),
            explicit
        );
    }
}
