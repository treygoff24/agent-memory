//! Governance write / supersede / forget request handlers and the meta model.
//!
//! Owns the governance write pipeline (`governance_write_response`, `_supersede_`,
//! `_forget_`), the claim-lock supersede machinery, privacy-classification glue,
//! policy/tombstone loading, the contradiction engine adapters, and the
//! `GovernanceMeta` deserialization model. Shared helpers (`serialized_payload_len`,
//! `sanitize_forget_reason`, the `*_meta` accessors, the `candidate_memory` cluster)
//! remain in `mod.rs` and are reached via `use super::*`.

use super::*;

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

pub(crate) async fn governance_write_response(
    substrate: &Substrate,
    request: GovernanceWriteRequest,
) -> Result<ResponsePayload, HandlerError> {
    let _governance_guard = GOVERNANCE_MUTATION_LOCK.lock().await;
    let input = GovernanceWriteInput::parse(GovernanceWriteInputParts {
        body: request.body,
        title: request.title,
        tags: request.tags,
        meta: request.meta,
        source: MetaSource::McpHumanWrite,
    })?;
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
    let engine = governance_engine(GovernanceEngineInput {
        policies,
        active,
        tombstones,
        tiebreak_mode: TiebreakMode::Unclear,
        allow_top_k: false,
        repo_root: substrate.roots().repo.clone(),
    });
    let decision = engine.evaluate_write(&candidate);
    let response =
        execute_write_decision(substrate, WriteExecution { input, id, decision, policy_source, privacy }).await?;
    Ok(ResponsePayload::GovernanceWrite(response))
}

pub(crate) async fn governance_supersede_response(
    substrate: &Substrate,
    state: Option<&HandlerState>,
    request: GovernanceSupersedeRequest,
) -> Result<ResponsePayload, HandlerError> {
    let _governance_guard = GOVERNANCE_MUTATION_LOCK.lock().await;
    let GovernanceSupersedeRequest { old_id, content, reason, meta } = request;
    let old_memory_id = HandlerError::parse_memory_id(old_id.clone())?;
    let input = GovernanceWriteInput::parse(GovernanceWriteInputParts {
        body: content,
        title: None,
        tags: Vec::new(),
        meta,
        source: MetaSource::Default,
    })?;
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
    let old_envelope = substrate.read_memory_envelope(&old_memory_id).await.map_err(HandlerError::substrate)?;

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

    let (active, tiebreak_mode, allow_top_k) = match &old_plaintext_body {
        Some(body) => (
            vec![existing_summary_from_memory(old_envelope.metadata.clone(), body.clone())],
            TiebreakMode::Contradiction { existing_id: old_id.clone() },
            true,
        ),
        None => (Vec::new(), TiebreakMode::Unclear, false),
    };
    let engine = governance_engine(GovernanceEngineInput {
        policies,
        active,
        tombstones,
        tiebreak_mode,
        allow_top_k,
        repo_root: substrate.roots().repo.clone(),
    });
    let decision = engine.evaluate_write(&candidate);
    let policy_applied = match resolve_supersede_policy_applied(old_is_encrypted, decision, &old_id, policy_source) {
        Ok(policy_applied) => policy_applied,
        Err(response) => return Ok(ResponsePayload::GovernanceSupersede(*response)),
    };

    let mut replacement = input.to_memory(
        new_id.clone(),
        GovernedLifecycle::new(MemoryStatus::Active, TrustLevel::Trusted, policy_applied.clone()),
        &privacy,
    )?;
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
            classification: ClassificationOutcome::Trusted,
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

struct SupersedeClaimLock<'a> {
    state: Option<&'a HandlerState>,
    memory_id: String,
    harness: String,
    session_id: String,
    release_on_success: bool,
    rollback: ClaimLockRollback,
    warning: Option<ClaimLockWarning>,
    completed: bool,
}

impl<'a> SupersedeClaimLock<'a> {
    fn inactive() -> Self {
        Self {
            state: None,
            memory_id: String::new(),
            harness: String::new(),
            session_id: String::new(),
            release_on_success: false,
            rollback: ClaimLockRollback::None,
            warning: None,
            completed: true,
        }
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
        Self {
            state: Some(state),
            memory_id: identity.memory_id,
            harness: identity.harness,
            session_id: identity.session_id,
            release_on_success: true,
            rollback,
            warning,
            completed: false,
        }
    }

