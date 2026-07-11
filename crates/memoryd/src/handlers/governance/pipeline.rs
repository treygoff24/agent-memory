//! Governance write / supersede / forget request handlers and the write executor.
//!
//! Owns the serialized mutation pipeline: the three top-level handlers
//! (`governance_write_response`, `_supersede_`, `_forget_`), the
//! `GOVERNANCE_MUTATION_LOCK` that serializes them, `execute_write_decision`, the
//! privacy-mediated write primitive (`write_privacy_memory`), the supersede
//! claim-lock machinery, the refusal-response builders, and the request DTOs.

use memorum_coordination::claim_lock::{ClaimLockAcquireRequest, ClaimLockAcquireResult};
use memorum_coordination::ClaimLockInfo;
use memory_governance::{GovernanceWriteDecision, PolicySet, PolicySource, TombstoneIndex};
use memory_privacy::{FileKeyProvider, PrivacyDecision, PrivacyEncryptor};
use memory_substrate::{
    events::EventKind, ClassificationOutcome, EncryptedWriteRequest, EventContext, Memory, MemoryContent, MemoryId,
    MemoryStatus, Sensitivity, Substrate, SupersedeRequest as SubstrateSupersedeRequest, TombstoneRequest, TrustLevel,
    WriteMode, WriteRequest as SubstrateWriteRequest,
};
use serde_json::{Map, Value};

use super::meta::{GovernanceMeta, GovernanceWriteInput, GovernanceWriteInputParts, GovernedLifecycle, MetaSource};
use super::policy::{
    active_memory_summaries, existing_summary_from_memory, governance_engine, load_policy_set, load_tombstone_index,
    resolve_similarity_candidates, write_tombstone_rule, GovernanceEngineInput, SimilarityResolution, TiebreakMode,
    TopKSource,
};
use super::privacy::{attach_privacy_scan, classify_input_privacy};
use crate::handlers::{
    namespace_bucket_for_scope, policy_source_string, safe_index_projection, sanitize_forget_reason, HandlerError,
    HandlerState,
};
use crate::protocol::{
    ClaimLockWarning, GovernanceForgetResponse, GovernanceRefusalReason, GovernanceStatus, GovernanceSupersedeResponse,
    GovernanceWriteResponse, ResponsePayload,
};

/// Serializes governance mutations (write / supersede / forget) so each one
/// observes a consistent active-memory snapshot for its duplicate- and
/// contradiction-detection step.
///
/// Duplicate/contradiction detection is a read-active-set → evaluate → write
/// sequence spanning several `.await`s. Without serialization, two concurrent
/// writes of the same claim can each read the active set before either commits,
/// both conclude "no duplicate", and both persist — a duplicate neither
/// detected. This window is reachable on any multi-threaded runtime and widened
/// by the index-backed active-set read. The mutation paths are low-frequency
/// relative to recall/reads, so coarse serialization is an acceptable cost;
/// this mirrors `HandlerState::reality_check_lock`. Acquired before any
/// active-set read and released when the handler returns.
static GOVERNANCE_MUTATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Extra neighbours fetched on top of the engine's effective top-K width for the
/// write-path KNN similarity query.
///
/// We ask the substrate for a few more neighbours than the engine will gate on
/// so that after mapping KNN ids back to the active snapshot (skipping any not
/// yet in it) the engine still sees enough candidates. The engine itself
/// re-truncates to its configured width. With the historical default top-K of 5,
/// `5 + 3 = 8` reproduces the previous fixed over-fetch width exactly.
const WRITE_PATH_SIMILARITY_HEADROOM: usize = 3;

