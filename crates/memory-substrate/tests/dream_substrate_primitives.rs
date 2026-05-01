use chrono::{TimeZone, Utc};
use memory_substrate::events::EventKind;
use memory_substrate::tree::{validate_tree, TreeValidationMode};
use memory_substrate::*;

#[tokio::test]
async fn append_plaintext_substrate_fragment_under_device_date_jsonl() {
    let (_temp, substrate) = initialized_substrate().await;

    let outcome = substrate
        .append_substrate_fragment(plaintext_request("sub_01HZXJK7J7W0X4Q4KJ7A2R8V1A", "plain observation"))
        .await
        .expect("append plaintext fragment");

    assert_eq!(outcome.path, RepoPath::new("substrate/dev_test/2026-04-10.jsonl"));
    let records = read_jsonl(&substrate.roots().repo.join(outcome.path.as_path()));
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1A");
    assert_eq!(records[0]["text"], "plain observation");
    assert_eq!(records[0]["kind"], "pattern");
}

#[tokio::test]
async fn append_encrypted_substrate_fragment_without_text_field() {
    let (_temp, substrate) = initialized_substrate().await;

    let outcome = substrate
        .append_substrate_fragment(encrypted_request("sub_01HZXJK7J7W0X4Q4KJ7A2R8V1B"))
        .await
        .expect("append encrypted fragment");

    assert_eq!(outcome.path, RepoPath::new("encrypted/substrate/dev_test/2026-04-10.jsonl"));
    let records = read_jsonl(&substrate.roots().repo.join(outcome.path.as_path()));
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1B");
    assert!(records[0].get("text").is_none(), "encrypted substrate must not persist plaintext");
    assert_eq!(records[0]["encryption"]["recipient"], "age1test");
    assert_eq!(records[0]["descriptor"]["summary_safe"], "safe auth descriptor");
}

#[tokio::test]
async fn append_substrate_fragment_emits_event() {
    let (_temp, substrate) = initialized_substrate().await;

    substrate
        .append_substrate_fragment(plaintext_request("sub_01HZXJK7J7W0X4Q4KJ7A2R8V1C", "eventful observation"))
        .await
        .expect("append plaintext fragment");

    let events = substrate.events().expect("events");
    assert!(events.iter().any(|event| {
        matches!(
            &event.kind,
            EventKind::SubstrateFragmentWritten { id, path, classification }
                if id == "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1C"
                    && path == &RepoPath::new("substrate/dev_test/2026-04-10.jsonl")
                    && classification == &ClassificationOutcome::Trusted
        )
    }));
}

#[tokio::test]
async fn archive_expired_plaintext_fragments_idempotently() {
    let (_temp, substrate) = initialized_substrate().await;
    substrate
        .append_substrate_fragment(plaintext_request("sub_01HZXJK7J7W0X4Q4KJ7A2R8V1D", "expired"))
        .await
        .expect("append expired");
    substrate
        .append_substrate_fragment(fresh_plaintext_request("sub_01HZXJK7J7W0X4Q4KJ7A2R8V1E"))
        .await
        .expect("append fresh");

    let first = substrate
        .archive_expired_substrate_fragments(Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap(), 14)
        .await
        .expect("archive expired");
    let second = substrate
        .archive_expired_substrate_fragments(Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap(), 14)
        .await
        .expect("archive again");

    assert_eq!(first.fragments_archived, 1);
    assert_eq!(second.fragments_archived, 0);
    let live_records = read_jsonl(&substrate.roots().repo.join("substrate/dev_test/2026-04-10.jsonl"));
    assert!(live_records.is_empty(), "expired source file is drained after archival");
    let fresh_records = read_jsonl(&substrate.roots().repo.join("substrate/dev_test/2026-04-29.jsonl"));
    assert_eq!(fresh_records.len(), 1, "fresh fragments remain live");
    let archive_records = read_jsonl(&substrate.roots().repo.join("substrate/archive/dev_test/2026-04.jsonl"));
    assert_eq!(archive_records.len(), 1);
    assert_eq!(archive_records[0]["id"], "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1D");
    validate_tree(&substrate.roots().repo, TreeValidationMode::FullySynced).expect("archive path remains tree-valid");
}

