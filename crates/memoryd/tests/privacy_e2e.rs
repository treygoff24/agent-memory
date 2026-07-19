use memory_privacy::FileKeyProvider;
use memory_substrate::{events::EventKind, InitOptions, MemoryContent, MemoryId, MemoryQuery, Roots, Substrate};
use memoryd::handlers::handle_request;
use memoryd::protocol::{
    GovernanceRefusalReason, GovernanceStatus, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult,
};

const TEST_PROJECT_CANONICAL_ID: &str = "proj_privacy_e2e";
const TEST_PROJECT_ALIAS: &str = "privacy-e2e";

#[tokio::test]
async fn privacy_e2e_secret_in_cue_is_refused_before_any_disk_effect() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let secret = "AKIA1234567890ABCDEF";
    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "secret-cue",
            RequestPayload::WriteMemory {
                body: "Public body".to_string(),
                title: Some("Public title".to_string()),
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "project", "type": "claim", "summary": "Public summary",
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID, "confidence": 0.95,
                    "source_kind": "user", "explicit_user_context": true,
                    "cues": [secret]
                }),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governance refusal")
    };
    assert_eq!(write.status, GovernanceStatus::Refused);
    assert_eq!(write.reason, Some(GovernanceRefusalReason::Privacy));
    assert!(!repo_contains(temp.path(), secret));
}

#[tokio::test]
async fn privacy_e2e_secret_governed_write_is_refused_before_disk_effects() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let secret = "AKIA1234567890ABCDEF";

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "secret-write",
            RequestPayload::WriteMemory {
                body: format!("Do not persist {secret}."),
                title: Some("secret".to_string()),
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": "secret canary",
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.95,
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governance response, got {:?}", response.result);
    };
    assert_eq!(write.status, GovernanceStatus::Refused);
    assert_eq!(write.reason, Some(GovernanceRefusalReason::Privacy));
    assert!(write.id.is_none());
    assert!(!repo_contains(temp.path(), secret), "secret canary must not be written to repo/runtime");
}

#[tokio::test]
async fn privacy_e2e_personal_write_is_encrypted_and_not_raw_searchable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    FileKeyProvider::runtime_default(&temp.path().join("runtime")).onboard_local_file().expect("privacy key");
    let raw = "trey@example.com";

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "personal-write",
            RequestPayload::WriteMemory {
                body: format!("Contact {raw} for the launch."),
                title: Some("contact".to_string()),
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": "contact",
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.95,
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governance response, got {:?}", response.result);
    };
    assert!(matches!(write.status, GovernanceStatus::Promoted | GovernanceStatus::Candidate));
    let id = write.id.expect("encrypted write id");
    let envelope = substrate.read_memory_envelope(&MemoryId::new(&id)).await.expect("read envelope");
    assert!(matches!(envelope.content, MemoryContent::Ciphertext { .. }));
    assert!(envelope.metadata.path.as_ref().expect("path").as_str().starts_with("encrypted/"));
    assert_eq!(envelope.metadata.frontmatter.sensitivity, memory_substrate::Sensitivity::Internal);
    assert!(!repo_contains(temp.path(), raw), "raw personal canary must not be present in repo/runtime");

    let search = handle_request(
        &substrate,
        RequestEnvelope::new(
            "search-raw",
            RequestPayload::Search { query: raw.to_string(), limit: Some(10), include_body: false, cwd: None },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Search(search)) = search.result else {
        panic!("expected search response, got {:?}", search.result);
    };
    assert_eq!(search.total, 0, "raw personal body must not be searchable");

    let hits = substrate
        .query_memory(MemoryQuery {
            id: Some(MemoryId::new(&id)),
            tag: None,
            include_metadata_only: true,
            ..MemoryQuery::default()
        })
        .await
        .expect("metadata query");
    assert_eq!(hits.len(), 1);
}