    fn release_after_success(mut self) -> Option<ClaimLockWarning> {
        if self.release_on_success {
            if let Some(state) = self.state {
                state.claim_locks.release(&self.memory_id, &self.harness, &self.session_id);
            }
        }
        self.completed = true;
        self.warning.take()
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
        if self.completed {
            return;
        }

        let Some(state) = self.state else {
            return;
        };

        match &self.rollback {
            ClaimLockRollback::None => {}
            ClaimLockRollback::ReleaseAcquired => {
                state.claim_locks.release(&self.memory_id, &self.harness, &self.session_id);
            }
            ClaimLockRollback::RestorePrevious(previous_holder) => {
                state.claim_locks.release(&self.memory_id, &self.harness, &self.session_id);
                let _restored = state.claim_locks.restore(previous_holder.clone());
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
        }),
        GovernanceWriteDecision::Supersession { existing_id, policy_applied, .. } => Ok(GovernanceWriteResponse {
            status: GovernanceStatus::Candidate,
            id: Some(id.as_str().to_string()),
            namespace: Some(input.response_namespace()),
            reason: None,
            next_actions: vec!["memory_supersede".to_string()],
            policy_applied: Some(policy_applied),
            policy_source: Some(policy_source_string(policy_source)),
            existing_id: Some(existing_id),
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

fn classify_input_privacy(input: &GovernanceWriteInput) -> Result<PrivacyDecision, HandlerError> {
    classify_privacy(&input.privacy_scan_text(), input.privacy_namespace(), input.caller_sensitivity())
}

pub(crate) fn classify_privacy(
    text: &str,
    namespace: PrivacyNamespace,
    caller: Option<CallerSensitivity>,
) -> Result<PrivacyDecision, HandlerError> {
    DeterministicPrivacyClassifier::new().classify(text, namespace, caller).map_err(HandlerError::privacy)
}

fn attach_privacy_scan(memory: &mut Memory, privacy: &PrivacyDecision) {
    memory.frontmatter.extras.insert(
        "privacy_scan".to_string(),
        serde_json::to_value(&privacy.scan).expect("privacy scan always serializes"),
    );
}

pub(crate) fn load_policy_set(repo: &Path) -> Result<(PolicySet, PolicySource), HandlerError> {
    let policy_dir = repo.join("policies");
    let has_yaml = std::fs::read_dir(&policy_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| entry.path().extension().is_some_and(|extension| extension == "yaml"));

    if has_yaml {
        match PolicySet::load_from_dir(&policy_dir) {
            Ok(policies) => return Ok((policies, PolicySource::Disk)),
            Err(error) => return Err(HandlerError::invalid_request(format!("invalid governance policy: {error}"))),
        }
    }

    Ok((PolicySet::builtin(), PolicySource::BuiltInFallback))
}

fn load_tombstone_index(repo: &Path) -> Result<TombstoneIndex, HandlerError> {
    let tombstone_dir = repo.join("tombstones");
    if !tombstone_dir.exists() {
        return Ok(TombstoneIndex::default());
    }
    TombstoneIndex::load_jsonl_dir(&tombstone_dir)
        .map_err(|error| HandlerError::invalid_request(format!("invalid tombstone rules: {error}")))
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

fn existing_summary_from_memory(memory: Memory, body: String) -> ExistingMemorySummary {
    ExistingMemorySummary::new(
        memory.frontmatter.id.as_str().to_string(),
        namespace_for_frontmatter(&memory.frontmatter),
        body,
        1.0,
    )
    .with_entity_ids(entity_ids(&memory.frontmatter))
}

fn write_tombstone_rule(repo: &Path, memory: &Memory, claim: &str, reason: &str) -> Result<(), HandlerError> {
    let tombstone_dir = repo.join("tombstones");
    std::fs::create_dir_all(&tombstone_dir)
        .map_err(|error| HandlerError::substrate(format!("create tombstone dir: {error}")))?;
    let key = memory_governance::CandidateTombstoneKey::from_claim(claim, entity_ids(&memory.frontmatter))
        .with_target_memory_id(memory.frontmatter.id.as_str().to_string());
    let rule = TombstoneRule {
        id: format!("tomb_{}", memory.frontmatter.id.as_str()),
        target_memory_id: Some(memory.frontmatter.id.as_str().to_string()),
        content_hash: key.content_hash,
        entity_hash: key.entity_hash,
        reason: TombstoneKind::UserForget,
        reason_text: Some(reason.to_string()),
        active: true,
    };
    let path = tombstone_dir.join("memoryd-forget.jsonl");
    let mut file =
        OpenOptions::new().create(true).append(true).open(&path).map_err(|error| {
            HandlerError::substrate(format!("open tombstone rule file {}: {error}", path.display()))
        })?;
    let line = serde_json::to_string(&rule)
        .map_err(|error| HandlerError::substrate(format!("serialize tombstone rule: {error}")))?;
    writeln!(file, "{line}")
        .map_err(|error| HandlerError::substrate(format!("append tombstone rule file {}: {error}", path.display())))?;
    Ok(())
}

struct GovernanceEngineInput {
    policies: PolicySet,
    active: Vec<ExistingMemorySummary>,
    tombstones: TombstoneIndex,
    tiebreak_mode: TiebreakMode,
    allow_top_k: bool,
    repo_root: PathBuf,
}

fn governance_engine(
    input: GovernanceEngineInput,
) -> GovernanceEngine<MemorydSimilaritySearch, MemorydTiebreaker, MemorydSessionResolver, ArtifactStore> {
    GovernanceEngine::new(
        input.policies,
        GroundingVerifier::new_with_web_capture_resolver(
            FileSourceResolver,
            MemorydSessionResolver,
            ArtifactStore::new(input.repo_root),
        ),
        input.tombstones,
        GovernanceProviders::new(
            MemorydSimilaritySearch::new(input.active, input.allow_top_k),
            MemorydTiebreaker { tiebreak_mode: input.tiebreak_mode },
        ),
    )
}

/// Upper bound on active-memory envelope reads in flight at once.
///
/// `active_memory_summaries` reads one canonical file per active memory, and
/// each read is a synchronous `std::fs` read + Markdown parse moved onto the
/// blocking pool. Spawning one task per memory with no cap would flood the
/// runtime (the active set is unbounded, unlike search hits which are capped at
/// `SEARCH_LIMIT_MAX`); a fixed window keeps the fan-out wide enough to hide
/// per-read latency while bounding blocking-pool and file-descriptor pressure
/// regardless of corpus size.
const ACTIVE_SUMMARY_READ_CONCURRENCY: usize = 16;

/// Build the active-memory candidate set for governance contradiction / claim-hash
/// matching.
///
/// Index-first: the derived index already knows which memories are `Active` and
/// plaintext, so we ask it for exactly those paths instead of reading and
/// frontmatter-parsing *every* canonical file just to discard non-active /
/// encrypted ones. The candidate set the engine actually needs (claim hash,
/// entity hash, namespace) still requires each memory's body to hash, so we read
/// only the active-plaintext envelopes.
///
/// Each read is a synchronous disk read + Markdown parse, so we move the reads
/// onto the blocking pool via `spawn_blocking` (calling the synchronous
/// `read_path_envelope_blocking`) rather than occupying async worker threads,
/// and gate the fan-out with a semaphore at `ACTIVE_SUMMARY_READ_CONCURRENCY` so
/// a large active set cannot saturate the runtime. Results are reassembled by
/// position so the candidate set order still matches the index query.
///
/// Per-memory derivation (namespace, entity ids, body) is computed from the read
/// envelope's frontmatter exactly as before, so the engine sees an identical
/// candidate set; only its construction moved off the full repo walk.
async fn active_memory_summaries(substrate: &Substrate) -> Result<Vec<ExistingMemorySummary>, HandlerError> {
    let active_rows = substrate
        .query_recall_index(RecallIndexQuery {
            statuses: vec![MemoryStatus::Active],
            hydrate: AuxScope::None,
            ..RecallIndexQuery::default()
        })
        .await
        .map_err(HandlerError::substrate)?;

    let semaphore = Arc::new(tokio::sync::Semaphore::new(ACTIVE_SUMMARY_READ_CONCURRENCY));
    let mut reads = tokio::task::JoinSet::new();
    for (position, row) in active_rows.iter().enumerate() {
        let substrate = substrate.clone();
        let path = row.path.clone();
        let semaphore = Arc::clone(&semaphore);
        reads.spawn(async move {
            // Acquire before touching disk so at most
            // `ACTIVE_SUMMARY_READ_CONCURRENCY` reads run at once. The semaphore
            // is never closed, so `acquire_owned` cannot fail.
            let _permit = semaphore.acquire_owned().await.expect("active-summary semaphore is open");
            // The read is a synchronous `std::fs` read + Markdown parse; run it
            // on the blocking pool via the dedicated sync method so it never
            // occupies an async worker thread (works on both the multi-thread
            // daemon runtime and the current-thread test/bench runtimes).
            let envelope = tokio::task::spawn_blocking(move || substrate.read_path_envelope_blocking(&path)).await;
            (position, envelope)
        });
    }

    // Collect into a position-indexed buffer so the candidate set order matches
    // the index query (deterministic by `memories.id`), independent of task
    // completion order.
    let mut buffered: Vec<Option<ExistingMemorySummary>> = (0..active_rows.len()).map(|_| None).collect();
    while let Some(joined) = reads.join_next().await {
        let (position, blocking_result) =
            joined.map_err(|err| HandlerError::substrate(format!("active-memory read task: {err}")))?;
        let envelope =
            blocking_result.map_err(|err| HandlerError::substrate(format!("active-memory read task: {err}")))?;
        let envelope = envelope.map_err(HandlerError::substrate)?;
        // The index row was `Active`; re-confirm against the read envelope and
        // require plaintext content (the encrypted body cannot be hashed), which
        // preserves the prior walk's exact membership filter.
        if !matches!(envelope.metadata.frontmatter.status, MemoryStatus::Active) {
            continue;
        }
        let MemoryContent::Plaintext(body) = envelope.content else {
            continue;
        };
        buffered[position] = Some(
            ExistingMemorySummary::new(
                envelope.metadata.frontmatter.id.as_str().to_string(),
                namespace_for_frontmatter(&envelope.metadata.frontmatter),
                body,
                1.0,
            )
            .with_entity_ids(entity_ids(&envelope.metadata.frontmatter)),
        );
    }

    Ok(buffered.into_iter().flatten().collect())
}

#[derive(Clone, Debug)]
struct MemorydSimilaritySearch {
    active: Vec<ExistingMemorySummary>,
    /// Index of the active set keyed by the exact-duplicate match key
    /// `(namespace, claim_hash, entity_hash)`, pointing at the position of the
    /// first occurrence in `active`. Turns `find_active_by_claim_hash` into an
    /// O(1) lookup instead of a linear scan over the whole active set per call,
    /// while preserving the prior "first match by candidate-set order" result —
    /// up to exact-duplicate ties. The candidate-set order itself moved from the
    /// filesystem walk to the index query (deterministic by `memories.id`), so
    /// when more than one active memory shares the exact full triple the *winning
    /// record* may differ from the old walk's pick. That is observationally safe
    /// today: a full-triple collision means the records are true exact duplicates
    /// (the dedup/contradiction decision is the same whichever wins), and the
    /// only surfaced field is `existing_id`, which names a genuine duplicate
    /// either way. A future change that reads order-sensitive *non-key* fields off
    /// the returned summary would need a stable secondary sort (e.g. by id) here.
    by_claim_key: std::collections::HashMap<(String, String, String), usize>,
    allow_top_k: bool,
}

impl MemorydSimilaritySearch {
    fn new(active: Vec<ExistingMemorySummary>, allow_top_k: bool) -> Self {
        let mut by_claim_key = std::collections::HashMap::with_capacity(active.len());
        for (position, memory) in active.iter().enumerate() {
            // First occurrence wins, matching the prior `Iterator::find` semantics.
            by_claim_key
                .entry((
                    memory.namespace().to_string(),
                    memory.canonical_claim_hash().to_string(),
                    memory.entity_hash().to_string(),
                ))
                .or_insert(position);
        }
        Self { active, by_claim_key, allow_top_k }
    }
}

impl SimilaritySearch for MemorydSimilaritySearch {
    fn find_active_by_claim_hash(&self, candidate: &CandidateMemory) -> Option<ExistingMemorySummary> {
        let key = (
            candidate.namespace().to_string(),
            candidate.canonical_claim_hash().to_string(),
            candidate.entity_hash().to_string(),
        );
        self.by_claim_key.get(&key).and_then(|&position| self.active.get(position)).cloned()
    }

    fn top_k(&self, _candidate: &CandidateMemory, limit: usize) -> Vec<ExistingMemorySummary> {
        if !self.allow_top_k {
            return Vec::new();
        }
        self.active.iter().take(limit).cloned().collect()
    }
}

#[derive(Clone, Debug)]
struct MemorydTiebreaker {
    tiebreak_mode: TiebreakMode,
}

#[derive(Clone, Debug)]
enum TiebreakMode {
    Unclear,
    Contradiction { existing_id: String },
}

impl ContradictionTiebreaker for MemorydTiebreaker {
    fn tiebreak(&self, _candidate: &CandidateMemory, _hits: &[ExistingMemorySummary]) -> TiebreakOutcome {
        match &self.tiebreak_mode {
            TiebreakMode::Unclear => TiebreakOutcome::Unclear,
            TiebreakMode::Contradiction { existing_id } => {
                TiebreakOutcome::Contradiction { existing_id: existing_id.clone() }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MemorydSessionResolver;

impl SessionSpawnResolver for MemorydSessionResolver {
    fn spawned_in_session(&self, _spawn_id: &str) -> bool {
        false
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
}

#[derive(Clone, Debug)]
struct WriteExecution {
    input: GovernanceWriteInput,
    id: MemoryId,
    decision: GovernanceWriteDecision,
    policy_source: PolicySource,
    privacy: PrivacyDecision,
}

#[derive(Clone, Debug)]
struct GovernedLifecycle {
    status: MemoryStatus,
    trust_level: TrustLevel,
    policy_applied: String,
}

impl GovernedLifecycle {
    fn new(status: MemoryStatus, trust_level: TrustLevel, policy_applied: String) -> Self {
        Self { status, trust_level, policy_applied }
    }
}

#[derive(Clone, Debug)]
struct GovernanceWriteInput {
    body: String,
    title: Option<String>,
    tags: Vec<String>,
    meta: GovernanceMeta,
}

struct GovernanceWriteInputParts {
    body: String,
    title: Option<String>,
    tags: Vec<String>,
    meta: Value,
    source: MetaSource,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct GovernanceMeta {
    namespace: GovernanceNamespace,
    #[serde(rename = "type")]
    memory_type: GovernanceMemoryType,
    summary: Option<String>,
    confidence: f64,
    sensitivity: Option<GovernanceSensitivity>,
    source_kind: GovernanceSourceKindMeta,
    source_ref: Option<String>,
    explicit_user_context: bool,
    privacy_descriptors: Option<PrivacyDescriptors>,
    #[serde(default = "default_supersede_session_id")]
    session_id: String,
    #[serde(default = "default_supersede_harness")]
    harness: String,
    pub(crate) concurrent_session_mode: Option<ConcurrentSessionMode>,
    // Importer-provenance fields (additive per Stream A §6.2/§6.5; all Option-wrapped so
    // existing callers continue to work without supplying them). The daemon mints
    // `Entity`/`Evidence` ids and `quote_norm_hash` from the caller-supplied surface form.
    entities: Option<Vec<EntityMeta>>,
    aliases: Option<Vec<String>>,
    related: Option<Vec<String>>,
    evidence: Option<Vec<EvidenceMeta>>,
    supersedes: Option<Vec<String>>,
    canonical_namespace_id: Option<String>,
    requires_user_confirmation: Option<bool>,
}

/// Caller-supplied entity surface form. The substrate `Entity` struct adds nothing
/// the daemon needs to compute, so this is a direct field-for-field carry.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityMeta {
    id: String,
    label: String,
    #[serde(default)]
    aliases: Vec<String>,
}

/// Caller-supplied evidence surface form. The daemon mints `id = ev_<ulid>` and
/// computes `quote_norm_hash = sha256:<hex>` over the whitespace-normalized quote.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceMeta {
    #[serde(rename = "ref")]
    reference: String,
    #[serde(default)]
    quote: Option<String>,
    #[serde(default)]
    observed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PrivacyDescriptors {
    subject: Option<String>,
    role: Option<String>,
    organization: Option<String>,
    office: Option<String>,
    value_kind: Option<String>,
    lookup_hints: Vec<String>,
}

impl PrivacyDescriptors {
    fn values(&self) -> Vec<String> {
        let mut values = [
            self.subject.clone(),
            self.role.clone(),
            self.organization.clone(),
            self.office.clone(),
            self.value_kind.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        values.extend(self.lookup_hints.iter().cloned());
        values
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GovernanceNamespace {
    Me,
    Project,
    Agent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GovernanceMemoryType {
    Project,
    Claim,
    Decision,
    Pattern,
    Playbook,
    Procedure,
    Artifact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GovernanceSensitivity {
    Public,
    Internal,
    Confidential,
    Personal,
    Sensitive,
    Secret,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GovernanceSourceKindMeta {
    User,
    AgentPrimary,
    Subagent,
    File,
    WebCapture,
    /// Backfill from a prior harness's memory layer (Claude Code, Codex CLI).
    /// Wire JSON is `"import"`; daemon-side mapping in `author()` and
    /// `substrate_source()` records the import as an agent-authored file load
    /// with `harness = "memoryd-import"`.
    #[serde(rename = "import")]
    Import,
}

impl Default for GovernanceMeta {
    fn default() -> Self {
        Self {
            namespace: GovernanceNamespace::Project,
            memory_type: GovernanceMemoryType::Project,
            summary: None,
            confidence: 0.85,
            sensitivity: None,
            source_kind: GovernanceSourceKindMeta::User,
            source_ref: None,
            explicit_user_context: false,
            privacy_descriptors: None,
            session_id: default_supersede_session_id(),
            harness: default_supersede_harness(),
            concurrent_session_mode: None,
            entities: None,
            aliases: None,
            related: None,
            evidence: None,
            supersedes: None,
            canonical_namespace_id: None,
            requires_user_confirmation: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MetaSource {
    Default,
    McpHumanWrite,
}

impl GovernanceMeta {
    fn empty_for(source: MetaSource) -> Self {
        match source {
            MetaSource::Default => Self::default(),
            MetaSource::McpHumanWrite => Self::for_mcp_human_write(),
        }
    }

    fn for_mcp_human_write() -> Self {
        Self { explicit_user_context: true, confidence: 0.9, ..Self::default() }
    }
}

fn default_supersede_session_id() -> String {
    DEFAULT_SUPERSEDE_SESSION_ID.to_owned()
}

fn default_supersede_harness() -> String {
    DEFAULT_SUPERSEDE_HARNESS.to_owned()
}

impl Default for GovernanceNamespace {
    fn default() -> Self {
        Self::Project
    }
}

impl<'de> Deserialize<'de> for GovernanceNamespace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "me" | "user" => Ok(Self::Me),
            "project" => Ok(Self::Project),
            "agent" => Ok(Self::Agent),
            other => Err(serde::de::Error::custom(format!("unsupported namespace `{other}`"))),
        }
    }
}

fn parse_governance_meta(meta: Value, source: MetaSource) -> Result<GovernanceMeta, HandlerError> {
    if meta.is_null() {
        return Ok(GovernanceMeta::empty_for(source));
    }

    let mut meta = meta;
    if source == MetaSource::McpHumanWrite {
        let Value::Object(fields) = &mut meta else {
            return Err(HandlerError::invalid_request("governance meta must be an object or null"));
        };
        fields.entry("explicit_user_context".to_string()).or_insert(Value::Bool(true));
        fields.entry("confidence".to_string()).or_insert(serde_json::json!(0.9));
    }
    serde_json::from_value(meta).map_err(|err| HandlerError::invalid_request(err.to_string()))
}

impl GovernanceWriteInput {
    fn parse(parts: GovernanceWriteInputParts) -> Result<Self, HandlerError> {
        let GovernanceWriteInputParts { body, title, tags, meta, source } = parts;
        let body = body.trim().to_string();
        if body.is_empty() {
            return Err(HandlerError::invalid_request("memory body must not be empty"));
        }
        let mut meta = parse_governance_meta(meta, source)?;
        meta.session_id = validated_claim_lock_identity_field("session_id", meta.session_id)?;
        meta.harness = validated_claim_lock_identity_field("harness", meta.harness)?;
        if !meta.confidence.is_finite() || !(0.0..=1.0).contains(&meta.confidence) {
            return Err(HandlerError::invalid_request("confidence must be finite and between 0.0 and 1.0"));
        }
        Ok(Self { body, title, tags, meta })
    }

    fn privacy_scan_text(&self) -> String {
        let mut fields = vec![self.body.as_str()];
        if let Some(title) = &self.title {
            fields.push(title.as_str());
        }
        if let Some(summary) = &self.meta.summary {
            fields.push(summary.as_str());
        }
        // Skip provenance *locators* from the privacy scan: a WebCapture URL or a
        // `file:`-grounded import/file path is a machine-generated reference, not
        // user-authored content. Scanning them produces false positives — a
        // filesystem path's numeric run (PID, nanosecond timestamp) can be
        // Luhn-valid and trip the credit-card detector, refusing an otherwise-clean
        // import for privacy. Body, title, summary, and tags are still scanned, so
        // genuine secret *content* is still caught.
        //
        // The exclusion is gated on the trusted provenance *source_kind*
        // (`Import`/`File`/`WebCapture`), not on any `file:`-prefixed
        // source_ref. A caller-authored write (e.g. `User`) cannot launder a
        // secret past the field scan by stuffing it into a `file:`-prefixed
        // source_ref, because its source_kind is still scanned.
        if let Some(source_ref) = &self.meta.source_ref {
            let is_provenance_locator = match self.meta.source_kind {
                GovernanceSourceKindMeta::WebCapture => true,
                GovernanceSourceKindMeta::Import | GovernanceSourceKindMeta::File => source_ref.starts_with("file:"),
                GovernanceSourceKindMeta::User
                | GovernanceSourceKindMeta::AgentPrimary
                | GovernanceSourceKindMeta::Subagent => false,
            };
            if !is_provenance_locator {
                fields.push(source_ref.as_str());
            }
        }
        fields.extend(self.tags.iter().map(String::as_str));
        let mut text = fields.join("\n");
        if let Some(descriptors) = &self.meta.privacy_descriptors {
            for value in descriptors.values() {
                text.push('\n');
                text.push_str(&value);
            }
        }
        text
    }

    fn privacy_refusal(&self, privacy: &PrivacyDecision) -> Option<GovernanceWriteResponse> {
        match privacy.storage_action {
            PrivacyStorageAction::Refuse => Some(GovernanceWriteResponse {
                status: GovernanceStatus::Refused,
                id: None,
                namespace: Some(self.response_namespace()),
                reason: Some(GovernanceRefusalReason::Privacy),
                next_actions: vec!["remove_secret_material".to_string()],
                policy_applied: None,
                policy_source: None,
                existing_id: None,
            }),
            PrivacyStorageAction::Plaintext | PrivacyStorageAction::EncryptAtRest => None,
        }
    }

    fn candidate(&self, id: &str) -> CandidateMemory {
        let mut candidate =
            CandidateMemory::new(id, self.response_namespace(), self.body.clone(), self.governance_scope())
                .with_confidence(self.meta.confidence as f32)
                .with_sources(self.governance_sources());
        if self.meta.explicit_user_context {
            candidate = candidate.with_explicit_user_context();
        }
        candidate
    }

    /// Build a [`Memory`] from this write input, applying lifecycle, privacy, and any
    /// caller-supplied importer-provenance fields.
    ///
    /// Mapping notes for `GovernanceSourceKindMeta::Import`:
    /// - `author = Author { kind: Agent, harness: Some("memoryd-import"), .. }`
    ///   (recorded as agent-authored, not user-authored, even though the content
    ///   originated from the user's prior harness sessions).
    /// - `source.kind = SourceKind::File` (the source IS a local file on disk,
    ///   even though the upstream `source_kind` tag is `"import"`).
    /// - `source.harness = Some("memoryd-import")` so downstream consumers can
    ///   filter the backfill in dashboards and recall ranking.
    ///
    /// Evidence ids and `quote_norm_hash` are minted here from the caller-supplied
    /// `EvidenceMeta` surface form so the importer never has to invent identifiers.
    fn to_memory(
        &self,
        id: MemoryId,
        lifecycle: GovernedLifecycle,
        privacy: &PrivacyDecision,
    ) -> Result<Memory, HandlerError> {
        let now = chrono::Utc::now();
        let summary = self.summary(privacy.storage_action);
        let requires_review = matches!(lifecycle.status, MemoryStatus::Candidate | MemoryStatus::Quarantined);
        let review_state = match lifecycle.status {
            MemoryStatus::Candidate => Some("candidate".to_string()),
            MemoryStatus::Quarantined => Some("quarantined".to_string()),
            _ => None,
        };
        let mut extras = BTreeMap::new();
        if matches!(lifecycle.status, MemoryStatus::Quarantined) {
            extras.insert("governance_reason".to_string(), serde_json::json!("governance quarantine"));
        }

        let sensitivity = privacy.tier.persisted_sensitivity().unwrap_or(Sensitivity::Internal);
        let encrypted = privacy.storage_action.requires_encryption();
        let indexable = !encrypted && !matches!(lifecycle.status, MemoryStatus::Quarantined);
        if let Some(descriptors) = self.safe_privacy_descriptors_value() {
            extras.insert("privacy_descriptors".to_string(), descriptors);
        }
        let entities = self.entities_for_persist();
        let aliases = self.aliases_for_persist();
        let related = self.related_for_persist()?;
        let supersedes = self.supersedes_for_persist()?;
        let evidence = self.evidence_for_persist();
        let canonical_namespace_id = self.meta.canonical_namespace_id.clone().or_else(|| self.substrate_namespace());
        // Importer writes carry already-vetted content from prior harness sessions and
        // should not flood the Reality Check review queue with low-confidence guesses.
        // Caller can suppress the review flag for non-candidate writes; lifecycle still
        // forces review for `Candidate`/`Quarantined` so the override never weakens
        // governance.
        let requires_user_confirmation =
            self.meta.requires_user_confirmation.map_or(requires_review, |caller| requires_review || caller);
        Ok(Memory {
            frontmatter: Frontmatter {
                schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
                id: id.clone(),
                memory_type: self.memory_type(),
                scope: self.substrate_scope(),
                summary,
                confidence: self.meta.confidence,
                original_confidence: None,
                trust_level: lifecycle.trust_level,
                sensitivity,
                status: lifecycle.status,
                created_at: now,
                updated_at: now,
                observed_at: None,
                author: self.author(),
                namespace: self.substrate_namespace(),
                canonical_namespace_id,
                tags: self.persisted_tags(privacy.storage_action),
                entities,
                aliases,
                source: self.substrate_source(privacy.storage_action),
                evidence,
                requires_user_confirmation,
                review_state,
                supersedes,
                superseded_by: Vec::new(),
                related,
                tombstone_events: Vec::new(),
                retrieval_policy: RetrievalPolicy {
                    passive_recall: !matches!(lifecycle.status, MemoryStatus::Quarantined),
                    max_scope: self.substrate_scope(),
                    mask_personal_for_synthesis: encrypted,
                    index_body: indexable,
                    index_embeddings: indexable,
                },
                write_policy: WritePolicy {
                    human_review_required: requires_review,
                    policy_applied: lifecycle.policy_applied,
                    expected_base_hash: None,
                },
                merge_diagnostics: matches!(lifecycle.status, MemoryStatus::Quarantined).then(|| {
                    serde_json::json!({
                        "human_reason": "governance quarantine",
                        "preserved_sources": [],
                        "lifecycle_notes": [],
                        "evidence_near_duplicates": []
                    })
                }),
                extras,
            },
            body: self.body.clone(),
            path: Some(self.repo_path(id.as_str())),
        })
    }

    fn entities_for_persist(&self) -> Vec<Entity> {
        self.meta
            .entities
            .as_ref()
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| Entity {
                        id: entry.id.clone(),
                        label: entry.label.clone(),
                        aliases: entry.aliases.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn aliases_for_persist(&self) -> Vec<String> {
        self.meta.aliases.clone().unwrap_or_default()
    }

    fn related_for_persist(&self) -> Result<Vec<MemoryId>, HandlerError> {
        let Some(ids) = self.meta.related.as_ref() else {
            return Ok(Vec::new());
        };
        ids.iter()
            .map(|id| {
                MemoryId::try_new(id.clone()).map_err(|err| {
                    HandlerError::invalid_request(format!("invalid meta.related memory id `{id}`: {err}"))
                })
            })
            .collect()
    }

    fn supersedes_for_persist(&self) -> Result<Vec<MemoryId>, HandlerError> {
        let Some(ids) = self.meta.supersedes.as_ref() else {
            return Ok(Vec::new());
        };
        ids.iter()
            .map(|id| {
                MemoryId::try_new(id.clone()).map_err(|err| {
                    HandlerError::invalid_request(format!("invalid meta.supersedes memory id `{id}`: {err}"))
                })
            })
            .collect()
    }

    fn evidence_for_persist(&self) -> Vec<Evidence> {
        let Some(entries) = self.meta.evidence.as_ref() else {
            return Vec::new();
        };
        entries
            .iter()
            .map(|entry| {
                let quote = entry.quote.clone().unwrap_or_default();
                let quote_norm_hash = (!quote.is_empty()).then(|| compute_quote_norm_hash(&quote));
                Evidence {
                    id: format!("ev_{}", ulid::Ulid::new()),
                    quote,
                    quote_norm_hash,
                    reference: entry.reference.clone(),
                    weight: 1.0,
                    observed_at: entry.observed_at,
                    source: None,
                }
            })
            .collect()
    }

    fn summary(&self, storage_action: PrivacyStorageAction) -> String {
        let candidate = self.meta.summary.clone().or_else(|| self.title.clone());
        if storage_action.requires_encryption() {
            return candidate
                .filter(|value| is_safe_plaintext_for_indexing(value))
                .unwrap_or_else(|| "encrypted memory".to_string());
        }
        candidate.unwrap_or_else(|| bounded(&self.body, 120))
    }

    fn persisted_tags(&self, storage_action: PrivacyStorageAction) -> Vec<String> {
        if storage_action.requires_encryption() {
            self.tags.iter().filter(|tag| is_safe_plaintext_for_indexing(tag)).cloned().collect()
        } else {
            self.tags.clone()
        }
    }

    fn response_namespace(&self) -> String {
        match self.meta.namespace {
            GovernanceNamespace::Me => "me".to_string(),
            GovernanceNamespace::Project => "project".to_string(),
            GovernanceNamespace::Agent => "agent".to_string(),
        }
    }

    fn governance_scope(&self) -> memory_governance::Scope {
        match self.meta.namespace {
            GovernanceNamespace::Me => memory_governance::Scope::Me,
            GovernanceNamespace::Project => memory_governance::Scope::Project,
            GovernanceNamespace::Agent => memory_governance::Scope::Agent,
        }
    }

    fn privacy_namespace(&self) -> PrivacyNamespace {
        match self.meta.namespace {
            GovernanceNamespace::Me => PrivacyNamespace::Me,
            GovernanceNamespace::Project => PrivacyNamespace::Project,
            GovernanceNamespace::Agent => PrivacyNamespace::Agent,
        }
    }

    fn caller_sensitivity(&self) -> Option<CallerSensitivity> {
        self.meta.sensitivity.map(|sensitivity| match sensitivity {
            GovernanceSensitivity::Public => CallerSensitivity::Public,
            GovernanceSensitivity::Internal => CallerSensitivity::Internal,
            GovernanceSensitivity::Confidential => CallerSensitivity::Confidential,
            GovernanceSensitivity::Personal => CallerSensitivity::Personal,
            GovernanceSensitivity::Sensitive => CallerSensitivity::Sensitive,
            GovernanceSensitivity::Secret => CallerSensitivity::Secret,
        })
    }

    fn substrate_scope(&self) -> Scope {
        match self.meta.namespace {
            GovernanceNamespace::Me => Scope::User,
            GovernanceNamespace::Project => Scope::Project,
            GovernanceNamespace::Agent => Scope::Agent,
        }
    }

    fn substrate_namespace(&self) -> Option<String> {
        matches!(self.meta.namespace, GovernanceNamespace::Project).then(|| DEFAULT_PROJECT_NAMESPACE.to_string())
    }

    fn governance_sources(&self) -> Vec<GovernanceSource> {
        let kind = match self.meta.source_kind {
            GovernanceSourceKindMeta::User => GovernanceSourceKind::User,
            GovernanceSourceKindMeta::Subagent => GovernanceSourceKind::Subagent,
            GovernanceSourceKindMeta::WebCapture => GovernanceSourceKind::WebCapture,
            GovernanceSourceKindMeta::AgentPrimary
            | GovernanceSourceKindMeta::File
            | GovernanceSourceKindMeta::Import => GovernanceSourceKind::AgentPrimary,
        };
        vec![GovernanceSource::new(kind, self.meta.source_ref.clone())]
    }

    fn substrate_source(&self, storage_action: PrivacyStorageAction) -> Source {
        let kind = match self.meta.source_kind {
            GovernanceSourceKindMeta::User => SourceKind::User,
            GovernanceSourceKindMeta::Subagent => SourceKind::AgentSubagent,
            GovernanceSourceKindMeta::WebCapture => SourceKind::Web,
            // The importer reads files off disk, so the substrate source kind is `File`
            // regardless of the upstream `source_kind = "import"` tag. The `harness`
            // field below distinguishes import writes from generic file writes.
            GovernanceSourceKindMeta::File | GovernanceSourceKindMeta::Import => SourceKind::File,
            GovernanceSourceKindMeta::AgentPrimary => SourceKind::AgentPrimary,
        };
        let harness =
            matches!(self.meta.source_kind, GovernanceSourceKindMeta::Import).then(|| "memoryd-import".to_string());
        Source {
            kind,
            reference: if storage_action.requires_encryption() {
                self.meta
                    .source_ref
                    .clone()
                    .filter(|reference| is_safe_plaintext_for_indexing(reference))
                    .or_else(|| Some("memoryd.governance".to_string()))
            } else {
                self.meta.source_ref.clone().or_else(|| Some("memoryd.governance".to_string()))
            },
            harness,
            harness_version: None,
            session_id: None,
            subagent_id: None,
            device: None,
        }
    }

    fn safe_privacy_descriptors_value(&self) -> Option<Value> {
        let descriptors = self.meta.privacy_descriptors.as_ref()?;
        let mut object = serde_json::Map::new();
        insert_safe_descriptor(&mut object, "subject", descriptors.subject.as_deref());
        insert_safe_descriptor(&mut object, "role", descriptors.role.as_deref());
        insert_safe_descriptor(&mut object, "organization", descriptors.organization.as_deref());
        insert_safe_descriptor(&mut object, "office", descriptors.office.as_deref());
        insert_safe_descriptor(&mut object, "value_kind", descriptors.value_kind.as_deref());
        let hints = descriptors
            .lookup_hints
            .iter()
            .filter(|hint| is_safe_plaintext_for_indexing(hint))
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>();
        if !hints.is_empty() {
            object.insert("lookup_hints".to_string(), Value::Array(hints));
        }
        (!object.is_empty()).then_some(Value::Object(object))
    }

    fn author(&self) -> Author {
        match self.meta.source_kind {
            GovernanceSourceKindMeta::User => Author {
                kind: AuthorKind::User,
                user_handle: Some("memoryd-user".to_string()),
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: None,
            },
            GovernanceSourceKindMeta::Subagent => Author {
                kind: AuthorKind::Subagent,
                user_handle: None,
                harness: Some("memoryd".to_string()),
                harness_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                session_id: Some("memoryd-session".to_string()),
                subagent_id: Some("memoryd-subagent".to_string()),
                phase: None,
                component: None,
            },
            GovernanceSourceKindMeta::Import => Author {
                kind: AuthorKind::Agent,
                user_handle: None,
                harness: Some("memoryd-import".to_string()),
                harness_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                session_id: Some("memoryd-session".to_string()),
                subagent_id: None,
                phase: None,
                component: None,
            },
            GovernanceSourceKindMeta::AgentPrimary
            | GovernanceSourceKindMeta::File
            | GovernanceSourceKindMeta::WebCapture => Author {
                kind: AuthorKind::Agent,
                user_handle: None,
                harness: Some("memoryd".to_string()),
                harness_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                session_id: Some("memoryd-session".to_string()),
                subagent_id: None,
                phase: None,
                component: None,
            },
        }
    }

    fn memory_type(&self) -> MemoryType {
        match self.meta.memory_type {
            GovernanceMemoryType::Claim => MemoryType::Claim,
            GovernanceMemoryType::Decision => MemoryType::Decision,
            GovernanceMemoryType::Pattern => MemoryType::Pattern,
            GovernanceMemoryType::Playbook => MemoryType::Playbook,
            GovernanceMemoryType::Procedure => MemoryType::Procedure,
            GovernanceMemoryType::Artifact => MemoryType::Artifact,
            GovernanceMemoryType::Project => MemoryType::Project,
        }
    }

    fn repo_path(&self, id: &str) -> RepoPath {
        match self.meta.namespace {
            GovernanceNamespace::Me => RepoPath::new(format!("me/knowledge/{id}.md")),
            GovernanceNamespace::Project => {
                RepoPath::new(format!("projects/{DEFAULT_PROJECT_NAMESPACE}/decisions/{id}.md"))
            }
            GovernanceNamespace::Agent => RepoPath::new(format!("agent/patterns/{id}.md")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // T00: importer-provenance fields on GovernanceMeta. The tests below lock the
    // additive-extension contract — new optional fields round-trip, defaults stay
    // None, `deny_unknown_fields` still rejects unknown keys, and `source_kind:
    // "import"` maps to a file-source agent-author with the `memoryd-import` harness.

    fn write_input(meta: Value) -> GovernanceWriteInput {
        GovernanceWriteInput::parse(GovernanceWriteInputParts {
            body: "Body text".to_string(),
            title: Some("Title".to_string()),
            tags: Vec::new(),
            meta,
            source: MetaSource::Default,
        })
        .expect("write input parses")
    }

    fn plaintext_privacy_decision() -> memory_privacy::PrivacyDecision {
        memory_privacy::PrivacyDecision::new(
            memory_privacy::PrivacyTier::Internal,
            memory_privacy::PrivacyStorageAction::Plaintext,
            Vec::new(),
            "test-classifier",
        )
    }

    fn promoted_lifecycle() -> GovernedLifecycle {
        GovernedLifecycle::new(MemoryStatus::Active, TrustLevel::Trusted, "test-policy".to_string())
    }

    #[test]
    fn governance_meta_empty_payload_preserves_existing_defaults() {
        let meta: GovernanceMeta = parse_governance_meta(Value::Null, MetaSource::Default).expect("null parses");
        assert!(meta.entities.is_none());
        assert!(meta.aliases.is_none());
        assert!(meta.related.is_none());
        assert!(meta.evidence.is_none());
        assert!(meta.supersedes.is_none());
        assert!(meta.canonical_namespace_id.is_none());
        assert!(meta.requires_user_confirmation.is_none());

        // Backward-compat: an empty payload should produce the exact same Memory shape
        // as before the additive extension — empty entities/aliases/related/evidence/supersedes
        // and canonical_namespace_id falling back to the default project namespace.
        let input = write_input(Value::Null);
        let memory = input
            .to_memory(
                MemoryId::new("mem_20260527_a1b2c3d4e5f60718_000001"),
                promoted_lifecycle(),
                &plaintext_privacy_decision(),
            )
            .expect("empty meta converts to memory");
        assert!(memory.frontmatter.entities.is_empty());
        assert!(memory.frontmatter.aliases.is_empty());
        assert!(memory.frontmatter.related.is_empty());
        assert!(memory.frontmatter.evidence.is_empty());
        assert!(memory.frontmatter.supersedes.is_empty());
        assert_eq!(memory.frontmatter.canonical_namespace_id.as_deref(), Some(DEFAULT_PROJECT_NAMESPACE));
        assert!(!memory.frontmatter.requires_user_confirmation);
    }

    #[test]
    fn privacy_scan_excludes_file_locator_so_path_digits_do_not_false_positive() {
        // A grounded import carries `source_ref = file:<abs path>`. Filesystem
        // paths routinely contain long digit runs (PIDs, nanosecond timestamps)
        // that can be Luhn-valid and trip the credit-card secret detector. Such a
        // locator is machine-generated provenance, not user content, so it must NOT
        // be privacy-scanned — otherwise an otherwise-clean import is refused at
        // random (~10% of nonces, see import::pipeline::groundable_source_ref). The
        // canonical Visa test number `4111111111111111` is Luhn-valid and stands in
        // for any such path component.
        let luhn = "4111111111111111";
        let file_ref = format!("file:/tmp/memd-run-{luhn}/topic.md");
        let input = write_input(serde_json::json!({
            "source_kind": "import",
            "source_ref": file_ref,
        }));
        assert!(!input.privacy_scan_text().contains(luhn), "file: locator must be excluded from the privacy scan text");
        let decision = classify_input_privacy(&input).expect("classify file-locator input");
        assert_ne!(
            decision.storage_action,
            PrivacyStorageAction::Refuse,
            "a file: locator with a Luhn-valid path component must not be refused for privacy"
        );

        // Positive control: the same value in user *content* (body) is still
        // scanned and refused, proving the exclusion is scoped to the locator.
        let body_input = GovernanceWriteInput::parse(GovernanceWriteInputParts {
            body: format!("card {luhn} on file"),
            title: None,
            tags: Vec::new(),
            meta: serde_json::json!({ "source_kind": "import" }),
            source: MetaSource::Default,
        })
        .expect("body input parses");
        assert_eq!(
            classify_input_privacy(&body_input).expect("classify body input").storage_action,
            PrivacyStorageAction::Refuse,
            "a Luhn-valid number in the body is genuine secret content and must still be refused"
        );
    }

    #[test]
    fn privacy_scan_includes_file_source_ref_for_untrusted_source_kind() {
        // The `file:` exclusion is gated on a trusted provenance source_kind
        // (import/file/web_capture). A caller-authored write (`source_kind:
        // user`) must NOT be able to launder a secret-shaped value past the
        // field scan by stuffing it into a `file:`-prefixed source_ref.
        let luhn = "4111111111111111";
        let file_ref = format!("file:/tmp/{luhn}/note.md");
        let input = write_input(serde_json::json!({
            "source_kind": "user",
            "source_ref": file_ref,
        }));
        assert!(
            input.privacy_scan_text().contains(luhn),
            "a user-authored file: source_ref must still be privacy-scanned"
        );
    }

    #[test]
    fn governance_meta_accepts_importer_provenance_fields_and_round_trips_through_to_memory() {
        let payload = serde_json::json!({
            "namespace": "project",
            "source_kind": "import",
            "source_ref": "/Users/treygoff/.claude/projects/example/memory/topic.md",
            "confidence": 0.7,
            "requires_user_confirmation": false,
            "canonical_namespace_id": "proj_0123456789abcdef",
            "entities": [
                { "id": "ent_acme", "label": "Acme Corp", "aliases": ["Acme", "ACME"] }
            ],
            "aliases": ["topic.md"],
            "related": ["mem_20260527_a1b2c3d4e5f60718_000010"],
            "supersedes": ["mem_20260527_a1b2c3d4e5f60718_000003"],
            "evidence": [
                {
                    "ref": "file:///Users/treygoff/.codex/memories/rollouts/abc.md",
                    "quote": "  shipped\n  fix  ",
                    "observed_at": "2026-05-27T22:33:00Z"
                }
            ]
        });
        let input = write_input(payload);
        let memory = input
            .to_memory(
                MemoryId::new("mem_20260527_a1b2c3d4e5f60718_000042"),
                promoted_lifecycle(),
                &plaintext_privacy_decision(),
            )
            .expect("importer meta converts to memory");

        assert_eq!(memory.frontmatter.entities.len(), 1);
        assert_eq!(memory.frontmatter.entities[0].id, "ent_acme");
        assert_eq!(memory.frontmatter.entities[0].aliases, vec!["Acme".to_string(), "ACME".to_string()]);
        assert_eq!(memory.frontmatter.aliases, vec!["topic.md".to_string()]);
        assert_eq!(memory.frontmatter.related[0].as_str(), "mem_20260527_a1b2c3d4e5f60718_000010");
        assert_eq!(memory.frontmatter.supersedes[0].as_str(), "mem_20260527_a1b2c3d4e5f60718_000003");
        assert_eq!(memory.frontmatter.canonical_namespace_id.as_deref(), Some("proj_0123456789abcdef"));

        // Evidence id is minted as `ev_<ulid>`; quote_norm_hash is `sha256:<hex>` over
        // the whitespace-collapsed quote (so "  shipped\n  fix  " hashes the same as
        // "shipped fix").
        let evidence = &memory.frontmatter.evidence[0];
        assert!(evidence.id.starts_with("ev_"));
        assert_eq!(evidence.reference, "file:///Users/treygoff/.codex/memories/rollouts/abc.md");
        assert_eq!(evidence.quote, "  shipped\n  fix  ");
        let expected_hash = compute_quote_norm_hash("shipped fix");
        assert_eq!(evidence.quote_norm_hash.as_deref(), Some(expected_hash.as_str()));
        assert!(evidence.observed_at.is_some());
    }

    #[test]
    fn governance_meta_import_source_kind_maps_to_file_source_and_memoryd_import_harness() {
        let payload = serde_json::json!({
            "namespace": "project",
            "source_kind": "import",
            "source_ref": "/Users/treygoff/.claude/projects/x/memory/y.md"
        });
        let input = write_input(payload);
        assert!(matches!(input.meta.source_kind, GovernanceSourceKindMeta::Import));

        let memory = input
            .to_memory(
                MemoryId::new("mem_20260527_a1b2c3d4e5f60718_000007"),
                promoted_lifecycle(),
                &plaintext_privacy_decision(),
            )
            .expect("import source meta converts to memory");

        // Author records the agent-authored import with the dedicated harness tag so
        // dashboards and recall ranking can identify backfilled content.
        assert!(matches!(memory.frontmatter.author.kind, AuthorKind::Agent));
        assert_eq!(memory.frontmatter.author.harness.as_deref(), Some("memoryd-import"));

        // Substrate Source stays `File` (the source IS a local file) but the harness
        // tag differentiates it from generic file writes.
        assert!(matches!(memory.frontmatter.source.kind, SourceKind::File));
        assert_eq!(memory.frontmatter.source.harness.as_deref(), Some("memoryd-import"));
        assert_eq!(
            memory.frontmatter.source.reference.as_deref(),
            Some("/Users/treygoff/.claude/projects/x/memory/y.md")
        );
    }

    #[test]
    fn governance_meta_rejects_unknown_field() {
        let payload = serde_json::json!({
            "namespace": "project",
            "source_kind": "user",
            "zzz_unknown_field": 1
        });
        let err = parse_governance_meta(payload, MetaSource::Default).expect_err("unknown field is rejected");
        assert!(err.message.contains("zzz_unknown_field"), "error mentions the field: {}", err.message);
    }

    #[test]
    fn governance_meta_serializes_import_source_kind_as_lowercase_token() {
        // Lock the wire format: the import variant must serialize as the JSON token
        // `"import"` (matches Stream A spec §6 frontmatter source.kind) so MCP clients
        // can submit the same shape that the importer uses internally.
        let payload = serde_json::json!({ "source_kind": "import" });
        let meta: GovernanceMeta = parse_governance_meta(payload, MetaSource::Default).expect("import parses");
        assert!(matches!(meta.source_kind, GovernanceSourceKindMeta::Import));
    }
}
