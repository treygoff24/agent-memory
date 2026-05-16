use std::fs;
use std::path::Path;
use std::process::Command;

use chrono::{DateTime, TimeZone, Utc};
use memory_substrate::events::{append_event, read_events, Event, EventKind, EVENT_SCHEMA_VERSION};
use memory_substrate::{
    ClassificationOutcome, DeviceId, EventId, InitOptions, MemoryId, OperationId, RepoPath, Roots, Substrate,
};
use memoryd::dream::cleanup::{run_cleanup, CleanupConfig};

#[tokio::test]
async fn archive_failpoint_before_rename_leaves_source_intact_and_archive_absent() {
    let fixture = Fixture::new().await;
    seed_events(&fixture);
    fixture.commit_baseline("baseline events");
    fixture.set_failpoint("before_archive_rename");

    let error = run_cleanup(&fixture.substrate, config()).await.expect_err("failpoint should abort cleanup");
    assert!(error.to_string().contains("before_archive_rename"), "unexpected error: {error}");

    assert!(!fixture.archive_path().exists(), "archive should not appear before rename succeeds");
    assert_event_ids(&fixture.live_events(), &["evt_new", "evt_old"]);

    fixture.clear_failpoint();
    run_cleanup(&fixture.substrate, config()).await.expect("rerun cleanup");
    assert_archive_event_ids(&fixture.archive_path(), &["evt_old"]);
    assert_event_ids(&fixture.live_events(), &["evt_new"]);
}

#[tokio::test]
async fn archive_failpoint_after_rename_is_idempotently_pruned_on_rerun() {
    let fixture = Fixture::new().await;
    seed_events(&fixture);
    fixture.commit_baseline("baseline events");
    fixture.set_failpoint("after_archive_rename");

    let error = run_cleanup(&fixture.substrate, config()).await.expect_err("failpoint should abort cleanup");
    assert!(error.to_string().contains("after_archive_rename"), "unexpected error: {error}");

    assert_archive_event_ids(&fixture.archive_path(), &["evt_old"]);
    assert_event_ids(&fixture.live_events(), &["evt_new", "evt_old"]);

    fixture.clear_failpoint();
    run_cleanup(&fixture.substrate, config()).await.expect("rerun cleanup");
    assert_archive_event_ids(&fixture.archive_path(), &["evt_old"]);
    assert_event_ids(&fixture.live_events(), &["evt_new"]);
}

struct Fixture {
    roots: Roots,
    substrate: Substrate,
    _temp: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate = Substrate::init(
            roots.clone(),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_cleanup".to_string()) },
        )
        .await
        .expect("init substrate");
        Self { roots, substrate, _temp: temp }
    }

    fn log_path(&self) -> std::path::PathBuf {
        self.roots.repo.join("events/dev_cleanup.jsonl")
    }

    fn archive_path(&self) -> std::path::PathBuf {
        self.roots.repo.join("events/archive/2025-12.jsonl.zst")
    }

    fn live_events(&self) -> Vec<Event> {
        read_events(&self.log_path()).expect("read live events")
    }

    fn set_failpoint(&self, value: &str) {
        fs::write(self.roots.repo.join(".memorum/cleanup-failpoint"), value).expect("write failpoint");
    }

    fn clear_failpoint(&self) {
        fs::remove_file(self.roots.repo.join(".memorum/cleanup-failpoint")).expect("remove failpoint");
    }

    fn commit_baseline(&self, message: &str) {
        git(&self.roots.repo, &["add", "-A"]).expect("git add");
        let _ = git_with_test_identity(&self.roots.repo, &["commit", "-m", message]);
    }
}

fn seed_events(fixture: &Fixture) {
    append_event(&fixture.log_path(), &event("evt_old", "op_old", ymd(2025, 12, 15))).expect("old event");
    append_event(&fixture.log_path(), &event("evt_new", "op_new", ymd(2026, 4, 29))).expect("new event");
}

fn config() -> CleanupConfig {
    CleanupConfig {
        device_id: "dev_cleanup".to_string(),
        now: ymd(2026, 4, 30),
        fragment_lifetime_days: 14,
        candidate_stale_days: 30,
        event_compaction_days: 90,
    }
}

fn event(id: &str, op: &str, at: DateTime<Utc>) -> Event {
    Event {
        schema: EVENT_SCHEMA_VERSION,
        id: EventId::new(id),
        at,
        device: DeviceId::new("dev_cleanup"),
        seq: 1,
        operation_id: Some(OperationId::new(op)),
        kind: EventKind::WriteCommitted {
            id: MemoryId::new("mem_20260401_a1b2c3d4e5f60718_000999"),
            path: RepoPath::new("agent/patterns/mem_20260401_a1b2c3d4e5f60718_000999.md"),
            classification: ClassificationOutcome::Trusted,
        },
        crc32c: 0,
    }
}

fn ymd(year: i32, month: u32, day: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, 12, 0, 0).single().expect("valid date")
}

fn assert_event_ids(events: &[Event], expected: &[&str]) {
    let actual = events.iter().map(|event| event.id.as_str()).collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

fn assert_archive_event_ids(path: &Path, expected: &[&str]) {
    let archive = fs::File::open(path).expect("archive exists");
    let text = String::from_utf8(zstd::stream::decode_all(archive).expect("decode zstd")).expect("utf8 archive");
    let events = text
        .lines()
        .map(|line| serde_json::from_str::<Event>(line).expect("archive event is JSON"))
        .collect::<Vec<_>>();
    assert_event_ids(&events, expected);
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git").args(args).current_dir(repo).output().map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn git_with_test_identity(repo: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command
        .args(["-c", "user.name=Memorum Test", "-c", "user.email=memorum-test@example.invalid"])
        .args(args)
        .current_dir(repo);
    let output = command.output().map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}