#[tokio::test]
async fn privacy_e2e_caller_confidential_without_spans_is_metadata_only_encrypted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    FileKeyProvider::runtime_default(&temp.path().join("runtime")).onboard_local_file().expect("privacy key");
    let raw = "Acquisition target is Northstar";

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "confidential-no-spans",
            RequestPayload::WriteMemory {
                body: raw.to_string(),
                title: Some("confidential".to_string()),
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": "confidential acquisition note",
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.95,
                    "sensitivity": "confidential",
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governance response, got {:?}", response.result);
    };
    assert_eq!(write.status, GovernanceStatus::Promoted);
    let id = write.id.expect("encrypted write id");
    let envelope = substrate.read_memory_envelope(&MemoryId::new(&id)).await.expect("read envelope");
    assert!(matches!(envelope.content, MemoryContent::Ciphertext { .. }));
    assert_eq!(envelope.metadata.frontmatter.summary, "confidential acquisition note");
    assert!(!repo_contains(temp.path(), raw), "raw confidential text must not be persisted as safe projection");

    let search = handle_request(
        &substrate,
        RequestEnvelope::new(
            "search-confidential-raw",
            RequestPayload::Search { query: "Northstar".to_string(), limit: Some(10), include_body: false, cwd: None },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Search(search)) = search.result else {
        panic!("expected search response, got {:?}", search.result);
    };
    assert_eq!(search.total, 0, "raw confidential body must not be indexed");
}