pub(crate) async fn governance_write_response(
    substrate: &Substrate,
    state: Option<&HandlerState>,
    request: GovernanceWriteRequest,
) -> Result<ResponsePayload, HandlerError> {
    let _governance_guard = GOVERNANCE_MUTATION_LOCK.lock().await;
    let mut input = GovernanceWriteInput::parse(GovernanceWriteInputParts {
        body: request.body,
        title: request.title,
        tags: request.tags,
        meta: request.meta,
        source: MetaSource::McpHumanWrite,
    })?;
    input.resolve_project_namespace().await?;
    let privacy = classify_input_privacy(&input)?;
    if let Some(response) = input.privacy_refusal(&privacy) {
        return Ok(ResponsePayload::GovernanceWrite(response));
    }

    let id = substrate.next_memory_id().await.map_err(HandlerError::substrate)?;
    let candidate = input.candidate(id.as_str());
    let (policies, policy_source) = match load_policy_set(substrate.roots().repo.as_path()) {
        Ok(loaded) => loaded,
        Err(error) => {
            return Ok(ResponsePayload::GovernanceWrite(policy_refusal(input.response_namespace(), error.message)))
        }
    };
    let tombstones = match load_tombstone_index(substrate.roots().repo.as_path()) {
        Ok(index) => index,
        Err(error) => {
            return Ok(ResponsePayload::GovernanceWrite(tombstone_refusal(
                input.response_namespace(),
                error.message,
                policy_source,
            )));
        }
    };
    let active = active_memory_summaries(substrate).await?;
    // Production contradiction detection: embed the candidate (query side) and
    // KNN against the active triple's vec table, restricted to in-scope active
    // memories. Normal daemon and public handler entrypoints carry a
    // `HandlerState` (the legacy `handle_request` shim now creates a fresh one),
    // so missing/failed embedding is reported as a degradation marker. The only
    // live `state: None` caller is the reality-check supersede utility path,
    // which intentionally does not participate in write-path similarity. When
    // state is present but the model hasn't loaded yet (or its triple disagrees,
    // the vec table is empty, or KNN/embedding fails), that degradation is
    // surfaced in the response's `similarity_degraded` decision-trace field
    // below rather than silently behaving as if nothing was similar
    // (invariant 3).
    // Over-fetch width tracks the *selected* policy's contradiction top-K so a
    // policy that widens `top_k` still gets enough KNN neighbours to gate on
    // (the engine re-truncates to exactly its width). Falls back to the crate
    // default top-K when the candidate's scope has no resolvable policy.
    let write_path_similarity_limit = policies
        .policy_for_scope(candidate.scope())
        .map(|policy| policy.contradiction_thresholds().top_k)
        .unwrap_or(memory_governance::DEFAULT_CONTRADICTION_TOP_K)
        .saturating_add(WRITE_PATH_SIMILARITY_HEADROOM);
    let similarity = match state {
        Some(state) => {
            let provider_slot = state.embedding_provider_slot();
            resolve_similarity_candidates(
                substrate,
                &provider_slot,
                candidate.claim(),
                candidate.namespace(),
                api_similarity_sensitivity(&privacy),
                &active,
                write_path_similarity_limit,
            )
            .await
        }
        None => SimilarityResolution::not_attempted(),
    };
    let degradation = similarity.degradation;
    let engine = governance_engine(GovernanceEngineInput {
        policies,
        active,
        tombstones,
        tiebreak_mode: TiebreakMode::Unclear,
        top_k_source: similarity.source,
        repo_root: substrate.roots().repo.clone(),
    });
    let decision = engine.evaluate_write(&candidate);
    let mut response =
        execute_write_decision(substrate, WriteExecution { input, id, decision, policy_source, privacy }).await?;
    // Record any degradation in the dedicated decision-trace field (not in
    // `next_actions`, which is a caller action list with an exact-shape
    // contract). A set marker tells the operator the "no contradiction" branch
    // was reached without a real similarity backend.
    response.similarity_degraded = degradation.map(str::to_string);
    Ok(ResponsePayload::GovernanceWrite(response))
}

fn api_similarity_sensitivity(privacy: &PrivacyDecision) -> Option<Sensitivity> {
    if privacy.storage_action.requires_encryption() {
        return None;
    }
    privacy.tier.persisted_sensitivity().filter(|sensitivity| sensitivity.api_lane_eligible())
}

