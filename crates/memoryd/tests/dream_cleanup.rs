use std::fs;
use std::path::Path;
use std::process::Command;

use chrono::{DateTime, Duration, TimeZone, Utc};
use memory_substrate::events::{append_event, read_events, Event, EventKind, EVENT_SCHEMA_VERSION};
use memory_substrate::frontmatter::serialize_document;
use memory_substrate::markdown::read_memory_file;
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, DeviceId, EventContext, EventId, Evidence, Frontmatter, InitOptions,
    Memory, MemoryId, MemoryStatus, MemoryType, ObserveKind, OperationId, RepoPath, RetrievalPolicy, Roots, Scope,
    Sensitivity, Source, SourceKind, Substrate, SubstrateFragmentAppendRequest, SubstrateFragmentPayload,
    TombstoneActor, TombstoneActorKind, TombstoneEvent, TombstoneKind, TrustLevel, WriteMode, WritePolicy,
    WriteRequest,
};
use memoryd::dream::cleanup::{run_cleanup, CleanupConfig};
use serde_json::Value;

#[tokio::test]
async fn expired_substrate_archival_is_idempotent() {
    let fixture = Fixture::new("dev_cleanup").await;
    append_fragment(&fixture.substrate, "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1A", days_ago(30)).await;

    let first = run_cleanup(&fixture.substrate, config("dev_cleanup")).await.expect("first cleanup");
    let second = run_cleanup(&fixture.substrate, config("dev_cleanup")).await.expect("second cleanup");

    assert_eq!(first.operations.fragments_archived, 1);
    assert_eq!(second.operations.fragments_archived, 0);
    let archived = read_jsonl(&fixture.repo_path("substrate/archive/dev_cleanup/2026-03.jsonl"));
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0]["id"], "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1A");
    assert!(read_jsonl(&fixture.repo_path("substrate/dev_cleanup/2026-03-31.jsonl")).is_empty());
}

#[tokio::test]
async fn stale_candidate_archival_mutates_frontmatter_without_deleting_body() {
    let fixture = Fixture::new("dev_cleanup").await;
    let mut candidate = sample_memory("mem_20260401_a1b2c3d4e5f60718_000001", MemoryStatus::Candidate);
    candidate.frontmatter.created_at = days_ago(60);
    candidate.frontmatter.updated_at = days_ago(60);
    candidate.frontmatter.trust_level = TrustLevel::Candidate;
    candidate.frontmatter.review_state = Some("candidate".to_string());
    candidate.body = "candidate body must remain".to_string();
    write_memory(&fixture.substrate, candidate.clone()).await;

    let report = run_cleanup(&fixture.substrate, config("dev_cleanup")).await.expect("cleanup");
    let (saved, _) = read_memory_file(&fixture.roots.repo, candidate.path.as_ref().expect("candidate path"))
        .expect("read archived candidate file");
    let events = read_events(&fixture.repo_path("events/dev_cleanup.jsonl")).expect("read event log");
    let write_events = events
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                EventKind::WriteCommitted { id, path, classification: ClassificationOutcome::Trusted }
                    if id == &candidate.frontmatter.id && path.as_str() == candidate.path.as_ref().unwrap().as_str()
            )
        })
        .count();

    assert_eq!(report.operations.candidates_archived, 1);
    assert_eq!(saved.frontmatter.status, MemoryStatus::Archived);
    assert_eq!(saved.body, "candidate body must remain");
    assert_eq!(write_events, 2, "initial write plus cleanup archive mutation should both emit substrate events");
}

