use std::path::Path;

use base64::Engine;
use memory_privacy::{EncryptedPayload, FileKeyProvider, PrivacyEncryptor};
use memory_substrate::{InitOptions, MemoryQuery, ObserveKind, Roots, Substrate};
use memoryd::handlers::handle_request;
use memoryd::protocol::{ObserveTarget, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};

#[tokio::test]
async fn memory_observe_appends_plaintext_fragment_for_all_observe_kinds() {
    for (kind, expected_kind) in
        [(ObserveKind::Observation, "observation"), (ObserveKind::Pattern, "pattern"), (ObserveKind::Signal, "signal")]
    {
        let temp = tempfile::tempdir().expect("tempdir");
        let substrate = init_substrate(&temp).await;
        let text = format!("dream substrate {expected_kind} about retry backoff");

        let observe =
            observe(&substrate, ObserveInput::new("observe-plain", &text, kind).with_entity("ent_retry")).await;

        let ResponseResult::Success(ResponsePayload::Observe(response)) = observe.result else {
            panic!("expected observe success, got {:?}", observe.result);
        };
        assert_eq!(response.target, ObserveTarget::PlaintextSubstrate);
        assert!(response.fragment_id.starts_with("sub_"));

        let records = read_jsonl_records(&substrate.roots().repo.join("substrate/dev_dreamtest"));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["id"], response.fragment_id);
        assert_eq!(records[0]["kind"], expected_kind);
        assert_eq!(records[0]["text"], text);
        assert_eq!(records[0]["session"], "sess_observe");
        assert_eq!(records[0]["harness"], "codex");
        assert_eq!(records[0]["scope"], "agent");
        assert_eq!(records[0]["source_ref"], "session:sess_observe:memory_observe");
        assert_eq!(records[0]["entities"], serde_json::json!(["ent_retry"]));
        assert!(records[0].get("encryption").is_none());
    }
}

#[tokio::test]
async fn memory_observe_routes_pii_to_encrypted_substrate_without_plaintext_leak() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    FileKeyProvider::runtime_default(&temp.path().join("runtime")).onboard_local_file().expect("privacy key");
    let raw = "reviewer@example.com";
    let text = format!("Follow up with {raw} about the launch checklist.");

    let observe = observe(
        &substrate,
        ObserveInput::new("observe-pii", &text, ObserveKind::Observation).with_entity("ent_launch"),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::Observe(response)) = observe.result else {
        panic!("expected encrypted observe success, got {:?}", observe.result);
    };
    assert_eq!(response.target, ObserveTarget::EncryptedSubstrate);

    let encrypted_records = read_jsonl_records(&substrate.roots().repo.join("encrypted/substrate/dev_dreamtest"));
    assert_eq!(encrypted_records.len(), 1);
    assert_eq!(encrypted_records[0]["id"], response.fragment_id);
    assert!(encrypted_records[0].get("text").is_none(), "encrypted substrate must not include text field");
    let encryption = &encrypted_records[0]["encryption"];
    let recipient = encryption["recipient"].as_str().expect("encrypted fragment recipient");
    assert!(!recipient.is_empty(), "encrypted fragment recipient must be persisted");
    let ciphertext_b64 = encryption["ciphertext_b64"].as_str().expect("encrypted fragment ciphertext");
    assert!(!ciphertext_b64.is_empty(), "encrypted fragment ciphertext must be persisted");
    let ciphertext = base64::engine::general_purpose::STANDARD.decode(ciphertext_b64).expect("ciphertext is base64");
    let decrypted = PrivacyEncryptor::new(FileKeyProvider::runtime_default(&substrate.roots().runtime))
        .decrypt(&EncryptedPayload {
            ciphertext,
            envelope: serde_json::json!({
                "scheme": "age-x25519",
                "recipient": recipient,
            }),
        })
        .expect("encrypted observe ciphertext decrypts with runtime key");
    assert_eq!(decrypted, text, "encrypted observe ciphertext must recover original observation");
    let summary_safe = encrypted_records[0]["descriptor"]["summary_safe"].as_str().expect("summary_safe string");
    assert!(summary_safe.contains("launch checklist"), "descriptor should retain safe signal: {summary_safe}");
    assert!(!summary_safe.contains(raw), "descriptor must not leak raw PII: {summary_safe}");
    assert!(encrypted_records[0]["descriptor"]["tag_safe"]
        .as_array()
        .is_some_and(|tags| tags.iter().any(|tag| tag.as_str() == Some("launch"))));
    assert!(encrypted_records[0]["privacy_spans"].as_array().is_some_and(|spans| !spans.is_empty()));
    assert!(!repo_contains(temp.path(), raw), "raw PII must not leak into repo/runtime plaintext");
    assert!(!repo_contains(temp.path(), text.as_str()), "raw observed text must not leak into repo/runtime plaintext");
}

