//! Bounded abstraction/cue metadata amendment used by dream compilation.

use memory_privacy::PrivacyStorageAction;
use memory_substrate::{
    frontmatter::{normalize_abstraction_cues, validate_frontmatter},
    markdown::hash_bytes,
    metadata_amend_changed_fields, metadata_amend_updated_at, Memory, MemoryContent, MemoryId, MemoryStatus,
    MetadataAmendWriteRequest, MetadataAmendedEvent, ReadError, Sha256, Substrate, WriteFailure, WriteFailureKind,
};
use serde::Deserialize;

use super::classify_metadata_amendment_decision;

const METADATA_AMEND_ACTOR: &str = "memoryd-abstraction-compile";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MetadataAmendRequest {
    pub(crate) id: String,
    pub(crate) expected_base_hash: Sha256,
    pub(crate) abstraction: Option<String>,
    pub(crate) cues: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct MetadataAmendOutcome {
    pub(crate) changed: bool,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum MetadataAmendmentError {
    #[error("metadata amendment stale base")]
    MetadataAmendmentStaleBase,
    #[error("metadata amendment tier increase refused")]
    MetadataAmendmentTierIncreaseRefused,
    #[error("metadata amendment validation failed: {0}")]
    MetadataAmendmentValidationFailed(String),
    #[error("metadata amendment id is missing")]
    MetadataAmendmentMissingId,
    #[error("metadata amendment actor mismatch")]
    MetadataAmendmentActorMismatch,
    #[error("secret refused")]
    SecretRefused,
    #[error("metadata amendment lifecycle is not amendable")]
    MetadataAmendmentLifecycleNotAmendable,
    #[error("metadata amendment operational failure: {0}")]
    Operational(String),
}

impl MetadataAmendmentError {
    pub(crate) fn refusal_reason(&self) -> Option<&'static str> {
        match self {
            Self::MetadataAmendmentStaleBase => Some("metadata_amendment_stale_base"),
            Self::MetadataAmendmentTierIncreaseRefused => Some("metadata_amendment_tier_increase_refused"),
            Self::MetadataAmendmentValidationFailed(_) => Some("metadata_amendment_validation_failed"),
            Self::MetadataAmendmentMissingId => Some("metadata_amendment_missing_id"),
            Self::MetadataAmendmentActorMismatch => Some("metadata_amendment_actor_mismatch"),
            Self::SecretRefused => Some("secret_refused"),
            Self::MetadataAmendmentLifecycleNotAmendable => Some("metadata_amendment_lifecycle_not_amendable"),
            Self::Operational(_) => None,
        }
    }

    pub(crate) fn write_failure_kind(&self) -> Option<WriteFailureKind> {
        matches!(self, Self::SecretRefused).then_some(WriteFailureKind::SecretRefused)
    }
}

pub(crate) async fn metadata_amend(
    substrate: &Substrate,
    actor: &str,
    request: MetadataAmendRequest,
) -> Result<MetadataAmendOutcome, MetadataAmendmentError> {
    metadata_amend_after_lifecycle_gate(substrate, actor, request, || {}).await
}

async fn metadata_amend_after_lifecycle_gate<F>(
    substrate: &Substrate,
    actor: &str,
    request: MetadataAmendRequest,
    after_lifecycle_gate: F,
) -> Result<MetadataAmendOutcome, MetadataAmendmentError>
where
    F: FnOnce(),
{
    if actor != METADATA_AMEND_ACTOR {
        return Err(MetadataAmendmentError::MetadataAmendmentActorMismatch);
    }
    let id = MemoryId::try_new(request.id).map_err(|_| MetadataAmendmentError::MetadataAmendmentMissingId)?;
    let (envelope, base_hash) = substrate.read_memory_envelope_with_hash(&id).await.map_err(map_initial_read_error)?;
    if base_hash != request.expected_base_hash {
        return Err(MetadataAmendmentError::MetadataAmendmentStaleBase);
    }
    if !matches!(envelope.metadata.frontmatter.status, MemoryStatus::Active | MemoryStatus::Pinned) {
        return Err(MetadataAmendmentError::MetadataAmendmentLifecycleNotAmendable);
    }
    after_lifecycle_gate();

    let mut proposed = envelope.metadata.clone();
    proposed.frontmatter.abstraction = request.abstraction;
    proposed.frontmatter.cues = request.cues;
    normalize_abstraction_cues(&mut proposed.frontmatter)
        .map_err(|error| MetadataAmendmentError::MetadataAmendmentValidationFailed(error.to_string()))?;
    validate_frontmatter(&proposed.frontmatter)
        .map_err(|error| MetadataAmendmentError::MetadataAmendmentValidationFailed(error.to_string()))?;
    let body_available = matches!(envelope.content, MemoryContent::Plaintext(_));
    let privacy = classify_metadata_amendment_decision(&proposed, body_available)
        .map_err(|error| MetadataAmendmentError::Operational(error.message))?;
    if privacy.storage_action.refuses_storage() {
        let error = MetadataAmendmentError::SecretRefused;
        debug_assert_eq!(error.write_failure_kind(), Some(WriteFailureKind::SecretRefused));
        return Err(error);
    }
    if amendment_increases_tier(&proposed, body_available, &privacy) {
        return Err(MetadataAmendmentError::MetadataAmendmentTierIncreaseRefused);
    }

    let expected_path = proposed
        .path
        .clone()
        .ok_or_else(|| MetadataAmendmentError::Operational("canonical memory path is missing".to_string()))?;
    let write_request = MetadataAmendWriteRequest {
        id,
        expected_base_hash: request.expected_base_hash,
        expected_path,
        abstraction: proposed.frontmatter.abstraction,
        cues: proposed.frontmatter.cues,
    };
    if body_available {
        return substrate
            .amend_plaintext_memory_metadata(write_request, actor)
            .await
            .map(|outcome| MetadataAmendOutcome { changed: outcome.changed })
            .map_err(map_write_failure);
    }

    amend_encrypted(substrate, write_request, actor).await
}

fn amendment_increases_tier(
    proposed: &Memory,
    body_available: bool,
    privacy: &memory_privacy::PrivacyDecision,
) -> bool {
    let stored_is_encrypted = !body_available;
    let storage_increase = privacy.storage_action == PrivacyStorageAction::EncryptAtRest && !stored_is_encrypted;
    let sensitivity_increase =
        privacy.tier.persisted_sensitivity().is_some_and(|required| required > proposed.frontmatter.sensitivity);
    storage_increase || sensitivity_increase
}

async fn amend_encrypted(
    substrate: &Substrate,
    request: MetadataAmendWriteRequest,
    actor: &str,
) -> Result<MetadataAmendOutcome, MetadataAmendmentError> {
    let MetadataAmendWriteRequest { id, expected_base_hash, expected_path, abstraction, cues } = request;
    let repo = substrate.roots().repo.clone();
    let mut closure_error = None;
    let mut changed_fields = Vec::new();
    substrate
        .update_encrypted_memory_metadata(&id, Some(actor), |current| {
            let current_path = current.path.clone();
            let Some(path) = current_path.as_ref() else {
                closure_error = Some(MetadataAmendmentError::MetadataAmendmentStaleBase);
                return;
            };
            let bytes = match std::fs::read(repo.join(path.as_path())) {
                Ok(bytes) => bytes,
                Err(error) => {
                    closure_error = Some(MetadataAmendmentError::Operational(format!(
                        "failed to revalidate encrypted canonical bytes: {error}"
                    )));
                    return;
                }
            };
            if hash_bytes(&bytes) != expected_base_hash || path != &expected_path {
                closure_error = Some(MetadataAmendmentError::MetadataAmendmentStaleBase);
                return;
            }
            if !matches!(current.frontmatter.status, MemoryStatus::Active | MemoryStatus::Pinned) {
                closure_error = Some(MetadataAmendmentError::MetadataAmendmentLifecycleNotAmendable);
                return;
            }
            changed_fields = metadata_amend_changed_fields(&current.frontmatter, abstraction.as_ref(), &cues);
            if changed_fields.is_empty() {
                return;
            }
            current.frontmatter.abstraction = abstraction;
            current.frontmatter.cues = cues;
            current.frontmatter.updated_at = metadata_amend_updated_at(current.frontmatter.updated_at);
        })
        .await
        .map_err(map_write_failure)?;
    if let Some(error) = closure_error {
        return Err(error);
    }
    if changed_fields.is_empty() {
        return Ok(MetadataAmendOutcome { changed: false });
    }
    substrate
        .record_metadata_amended(MetadataAmendedEvent {
            id,
            path: expected_path,
            actor: actor.to_string(),
            changed_fields,
        })
        .map_err(map_write_failure)?;
    Ok(MetadataAmendOutcome { changed: true })
}

fn map_initial_read_error(error: ReadError) -> MetadataAmendmentError {
    match error {
        ReadError::NotFound(_) => MetadataAmendmentError::MetadataAmendmentMissingId,
        other => MetadataAmendmentError::Operational(other.to_string()),
    }
}

fn map_write_failure(error: WriteFailure) -> MetadataAmendmentError {
    match error.kind {
        WriteFailureKind::StaleBase => MetadataAmendmentError::MetadataAmendmentStaleBase,
        WriteFailureKind::MetadataAmendmentLifecycleNotAmendable => {
            MetadataAmendmentError::MetadataAmendmentLifecycleNotAmendable
        }
        WriteFailureKind::ValidationTyped(validation) => {
            MetadataAmendmentError::MetadataAmendmentValidationFailed(validation.to_string())
        }
        WriteFailureKind::SecretRefused => MetadataAmendmentError::SecretRefused,
        other => MetadataAmendmentError::Operational(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::privacy::classify_plaintext_memory_decision;
    use super::*;
    use memory_substrate::{
        events::EventKind,
        frontmatter::{default_retrieval_policy, serialize_document},
        ClassificationOutcome, EmbeddingLaneEligibility, EncryptedWriteRequest, EventContext, InitOptions, RepoPath,
        Roots, Scope, Sensitivity, WriteMode, WriteRequest,
    };

    #[test]
    fn request_refuses_non_amendable_fields() {
        let error = serde_json::from_value::<MetadataAmendRequest>(serde_json::json!({
            "id": "mem_20260715_aaaaaaaaaaaaaaaa_000001",
            "expected_base_hash": "sha256:test",
            "abstraction": "bounded metadata",
            "cues": [],
            "body": "immutable"
        }))
        .expect_err("body is not in the fixed request shape");
        assert!(error.to_string().contains("unknown field `body`"));
    }

    #[test]
    fn secret_refusal_maps_to_stream_a_write_failure_kind() {
        assert_eq!(MetadataAmendmentError::SecretRefused.write_failure_kind(), Some(WriteFailureKind::SecretRefused));
    }

    #[test]
    fn refusal_reasons_are_the_closed_cli_set() {
        let errors = [
            MetadataAmendmentError::MetadataAmendmentStaleBase,
            MetadataAmendmentError::MetadataAmendmentTierIncreaseRefused,
            MetadataAmendmentError::MetadataAmendmentValidationFailed("fixture".to_string()),
            MetadataAmendmentError::MetadataAmendmentMissingId,
            MetadataAmendmentError::MetadataAmendmentActorMismatch,
            MetadataAmendmentError::SecretRefused,
            MetadataAmendmentError::MetadataAmendmentLifecycleNotAmendable,
        ];
        assert_eq!(
            errors.map(|error| error.refusal_reason().expect("typed refusal")),
            [
                "metadata_amendment_stale_base",
                "metadata_amendment_tier_increase_refused",
                "metadata_amendment_validation_failed",
                "metadata_amendment_missing_id",
                "metadata_amendment_actor_mismatch",
                "secret_refused",
                "metadata_amendment_lifecycle_not_amendable",
            ]
        );
    }

    #[test]
    fn me_scope_sets_personal_privacy_floor() {
        let mut memory = fixture_memory();
        memory.frontmatter.scope = memory_substrate::Scope::User;
        memory.frontmatter.sensitivity = memory_substrate::Sensitivity::Personal;
        let decision = classify_plaintext_memory_decision(&memory, false).expect("classification");
        assert_eq!(decision.tier, memory_privacy::PrivacyTier::Personal);
    }

    #[test]
    fn lifecycle_scan_set_matches_pre_b3_baseline_and_amend_adds_tags() {
        // Pre-B3 lifecycle baseline scans summary+body+abstraction+cues but NOT
        // tags; only the amend classifier scans tags. Pinned so neither scan
        // set silently widens or narrows again.
        let mut tag = fixture_memory();
        tag.frontmatter.tags = vec!["4111111111111111".to_string()];
        let lifecycle = classify_plaintext_memory_decision(&tag, true).expect("lifecycle classification");
        let amendment = classify_metadata_amendment_decision(&tag, true).expect("amendment classification");
        assert_eq!(lifecycle.storage_action, PrivacyStorageAction::Plaintext);
        assert!(amendment.storage_action.refuses_storage());

        let mut abstraction = fixture_memory();
        abstraction.frontmatter.abstraction = Some("4111111111111111".to_string());
        let mut cues = fixture_memory();
        cues.frontmatter.cues = vec!["4111111111111111".to_string()];
        for memory in [abstraction, cues] {
            let lifecycle = classify_plaintext_memory_decision(&memory, true).expect("lifecycle classification");
            let amendment = classify_metadata_amendment_decision(&memory, true).expect("amendment classification");
            assert!(lifecycle.storage_action.refuses_storage());
            assert!(amendment.storage_action.refuses_storage());
        }
    }

    #[tokio::test]
    async fn plaintext_amend_preserves_immutable_fields_replaces_aux_and_emits_once() {
        let (_temp, substrate) = substrate("dev_b3plain").await;
        let mut memory = fixture_memory();
        memory.frontmatter.abstraction = Some("old retrieval metadata".to_string());
        memory.frontmatter.cues = vec!["old cue".to_string()];
        memory.frontmatter.retrieval_policy.index_embeddings = true;
        let id = memory.frontmatter.id.clone();
        write_plaintext(&substrate, memory).await;
        let (before_envelope, hash) = substrate.read_memory_envelope_with_hash(&id).await.expect("before");
        let before = before_envelope.metadata;
        let old_hash = substrate
            .pending_aux_embedding_jobs(10, EmbeddingLaneEligibility::AllTiers)
            .await
            .expect("old jobs")
            .into_iter()
            .find(|job| job.target_id == id.as_str())
            .expect("old abstraction job")
            .content_hash;

        let outcome = metadata_amend(
            &substrate,
            METADATA_AMEND_ACTOR,
            request(&id, hash, Some("new retrieval metadata"), &["new cue"]),
        )
        .await
        .expect("amend");
        assert!(outcome.changed);

        let after = substrate.read_memory(&id).await.expect("after");
        assert_eq!(after.body, before.body);
        assert_eq!(after.frontmatter.id, before.frontmatter.id);
        assert_eq!(after.frontmatter.status, before.frontmatter.status);
        assert_eq!(after.frontmatter.scope, before.frontmatter.scope);
        assert_eq!(after.frontmatter.sensitivity, before.frontmatter.sensitivity);
        assert_eq!(after.frontmatter.evidence, before.frontmatter.evidence);
        assert_eq!(after.frontmatter.created_at, before.frontmatter.created_at);
        assert_eq!(after.path, before.path);
        assert!(after.frontmatter.updated_at > before.frontmatter.updated_at);
        assert_eq!(after.frontmatter.abstraction.as_deref(), Some("new retrieval metadata"));
        assert_eq!(after.frontmatter.cues, vec!["new cue".to_string()]);

        let jobs = substrate
            .pending_aux_embedding_jobs(10, EmbeddingLaneEligibility::AllTiers)
            .await
            .expect("replacement jobs");
        let replacement = jobs.iter().find(|job| job.target_id == id.as_str()).expect("new abstraction job");
        assert_ne!(replacement.content_hash, old_hash);
        assert!(!jobs.iter().any(|job| job.content_hash == old_hash));
        let events = metadata_events(&substrate, &id);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            EventKind::MetadataAmended { actor, changed_fields, .. }
                if actor == METADATA_AMEND_ACTOR
                    && changed_fields.iter().map(String::as_str).eq(["abstraction", "cues"])
        ));
    }

    #[tokio::test]
    async fn stale_hash_precedes_identical_short_circuit_and_current_identical_is_eventless() {
        let (_temp, substrate) = substrate("dev_b3cas").await;
        let mut memory = fixture_memory();
        memory.frontmatter.abstraction = Some("existing metadata".to_string());
        memory.frontmatter.cues = vec!["existing cue".to_string()];
        let id = memory.frontmatter.id.clone();
        write_plaintext(&substrate, memory).await;
        let (_, hash) = substrate.read_memory_envelope_with_hash(&id).await.expect("hash");

        let stale = metadata_amend(
            &substrate,
            METADATA_AMEND_ACTOR,
            request(&id, Sha256::new("sha256:stale"), Some("existing metadata"), &["existing cue"]),
        )
        .await
        .expect_err("stale identical request");
        assert!(matches!(stale, MetadataAmendmentError::MetadataAmendmentStaleBase));

        let unchanged = metadata_amend(
            &substrate,
            METADATA_AMEND_ACTOR,
            request(&id, hash, Some("existing metadata"), &["existing cue"]),
        )
        .await
        .expect("current identical request");
        assert!(!unchanged.changed);
        assert!(metadata_events(&substrate, &id).is_empty());
    }

    #[tokio::test]
    async fn typed_refusals_leave_plaintext_unchanged() {
        let (_temp, substrate) = substrate("dev_b3refuse").await;
        let memory = fixture_memory();
        let id = memory.frontmatter.id.clone();
        write_plaintext(&substrate, memory).await;
        let (_, hash) = substrate.read_memory_envelope_with_hash(&id).await.expect("hash");

        let cases = [
            (
                request(&id, hash.clone(), Some("contact reviewer@example.com"), &[]),
                "metadata_amendment_tier_increase_refused",
            ),
            (request(&id, hash.clone(), Some("card 4111111111111111"), &[]), "secret_refused"),
            (
                request(&id, hash.clone(), Some("one two three four five six seven eight nine"), &[]),
                "metadata_amendment_validation_failed",
            ),
        ];
        for (request, reason) in cases {
            let error = metadata_amend(&substrate, METADATA_AMEND_ACTOR, request).await.expect_err(reason);
            assert_eq!(error.refusal_reason(), Some(reason));
        }
        let actor =
            metadata_amend(&substrate, "operator-supplied", request(&id, hash.clone(), Some("valid metadata"), &[]))
                .await
                .expect_err("actor mismatch");
        assert!(matches!(actor, MetadataAmendmentError::MetadataAmendmentActorMismatch));
        let missing = metadata_amend(
            &substrate,
            METADATA_AMEND_ACTOR,
            MetadataAmendRequest {
                id: "".to_string(),
                expected_base_hash: hash.clone(),
                abstraction: None,
                cues: Vec::new(),
            },
        )
        .await
        .expect_err("missing id");
        assert!(matches!(missing, MetadataAmendmentError::MetadataAmendmentMissingId));
        let (_, after_hash) = substrate.read_memory_envelope_with_hash(&id).await.expect("after hash");
        assert_eq!(after_hash, hash);
        assert!(metadata_events(&substrate, &id).is_empty());
    }

    #[tokio::test]
    async fn lifecycle_gate_refuses_archived_and_allows_pinned() {
        let (_archived_temp, archived) = substrate("dev_b3archived").await;
        let mut archived_memory = fixture_memory();
        archived_memory.frontmatter.status = MemoryStatus::Archived;
        let archived_id = archived_memory.frontmatter.id.clone();
        write_plaintext(&archived, archived_memory).await;
        let (_, archived_hash) = archived.read_memory_envelope_with_hash(&archived_id).await.expect("archived hash");
        let error = metadata_amend(
            &archived,
            METADATA_AMEND_ACTOR,
            request(&archived_id, archived_hash, Some("archived metadata"), &[]),
        )
        .await
        .expect_err("archived refusal");
        assert!(matches!(error, MetadataAmendmentError::MetadataAmendmentLifecycleNotAmendable));

        let (_pinned_temp, pinned) = substrate("dev_b3pinned").await;
        let mut pinned_memory = fixture_memory();
        pinned_memory.frontmatter.status = MemoryStatus::Pinned;
        pinned_memory.frontmatter.trust_level = memory_substrate::TrustLevel::Pinned;
        let pinned_id = pinned_memory.frontmatter.id.clone();
        write_plaintext(&pinned, pinned_memory).await;
        let (_, pinned_hash) = pinned.read_memory_envelope_with_hash(&pinned_id).await.expect("pinned hash");
        let outcome = metadata_amend(
            &pinned,
            METADATA_AMEND_ACTOR,
            request(&pinned_id, pinned_hash, Some("pinned metadata"), &[]),
        )
        .await
        .expect("pinned allowed");
        assert!(outcome.changed);
    }

    #[tokio::test]
    async fn lifecycle_toctou_refusal_wires_the_typed_reason() {
        let (_temp, substrate) = substrate("dev_b3toctou").await;
        let memory = fixture_memory();
        let id = memory.frontmatter.id.clone();
        write_plaintext(&substrate, memory).await;
        let (before, hash) = substrate.read_memory_envelope_with_hash(&id).await.expect("before");
        let path = before.metadata.path.clone().expect("path");
        let repo = substrate.roots().repo.clone();

        let error = metadata_amend_after_lifecycle_gate(
            &substrate,
            METADATA_AMEND_ACTOR,
            request(&id, hash, Some("new metadata"), &[]),
            move || archive_canonical_memory(&repo, &path),
        )
        .await
        .expect_err("status flipped after handler gate");

        assert_eq!(error.refusal_reason(), Some("metadata_amendment_lifecycle_not_amendable"));
    }

    #[tokio::test]
    async fn tier_fence_allows_existing_encrypted_and_plaintext_floors() {
        let (_user_temp, user) = substrate("dev_b3user").await;
        let mut user_memory = fixture_memory();
        set_scope_and_sensitivity(&mut user_memory, Scope::User, Sensitivity::Personal);
        let user_id = user_memory.frontmatter.id.clone();
        user.write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: user_memory,
            ciphertext: b"opaque ciphertext".to_vec(),
            safe_index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect("encrypted user fixture");
        let (_, user_hash) = user.read_memory_envelope_with_hash(&user_id).await.expect("user hash");
        assert!(
            metadata_amend(&user, METADATA_AMEND_ACTOR, request(&user_id, user_hash, Some("metadata"), &[]))
                .await
                .expect("encrypted user floor")
                .changed
        );

        for (device, scope) in [("dev_b3agent", Scope::Agent), ("dev_b3project", Scope::Project)] {
            let (_temp, substrate) = substrate(device).await;
            let mut memory = fixture_memory();
            set_scope_and_sensitivity(&mut memory, scope, Sensitivity::Internal);
            let id = memory.frontmatter.id.clone();
            write_plaintext(&substrate, memory).await;
            let (_, hash) = substrate.read_memory_envelope_with_hash(&id).await.expect("plaintext hash");
            assert!(
                metadata_amend(&substrate, METADATA_AMEND_ACTOR, request(&id, hash, Some("metadata"), &[]))
                    .await
                    .expect("plaintext floor")
                    .changed
            );
        }
    }

    #[tokio::test]
    async fn tier_fence_refuses_legacy_user_confidential_plaintext_row() {
        let (_temp, substrate) = substrate("dev_b3legacy").await;
        let memory = fixture_memory();
        let id = memory.frontmatter.id.clone();
        write_plaintext(&substrate, memory).await;
        let (before, _) = substrate.read_memory_envelope_with_hash(&id).await.expect("before");
        let path = before.metadata.path.clone().expect("path");
        let mut legacy = before.metadata;
        set_scope_and_sensitivity(&mut legacy, Scope::User, Sensitivity::Confidential);
        std::fs::write(
            substrate.roots().repo.join(path.as_path()),
            serialize_document(&legacy).expect("legacy document"),
        )
        .expect("write legacy fixture");
        let (_, hash) = substrate.read_memory_envelope_with_hash(&id).await.expect("legacy hash");

        let error = metadata_amend(&substrate, METADATA_AMEND_ACTOR, request(&id, hash, Some("metadata"), &[]))
            .await
            .expect_err("tier increase");
        assert_eq!(error.refusal_reason(), Some("metadata_amendment_tier_increase_refused"));
    }

    #[tokio::test]
    async fn encrypted_amend_preserves_ciphertext_and_checks_hash_inside_closure() {
        let (_temp, substrate) = substrate("dev_b3encrypted").await;
        let mut memory = fixture_memory();
        memory.frontmatter.sensitivity = memory_substrate::Sensitivity::Confidential;
        memory.frontmatter.retrieval_policy.index_body = false;
        memory.frontmatter.retrieval_policy.index_embeddings = false;
        let id = memory.frontmatter.id.clone();
        let ciphertext = b"opaque ciphertext bytes".to_vec();
        substrate
            .write_encrypted(EncryptedWriteRequest {
                operation_id: None,
                metadata_memory: memory,
                ciphertext: ciphertext.clone(),
                safe_index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::RequiresEncryption,
            })
            .await
            .expect("encrypted write");
        let (before, hash) = substrate.read_memory_envelope_with_hash(&id).await.expect("before");
        let path = before.metadata.path.clone().expect("encrypted path");

        let stale = amend_encrypted(
            &substrate,
            MetadataAmendWriteRequest {
                id: id.clone(),
                expected_base_hash: Sha256::new("sha256:stale"),
                expected_path: path.clone(),
                abstraction: Some("encrypted metadata".to_string()),
                cues: Vec::new(),
            },
            METADATA_AMEND_ACTOR,
        )
        .await
        .expect_err("closure stale check");
        assert!(matches!(stale, MetadataAmendmentError::MetadataAmendmentStaleBase));

        let changed =
            metadata_amend(&substrate, METADATA_AMEND_ACTOR, request(&id, hash, Some("encrypted metadata"), &[]))
                .await
                .expect("encrypted amend");
        assert!(changed.changed);
        let after = substrate.read_memory_envelope(&id).await.expect("after");
        assert!(matches!(after.content, MemoryContent::Ciphertext { bytes, .. } if bytes == ciphertext));
        assert_eq!(
            after.metadata.frontmatter.extras.get("encryption"),
            before.metadata.frontmatter.extras.get("encryption")
        );
        assert_eq!(metadata_events(&substrate, &id).len(), 1);
    }

    async fn substrate(device: &str) -> (tempfile::TempDir, Substrate) {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate =
            Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some(device.to_string()) })
                .await
                .expect("substrate");
        (temp, substrate)
    }

    async fn write_plaintext(substrate: &Substrate, memory: Memory) {
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
            .expect("write fixture");
    }

    fn request(
        id: &MemoryId,
        expected_base_hash: Sha256,
        abstraction: Option<&str>,
        cues: &[&str],
    ) -> MetadataAmendRequest {
        MetadataAmendRequest {
            id: id.as_str().to_string(),
            expected_base_hash,
            abstraction: abstraction.map(str::to_string),
            cues: cues.iter().map(|cue| (*cue).to_string()).collect(),
        }
    }

    fn metadata_events(substrate: &Substrate, id: &MemoryId) -> Vec<EventKind> {
        substrate
            .events()
            .expect("events")
            .into_iter()
            .filter(|event| matches!(&event.kind, EventKind::MetadataAmended { id: event_id, .. } if event_id == id))
            .map(|event| event.kind)
            .collect()
    }

    fn archive_canonical_memory(repo: &std::path::Path, path: &RepoPath) {
        let canonical = repo.join(path.as_path());
        let mut memory = memory_substrate::frontmatter::parse_document(
            &std::fs::read_to_string(&canonical).expect("canonical document"),
            Some(path.clone()),
        )
        .expect("parse canonical document")
        .memory;
        memory.frontmatter.status = MemoryStatus::Archived;
        std::fs::write(canonical, serialize_document(&memory).expect("archived document"))
            .expect("archive canonical memory");
    }

    fn set_scope_and_sensitivity(memory: &mut Memory, scope: Scope, sensitivity: Sensitivity) {
        memory.frontmatter.scope = scope;
        memory.frontmatter.sensitivity = sensitivity;
        memory.frontmatter.retrieval_policy = default_retrieval_policy(scope, sensitivity);
        if scope == Scope::Project {
            memory.frontmatter.namespace = Some("project".to_string());
            memory.frontmatter.canonical_namespace_id = Some("proj_b3".to_string());
        }
    }

    fn fixture_memory() -> Memory {
        memory_substrate::frontmatter::parse_document(
            "---\nschema_version: 1\nid: mem_20260715_aaaaaaaaaaaaaaaa_000009\ntype: pattern\nscope: agent\nsummary: fixture\nconfidence: 0.9\ntrust_level: trusted\nsensitivity: internal\nstatus: active\ncreated_at: 2026-07-15T00:00:00Z\nupdated_at: 2026-07-15T00:00:00Z\nauthor:\n  kind: system\n  component: test\n---\nbody",
            Some(RepoPath::new("agent/patterns/fixture.md")),
        )
        .expect("fixture")
        .memory
    }
}
