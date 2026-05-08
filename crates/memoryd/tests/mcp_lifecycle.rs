use memoryd::socket::{probe_live_socket, resolve_socket_path, SocketProbe};

#[test]
fn task4_socket_stub_resolves_runtime_socket_and_absent_probe() {
    let temp = tempfile::tempdir().expect("tempdir");
    let socket = resolve_socket_path(temp.path());

    assert_eq!(socket, temp.path().join("memoryd.sock"));
    assert_eq!(probe_live_socket(&socket), SocketProbe::Absent);
}

#[test]
fn task4_socket_probe_marks_existing_non_socket_path_stale() {
    let temp = tempfile::tempdir().expect("tempdir");
    let socket = resolve_socket_path(temp.path());
    std::fs::write(&socket, b"stale").expect("stale socket marker");

    assert_eq!(probe_live_socket(&socket), SocketProbe::Stale);
}