#[tokio::test]
async fn memory_observe_refuses_secret_entity_without_fragment_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let secret = fake_aws_key();

    let observe = observe(
        &substrate,
        ObserveInput::new("observe-secret-entity", "safe observation", ObserveKind::Signal).with_entity(&secret),
    )
    .await;

    let ResponseResult::Error(error) = observe.result else {
        panic!("expected entity refusal, got {:?}", observe.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert_no_substrate_records(&substrate, "secret entity observe");
    assert!(!repo_contains(temp.path(), &secret), "secret entity canary must not be written");
}

#[tokio::test]
async fn memory_observe_refuses_email_entity_without_fragment_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let email = "reviewer@example.com";

    let observe = observe(
        &substrate,
        ObserveInput::new("observe-email-entity", "safe observation", ObserveKind::Observation).with_entity(email),
    )
    .await;

    let ResponseResult::Error(error) = observe.result else {
        panic!("expected entity refusal, got {:?}", observe.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert_no_substrate_records(&substrate, "email entity observe");
    assert!(!repo_contains(temp.path(), email), "email entity canary must not be written");
}

#[tokio::test]
async fn memory_observe_refuses_sensitive_canonical_entity_ids_without_fragment_file() {
    for entity in
        [format!("ent_{}", fake_aws_key()), "ent_202-555-0198".to_string(), "ent_reviewer@example.com".to_string()]
    {
        let temp = tempfile::tempdir().expect("tempdir");
        let substrate = init_substrate(&temp).await;

        let observe = observe(
            &substrate,
            ObserveInput::new("observe-sensitive-entity-id", "safe observation", ObserveKind::Signal)
                .with_entity(&entity),
        )
        .await;

        let ResponseResult::Error(error) = observe.result else {
            panic!("{entity}: expected sensitive entity refusal, got {:?}", observe.result);
        };
        assert_eq!(error.code, "invalid_request", "{entity}");
        assert_no_substrate_records(&substrate, &entity);
        assert!(!repo_contains(temp.path(), &entity), "{entity} canary must not be written");
    }
}

#[tokio::test]
async fn memory_observe_allows_phone_like_entity_ids() {
    for entity in ["ent_202.555.0198", "ent_2025550198", "ent_202_555_0198"] {
        let temp = tempfile::tempdir().expect("tempdir");
        let substrate = init_substrate(&temp).await;

        let observe = observe(
            &substrate,
            ObserveInput::new("observe-phone-like-entity", "safe observation", ObserveKind::Signal).with_entity(entity),
        )
        .await;

        let ResponseResult::Success(ResponsePayload::Observe(response)) = observe.result else {
            panic!("{entity}: expected observe success, got {:?}", observe.result);
        };
        assert!(response.fragment_id.starts_with("sub_"), "{entity}");
    }
}

#[tokio::test]
async fn memory_observe_refuses_sensitive_binding_metadata_without_fragment_file() {
    for (field, canary) in [
        ("session_id", format!("sess_{}", fake_aws_key())),
        ("session_id", "sess_202-555-0198".to_string()),
        ("harness", format!("codex_{}", fake_aws_key())),
        ("harness", "codex_202-555-0198".to_string()),
        ("harness_version", format!("v1_{}", fake_aws_key())),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let substrate = init_substrate(&temp).await;
        let mut input = ObserveInput::new("observe-sensitive-binding", "safe observation", ObserveKind::Signal);
        match field {
            "session_id" => input.session_id = canary.clone(),
            "harness" => input.harness = canary.clone(),
            "harness_version" => input.harness_version = Some(canary.clone()),
            other => panic!("unexpected field {other}"),
        }

        let observe = observe(&substrate, input).await;

        let ResponseResult::Error(error) = observe.result else {
            panic!("{field}={canary}: expected sensitive binding refusal, got {:?}", observe.result);
        };
        assert_eq!(error.code, "invalid_request", "{field}={canary}");
        assert_no_substrate_records(&substrate, &canary);
        assert!(!repo_contains(temp.path(), &canary), "{field} canary must not be written");
    }
}

#[tokio::test]
async fn memory_observe_allows_phone_like_binding_metadata() {
    for (field, canary) in [
        ("session_id", "sess_202.555.0198"),
        ("session_id", "sess_2025550198"),
        ("session_id", "sess_202_555_0198"),
        ("harness", "codex_202.555.0198"),
        ("harness", "codex_2025550198"),
        ("harness", "codex_202_555_0198"),
        ("harness_version", "v1_202.555.0198"),
        ("harness_version", "v1_2025550198"),
        ("harness_version", "v1_202_555_0198"),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let substrate = init_substrate(&temp).await;
        let mut input = ObserveInput::new("observe-phone-like-binding", "safe observation", ObserveKind::Signal);
        match field {
            "session_id" => input.session_id = canary.to_string(),
            "harness" => input.harness = canary.to_string(),
            "harness_version" => input.harness_version = Some(canary.to_string()),
            other => panic!("unexpected field {other}"),
        }

        let observe = observe(&substrate, input).await;

        let ResponseResult::Success(ResponsePayload::Observe(response)) = observe.result else {
            panic!("{field}={canary}: expected observe success, got {:?}", observe.result);
        };
        assert!(response.fragment_id.starts_with("sub_"), "{field}={canary}");
    }
}

#[tokio::test]
async fn memory_observe_refuses_secret_without_fragment_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let secret = fake_aws_key();

    let observe = observe(
        &substrate,
        ObserveInput::new("observe-secret", &format!("Never persist {secret}"), ObserveKind::Signal),
    )
    .await;

    let ResponseResult::Error(error) = observe.result else {
        panic!("expected privacy refusal, got {:?}", observe.result);
    };
    assert!(matches!(error.code.as_str(), "privacy_error" | "invalid_request"));
    assert_no_substrate_records(&substrate, "secret observe");
    assert!(!repo_contains(temp.path(), &secret), "secret canary must not be written");
}

#[tokio::test]
async fn memory_note_still_writes_canonical_memory_only() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;

    let note = handle_request(
        &substrate,
        RequestEnvelope::new("note", RequestPayload::WriteNote { text: "canonical note only".to_string() }),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::WriteNote(response)) = note.result else {
        panic!("expected note success, got {:?}", note.result);
    };
    assert!(response.id.starts_with("mem_"));
    assert_no_substrate_records(&substrate, "memory_note");
    let memories = substrate.query_memory(MemoryQuery::default()).await.expect("query memories");
    assert_eq!(memories.len(), 1, "memory_note should still write exactly one canonical memory");
}

