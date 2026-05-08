use clap::Parser;
use memory_substrate::{events::EventKind, InitOptions, Roots, Substrate};
use memoryd::cli::{Cli, Command};

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
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots,
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_doctorreindex".to_string()) },
    )
    .await
    .expect("init");

    substrate
        .record_event_best_effort(EventKind::OperatorRepairRequired { reason: "test repair".to_string() })
        .expect("record event");

    let rebuilt = substrate.doctor_reindex_events_log().expect("reindex");
    let health = substrate.events_log_mirror_health().expect("health");

    assert!(rebuilt >= 1);
    assert_eq!(health.lag, 0);
    assert_eq!(health.missing_count, 0);
}