pub(crate) async fn governance_supersede_response(
    substrate: &Substrate,
    state: Option<&HandlerState>,
    request: GovernanceSupersedeRequest,
) -> Result<ResponsePayload, HandlerError> {
    let _governance_guard = GOVERNANCE_MUTATION_LOCK.lock().await;
    let GovernanceSupersedeRequest { old_id, content, reason, meta, preserve_frontmatter } = request;
    let old_memory_id = HandlerError::parse_memory_id(old_id.clone())?;
    let old_envelope = substrate.read_memory_envelope(&old_memory_id).await.map_err(HandlerError::substrate)?;
    let meta = inherit_supersede_namespace_meta(meta, &old_envelope.metadata.frontmatter);
    let mut input = GovernanceWriteInput::parse(GovernanceWriteInputParts {
        body: content,
        title: None,
        tags: Vec::new(),
        meta,
        source: MetaSource::Default,
    })?;
    input.resolve_project_namespace().await?;
    let privacy = classify_input_privacy(&input)?;
    if let Some(refusal) = input.privacy_refusal(&privacy) {
        return Ok(ResponsePayload::GovernanceSupersede(supersede_refused(
            old_id,
            refusal.reason,
            refusal.policy_applied,
            refusal.policy_source,
        )));
    }

    let new_id = substrate.next_memory_id().await.map_err(HandlerError::substrate)?;
    let candidate = input.candidate(new_id.as_str());
    let (policies, policy_source, tombstones) = match load_supersede_inputs(substrate, &old_id) {
        Ok(inputs) => inputs,
        Err(response) => return Ok(ResponsePayload::GovernanceSupersede(*response)),
    };
    // The contradiction detector compares the new candidate against the old body. For
    // encrypted-old memories we can't read the body without an explicit reveal, so we
    // skip body-based contradiction and let the explicit supersede call carry intent:
    // the user has named `old_id`, so we trust the target and only verify the new
    // content passes grounding + policy on its own.
    let old_plaintext_body = match &old_envelope.content {
        MemoryContent::Plaintext(body) => Some(body.clone()),
        MemoryContent::Ciphertext { .. } | MemoryContent::MetadataOnly => None,
    };
    let old_is_encrypted = old_plaintext_body.is_none();
    let bucket_repair_policy_applied = old_plaintext_body
        .as_deref()
        .filter(|old_body| input.is_same_body_bucket_repair(&old_envelope.metadata.frontmatter, old_body))
        .map(|_| old_envelope.metadata.frontmatter.write_policy.policy_applied.clone());

    let (active, tiebreak_mode, top_k_source) = match &old_plaintext_body {
        // Explicit supersede of a plaintext old memory: force the named old
        // memory into the tiebreaker by surfacing it directly from the active
        // set (no embedding needed — the caller already named the target).
        Some(body) => (
            vec![existing_summary_from_memory(old_envelope.metadata.clone(), body.clone())],
            TiebreakMode::Contradiction { existing_id: old_id.clone() },
            TopKSource::ActiveSet,
        ),
        // Encrypted old memory: no body to compare, so contradiction detection
        // is intentionally inert (a real empty answer, not a degraded backend).
        None => (Vec::new(), TiebreakMode::Unclear, TopKSource::Knn(Vec::new())),
    };
    let engine = governance_engine(GovernanceEngineInput {
        policies,
        active,
        tombstones,
        tiebreak_mode,
        top_k_source,
        repo_root: substrate.roots().repo.clone(),
    });
    let decision = engine.evaluate_write(&candidate);
    let policy_applied = match bucket_repair_policy_applied {
        Some(policy_applied) => policy_applied,
        None => match resolve_supersede_policy_applied(old_is_encrypted, decision, &old_id, policy_source) {
            Ok(policy_applied) => policy_applied,
            Err(response) => return Ok(ResponsePayload::GovernanceSupersede(*response)),
        },
    };

    let mut replacement = input.to_memory(
        new_id.clone(),
        GovernedLifecycle::new(MemoryStatus::Active, TrustLevel::Trusted, policy_applied.clone()),
        &privacy,
    )?;
    if preserve_frontmatter {
        let generated = replacement.frontmatter;
        replacement.frontmatter = old_envelope.metadata.frontmatter.clone();
        replacement.frontmatter.id = generated.id;
        replacement.frontmatter.abstraction = generated.abstraction;
        replacement.frontmatter.cues = generated.cues;
        replacement.frontmatter.status = generated.status;
        replacement.frontmatter.trust_level = generated.trust_level;
        replacement.frontmatter.updated_at = generated.updated_at;
        replacement.frontmatter.superseded_by.clear();
    }
    replacement.frontmatter.supersedes.push(old_memory_id.clone());

    let claim_lock = match state {
        Some(state) => acquire_claim_lock_for_supersede(substrate, state, &old_memory_id, &input.meta),
        None => SupersedeClaimLock::inactive(),
    };

    // Write the replacement + mark the old superseded. Stream A's `supersede_memory`
    // is plaintext-only (`read_memory_with_hash` skips encrypted/ paths and
    // `write_memory` refuses RequiresEncryption classifications), so for the three
    // mixed cases we route the writes ourselves and call the existing `write_privacy_memory`
    // and `update_encrypted_memory_metadata` primitives — same building blocks the
    // governance write + forget paths already use for encrypted records.
    let new_is_encrypted = privacy.storage_action.requires_encryption();
    if !old_is_encrypted && !new_is_encrypted {
        substrate
            .supersede_memory(SubstrateSupersedeRequest {
                old_id: old_memory_id.clone(),
                replacement,
                reason: reason.clone(),
                classification: privacy.tier.classification(),
                allow_best_effort_durability: true,
            })
            .await
            .map_err(HandlerError::substrate)?;
    } else {
        write_privacy_memory(
            substrate,
            replacement,
            &privacy,
            EventContext { actor: Some("memoryd-supersede".to_string()), reason: Some(reason.clone()) },
        )
        .await?;
        mark_old_superseded(
            substrate,
            MarkOldSuperseded { old_id: &old_memory_id, new_id: &new_id, old_is_encrypted, reason: &reason },
        )
        .await?;
    }

    let warning = claim_lock.release_after_success();

    Ok(ResponsePayload::GovernanceSupersede(GovernanceSupersedeResponse {
        status: GovernanceStatus::Promoted,
        new_id: Some(new_id.as_str().to_string()),
        old_id: Some(old_id.clone()),
        reason: None,
        chain: Some(serde_json::json!({ "supersedes": [old_id] })),
        policy_applied: Some(policy_applied),
        policy_source: Some(policy_source_string(policy_source)),
        warning,
    }))
}