#[tokio::test]
async fn memory_observe_rejects_invalid_text_and_entities() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;

    for (name, text, entities) in [
        ("empty-text", "   ".to_string(), Vec::new()),
        ("too-long-text", "x".repeat(16 * 1024 + 1), Vec::new()),
        ("too-many-entities", "valid".to_string(), (0..33).map(|idx| format!("ent_{idx}")).collect()),
        ("empty-entity", "valid".to_string(), vec!["ent_ok".to_string(), "".to_string()]),
        ("too-long-entity", "valid".to_string(), vec!["é".repeat(65)]),
        ("leading-whitespace-entity", "valid".to_string(), vec![" ent_whitespace".to_string()]),
        ("trailing-whitespace-entity", "valid".to_string(), vec!["ent_whitespace ".to_string()]),
        ("non-id-entity", "valid".to_string(), vec!["auth_flow".to_string()]),
    ] {
        let mut input = ObserveInput::new(name, &text, ObserveKind::Observation);
        input.entities = entities;
        let observe = observe(&substrate, input).await;
        let ResponseResult::Error(error) = observe.result else {
            panic!("{name}: expected invalid_request, got {:?}", observe.result);
        };
        assert_eq!(error.code, "invalid_request", "{name}");
    }
    assert_no_substrate_records(&substrate, "invalid observe requests");
}

#[tokio::test]
async fn memory_observe_uses_project_binding_scope_per_cwd() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let project_a = project_dir(temp.path(), "project-a", "proj_alpha");
    let project_b = project_dir(temp.path(), "project-b", "proj_beta");

    let first = observe(
        &substrate,
        ObserveInput::new("observe-project-a", "alpha project signal", ObserveKind::Signal).with_cwd(&project_a),
    )
    .await;
    let second = observe(
        &substrate,
        ObserveInput::new("observe-project-b", "beta project signal", ObserveKind::Signal).with_cwd(&project_b),
    )
    .await;

    assert!(matches!(first.result, ResponseResult::Success(ResponsePayload::Observe(_))));
    assert!(matches!(second.result, ResponseResult::Success(ResponsePayload::Observe(_))));

    let records = read_jsonl_records(&substrate.roots().repo.join("substrate/dev_dreamtest"));
    let scopes = records.iter().map(|record| record["scope"].as_str().expect("scope")).collect::<Vec<_>>();
    assert_eq!(scopes, ["project:proj_alpha", "project:proj_beta"]);
}