#[tokio::test]
async fn cleanup_records_per_file_findings_and_continues_candidate_archival() {
    let fixture = Fixture::new("dev_cleanup").await;
    let mut candidate = sample_memory("mem_20260401_a1b2c3d4e5f60718_000006", MemoryStatus::Candidate);
    candidate.frontmatter.created_at = days_ago(60);
    candidate.frontmatter.updated_at = days_ago(60);
    candidate.frontmatter.trust_level = TrustLevel::Candidate;
    candidate.frontmatter.review_state = Some("candidate".to_string());
    write_memory(&fixture.substrate, candidate.clone()).await;
    fs::write(fixture.repo_path("agent/patterns/corrupt.md"), "---\nschema_version: 999\n---\nbody")
        .expect("write corrupt memory");

    let report = run_cleanup(&fixture.substrate, config("dev_cleanup")).await.expect("cleanup continues");
    let (saved, _) = read_memory_file(&fixture.roots.repo, candidate.path.as_ref().expect("candidate path"))
        .expect("read archived candidate file");

    assert_eq!(report.operations.candidates_archived, 1);
    assert_eq!(saved.frontmatter.status, MemoryStatus::Archived);
    assert!(report
        .findings
        .iter()
        .any(|finding| { finding.kind == "memory_lint" && finding.path == "agent/patterns/corrupt.md" }));
}

#[tokio::test]
async fn entity_index_rebuild_reports_noop_when_projection_matches() {
    let fixture = Fixture::new("dev_cleanup").await;
    let memory = sample_memory("mem_20260401_a1b2c3d4e5f60718_000002", MemoryStatus::Active);
    write_memory(&fixture.substrate, memory).await;

    let report = run_cleanup(&fixture.substrate, config("dev_cleanup")).await.expect("cleanup");

    assert!(!report.operations.entity_index_rebuilt);
    assert_eq!(report.operations.entity_index_rows, 1);
}

#[tokio::test]
async fn lint_tombstone_and_supersession_findings_are_reported_without_auto_repair() {
    let fixture = Fixture::new("dev_cleanup").await;
    let mut tombstoned = sample_memory("mem_20260401_a1b2c3d4e5f60718_000003", MemoryStatus::Active);
    tombstoned.frontmatter.tombstone_events.push(tombstone_event());
    write_memory(&fixture.substrate, tombstoned.clone()).await;
    let mut dangling = sample_memory("mem_20260401_a1b2c3d4e5f60718_000004", MemoryStatus::Superseded);
    dangling.frontmatter.superseded_by = vec![MemoryId::new("mem_20260401_a1b2c3d4e5f60718_999999")];
    write_memory_file(&fixture, &dangling);
    fs::write(fixture.repo_path("agent/patterns/malformed.md"), "---\nschema_version: 999\n---\nbody")
        .expect("write malformed memory");

    let report = run_cleanup(&fixture.substrate, config("dev_cleanup")).await.expect("cleanup");
    let kinds = report.findings.iter().map(|finding| finding.kind.as_str()).collect::<Vec<_>>();

    assert!(kinds.contains(&"memory_lint"));
    assert!(kinds.contains(&"tombstone_integrity"));
    assert!(kinds.contains(&"supersession_orphan"));
    assert_eq!(
        fixture
            .substrate
            .read_memory(&tombstoned.frontmatter.id)
            .await
            .expect("tombstone still readable")
            .frontmatter
            .tombstone_events
            .len(),
        1
    );
    assert!(fixture.repo_path("agent/patterns/malformed.md").exists());
}

#[tokio::test]
async fn observed_at_refresh_is_deterministic_from_live_source_mtime() {
    let fixture = Fixture::new("dev_cleanup").await;
    let source = fixture.repo_path("docs/source.txt");
    fs::create_dir_all(source.parent().expect("parent")).expect("source parent");
    fs::write(&source, "source fact").expect("source");
    let source_mtime = DateTime::<Utc>::from(fs::metadata(&source).expect("metadata").modified().expect("mtime"));
    let mut memory = sample_memory("mem_20260401_a1b2c3d4e5f60718_000005", MemoryStatus::Active);
    memory.frontmatter.source.kind = SourceKind::File;
    memory.frontmatter.source.reference = Some("docs/source.txt".to_string());
    write_memory(&fixture.substrate, memory.clone()).await;

    let report = run_cleanup(&fixture.substrate, config("dev_cleanup")).await.expect("cleanup");
    let saved = fixture.substrate.read_memory(&memory.frontmatter.id).await.expect("read refreshed");

    assert_eq!(report.operations.observed_at_refreshed, 1);
    assert_eq!(saved.frontmatter.extras["observed_at"], serde_json::json!(source_mtime.to_rfc3339()));
}

