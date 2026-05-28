use std::path::PathBuf;

pub(crate) fn resolve_socket_arg(socket: &Option<PathBuf>) -> PathBuf {
    socket.clone().unwrap_or_else(|| crate::socket::resolve_socket_path(&crate::socket::default_runtime_root()))
}

pub(crate) fn resolve_socket_with_runtime(socket: &Option<PathBuf>, runtime: &std::path::Path) -> PathBuf {
    socket.clone().unwrap_or_else(|| crate::socket::resolve_socket_path(runtime))
}