#[tokio::test]
async fn memory_observe_rejects_invalid_binding_before_fragment_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;

    let observe = observe(
        &substrate,
        ObserveInput::new("observe-invalid-binding", "safe observation", ObserveKind::Observation).with_cwd("relative"),
    )
    .await;

    let ResponseResult::Error(error) = observe.result else {
        panic!("expected invalid binding refusal, got {:?}", observe.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert_no_substrate_records(&substrate, "invalid binding observe");
}

#[derive(Debug)]
struct ObserveInput {
    id: String,
    text: String,
    kind: ObserveKind,
    entities: Vec<String>,
    cwd: Option<String>,
    session_id: String,
    harness: String,
    harness_version: Option<String>,
}

impl ObserveInput {
    fn new(id: &str, text: &str, kind: ObserveKind) -> Self {
        Self {
            id: id.to_string(),
            text: text.to_string(),
            kind,
            entities: Vec::new(),
            cwd: None,
            session_id: "sess_observe".to_string(),
            harness: "codex".to_string(),
            harness_version: None,
        }
    }

    fn with_entity(mut self, entity: &str) -> Self {
        self.entities.push(entity.to_string());
        self
    }

    fn with_cwd(mut self, cwd: impl AsRef<Path>) -> Self {
        self.cwd = Some(cwd.as_ref().to_string_lossy().into_owned());
        self
    }
}

async fn observe(substrate: &Substrate, input: ObserveInput) -> memoryd::protocol::ResponseEnvelope {
    let cwd = input.cwd.unwrap_or_else(|| substrate.roots().repo.to_string_lossy().into_owned());
    handle_request(
        substrate,
        RequestEnvelope::new(
            input.id,
            RequestPayload::Observe {
                text: input.text,
                kind: input.kind,
                entities: input.entities,
                cwd,
                session_id: input.session_id,
                harness: input.harness,
                harness_version: input.harness_version,
            },
        ),
    )
    .await
}

async fn init_substrate(temp: &tempfile::TempDir) -> Substrate {
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_dreamtest".to_string()) })
        .await
        .expect("init substrate")
}

fn project_dir(root: &Path, name: &str, canonical_id: &str) -> String {
    let path = root.join(name);
    std::fs::create_dir_all(&path).expect("create project dir");
    std::fs::write(path.join(".memory-project.yaml"), format!("canonical_id: {canonical_id}\n"))
        .expect("write project binding");
    path.to_string_lossy().into_owned()
}

fn read_jsonl_records(root: &Path) -> Vec<serde_json::Value> {
    if !root.exists() {
        return Vec::new();
    }
    let mut records = Vec::new();
    collect_jsonl_records(root, &mut records);
    records
}

fn assert_no_substrate_records(substrate: &Substrate, context: &str) {
    assert!(
        read_jsonl_records(&substrate.roots().repo.join("substrate")).is_empty(),
        "{context} must not write plaintext substrate fragments"
    );
    assert!(
        read_jsonl_records(&substrate.roots().repo.join("encrypted/substrate")).is_empty(),
        "{context} must not write encrypted substrate fragments"
    );
}

fn collect_jsonl_records(path: &Path, records: &mut Vec<serde_json::Value>) {
    if path.is_dir() {
        for entry in std::fs::read_dir(path).expect("read dir") {
            collect_jsonl_records(&entry.expect("dir entry").path(), records);
        }
        return;
    }
    if path.extension().is_some_and(|extension| extension == "jsonl") {
        let content = std::fs::read_to_string(path).expect("read jsonl");
        records.extend(
            content
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| serde_json::from_str(line).expect("json line")),
        );
    }
}

fn repo_contains(root: &Path, needle: &str) -> bool {
    contains_needle(root, needle.as_bytes()).expect("walk repo/runtime for leak canary")
}

fn fake_aws_key() -> String {
    let suffix = (0..16).map(|index| char::from(b'A' + (index % 10) as u8)).collect::<String>();
    ["AK", "IA", &suffix].concat()
}

fn contains_needle(path: &Path, needle: &[u8]) -> std::io::Result<bool> {
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            if contains_needle(&entry?.path(), needle)? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    if path.is_file() {
        let bytes = std::fs::read(path)?;
        return Ok(bytes.windows(needle.len()).any(|window| window == needle));
    }
    Ok(false)
}
