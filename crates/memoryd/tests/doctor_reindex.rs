use clap::Parser;
use memory_substrate::{events::EventKind, InitOptions, Roots, Substrate};
use memoryd::cli::{Cli, Command};
use rusqlite::Connection;
use std::process::Command as ProcessCommand;

#[test]
fn doctor_reindex_flag_is_explicit_cli_surface() {
    let cli = Cli::parse_from([
        "memoryd",
        "doctor",
        "--repo",
        "/tmp/memorum-repo",
        "--runtime",
        "/tmp/memorum-runtime",
        "--reindex",
    ]);

    let Command::Doctor(args) = cli.command else {
        panic!("expected doctor command");
    };
    assert!(args.reindex);
}

#[tokio::test]
async fn doctor_reindex_rebuilds_from_canonical_event_logs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let roots = Roots::new(repo.clone(), runtime.clone());
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_doctorreindex".to_string()) },
    )
    .await
    .expect("init");

    substrate
        .record_event_best_effort(EventKind::OperatorRepairRequired { reason: "test repair".to_string() })
        .expect("record event");

    delete_events_log_mirror_rows(&runtime);
    let stale = substrate.events_log_mirror_health().expect("stale health");
    assert!(stale.lag > 0 || stale.missing_count > 0, "fixture must start with mirror drift: {stale:?}");
    drop(substrate);

    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_memoryd"))
        .args(["doctor", "--repo"])
        .arg(&repo)
        .args(["--runtime"])
        .arg(&runtime)
        .arg("--reindex")
        .output()
        .expect("run memoryd doctor --reindex");
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "doctor command should complete even if harness warnings make it unhealthy: status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("doctor reindexed 1 canonical event log entries"),
        "doctor --reindex should report repaired event count, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let reopened = Substrate::open(roots).await.expect("reopen repaired substrate");
    let health = reopened.events_log_mirror_health().expect("health");
    assert_eq!(health.lag, 0);
    assert_eq!(health.missing_count, 0);
}

fn delete_events_log_mirror_rows(runtime: &std::path::Path) {
    let conn = Connection::open(runtime.join("index.sqlite")).expect("open sqlite mirror");
    let deleted = conn.execute("DELETE FROM events_log", []).expect("delete mirrored events");
    assert!(deleted >= 1, "fixture should delete at least one mirrored event row");
}