fn inherit_supersede_namespace_meta(meta: Value, old: &memory_substrate::Frontmatter) -> Value {
    let mut fields = match meta {
        Value::Null => Map::new(),
        Value::Object(fields) => fields,
        other => return other,
    };

    let effective_namespace = match fields.get("namespace").and_then(Value::as_str) {
        Some(namespace) => namespace.to_string(),
        None => {
            let namespace = namespace_bucket_for_scope(old.scope);
            fields.insert("namespace".to_string(), Value::String(namespace.to_string()));
            namespace.to_string()
        }
    };

    // Inheritance from the old memory beats the bridge-injected `cwd`: a
    // supersede edits an existing memory in place, so its bucket must not
    // migrate to whatever project the caller happens to be sitting in. The
    // `cwd` fallback only fires for legacy project memories that carry no
    // namespace identity. Explicit caller-supplied ids still win via `entry`.
    if effective_namespace == "project" {
        if let Some(canonical_namespace_id) = &old.canonical_namespace_id {
            fields
                .entry("canonical_namespace_id".to_string())
                .or_insert_with(|| Value::String(canonical_namespace_id.clone()));
        }
        if let Some(namespace_alias) = &old.namespace {
            fields.entry("namespace_alias".to_string()).or_insert_with(|| Value::String(namespace_alias.clone()));
        }
    }

    Value::Object(fields)
}

struct MarkOldSuperseded<'a> {
    old_id: &'a MemoryId,
    new_id: &'a MemoryId,
    old_is_encrypted: bool,
    reason: &'a str,
}

/// Mark the old memory as `Superseded` and append `new_id` to its `superseded_by`
/// chain. Used by the mixed-encryption supersede paths, where Stream A's atomic
/// `supersede_memory` can't drive the two-write pair because either the old read
/// or the new write would land under `encrypted/`. Routes through the appropriate
/// Stream A primitive based on whether the old record is encrypted on disk.
async fn mark_old_superseded(
    substrate: &Substrate,
    MarkOldSuperseded { old_id, new_id, old_is_encrypted, reason }: MarkOldSuperseded<'_>,
) -> Result<(), HandlerError> {
    let new_id_for_chain = new_id.clone();
    if old_is_encrypted {
        substrate
            .update_encrypted_memory_metadata(old_id, |old| {
                old.frontmatter.status = MemoryStatus::Superseded;
                old.frontmatter.updated_at = chrono::Utc::now();
                if !old.frontmatter.superseded_by.contains(&new_id_for_chain) {
                    old.frontmatter.superseded_by.push(new_id_for_chain);
                }
            })
            .await
            .map_err(|err| HandlerError::substrate(format!("update encrypted metadata: {err:?}")))?;
        return Ok(());
    }
    // Plaintext old + encrypted new: rewrite the plaintext old in place. We pass
    // `expected_base_hash: None` here — Stream A's public surface doesn't expose
    // the read-hash, and the supersede call is daemon-mediated and synchronous,
    // so the TOCTOU window is tight. The same trade-off applies to the equivalent
    // path in `governance_forget_response` and `review_decision_response`.
    let mut old_memory = substrate
        .read_memory(old_id)
        .await
        .map_err(|err| HandlerError::substrate(format!("read old memory for supersede: {err:?}")))?;
    old_memory.frontmatter.status = MemoryStatus::Superseded;
    old_memory.frontmatter.updated_at = chrono::Utc::now();
    if !old_memory.frontmatter.superseded_by.contains(&new_id_for_chain) {
        old_memory.frontmatter.superseded_by.push(new_id_for_chain);
    }
    let old_classification = super::privacy::classify_plaintext_memory(&old_memory)?;
    substrate
        .write_memory(SubstrateWriteRequest {
            operation_id: None,
            memory: old_memory,
            expected_base_hash: None,
            write_mode: WriteMode::ReplaceExisting,
            index_projection: None,
            event_context: EventContext {
                actor: Some("memoryd-supersede".to_string()),
                reason: Some(reason.to_string()),
            },
            allow_best_effort_durability: true,
            classification: old_classification,
        })
        .await
        .map_err(|err| HandlerError::substrate(format!("mark old superseded: {err:?}")))?;
    Ok(())
}