#[tokio::test]
async fn observed_at_refresh_skips_absolute_traversal_and_symlink_source_refs() {
    let fixture = Fixture::new("dev_cleanup").await;
    let outside = fixture._temp.path().join("outside-source.txt");
    fs::write(&outside, "outside").expect("outside source");
    let link = fixture.repo_path("docs/outside-link.txt");
    fs::create_dir_all(link.parent().expect("link parent")).expect("link parent");
    std::os::unix::fs::symlink(&outside, &link).expect("outside symlink");

    for (index, source_ref) in [
        outside.display().to_string(),
        "file:/etc/passwd".to_string(),
        "../outside-source.txt".to_string(),
        "docs/outside-link.txt".to_string(),
    ]
    .into_iter()
    .enumerate()
    {
        let mut memory =
            sample_memory(&format!("mem_20260401_a1b2c3d4e5f60718_{:06}", 200000 + index), MemoryStatus::Active);
        memory.frontmatter.source.kind = SourceKind::File;
        memory.frontmatter.source.reference = Some(source_ref);
        write_memory_file(&fixture, &memory);
    }

    let report = run_cleanup(&fixture.substrate, config("dev_cleanup")).await.expect("cleanup");

    assert_eq!(report.operations.observed_at_refreshed, 0);
}

#[tokio::test]
async fn event_compaction_writes_monthly_zstd_archive_and_keeps_live_tail() {
    let fixture = Fixture::new("dev_cleanup").await;
    let log = fixture.repo_path("events/dev_cleanup.jsonl");
    fixture.commit_baseline("baseline clean fixture");
    append_event(&log, &event("evt_old", "op_old", ymd(2025, 12, 15))).expect("old event");
    append_event(&log, &event("evt_new", "op_new", ymd(2026, 4, 29))).expect("new event");

    let report = run_cleanup(&fixture.substrate, config("dev_cleanup")).await.expect("cleanup");
    let archive = fixture.repo_path("events/archive/2025-12.jsonl.zst");
    let archived_text =
        String::from_utf8(zstd::stream::decode_all(fs::File::open(archive).expect("archive")).expect("decode zstd"))
            .expect("utf8 archive");
    let live = read_events(&log).expect("read compacted live log");

    assert_eq!(report.operations.events_compacted, 1);
    assert!(archived_text.contains("\"id\":\"evt_old\""));
    assert!(live.iter().all(|event| event.id.as_str() != "evt_old"));
    assert!(live.iter().any(|event| event.id.as_str() == "evt_new"));
}

#[tokio::test]
async fn cleanup_report_json_shape_is_stable() {
    let fixture = Fixture::new("dev_cleanup").await;
    fixture.commit_baseline("baseline clean fixture");

    let report = run_cleanup(&fixture.substrate, config("dev_cleanup")).await.expect("cleanup");
    let json: Value = serde_json::from_str(
        &fs::read_to_string(fixture.repo_path("dreams/cleanup/dev_cleanup/2026-04-30.json")).unwrap(),
    )
    .expect("report json");

    assert_eq!(report.device_id, "dev_cleanup");
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["device_id"], "dev_cleanup");
    assert_eq!(json["date"], "2026-04-30");
    assert_eq!(json["operations"]["fragments_archived"], 0);
    assert_eq!(json["git"]["author"], "memoryd cleanup-bot <noreply@memoryd.local>");
    assert_eq!(json["commit_deferred"], false);
    assert!(json["findings"].as_array().is_some());
}

#[tokio::test]
async fn cleanup_commit_uses_bot_author_message_and_only_cleanup_staged_files() {
    let fixture = Fixture::new("dev_cleanup").await;
    append_fragment(&fixture.substrate, "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1B", days_ago(30)).await;
    fixture.commit_baseline("baseline fragment fixture");

    let report = run_cleanup(&fixture.substrate, config("dev_cleanup")).await.expect("cleanup");
    let commit = git(&fixture.roots.repo, &["log", "-1", "--format=%an <%ae>%n%B"]).expect("git log");
    let names = git(&fixture.roots.repo, &["show", "--name-only", "--format=", "HEAD"]).expect("git show");

    assert!(!report.commit_deferred);
    assert!(commit.contains("memoryd cleanup-bot <noreply@memoryd.local>"));
    assert!(commit.contains("dream: cleanup dev_cleanup 2026-04-30"));
    assert!(commit.contains("fragments_archived=1"));
    assert!(names.lines().all(|path| {
        path.starts_with("substrate/")
            || path.starts_with("dreams/cleanup/")
            || path.starts_with("events/")
            || path.starts_with("agent/")
    }));
}

