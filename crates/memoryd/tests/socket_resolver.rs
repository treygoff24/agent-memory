use clap::Parser as _;
use memoryd::cli::{Cli, Command as CliCommand, PeerCommand, ReviewCommand, WebCommand};
use memoryd::socket::{default_runtime_root, probe_live_socket, resolve_socket_path, SocketProbe};
use std::sync::{Mutex, OnceLock};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[test]
fn private_socket_path_resolves_under_runtime_root() {
    let runtime = tempfile::tempdir().expect("runtime");

    assert_eq!(resolve_socket_path(runtime.path()), runtime.path().join("memoryd.sock"));
}

#[test]
fn default_runtime_root_honors_memorum_runtime_env() {
    let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().expect("env lock");
    let temp = tempfile::tempdir().expect("runtime");
    std::env::set_var("MEMORUM_RUNTIME", temp.path());

    assert_eq!(default_runtime_root(), temp.path());

    std::env::remove_var("MEMORUM_RUNTIME");
}

#[test]
fn probe_distinguishes_absent_and_stale_paths() {
    let runtime = tempfile::tempdir().expect("runtime");
    let socket = resolve_socket_path(runtime.path());

    assert_eq!(probe_live_socket(&socket), SocketProbe::Absent);
    std::fs::write(&socket, b"not a socket").expect("stale marker");
    assert_eq!(probe_live_socket(&socket), SocketProbe::Stale);
}

#[test]
fn cli_socket_args_default_to_none_for_canonical_resolution() {
    let cases: Vec<Vec<&str>> = vec![
        vec!["memoryd", "status"],
        vec!["memoryd", "search", "query"],
        vec!["memoryd", "get", "mem_1"],
        vec!["memoryd", "write-note", "note"],
        vec!["memoryd", "write", "body"],
        vec!["memoryd", "source", "capture", "--url", "https://example.com"],
        vec!["memoryd", "supersede", "mem_1", "body", "--reason", "updated"],
        vec!["memoryd", "forget", "mem_1", "--reason", "wrong"],
        vec!["memoryd", "review", "queue"],
        vec!["memoryd", "review", "approve", "mem_1"],
        vec!["memoryd", "review", "reject", "--reason", "bad", "mem_1"],
        vec!["memoryd", "peer", "status"],
        vec!["memoryd", "peer", "activity"],
        vec!["memoryd", "peer", "release-lock", "mem_1", "--yes"],
        vec!["memoryd", "ui"],
        vec!["memoryd", "web", "enable"],
        vec!["memoryd", "web", "disable"],
        vec!["memoryd", "web", "status"],
        vec!["memoryd", "reality-check", "run", "--json"],
        vec!["memoryd", "reality-check", "skip"],
        vec!["memoryd", "reality-check", "snooze"],
    ];

    for args in cases {
        assert_socket_defaults_to_none(
            Cli::try_parse_from(args.iter().copied()).unwrap_or_else(|err| panic!("{args:?}: {err}")),
        );
    }
}

#[test]
fn explicit_socket_override_still_parses() {
    let cli = Cli::try_parse_from(["memoryd", "status", "--socket", "/tmp/explicit.sock"]).expect("status parses");

    let CliCommand::Status(args) = cli.command else { panic!("expected status") };
    assert_eq!(args.socket.as_deref(), Some(std::path::Path::new("/tmp/explicit.sock")));
}

fn assert_socket_defaults_to_none(cli: Cli) {
    match cli.command {
        CliCommand::Status(args) => assert!(args.socket.is_none()),
        CliCommand::Search(args) => assert!(args.socket.is_none()),
        CliCommand::Get(args) => assert!(args.socket.is_none()),
        CliCommand::WriteNote(args) => assert!(args.socket.is_none()),
        CliCommand::Write(args) => assert!(args.socket.is_none()),
        CliCommand::Source(args) => match args.command {
            memoryd::cli::SourceCommand::Capture(args) => assert!(args.socket.is_none()),
        },
        CliCommand::Supersede(args) => assert!(args.socket.is_none()),
        CliCommand::Forget(args) => assert!(args.socket.is_none()),
        CliCommand::Review(args) => match args.command {
            ReviewCommand::Queue(args) => assert!(args.socket.is_none()),
            ReviewCommand::Approve(args) => assert!(args.socket.is_none()),
            ReviewCommand::Reject(args) => assert!(args.socket.is_none()),
        },
        CliCommand::Peer(args) => match args.command {
            PeerCommand::Status(args) => assert!(args.socket.is_none()),
            PeerCommand::Activity(args) => assert!(args.socket.is_none()),
            PeerCommand::ReleaseLock(args) => assert!(args.socket.is_none()),
        },
        CliCommand::Ui(args) => assert!(args.socket.is_none()),
        CliCommand::Web(args) => match args.command {
            WebCommand::Enable(args) => assert!(args.socket.is_none()),
            WebCommand::Disable(args) => assert!(args.socket.is_none()),
            WebCommand::Status(args) => assert!(args.socket.is_none()),
        },
        CliCommand::RealityCheck(args) => match args.command {
            memoryd::cli::RealityCheckCommand::Run(args) => assert!(args.socket.is_none()),
            memoryd::cli::RealityCheckCommand::Skip(args) => assert!(args.socket.is_none()),
            memoryd::cli::RealityCheckCommand::Snooze(args) => assert!(args.socket.is_none()),
        },
        other => panic!("unexpected command in socket default test: {other:?}"),
    }
}