fn acquire_claim_lock_for_supersede<'a>(
    substrate: &Substrate,
    state: &'a HandlerState,
    memory_id: &MemoryId,
    meta: &GovernanceMeta,
) -> SupersedeClaimLock<'a> {
    if state.effective_coordination_level(meta) < 2 {
        return SupersedeClaimLock::inactive();
    }

    let result = state.claim_locks.acquire(ClaimLockAcquireRequest::new(
        memory_id.as_str(),
        meta.session_id.as_str(),
        meta.harness.as_str(),
        state.claim_lock_ttl(),
    ));
    match result {
        ClaimLockAcquireResult::Acquired(_) => SupersedeClaimLock::acquired(state, memory_id, meta),
        ClaimLockAcquireResult::AlreadyHeld(_) => SupersedeClaimLock::already_held(state, memory_id, meta),
        ClaimLockAcquireResult::Contended(contention) => {
            let holder = contention.holder_label();
            let contender = contention.contender_label();
            if let Err(error) = substrate.record_event_best_effort(EventKind::ClaimLockContention {
                memory_id: memory_id.clone(),
                holder: holder.clone(),
                contender,
            }) {
                tracing::warn!(
                    memory_id = memory_id.as_str(),
                    "claim-lock contention event append failed; proceeding with advisory warning: {error}"
                );
            }

            SupersedeClaimLock::contended(
                state,
                SupersedeClaimIdentity::new(memory_id, meta),
                contention.holder,
                ClaimLockWarning { code: contention.warning_code.to_string(), message: contention.message, holder },
            )
        }
    }
}

enum ClaimLockRollback {
    None,
    ReleaseAcquired,
    RestorePrevious(ClaimLockInfo),
}

/// A supersede claim-lock guard. Either `Inactive` (coordination disabled — no
/// lock was taken and nothing to release or roll back) or `Active`, which owns
/// the lock identity, the rollback strategy if the supersede fails before
/// success, and any advisory contention warning to surface on success.
/// RAII guard for a supersede claim lock. Holds `Some(ActiveClaimLock)` while a
/// lock is held and `None` when there is nothing to release or roll back.
/// `release_after_success` and `Drop` `take()` the active state to defuse the
/// guard — a type that implements `Drop` cannot be destructured to move its
/// payload out (E0509), so the active state lives behind an `Option` instead.
struct SupersedeClaimLock<'a> {
    active: Option<ActiveClaimLock<'a>>,
}

struct ActiveClaimLock<'a> {
    state: &'a HandlerState,
    identity: SupersedeClaimIdentity,
    rollback: ClaimLockRollback,
    warning: Option<ClaimLockWarning>,
}

impl<'a> SupersedeClaimLock<'a> {
    fn inactive() -> Self {
        Self { active: None }
    }

    fn acquired(state: &'a HandlerState, memory_id: &MemoryId, meta: &GovernanceMeta) -> Self {
        Self::active(state, SupersedeClaimIdentity::new(memory_id, meta), ClaimLockRollback::ReleaseAcquired, None)
    }

    fn already_held(state: &'a HandlerState, memory_id: &MemoryId, meta: &GovernanceMeta) -> Self {
        Self::active(state, SupersedeClaimIdentity::new(memory_id, meta), ClaimLockRollback::None, None)
    }

    fn contended(
        state: &'a HandlerState,
        identity: SupersedeClaimIdentity,
        previous_holder: ClaimLockInfo,
        warning: ClaimLockWarning,
    ) -> Self {
        Self::active(state, identity, ClaimLockRollback::RestorePrevious(previous_holder), Some(warning))
    }

    fn active(
        state: &'a HandlerState,
        identity: SupersedeClaimIdentity,
        rollback: ClaimLockRollback,
        warning: Option<ClaimLockWarning>,
    ) -> Self {
        Self { active: Some(ActiveClaimLock { state, identity, rollback, warning }) }
    }

    fn release_after_success(mut self) -> Option<ClaimLockWarning> {
        // Take the active state out so the trailing `Drop` sees `None` and skips
        // rollback: the supersede succeeded, so the lock is released cleanly and
        // only the advisory warning travels onward.
        let active = self.active.take()?;
        let ActiveClaimLock { state, identity, warning, .. } = active;
        state.claim_locks.release(&identity.memory_id, &identity.harness, &identity.session_id);
        warning
    }
}

struct SupersedeClaimIdentity {
    memory_id: String,
    harness: String,
    session_id: String,
}

impl SupersedeClaimIdentity {
    fn new(memory_id: &MemoryId, meta: &GovernanceMeta) -> Self {
        Self {
            memory_id: memory_id.as_str().to_string(),
            harness: meta.harness.clone(),
            session_id: meta.session_id.clone(),
        }
    }
}