#[tokio::test]
async fn dirty_tree_writes_report_stages_cleanup_files_and_defers_commit() {
    let fixture = Fixture::new("dev_cleanup").await;
    append_fragment(&fixture.substrate, "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1C", days_ago(30)).await;
    fs::write(fixture.repo_path("user-notes.txt"), "do not stage").expect("dirty file");

    let report = run_cleanup(&fixture.substrate, config("dev_cleanup")).await.expect("cleanup");
    let staged = git(&fixture.roots.repo, &["diff", "--cached", "--name-only"]).expect("staged");
    let last_subject = git(&fixture.roots.repo, &["log", "-1", "--format=%s"]).expect("subject");
    let report_json: Value = serde_json::from_str(
        &fs::read_to_string(fixture.repo_path("dreams/cleanup/dev_cleanup/2026-04-30.json")).unwrap(),
    )
    .expect("report json");

    assert!(report.commit_deferred);
    assert_eq!(report_json["commit_deferred"], true);
    assert!(!last_subject.contains("dream: cleanup"));
    assert!(staged.contains("dreams/cleanup/dev_cleanup/2026-04-30.json"));
    assert!(!staged.contains("user-notes.txt"));
}

#[tokio::test]
async fn two_simulated_devices_cleanup_converges_regardless_of_order() {
    let first = DualDeviceFixture::new().await;
    run_cleanup(&first.dev_a, config("dev_a")).await.expect("a then b: a");
    run_cleanup(&first.dev_b, config("dev_b")).await.expect("a then b: b");
    let first_state = first.archive_and_reports();

    let second = DualDeviceFixture::new().await;
    run_cleanup(&second.dev_b, config("dev_b")).await.expect("b then a: b");
    run_cleanup(&second.dev_a, config("dev_a")).await.expect("b then a: a");
    let second_state = second.archive_and_reports();

    assert_eq!(first_state, second_state);
}

struct Fixture {
    roots: Roots,
    substrate: Substrate,
    _temp: tempfile::TempDir,
}

impl Fixture {
    async fn new(device_id: &str) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate = Substrate::init(
            roots.clone(),
            InitOptions { force_unsafe_durability: true, device_id: Some(device_id.to_string()) },
        )
        .await
        .expect("init substrate");
        Self { roots, substrate, _temp: temp }
    }

    fn repo_path(&self, path: &str) -> std::path::PathBuf {
        self.roots.repo.join(path)
    }

    fn commit_baseline(&self, message: &str) {
        git(&self.roots.repo, &["add", "-A"]).expect("baseline git add");
        let _ = git_with_test_identity(&self.roots.repo, &["commit", "-m", message]);
    }
}

struct DualDeviceFixture {
    roots: Roots,
    dev_a: Substrate,
    dev_b: Substrate,
    _temp: tempfile::TempDir,
}

impl DualDeviceFixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime-a"));
        let dev_a = Substrate::init(
            roots.clone(),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_a".to_string()) },
        )
        .await
        .expect("init dev a");
        let dev_b_roots = Roots::new(roots.repo.clone(), temp.path().join("runtime-b"));
        let dev_b = Substrate::init(
            dev_b_roots,
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_b".to_string()) },
        )
        .await
        .expect("init dev b");
        append_fragment(&dev_a, "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1D", days_ago(30)).await;
        append_fragment(&dev_b, "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1E", days_ago(30)).await;
        let fixture = Self { roots, dev_a, dev_b, _temp: temp };
        fixture.commit_baseline("baseline dual device fixture");
        fixture
    }

    fn archive_and_reports(&self) -> Vec<(String, String)> {
        let mut files = ["substrate/archive/dev_a/2026-03.jsonl", "substrate/archive/dev_b/2026-03.jsonl"]
            .into_iter()
            .map(|path| (path.to_string(), fs::read_to_string(self.roots.repo.join(path)).unwrap_or_default()))
            .collect::<Vec<_>>();
        files.extend(
            ["dreams/cleanup/dev_a/2026-04-30.json", "dreams/cleanup/dev_b/2026-04-30.json"]
                .into_iter()
                .map(|path| (path.to_string(), fs::read_to_string(self.roots.repo.join(path)).unwrap_or_default())),
        );
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    }

    fn commit_baseline(&self, message: &str) {
        git(&self.roots.repo, &["add", "-A"]).expect("baseline git add");
        let _ = git_with_test_identity(&self.roots.repo, &["commit", "-m", message]);
    }
}

