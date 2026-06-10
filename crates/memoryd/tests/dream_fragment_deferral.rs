//! Citation-aware substrate fragment archival deferral (Memory Dynamics spec
//! v0.1 §4). Exercises the cleanup layer end-to-end via `run_cleanup`:
//!
//! - a cited, expired, under-cap fragment is **deferred** (stays active);
//! - an uncited expired fragment **archives on schedule**;
//! - a fragment past the immortality cap **archives despite citations**;
//! - `dynamics.enabled: false` falls back to the substrate's hard cutoff
//!   (cited fragments archive normally) — the spec §7 gating rule.

use std::fs;
use std::path::Path;

use chrono::{DateTime, Duration, TimeZone, Utc};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Evidence, Frontmatter, InitOptions, Memory, MemoryId,
    MemoryStatus, MemoryType, ObserveKind, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind,
    Substrate, SubstrateFragmentAppendRequest, SubstrateFragmentPayload, TrustLevel, WriteMode, WritePolicy,
    WriteRequest,
};
use memoryd::dream::cleanup::{run_cleanup, CleanupConfig};
use serde_json::Value;

const DEVICE: &str = "dev_defer";

#[tokio::test]
async fn cited_under_cap_fragment_is_deferred_not_archived() {
    let fixture = Fixture::new().await;
    // 20 days old: expired at base (14) but under the cap (42).
    append_fragment(&fixture.substrate, "sub_01HZXJK7J7W0X4Q4KJ7A2R8C01", days_ago(20)).await;
    // Two evidence refs from a live memory => citation count 2 (>= threshold 2).
    write_citing_memory(
        &fixture,
        "mem_20260401_a1b2c3d4e5f60718_000001",
        MemoryStatus::Active,
        &["sub_01HZXJK7J7W0X4Q4KJ7A2R8C01", "sub_01HZXJK7J7W0X4Q4KJ7A2R8C01"],
    )
    .await;

    let report = run_cleanup(&fixture.substrate, config()).await.expect("cleanup");

    assert_eq!(report.operations.fragments_archived, 0, "cited fragment must not archive");
    assert_eq!(report.deferred_fragments.len(), 1, "one fragment deferred");
    let deferred = &report.deferred_fragments[0];
    assert_eq!(deferred.fragment_id, "sub_01HZXJK7J7W0X4Q4KJ7A2R8C01");
    assert_eq!(deferred.citations, 2);
    // The fragment is still in the active tree.
    assert!(fixture.active_fragment_ids().contains(&"sub_01HZXJK7J7W0X4Q4KJ7A2R8C01".to_string()));
    assert!(fixture.archive_fragment_ids().is_empty(), "nothing archived");
}

#[tokio::test]
async fn uncited_expired_fragment_archives_on_schedule() {
    let fixture = Fixture::new().await;
    // 20 days old, expired at base, but nothing cites it.
    append_fragment(&fixture.substrate, "sub_01HZXJK7J7W0X4Q4KJ7A2R8C02", days_ago(20)).await;

    let report = run_cleanup(&fixture.substrate, config()).await.expect("cleanup");

    assert_eq!(report.operations.fragments_archived, 1, "uncited expired fragment archives");
    assert!(report.deferred_fragments.is_empty(), "nothing deferred");
    assert!(fixture.active_fragment_ids().is_empty(), "active tree drained");
    assert!(fixture.archive_fragment_ids().contains(&"sub_01HZXJK7J7W0X4Q4KJ7A2R8C02".to_string()));
}

#[tokio::test]
async fn under_threshold_citation_does_not_defer() {
    let fixture = Fixture::new().await;
    append_fragment(&fixture.substrate, "sub_01HZXJK7J7W0X4Q4KJ7A2R8C03", days_ago(20)).await;
    // A single citation (< threshold 2) is not enough to defer.
    write_citing_memory(
        &fixture,
        "mem_20260401_a1b2c3d4e5f60718_000003",
        MemoryStatus::Active,
        &["sub_01HZXJK7J7W0X4Q4KJ7A2R8C03"],
    )
    .await;

    let report = run_cleanup(&fixture.substrate, config()).await.expect("cleanup");

    assert_eq!(report.operations.fragments_archived, 1, "one citation < threshold archives");
    assert!(report.deferred_fragments.is_empty());
}