impl Drop for SupersedeClaimLock<'_> {
    fn drop(&mut self) {
        let Some(active) = self.active.take() else {
            return;
        };
        let ActiveClaimLock { state, identity, rollback, .. } = active;

        match rollback {
            ClaimLockRollback::None => {}
            ClaimLockRollback::ReleaseAcquired => {
                state.claim_locks.release(&identity.memory_id, &identity.harness, &identity.session_id);
            }
            ClaimLockRollback::RestorePrevious(previous_holder) => {
                state.claim_locks.release(&identity.memory_id, &identity.harness, &identity.session_id);
                let _restored = state.claim_locks.restore(previous_holder);
            }
        }
    }
}

pub(crate) async fn governance_forget_response(
    substrate: &Substrate,
    id: String,
    reason: String,
) -> Result<ResponsePayload, HandlerError> {
    let _governance_guard = GOVERNANCE_MUTATION_LOCK.lock().await;
    if reason.trim().is_empty() {
        return Err(HandlerError::invalid_request("forget reason must not be empty"));
    }
    let reason = sanitize_forget_reason(&reason);
    let memory_id = HandlerError::parse_memory_id(id.clone())?;
    let envelope = substrate.read_memory_envelope(&memory_id).await.map_err(HandlerError::substrate)?;
    let tombstone_claim = match &envelope.content {
        MemoryContent::Plaintext(body) if !body.is_empty() => body.clone(),
        MemoryContent::Ciphertext { .. } | MemoryContent::MetadataOnly | MemoryContent::Plaintext(_) => {
            envelope.metadata.frontmatter.summary.clone()
        }
    };
    substrate
        .tombstone_memory(TombstoneRequest { id: memory_id, reason: reason.clone() })
        .await
        .map_err(HandlerError::substrate)?;
    write_tombstone_rule(substrate.roots().repo.as_path(), &envelope.metadata, &tombstone_claim, &reason)?;
    Ok(ResponsePayload::GovernanceForget(GovernanceForgetResponse {
        status: GovernanceStatus::Tombstoned,
        id,
        tombstone_ref: Some("tombstone:stream-a".to_string()),
        reason: None,
    }))
}

async fn execute_write_decision(
    substrate: &Substrate,
    execution: WriteExecution,
) -> Result<GovernanceWriteResponse, HandlerError> {
    let WriteExecution { input, id, decision, policy_source, privacy } = execution;
    match decision {
        GovernanceWriteDecision::Promoted { namespace, policy_applied, .. } => {
            let memory = input.to_memory(
                id.clone(),
                GovernedLifecycle::new(MemoryStatus::Active, TrustLevel::Trusted, policy_applied.clone()),
                &privacy,
            )?;
            write_governed_memory(substrate, memory, &privacy).await?;
            Ok(GovernanceWriteResponse {
                status: GovernanceStatus::Promoted,
                id: Some(id.as_str().to_string()),
                namespace: Some(namespace),
                reason: None,
                next_actions: Vec::new(),
                policy_applied: Some(policy_applied),
                policy_source: Some(policy_source_string(policy_source)),
                existing_id: None,
                similarity_degraded: None,
            })
        }
        GovernanceWriteDecision::Candidate { reason, policy_applied, .. } => {
            let memory = input.to_memory(
                id.clone(),
                GovernedLifecycle::new(MemoryStatus::Candidate, TrustLevel::Candidate, policy_applied.clone()),
                &privacy,
            )?;
            write_governed_memory(substrate, memory, &privacy).await?;
            Ok(GovernanceWriteResponse {
                status: GovernanceStatus::Candidate,
                id: Some(id.as_str().to_string()),
                namespace: Some(input.response_namespace()),
                reason: None,
                next_actions: vec![reason],
                policy_applied: Some(policy_applied),
                policy_source: Some(policy_source_string(policy_source)),
                existing_id: None,
                similarity_degraded: None,
            })
        }
        GovernanceWriteDecision::Quarantined { reason, policy_applied, .. } => {
            let memory = input.to_memory(
                id.clone(),
                GovernedLifecycle::new(MemoryStatus::Quarantined, TrustLevel::Quarantined, policy_applied.clone()),
                &privacy,
            )?;
            write_governed_memory(substrate, memory, &privacy).await?;
            Ok(GovernanceWriteResponse {
                status: GovernanceStatus::Quarantined,
                id: Some(id.as_str().to_string()),
                namespace: Some(input.response_namespace()),
                reason: None,
                next_actions: vec![reason],
                policy_applied: Some(policy_applied),
                policy_source: Some(policy_source_string(policy_source)),
                existing_id: None,
                similarity_degraded: None,
            })
        }
        GovernanceWriteDecision::Duplicate { existing_id, .. } => Ok(GovernanceWriteResponse {
            status: GovernanceStatus::Promoted,
            id: Some(existing_id.clone()),
            namespace: Some(input.response_namespace()),
            reason: None,
            next_actions: Vec::new(),
            policy_applied: None,
            policy_source: Some(policy_source_string(policy_source)),
            existing_id: Some(existing_id),
            similarity_degraded: None,
        }),
        GovernanceWriteDecision::Refinement { existing_id, .. } => Ok(GovernanceWriteResponse {
            status: GovernanceStatus::Promoted,
            id: Some(existing_id.clone()),
            namespace: Some(input.response_namespace()),
            reason: None,
            next_actions: vec!["merge_evidence".to_string()],
            policy_applied: None,
            policy_source: Some(policy_source_string(policy_source)),
            existing_id: Some(existing_id),
            similarity_degraded: None,
        }),
        GovernanceWriteDecision::Supersession { existing_id, policy_applied, .. } => Ok(GovernanceWriteResponse {
            status: GovernanceStatus::Candidate,
            // The write path does not persist a new memory for this arm; callers
            // must invoke `memory_supersede` explicitly, so there is no new id to
            // return here.
            id: None,
            namespace: Some(input.response_namespace()),
            reason: None,
            next_actions: vec!["memory_supersede".to_string()],
            policy_applied: Some(policy_applied),
            policy_source: Some(policy_source_string(policy_source)),
            existing_id: Some(existing_id),
            similarity_degraded: None,
        }),
        GovernanceWriteDecision::Refused { reason, .. } => Ok(GovernanceWriteResponse {
            status: GovernanceStatus::Refused,
            id: None,
            namespace: Some(input.response_namespace()),
            reason: Some(reason),
            next_actions: Vec::new(),
            policy_applied: None,
            policy_source: Some(policy_source_string(policy_source)),
            existing_id: None,
            similarity_degraded: None,
        }),
    }
}