#[tokio::test]
async fn archive_output_is_concat_and_sort_by_id() {
    let (_temp, substrate) = initialized_substrate().await;
    std::fs::create_dir_all(substrate.roots().repo.join("substrate/archive/dev_test")).expect("archive dir");
    std::fs::write(
        substrate.roots().repo.join("substrate/archive/dev_test/2026-04.jsonl"),
        serde_json::to_string(&plaintext_record("sub_01HZXJK7J7W0X4Q4KJ7A2R8V1Z", "seeded")).expect("json") + "\n",
    )
    .expect("seed archive");
    substrate
        .append_substrate_fragment(plaintext_request("sub_01HZXJK7J7W0X4Q4KJ7A2R8V1A", "first after sort"))
        .await
        .expect("append first");

    substrate
        .archive_expired_substrate_fragments(Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap(), 14)
        .await
        .expect("archive expired");

    let ids: Vec<_> = read_jsonl(&substrate.roots().repo.join("substrate/archive/dev_test/2026-04.jsonl"))
        .into_iter()
        .map(|record| record["id"].as_str().expect("id").to_string())
        .collect();
    assert_eq!(ids, vec!["sub_01HZXJK7J7W0X4Q4KJ7A2R8V1A", "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1Z"]);
}

async fn initialized_substrate() -> (tempfile::TempDir, Substrate) {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");
    (temp, substrate)
}

fn plaintext_request(id: &str, text: &str) -> SubstrateFragmentAppendRequest {
    SubstrateFragmentAppendRequest {
        id: Some(id.to_string()),
        at: Utc.with_ymd_and_hms(2026, 4, 10, 12, 0, 0).unwrap(),
        session: Some("sess_test".to_string()),
        harness: Some("codex".to_string()),
        scope: "project:proj_abc".to_string(),
        entities: vec!["ent_auth_flow".to_string()],
        kind: ObserveKind::Pattern,
        source_ref: Some("session:sess_test:turn:47".to_string()),
        privacy_spans: Vec::new(),
        payload: SubstrateFragmentPayload::Plaintext { text: text.to_string() },
        classification: ClassificationOutcome::Trusted,
        operation_id: None,
    }
}

fn fresh_plaintext_request(id: &str) -> SubstrateFragmentAppendRequest {
    SubstrateFragmentAppendRequest {
        id: Some(id.to_string()),
        at: Utc.with_ymd_and_hms(2026, 4, 29, 12, 0, 0).unwrap(),
        ..plaintext_request(id, "fresh")
    }
}

fn encrypted_request(id: &str) -> SubstrateFragmentAppendRequest {
    SubstrateFragmentAppendRequest {
        id: Some(id.to_string()),
        at: Utc.with_ymd_and_hms(2026, 4, 10, 12, 0, 0).unwrap(),
        session: Some("sess_test".to_string()),
        harness: Some("codex".to_string()),
        scope: "project:proj_abc".to_string(),
        entities: vec!["ent_auth_flow".to_string()],
        kind: ObserveKind::Observation,
        source_ref: Some("session:sess_test:turn:48".to_string()),
        privacy_spans: vec![PrivacySpanRecord { label: "private_email".to_string(), start: 12, end: 34 }],
        payload: SubstrateFragmentPayload::Encrypted {
            encryption: SubstrateFragmentEncryption {
                recipient: "age1test".to_string(),
                ciphertext_b64: "Y2lwaGVydGV4dA==".to_string(),
            },
            descriptor: EncryptedSubstrateDescriptor {
                summary_safe: "safe auth descriptor".to_string(),
                tag_safe: vec!["auth".to_string()],
            },
        },
        classification: ClassificationOutcome::RequiresEncryption,
        operation_id: None,
    }
}

fn plaintext_record(id: &str, text: &str) -> SubstrateFragmentRecord {
    SubstrateFragmentRecord {
        id: id.to_string(),
        ts: Utc.with_ymd_and_hms(2026, 4, 1, 12, 0, 0).unwrap(),
        device: DeviceId::new("dev_test"),
        session: None,
        harness: None,
        scope: "me".to_string(),
        entities: Vec::new(),
        kind: ObserveKind::Observation,
        text: text.to_string(),
        source_ref: None,
        privacy_spans: Vec::new(),
    }
}

fn read_jsonl(path: &std::path::Path) -> Vec<serde_json::Value> {
    if !path.exists() {
        return Vec::new();
    }
    std::fs::read_to_string(path)
        .expect("read jsonl")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("json line"))
        .collect()
}