#[tokio::test]
async fn immortality_cap_forces_archival_despite_citations() {
    let fixture = Fixture::new().await;
    // 50 days old: past the 42-day immortality cap.
    append_fragment(&fixture.substrate, "sub_01HZXJK7J7W0X4Q4KJ7A2R8C04", days_ago(50)).await;
    write_citing_memory(
        &fixture,
        "mem_20260401_a1b2c3d4e5f60718_000004",
        MemoryStatus::Active,
        &["sub_01HZXJK7J7W0X4Q4KJ7A2R8C04", "sub_01HZXJK7J7W0X4Q4KJ7A2R8C04", "sub_01HZXJK7J7W0X4Q4KJ7A2R8C04"],
    )
    .await;

    let report = run_cleanup(&fixture.substrate, config()).await.expect("cleanup");

    assert_eq!(report.operations.fragments_archived, 1, "capped fragment archives despite 3 citations");
    assert!(report.deferred_fragments.is_empty(), "nothing deferred at the cap");
    assert!(fixture.archive_fragment_ids().contains(&"sub_01HZXJK7J7W0X4Q4KJ7A2R8C04".to_string()));
}

#[tokio::test]
async fn archived_memory_does_not_keep_fragment_alive() {
    let fixture = Fixture::new().await;
    append_fragment(&fixture.substrate, "sub_01HZXJK7J7W0X4Q4KJ7A2R8C05", days_ago(20)).await;
    // A non-live (archived) memory is not a live citer — its evidence refs do
    // not defer the fragment.
    write_citing_memory(
        &fixture,
        "mem_20260401_a1b2c3d4e5f60718_000005",
        MemoryStatus::Archived,
        &["sub_01HZXJK7J7W0X4Q4KJ7A2R8C05", "sub_01HZXJK7J7W0X4Q4KJ7A2R8C05"],
    )
    .await;

    let report = run_cleanup(&fixture.substrate, config()).await.expect("cleanup");

    assert_eq!(report.operations.fragments_archived, 1, "archived-memory citations do not defer");
    assert!(report.deferred_fragments.is_empty());
}

#[tokio::test]
async fn dynamics_disabled_archives_cited_fragments_via_hard_cutoff() {
    let fixture = Fixture::new().await;
    fixture.write_dynamics_config(false);
    append_fragment(&fixture.substrate, "sub_01HZXJK7J7W0X4Q4KJ7A2R8C06", days_ago(20)).await;
    write_citing_memory(
        &fixture,
        "mem_20260401_a1b2c3d4e5f60718_000006",
        MemoryStatus::Active,
        &["sub_01HZXJK7J7W0X4Q4KJ7A2R8C06", "sub_01HZXJK7J7W0X4Q4KJ7A2R8C06"],
    )
    .await;

    let report = run_cleanup(&fixture.substrate, config()).await.expect("cleanup");

    assert_eq!(report.operations.fragments_archived, 1, "dynamics off: cited fragment archives on hard cutoff");
    assert!(report.deferred_fragments.is_empty(), "no deferral surface when dynamics off");
    assert!(fixture.archive_fragment_ids().contains(&"sub_01HZXJK7J7W0X4Q4KJ7A2R8C06".to_string()));
}

#[tokio::test]
async fn deferral_is_stable_across_repeated_runs_until_cap() {
    let fixture = Fixture::new().await;
    append_fragment(&fixture.substrate, "sub_01HZXJK7J7W0X4Q4KJ7A2R8C07", days_ago(20)).await;
    write_citing_memory(
        &fixture,
        "mem_20260401_a1b2c3d4e5f60718_000007",
        MemoryStatus::Active,
        &["sub_01HZXJK7J7W0X4Q4KJ7A2R8C07", "sub_01HZXJK7J7W0X4Q4KJ7A2R8C07"],
    )
    .await;

    let first = run_cleanup(&fixture.substrate, config()).await.expect("first");
    let second = run_cleanup(&fixture.substrate, config()).await.expect("second");

    assert_eq!(first.deferred_fragments.len(), 1);
    assert_eq!(second.deferred_fragments.len(), 1, "deferral repeats while under cap");
    assert_eq!(first.operations.fragments_archived, 0);
    assert_eq!(second.operations.fragments_archived, 0);
}