async fn write_governed_memory(
    substrate: &Substrate,
    memory: Memory,
    privacy: &PrivacyDecision,
) -> Result<(), HandlerError> {
    write_privacy_memory(
        substrate,
        memory,
        privacy,
        EventContext {
            actor: Some("memoryd-governance".to_string()),
            reason: Some("governed privacy-mediated write".to_string()),
        },
    )
    .await
}

pub(crate) async fn write_privacy_memory(
    substrate: &Substrate,
    mut memory: Memory,
    privacy: &PrivacyDecision,
    event_context: EventContext,
) -> Result<(), HandlerError> {
    if privacy.storage_action.refuses_storage() {
        return Err(HandlerError::invalid_request("privacy refused secret before disk effects"));
    }
    attach_privacy_scan(&mut memory, privacy);
    if privacy.storage_action.requires_encryption() {
        let encryptor = PrivacyEncryptor::new(FileKeyProvider::runtime_default(&substrate.roots().runtime));
        let encrypted = encryptor.encrypt(&memory.body).map_err(HandlerError::privacy)?;
        memory.frontmatter.extras.insert("encryption".to_string(), encrypted.envelope);
        let safe_index_projection = safe_index_projection(&memory);
        substrate
            .write_encrypted(EncryptedWriteRequest {
                operation_id: None,
                metadata_memory: memory,
                ciphertext: encrypted.ciphertext,
                // Stream D: encrypted records index only descriptors already proven safe.
                // Do NOT project raw or masked body text here; see stream-d-security-review P0.
                safe_index_projection,
                event_context,
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::RequiresEncryption,
            })
            .await
            .map(|_| ())
            .map_err(HandlerError::substrate)
    } else {
        substrate
            .write_memory(SubstrateWriteRequest {
                operation_id: None,
                memory,
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context,
                allow_best_effort_durability: true,
                classification: privacy.tier.classification(),
            })
            .await
            .map(|_| ())
            .map_err(HandlerError::substrate)
    }
}

fn policy_refusal(namespace: String, message: String) -> GovernanceWriteResponse {
    GovernanceWriteResponse {
        status: GovernanceStatus::Refused,
        id: None,
        namespace: Some(namespace),
        reason: Some(GovernanceRefusalReason::Policy),
        next_actions: vec![message],
        policy_applied: None,
        policy_source: None,
        existing_id: None,
        similarity_degraded: None,
    }
}

fn tombstone_refusal(namespace: String, message: String, policy_source: PolicySource) -> GovernanceWriteResponse {
    GovernanceWriteResponse {
        status: GovernanceStatus::Refused,
        id: None,
        namespace: Some(namespace),
        reason: Some(GovernanceRefusalReason::Tombstone),
        next_actions: vec![message],
        policy_applied: None,
        policy_source: Some(policy_source_string(policy_source)),
        existing_id: None,
        similarity_degraded: None,
    }
}