#[tokio::test]
async fn privacy_e2e_project_url_and_date_stay_plaintext_and_searchable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let body = "See https://docs.example.com/foo for the 2026-04-28 release notes.";

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "url-date-plaintext",
            RequestPayload::WriteMemory {
                body: body.to_string(),
                title: Some("release notes link".to_string()),
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": "release notes link",
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.95,
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governance response, got {:?}", response.result);
    };
    let id = write.id.expect("plaintext write id");
    let envelope = substrate.read_memory_envelope(&MemoryId::new(&id)).await.expect("read envelope");
    assert!(matches!(envelope.content, MemoryContent::Plaintext(_)));

    let search = handle_request(
        &substrate,
        RequestEnvelope::new(
            "search-url",
            RequestPayload::Search {
                query: "docs.example.com".to_string(),
                limit: Some(10),
                include_body: false,
                cwd: None,
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Search(search)) = search.result else {
        panic!("expected search response, got {:?}", search.result);
    };
    assert_eq!(search.total, 1, "URL-bearing project memory should remain searchable");
}

#[tokio::test]
async fn privacy_e2e_phone_contact_is_encrypted_findable_and_revealable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    FileKeyProvider::runtime_default(&temp.path().join("runtime")).onboard_local_file().expect("privacy key");
    let raw_phone = "202-555-0198";

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "encrypted-contact",
            RequestPayload::WriteMemory {
                body: format!("Rep. Mills Chief of Staff cell is {raw_phone}."),
                title: Some("Rep. Mills Chief of Staff cell".to_string()),
                tags: vec!["contact".to_string(), "rep-mills".to_string(), "chief-of-staff".to_string()],
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": "Rep. Mills Chief of Staff cell phone",
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.95,
                    "source_kind": "user",
                    "explicit_user_context": true,
                    "privacy_descriptors": {
                        "subject": "Rep. Mills Chief of Staff",
                        "role": "Chief of Staff",
                        "organization": "Rep. Mills office",
                        "value_kind": "phone",
                        "lookup_hints": ["cell", "contact"]
                    }
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governance response, got {:?}", response.result);
    };
    let id = write.id.expect("encrypted contact id");
    let envelope = substrate.read_memory_envelope(&MemoryId::new(&id)).await.expect("read envelope");
    assert!(matches!(envelope.content, MemoryContent::Ciphertext { .. }));
    assert_eq!(envelope.metadata.frontmatter.sensitivity, memory_substrate::Sensitivity::Internal);
    assert_eq!(envelope.metadata.frontmatter.summary, "Rep. Mills Chief of Staff cell phone");
    assert!(envelope.metadata.frontmatter.tags.iter().any(|tag| tag == "rep-mills"));
    assert!(!repo_contains(temp.path(), raw_phone), "raw phone must not be written outside ciphertext");

    let descriptor_search = handle_request(
        &substrate,
        RequestEnvelope::new(
            "search-descriptor",
            RequestPayload::Search {
                query: "chief staff cell".to_string(),
                limit: Some(10),
                include_body: false,
                cwd: None,
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Search(search)) = descriptor_search.result else {
        panic!("expected search response, got {:?}", descriptor_search.result);
    };
    assert_eq!(search.total, 1, "safe descriptors should make encrypted contact findable");

    let raw_search = handle_request(
        &substrate,
        RequestEnvelope::new(
            "search-phone",
            RequestPayload::Search { query: raw_phone.to_string(), limit: Some(10), include_body: false, cwd: None },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Search(raw_search)) = raw_search.result else {
        panic!("expected raw search response, got {:?}", raw_search.result);
    };
    assert_eq!(raw_search.total, 0, "raw phone must not be indexed");

    let get = handle_request(
        &substrate,
        RequestEnvelope::new(
            "get-redacted",
            RequestPayload::Get { id: id.clone(), include_provenance: false, full_body: false },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Get(get)) = get.result else {
        panic!("expected get response, got {:?}", get.result);
    };
    assert_eq!(get.body, "[encrypted content omitted]");

    let reveal = handle_request(
        &substrate,
        RequestEnvelope::new(
            "reveal-contact",
            RequestPayload::Reveal {
                id: id.clone(),
                reason: "user asked for Rep. Mills Chief of Staff cell".to_string(),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Reveal(reveal)) = reveal.result else {
        panic!("expected reveal response, got {:?}", reveal.result);
    };
    assert!(reveal.body.contains(raw_phone));
    assert!(substrate
        .events()
        .expect("events")
        .iter()
        .any(|event| matches!(&event.kind, EventKind::EncryptedContentRevealed { id: event_id, .. } if event_id.as_str() == id)));

    // A reveal reason carrying PII (an email) is still accepted — revealing a contact and
    // explaining why is legitimate — but the reason is redacted before it is persisted to
    // the plaintext event log, so the PII never lands on disk.
    let sensitive_reason = handle_request(
        &substrate,
        RequestEnvelope::new(
            "reveal-sensitive-reason",
            RequestPayload::Reveal { id: id.clone(), reason: "send to reviewer@example.com".to_string() },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Reveal(reveal_with_sensitive_reason)) = sensitive_reason.result else {
        panic!("expected reveal reason with contact context to be accepted, got {:?}", sensitive_reason.result);
    };
    assert!(reveal_with_sensitive_reason.body.contains(raw_phone));
    assert!(
        !repo_contains(temp.path(), "reviewer@example.com"),
        "PII reveal reason must be redacted before persistence"
    );

    // A secret in the reveal reason is likewise accepted but redacted — invariant 1: a
    // secret must never reach disk, including via the audit trail.
    let secret_reason = handle_request(
        &substrate,
        RequestEnvelope::new(
            "reveal-secret-reason",
            RequestPayload::Reveal { id: id.clone(), reason: "exfil via sk-test-9f8e7d6c5b4a3210".to_string() },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Reveal(reveal_with_secret_reason)) = secret_reason.result else {
        panic!("expected reveal with secret reason to be accepted (redacted), got {:?}", secret_reason.result);
    };
    assert!(reveal_with_secret_reason.body.contains(raw_phone));
    assert!(!repo_contains(temp.path(), "sk-test-9f8e7d6c5b4a3210"), "secret reveal reason must never reach disk");

    // Both unsafe reasons were persisted as the redaction sentinel, not verbatim.
    let redacted_reveals = substrate
        .events()
        .expect("events")
        .into_iter()
        .filter(|event| {
            matches!(&event.kind, EventKind::EncryptedContentRevealed { id: event_id, reason }
                if event_id.as_str() == id && reason == "[redacted]")
        })
        .count();
    assert!(redacted_reveals >= 2, "both the PII and secret reveal reasons must be redacted in the event log");
}

#[tokio::test]
async fn privacy_e2e_reveal_fails_for_plaintext_and_missing_key() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let plain = governed_project_write(&substrate, "plain-reveal", "The deployment target is staging.").await;
    let plain_id = plain.id.expect("plain id");

    let plain_reveal = handle_request(
        &substrate,
        RequestEnvelope::new(
            "plain-reveal-fails",
            RequestPayload::Reveal { id: plain_id, reason: "user asked".to_string() },
        ),
    )
    .await;
    assert!(matches!(plain_reveal.result, ResponseResult::Error(_)));

    let key_provider = FileKeyProvider::runtime_default(&temp.path().join("runtime"));
    key_provider.onboard_local_file().expect("privacy key");
    let encrypted = handle_request(
        &substrate,
        RequestEnvelope::new(
            "encrypted-missing-key",
            RequestPayload::WriteMemory {
                body: "Contact missing-key@example.com for details.".to_string(),
                title: Some("missing key contact".to_string()),
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": "missing key contact",
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.95,
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = encrypted.result else {
        panic!("expected encrypted write, got {:?}", encrypted.result);
    };
    let encrypted_id = write.id.expect("encrypted id");
    std::fs::remove_file(key_provider.path()).expect("remove key");

    let missing_key_reveal = handle_request(
        &substrate,
        RequestEnvelope::new(
            "missing-key-reveal",
            RequestPayload::Reveal { id: encrypted_id, reason: "user asked".to_string() },
        ),
    )
    .await;
    assert!(matches!(missing_key_reveal.result, ResponseResult::Error(_)));
}

#[tokio::test]
async fn privacy_e2e_metadata_secret_is_refused_before_disk_effects() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let secret = "ghp_1234567890abcdefghijklmnop";

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "metadata-secret",
            RequestPayload::WriteMemory {
                body: "safe body".to_string(),
                title: Some(format!("leaked {secret}")),
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": "safe summary",
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.95,
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governance response, got {:?}", response.result);
    };
    assert_eq!(write.status, GovernanceStatus::Refused);
    assert_eq!(write.reason, Some(GovernanceRefusalReason::Privacy));
    assert!(!repo_contains(temp.path(), secret), "metadata secret must not be written");
}

#[tokio::test]
async fn privacy_e2e_encrypted_memory_can_be_forgotten_without_plaintext_leak() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    FileKeyProvider::runtime_default(&temp.path().join("runtime")).onboard_local_file().expect("privacy key");
    let raw = "forget-me@example.com";

    let write = handle_request(
        &substrate,
        RequestEnvelope::new(
            "personal-write-forget",
            RequestPayload::WriteMemory {
                body: format!("Remove contact {raw} after test."),
                title: Some("forget encrypted".to_string()),
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": "encrypted forget fixture",
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.95,
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = write.result else {
        panic!("expected write success, got {:?}", write.result);
    };
    let id = write.id.expect("encrypted write id");

    let forget = handle_request(
        &substrate,
        RequestEnvelope::new(
            "forget-encrypted",
            RequestPayload::Forget { id: id.clone(), reason: "user requested removal".to_string() },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceForget(forget)) = forget.result else {
        panic!("expected forget success, got {:?}", forget.result);
    };
    assert_eq!(forget.status, GovernanceStatus::Tombstoned);

    let envelope = substrate.read_memory_envelope(&MemoryId::new(&id)).await.expect("read tombstoned envelope");
    assert_eq!(envelope.metadata.frontmatter.status, memory_substrate::MemoryStatus::Tombstoned);
    assert!(matches!(envelope.content, MemoryContent::Ciphertext { .. }));
    assert!(!repo_contains(temp.path(), raw), "forgetting encrypted memory must not leak raw plaintext");
}

/// Plaintext old + encrypted replacement. Stream A's atomic `supersede_memory`
/// can't drive this pair (it routes the replacement through plaintext `write_memory`,
/// which refuses `RequiresEncryption`). The daemon now splits the write: the
/// replacement lands as ciphertext under `encrypted/`, the old plaintext is
/// rewritten in place with `status: superseded` and `superseded_by: [new_id]`,
/// and the raw PII canary never appears anywhere on disk.
#[tokio::test]
async fn privacy_e2e_supersede_can_promote_plaintext_old_to_encrypted_replacement() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    FileKeyProvider::runtime_default(&temp.path().join("runtime")).onboard_local_file().expect("privacy key");
    let old = governed_project_write(&substrate, "old-supersede", "The deployment target is staging.").await;
    let old_id = old.id.expect("old id");
    let raw = "ops@example.com";

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "supersede-encrypted-replacement",
            RequestPayload::Supersede {
                old_id: old_id.clone(),
                content: format!("The deployment target is production and contact is {raw}."),
                reason: "deployment target changed".to_string(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "project",
                    "summary": "Deployment target is production",
                    "confidence": 0.95,
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceSupersede(supersede)) = response.result else {
        panic!("expected supersede response, got {:?}", response.result);
    };
    assert_eq!(supersede.status, GovernanceStatus::Promoted);
    let new_id = supersede.new_id.expect("new id");

    // The replacement lands as ciphertext; the raw PII canary never hits plaintext on disk.
    let new_envelope = substrate.read_memory_envelope(&MemoryId::new(&new_id)).await.expect("new envelope");
    assert!(matches!(new_envelope.content, MemoryContent::Ciphertext { .. }));
    assert!(new_envelope.metadata.path.as_ref().expect("path").as_str().starts_with("encrypted/"));
    assert!(!repo_contains(temp.path(), raw), "encrypted replacement must not leak raw PII");

    // The plaintext old is now Superseded and points at the new id.
    let old_after = substrate.read_memory(&MemoryId::new(&old_id)).await.expect("old memory");
    assert_eq!(old_after.frontmatter.status, memory_substrate::MemoryStatus::Superseded);
    assert!(old_after.frontmatter.superseded_by.iter().any(|id| id.as_str() == new_id));
}

/// Encrypted old + plaintext replacement. The substrate can't read the encrypted
/// body to run body-based contradiction detection, but the supersede call carries
/// an explicit `old_id`, so the daemon trusts that intent and runs only grounding +
/// policy on the new candidate. The old envelope's frontmatter is mutated in place
/// via `update_encrypted_memory_metadata`; its body ciphertext is preserved.
#[tokio::test]
async fn privacy_e2e_supersede_can_replace_encrypted_old_with_plaintext_replacement() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    FileKeyProvider::runtime_default(&temp.path().join("runtime")).onboard_local_file().expect("privacy key");
    let encrypted = governed_project_write(&substrate, "old-encrypted-supersede", "Contact ops@example.com.").await;
    let old_id = encrypted.id.expect("encrypted old id");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "supersede-encrypted-old",
            RequestPayload::Supersede {
                old_id: old_id.clone(),
                content: "Replacement content is safe plaintext.".to_string(),
                reason: "user correction".to_string(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "project",
                    "summary": "safe replacement",
                    "confidence": 0.95,
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceSupersede(supersede)) = response.result else {
        panic!("expected supersede response, got {:?}", response.result);
    };
    assert_eq!(supersede.status, GovernanceStatus::Promoted);
    let new_id = supersede.new_id.expect("new id");

    // The plaintext replacement is readable as plaintext and lists the encrypted old in its supersedes chain.
    let new_envelope = substrate.read_memory_envelope(&MemoryId::new(&new_id)).await.expect("new envelope");
    assert!(matches!(new_envelope.content, MemoryContent::Plaintext(_)));
    assert!(new_envelope.metadata.frontmatter.supersedes.iter().any(|id| id.as_str() == old_id));

    // The encrypted old retains its ciphertext body but now reports Superseded status
    // with the new id appended to `superseded_by`.
    let old_envelope =
        substrate.read_memory_envelope(&MemoryId::new(&old_id)).await.expect("encrypted old still readable");
    assert!(matches!(old_envelope.content, MemoryContent::Ciphertext { .. }));
    assert_eq!(old_envelope.metadata.frontmatter.status, memory_substrate::MemoryStatus::Superseded);
    assert!(old_envelope.metadata.frontmatter.superseded_by.iter().any(|id| id.as_str() == new_id));
}

#[tokio::test]
async fn privacy_e2e_note_secret_is_refused() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let secret = "ghp_1234567890abcdefghijklmnop";

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "secret-note",
            RequestPayload::WriteNote { text: format!("temporary {secret}"), meta: serde_json::Value::Null },
        ),
    )
    .await;

    let ResponseResult::Error(error) = response.result else {
        panic!("expected protocol error for secret note, got {:?}", response.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(!repo_contains(temp.path(), secret), "secret note must not be written");
}

/// Encrypted candidates are reviewable: a review decision is a frontmatter-only
/// mutation routed through the encrypted-metadata path (W3 lifecycle validator,
/// actor `memoryd-review`). Found live: the entire 13-item import-dup drain on
/// the real corpus was encrypted and the old wholesale refusal made the W1
/// repair pass a no-op.
#[tokio::test]
async fn privacy_e2e_encrypted_candidate_reject_archives_via_encrypted_metadata_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    FileKeyProvider::runtime_default(&temp.path().join("runtime")).onboard_local_file().expect("privacy key");

    let write = handle_request(
        &substrate,
        RequestEnvelope::new(
            "encrypted-note",
            RequestPayload::WriteNote { text: "email reviewer@example.com".into(), meta: serde_json::Value::Null },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::WriteNote(note)) = write.result else {
        panic!("expected encrypted note write, got {:?}", write.result);
    };

    let review = handle_request(
        &substrate,
        RequestEnvelope::new(
            "reject-encrypted-note",
            RequestPayload::ReviewReject { id: note.id.clone(), reason: "not useful".to_string() },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::ReviewReject(decision)) = review.result else {
        panic!("expected encrypted candidate reject to succeed, got {:?}", review.result);
    };
    assert_eq!(decision.status, "rejected");

    // The decision landed as a frontmatter-only archive; a second decision now
    // fails eligibility (no longer in the queue) rather than leaking not-found.
    let again = handle_request(
        &substrate,
        RequestEnvelope::new(
            "reject-encrypted-note-again",
            RequestPayload::ReviewReject { id: note.id, reason: "still not useful".to_string() },
        ),
    )
    .await;
    let ResponseResult::Error(error) = again.result else {
        panic!("expected post-decision reject to fail eligibility, got {:?}", again.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(error.message.contains("not eligible"));
}

#[tokio::test]
async fn privacy_e2e_encrypted_candidate_approve_activates_via_encrypted_metadata_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    FileKeyProvider::runtime_default(&temp.path().join("runtime")).onboard_local_file().expect("privacy key");

    let write = handle_request(
        &substrate,
        RequestEnvelope::new(
            "encrypted-note-approve",
            RequestPayload::WriteNote { text: "email approver@example.com".into(), meta: serde_json::Value::Null },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::WriteNote(note)) = write.result else {
        panic!("expected encrypted note write, got {:?}", write.result);
    };

    let review = handle_request(
        &substrate,
        RequestEnvelope::new("approve-encrypted-note", RequestPayload::ReviewApprove { id: note.id.clone() }),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::ReviewApprove(decision)) = review.result else {
        panic!("expected encrypted candidate approve to succeed, got {:?}", review.result);
    };
    assert_eq!(decision.status, "approved");

    let get = handle_request(
        &substrate,
        RequestEnvelope::new(
            "get-approved-encrypted",
            RequestPayload::Get { id: note.id, include_provenance: false, full_body: false },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Get(got)) = get.result else {
        panic!("expected get of approved encrypted memory, got {:?}", get.result);
    };
    assert_eq!(
        got.status.map(|status| status.as_db_str().to_string()).as_deref(),
        Some("active"),
        "approve must persist candidate→active on disk"
    );
    assert!(got.encrypted, "memory must remain encrypted after the decision");
}

async fn governed_project_write(
    substrate: &Substrate,
    request_id: &str,
    body: &str,
) -> memoryd::protocol::GovernanceWriteResponse {
    let response = handle_request(
        substrate,
        RequestEnvelope::new(
            request_id,
            RequestPayload::WriteMemory {
                body: body.to_string(),
                title: Some(request_id.to_string()),
                tags: vec!["project".to_string()],
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "project",
                    "summary": request_id,
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.95,
                    "sensitivity": "internal",
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governed write success, got {:?}", response.result);
    };
    write
}

async fn init_substrate(temp: &tempfile::TempDir) -> Substrate {
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_privacye2e".to_string()) })
        .await
        .expect("init substrate")
}

fn repo_contains(root: &std::path::Path, needle: &str) -> bool {
    let needle = needle.as_bytes();
    contains_needle(root, needle).expect("walk repo/runtime for leak canary")
}

fn contains_needle(path: &std::path::Path, needle: &[u8]) -> std::io::Result<bool> {
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