// ---- fixture plumbing (mirrors dream_cleanup.rs) ----

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
            InitOptions { force_unsafe_durability: true, device_id: Some(DEVICE.to_string()) },
        )
        .await
        .expect("init substrate");
        Self { roots, substrate, _temp: temp }
    }

    fn write_dynamics_config(&self, enabled: bool) {
        let path = self.roots.repo.join("config.yaml");
        let body = format!("schema_version: 1\ndynamics:\n  enabled: {enabled}\n");
        fs::write(path, body).expect("write config.yaml");
    }

    fn active_fragment_ids(&self) -> Vec<String> {
        fragment_ids(&self.roots.repo.join("substrate").join(DEVICE))
    }

    fn archive_fragment_ids(&self) -> Vec<String> {
        fragment_ids(&self.roots.repo.join("substrate/archive").join(DEVICE))
    }
}

fn fragment_ids(root: &Path) -> Vec<String> {
    let mut ids = Vec::new();
    collect_ids(root, &mut ids);
    ids.sort();
    ids
}

fn collect_ids(path: &Path, ids: &mut Vec<String>) {
    if path.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            collect_ids(&entry.path(), ids);
        }
        return;
    }
    if path.extension().is_some_and(|ext| ext == "jsonl") {
        let text = fs::read_to_string(path).unwrap_or_default();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let value: Value = serde_json::from_str(line).expect("jsonl");
            if let Some(id) = value.get("id").and_then(Value::as_str) {
                ids.push(id.to_string());
            }
        }
    }
}

fn config() -> CleanupConfig {
    CleanupConfig {
        device_id: DEVICE.to_string(),
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
            session: Some("sess_defer".to_string()),
            harness: Some("codex".to_string()),
            scope: "agent".to_string(),
            entities: vec!["ent_defer".to_string()],
            kind: ObserveKind::Observation,
            source_ref: None,
            privacy_spans: Vec::new(),
            payload: SubstrateFragmentPayload::Plaintext { text: "deferral observation".to_string() },
            classification: ClassificationOutcome::Trusted,
            operation_id: None,
        })
        .await
        .expect("append substrate fragment");
}

async fn write_citing_memory(fixture: &Fixture, id: &str, status: MemoryStatus, fragment_refs: &[&str]) {
    let memory = citing_memory(id, status, fragment_refs);
    fixture
        .substrate
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
        .expect("write citing memory");
}

fn citing_memory(id: &str, status: MemoryStatus, fragment_refs: &[&str]) -> Memory {
    let now = fixed_now();
    let evidence = fragment_refs
        .iter()
        .enumerate()
        .map(|(index, reference)| Evidence {
            id: format!("ev_01HZXJK7J7W0X4Q4KJ7A2R8E{index:02}"),
            quote: "deferral observation".to_string(),
            quote_norm_hash: None,
            reference: reference.to_string(),
            weight: 1.0,
            observed_at: None,
            source: None,
        })
        .collect();
    Memory {
        frontmatter: Frontmatter {
            schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "deferral fixture".to_string(),
            confidence: 0.8,
            original_confidence: None,
            trust_level: if status == MemoryStatus::Candidate { TrustLevel::Candidate } else { TrustLevel::Trusted },
            sensitivity: Sensitivity::Internal,
            status,
            created_at: now,
            updated_at: now,
            observed_at: None,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("dream-deferral-test".to_string()),
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
            evidence,
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
                policy_applied: "deferral-test".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: Default::default(),
        },
        body: "deferral memory body".to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}

fn fixed_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 4, 30, 12, 0, 0).single().expect("valid date")
}

fn days_ago(days: i64) -> DateTime<Utc> {
    fixed_now() - Duration::days(days)
}