/// Build a refused supersede response. Centralizes the shared field shape
/// (`status: Refused`, no `new_id`/`chain`, no `warning`) so the handler's
/// several refusal exits read as one-liners.
fn supersede_refused(
    old_id: String,
    reason: Option<GovernanceRefusalReason>,
    policy_applied: Option<String>,
    policy_source: Option<String>,
) -> GovernanceSupersedeResponse {
    GovernanceSupersedeResponse {
        status: GovernanceStatus::Refused,
        new_id: None,
        old_id: Some(old_id),
        reason,
        chain: None,
        policy_applied,
        policy_source,
        warning: None,
    }
}

fn supersede_refusal(
    old_id: String,
    decision: GovernanceWriteDecision,
    policy_source: PolicySource,
) -> GovernanceSupersedeResponse {
    let (reason, policy_applied) = match decision {
        GovernanceWriteDecision::Refused { reason, .. } => (reason, None),
        GovernanceWriteDecision::Duplicate { .. } => (GovernanceRefusalReason::Superseded, None),
        GovernanceWriteDecision::Refinement { .. } => (GovernanceRefusalReason::Contradiction, None),
        GovernanceWriteDecision::Candidate { policy_applied, .. }
        | GovernanceWriteDecision::Quarantined { policy_applied, .. }
        | GovernanceWriteDecision::Promoted { policy_applied, .. } => {
            (GovernanceRefusalReason::Contradiction, Some(policy_applied))
        }
        GovernanceWriteDecision::Supersession { policy_applied, .. } => {
            (GovernanceRefusalReason::Contradiction, Some(policy_applied))
        }
    };
    supersede_refused(old_id, Some(reason), policy_applied, Some(policy_source_string(policy_source)))
}

/// Load the policy set and tombstone index for a supersede, mapping either load
/// failure to its refusal response. Returns the loaded inputs, or the refusal the
/// handler should surface.
fn load_supersede_inputs(
    substrate: &Substrate,
    old_id: &str,
) -> Result<(PolicySet, PolicySource, TombstoneIndex), Box<GovernanceSupersedeResponse>> {
    let (policies, policy_source) = load_policy_set(substrate.roots().repo.as_path()).map_err(|error| {
        Box::new(supersede_refused(
            old_id.to_string(),
            Some(GovernanceRefusalReason::Policy),
            None,
            Some(error.message),
        ))
    })?;
    let tombstones = load_tombstone_index(substrate.roots().repo.as_path()).map_err(|error| {
        Box::new(supersede_refused(
            old_id.to_string(),
            Some(GovernanceRefusalReason::Tombstone),
            None,
            Some(error.message),
        ))
    })?;
    Ok((policies, policy_source, tombstones))
}

/// Resolve the engine decision into the accepted supersede's `policy_applied`, or
/// the refusal to surface. The plaintext-old path requires the detected supersession
/// target to match the caller-named `old_id`; the encrypted-old path can't run
/// body-based contradiction (`active = []`), so it accepts Promoted/Candidate/
/// Supersession and lets the explicit supersede intent stand. Other decisions refuse.
fn resolve_supersede_policy_applied(
    old_is_encrypted: bool,
    decision: GovernanceWriteDecision,
    old_id: &str,
    policy_source: PolicySource,
) -> Result<String, Box<GovernanceSupersedeResponse>> {
    match (old_is_encrypted, decision) {
        (false, GovernanceWriteDecision::Supersession { existing_id, policy_applied, .. }) => {
            if existing_id != old_id {
                return Err(Box::new(supersede_refused(
                    old_id.to_string(),
                    Some(GovernanceRefusalReason::Contradiction),
                    Some(policy_applied),
                    Some(policy_source_string(policy_source)),
                )));
            }
            Ok(policy_applied)
        }
        (false, other) => Err(Box::new(supersede_refusal(old_id.to_string(), other, policy_source))),
        (true, GovernanceWriteDecision::Promoted { policy_applied, .. })
        | (true, GovernanceWriteDecision::Candidate { policy_applied, .. })
        | (true, GovernanceWriteDecision::Supersession { policy_applied, .. }) => Ok(policy_applied),
        (true, other) => Err(Box::new(supersede_refusal(old_id.to_string(), other, policy_source))),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GovernanceWriteRequest {
    pub(crate) body: String,
    pub(crate) title: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) meta: Value,
}

#[derive(Clone, Debug)]
pub(crate) struct GovernanceSupersedeRequest {
    pub(crate) old_id: String,
    pub(crate) content: String,
    pub(crate) reason: String,
    pub(crate) meta: Value,
    pub(crate) preserve_frontmatter: bool,
}

#[derive(Clone, Debug)]
struct WriteExecution {
    input: GovernanceWriteInput,
    id: MemoryId,
    decision: GovernanceWriteDecision,
    policy_source: PolicySource,
    privacy: PrivacyDecision,
}