fn config(device_id: &str) -> CleanupConfig {
    CleanupConfig {
        device_id: device_id.to_string(),
        now: fixed_now(),
        fragment_lifetime_days: 14,
        candidate_stale_days: 30,
        event_compaction_days: 90,
    }
}

async fn append_fragment(substrate: &Substrate, id: &str, at: DateTime<Utc>) {
    substrate
        .append_substrate_fragment(SubstrateFragmentAppendRequest {
            id: Some(id.to_string()),
            at,
            session: Some("sess_cleanup".to_string()),
            harness: Some("codex".to_string()),
            scope: "agent".to_string(),
            entities: vec!["ent_cleanup".to_string()],
            kind: ObserveKind::Observation,
            source_ref: None,
            privacy_spans: Vec::new(),
            payload: SubstrateFragmentPayload::Plaintext { text: "cleanup observation".to_string() },
            classification: ClassificationOutcome::Trusted,
            operation_id: None,
        })
        .await
        .expect("append substrate fragment");
}

async fn write_memory(substrate: &Substrate, memory: Memory) {
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write memory");
}

fn write_memory_file(fixture: &Fixture, memory: &Memory) {
    let path = fixture.repo_path(memory.path.as_ref().expect("path").as_str());
    fs::create_dir_all(path.parent().expect("parent")).expect("memory parent");
    fs::write(path, serialize_document(memory).expect("serialize memory")).expect("write memory file");
}

fn sample_memory(id: &str, status: MemoryStatus) -> Memory {
    let now = fixed_now();
    Memory {
        frontmatter: Frontmatter {
            schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "cleanup fixture".to_string(),
            confidence: 0.8,
            trust_level: if status == MemoryStatus::Candidate { TrustLevel::Candidate } else { TrustLevel::Trusted },
            sensitivity: Sensitivity::Internal,
            status,
            created_at: now,
            updated_at: now,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("dream-cleanup-test".to_string()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: Vec::new(),
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::Import,
                reference: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                device: None,
            },
            evidence: vec![Evidence {
                id: "ev_01HZXJK7J7W0X4Q4KJ7A2R8V1A".to_string(),
                quote: "cleanup fixture".to_string(),
                quote_norm_hash: None,
                reference: "fixture".to_string(),
                weight: 1.0,
                observed_at: None,
                source: None,
            }],
            requires_user_confirmation: false,
            review_state: None,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            related: Vec::new(),
            tombstone_events: Vec::new(),
            retrieval_policy: RetrievalPolicy {
                passive_recall: true,
                max_scope: Scope::Agent,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: true,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "cleanup-test".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: Default::default(),
        },
        body: "cleanup memory body".to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
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

fn tombstone_event() -> TombstoneEvent {
    TombstoneEvent {
        id: "tomb_01HZXJK7J7W0X4Q4KJ7A2R8V1A".to_string(),
        applied_at: fixed_now(),
        actor: TombstoneActor { kind: TombstoneActorKind::System, reference: "cleanup-test".to_string() },
        reason: TombstoneKind::Stale,
        reason_text: Some("stale".to_string()),
        reason_hash: None,
        prior_status: MemoryStatus::Active,
    }
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("jsonl object"))
        .collect()
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
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn fixed_now() -> DateTime<Utc> {
    ymd(2026, 4, 30)
}

fn days_ago(days: i64) -> DateTime<Utc> {
    fixed_now() - Duration::days(days)
}

fn ymd(year: i32, month: u32, day: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, 12, 0, 0).single().expect("valid date")
}
