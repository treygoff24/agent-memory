//! Write paths: plaintext write, supersede, encrypted write/update, tombstone,
//! id allocation, the pre-disk refusal gates, and the shared post-commit
//! lifecycle-event ladder.

use super::*;

impl Substrate {
    /// Write plaintext memory.
    pub async fn write_memory(&self, request: WriteRequest) -> Result<WriteOutcome, WriteFailure> {
        let operation_id = request.operation_id.clone().unwrap_or_else(new_operation_id);
        let outcome = WriteOutcome::not_committed(operation_id.clone(), self.durability);
        // Pre-disk refusal gates emit `WriteRefused` audit events per spec §8.7 step 6.
        let audit_ctx = RefusalAuditContext {
            id: request.memory.frontmatter.id.clone(),
            path: request.memory.path.clone(),
            classification: request.classification,
            operation_id: operation_id.clone(),
        };
        self.run_gate(
            &audit_ctx,
            self.enforce_best_effort_opt_in(request.allow_best_effort_durability, outcome.clone()),
        )?;
        self.run_gate(&audit_ctx, self.enforce_plaintext_classification(&request, outcome.clone()))?;
        self.run_gate(&audit_ctx, self.validate_memory_path(&request.memory, outcome.clone()))?;
        self.run_gate(&audit_ctx, enforce_no_dream_prose_sources(&request.memory, outcome.clone()))?;
        self.run_gate(
            &audit_ctx,
            validate_frontmatter(&request.memory.frontmatter).map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err.to_string())),
            }),
        )?;
        let final_hash = atomic_write(crate::markdown::AtomicWrite {
            repo: &self.roots.repo,
            memory: &request.memory,
            expected_base_hash: request.expected_base_hash.as_ref(),
            mode: request.write_mode,
            operation_id: &operation_id,
            durability: self.durability,
            suppression: Some(&self.suppression),
            allow_encrypted_namespace: false,
        })?;
        let pending_index_op = || PendingIndexOp {
            op_id: operation_id.clone(),
            kind: PendingIndexKind::UpsertPath,
            path: request.memory.path.clone().unwrap_or_else(|| default_memory_path(&request.memory)),
            memory_id: Some(request.memory.frontmatter.id.clone()),
            expected_file_hash: Some(final_hash.clone()),
            enqueued_at: Utc::now(),
            attempts: 0,
            last_error: None,
        };
        let missing_supersession_targets = {
            let mut index_guard = lock_index(&self.index);
            match index_guard.upsert_memory_with_file_hash(&request.memory, false, Some(&final_hash)) {
                Ok(()) => index_guard.missing_supersession_targets(&request.memory.frontmatter.supersedes),
                Err(err) => Err(err),
            }
        };
        let missing_supersession_targets = match missing_supersession_targets {
            Ok(targets) => targets,
            Err(_idx_err) => {
                return Err(RepairCascade {
                    runtime: &self.roots.runtime,
                    op: IndexRepairOp::Plain(pending_index_op()),
                    marker_reason: "pending index enqueue failed",
                    failure_kinds: CascadeFailureKinds::AlwaysIndexAfterCommit,
                    durability: self.durability,
                    operation_id: operation_id.clone(),
                }
                .into_failure());
            }
        };
        if !missing_supersession_targets.is_empty() {
            return Err(RepairCascade {
                runtime: &self.roots.runtime,
                op: IndexRepairOp::Plain(pending_index_op()),
                marker_reason: "pending supersession index enqueue failed",
                failure_kinds: CascadeFailureKinds::AlwaysIndexAfterCommit,
                durability: self.durability,
                operation_id: operation_id.clone(),
            }
            .into_failure());
        }
        let write_event_kind = EventKind::WriteCommitted {
            id: request.memory.frontmatter.id.clone(),
            path: request.memory.path.clone().unwrap_or_else(|| default_memory_path(&request.memory)),
            classification: request.classification,
        };
        self.commit_lifecycle_event(write_event_kind, &operation_id, "pending event enqueue failed", &outcome)
    }

    /// Supersede an existing memory with a replacement memory.
    ///
    /// Stream A cannot atomically write two Markdown files, so the visible order
    /// is explicit: write the replacement first with `supersedes = old_id`, then
    /// mutate the old memory to `status = superseded` with `superseded_by =
    /// new_id`. If the second write fails after the replacement committed, the
    /// returned `WriteFailure.outcome` reports that committed side effect so the
    /// daemon can stop accepting lifecycle writes until repair is visible.
    pub async fn supersede_memory(&self, request: SupersedeRequest) -> Result<SupersedeOutcome, WriteFailure> {
        let operation_id = new_operation_id();
        let old_id = request.old_id;
        let mut replacement = request.replacement;
        let new_id = replacement.frontmatter.id.clone();
        let (mut old_memory, old_base_hash) =
            self.read_memory_with_hash(&old_id).await.map_err(|err| WriteFailure {
                outcome: WriteOutcome::not_committed(operation_id.clone(), self.durability),
                kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err.to_string())),
            })?;

        if !replacement.frontmatter.supersedes.contains(&old_id) {
            replacement.frontmatter.supersedes.push(old_id.clone());
        }
        replacement.frontmatter.updated_at = lifecycle_updated_at(&replacement.frontmatter);

        let new_outcome = self
            .write_memory(WriteRequest {
                operation_id: Some(operation_id.clone()),
                memory: replacement,
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext { actor: None, reason: Some(request.reason.clone()) },
                allow_best_effort_durability: request.allow_best_effort_durability,
                classification: request.classification,
            })
            .await?;

        old_memory.frontmatter.status = MemoryStatus::Superseded;
        old_memory.frontmatter.updated_at = lifecycle_updated_at(&old_memory.frontmatter);
        if !old_memory.frontmatter.superseded_by.contains(&new_id) {
            old_memory.frontmatter.superseded_by.push(new_id.clone());
        }

        let old_outcome = self
            .write_memory(WriteRequest {
                operation_id: Some(operation_id),
                memory: old_memory,
                expected_base_hash: Some(old_base_hash),
                write_mode: WriteMode::ReplaceExisting,
                index_projection: None,
                event_context: EventContext { actor: None, reason: Some(request.reason) },
                allow_best_effort_durability: request.allow_best_effort_durability,
                classification: request.classification,
            })
            .await
            .map_err(|failure| committed_lifecycle_failure(failure, &new_outcome))?;

        Ok(SupersedeOutcome { old_id, new_id, old_outcome, new_outcome })
    }

    /// Write encrypted memory metadata plus ciphertext.
    pub async fn write_encrypted(&self, request: EncryptedWriteRequest) -> Result<WriteOutcome, WriteFailure> {
        let operation_id = request.operation_id.clone().unwrap_or_else(new_operation_id);
        let outcome = WriteOutcome::not_committed(operation_id.clone(), self.durability);
        let audit_ctx = RefusalAuditContext {
            id: request.metadata_memory.frontmatter.id.clone(),
            path: request.metadata_memory.path.clone(),
            classification: request.classification,
            operation_id: operation_id.clone(),
        };
        self.run_gate(
            &audit_ctx,
            self.enforce_best_effort_opt_in(request.allow_best_effort_durability, outcome.clone()),
        )?;
        let classification_check = match request.classification {
            ClassificationOutcome::RequiresEncryption => Ok(()),
            ClassificationOutcome::Secret => {
                Err(WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::SecretRefused })
            }
            ClassificationOutcome::Trusted => {
                Err(WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::EncryptionRequired })
            }
        };
        self.run_gate(&audit_ctx, classification_check)?;
        self.run_gate(&audit_ctx, self.validate_memory_path(&request.metadata_memory, outcome.clone()))?;
        self.run_gate(&audit_ctx, enforce_no_dream_prose_sources(&request.metadata_memory, outcome.clone()))?;
        validate_frontmatter(&request.metadata_memory.frontmatter).map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err.to_string())),
        })?;
        let path = encrypted_ciphertext_path(&request.metadata_memory).map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err)),
        })?;
        let mut stored_memory = request.metadata_memory.clone();
        stored_memory.frontmatter.extras.entry("encryption".to_string()).or_insert_with(|| {
            serde_json::json!({
                "scheme": "unspecified",
                "recipient": "unspecified",
            })
        });
        if let Some(safe_body) =
            request.safe_index_projection.as_ref().and_then(|projection| projection.safe_body.as_ref())
        {
            stored_memory.frontmatter.extras.insert(
                "index_projection".to_string(),
                serde_json::json!({
                    "safe_body": safe_body,
                }),
            );
        }
        stored_memory.body = BASE64_STANDARD.encode(&request.ciphertext);
        let encrypted_document =
            crate::frontmatter::serialize_document(&stored_memory).map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err.to_string())),
            })?;
        let ciphertext_hash = crate::markdown::hash_bytes(encrypted_document.as_bytes());
        atomic_write_bytes(BinaryWrite {
            repo: &self.roots.repo,
            path: &path,
            bytes: encrypted_document.as_bytes(),
            operation_id: &operation_id,
            durability: self.durability,
            suppression: Some(&self.suppression),
        })
        .map_err(|err| WriteFailure {
            outcome: WriteOutcome::not_committed(operation_id.clone(), self.durability),
            kind: if err.kind() == std::io::ErrorKind::AlreadyExists {
                WriteFailureKind::AlreadyExists
            } else {
                WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() }
            },
        })?;
        let mut stored_for_index = stored_memory.clone();
        stored_for_index.path = Some(path.clone());
        let (indexed_memory, metadata_only) = encrypted_index_projection(&stored_for_index);
        self.index
            .lock()
            .map_err(|err| WriteFailure {
                outcome: WriteOutcome {
                    committed: true,
                    indexed: false,
                    event_recorded: false,
                    durability: self.durability,
                    repair_required: Some(RepairRequired::OperatorRequired(
                        "encrypted metadata index lock failed after ciphertext commit".to_string(),
                    )),
                    operation_id: operation_id.clone(),
                },
                kind: WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() },
            })?
            .upsert_memory_with_file_hash(&indexed_memory, metadata_only, Some(&ciphertext_hash))
            .map_err(|_err| {
                let pending = PendingEncryptedIndexOp {
                    op_id: operation_id.clone(),
                    indexed_memory: indexed_memory.clone(),
                    metadata_only,
                    expected_ciphertext_hash: ciphertext_hash.clone(),
                    enqueued_at: Utc::now(),
                    attempts: 0,
                    last_error: None,
                };
                RepairCascade {
                    runtime: &self.roots.runtime,
                    op: IndexRepairOp::Encrypted(Box::new(pending)),
                    marker_reason: "pending encrypted index enqueue failed",
                    failure_kinds: CascadeFailureKinds::Tiered,
                    durability: self.durability,
                    operation_id: operation_id.clone(),
                }
                .into_failure()
            })?;
        let encrypted_event_kind = EventKind::EncryptedWriteCommitted {
            id: request.metadata_memory.frontmatter.id.clone(),
            path,
            classification: request.classification,
        };
        self.commit_lifecycle_event(
            encrypted_event_kind,
            &operation_id,
            "pending encrypted event enqueue failed",
            &outcome,
        )
    }

    /// Update encrypted memory metadata without decrypting or replacing the ciphertext body.
    pub async fn update_encrypted_memory_metadata(
        &self,
        id: &MemoryId,
        mutate: impl FnOnce(&mut Memory),
    ) -> Result<(), WriteFailure> {
        let operation_id = new_operation_id();
        let outcome = WriteOutcome::not_committed(operation_id.clone(), self.durability);
        let envelope = self.read_memory_envelope(id).await.map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err.to_string())),
        })?;
        if matches!(envelope.content, MemoryContent::Plaintext(_)) {
            return Err(WriteFailure {
                outcome,
                kind: WriteFailureKind::ValidationTyped(ValidationError::Other(
                    "encrypted metadata update requires encrypted content".to_string(),
                )),
            });
        }

        let mut memory = envelope.metadata;
        let path = encrypted_metadata_path(&memory).map_err(|message| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::ValidationTyped(ValidationError::Other(message)),
        })?;
        let current_hash = std::fs::read(self.roots.repo.join(path.as_path()))
            .map(|bytes| crate::markdown::hash_bytes(&bytes))
            .map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() },
            })?;
        let preserved_body = memory.body.clone();
        let preserved_encryption = memory.frontmatter.extras.get("encryption").cloned();

        mutate(&mut memory);

        memory.path = Some(path.clone());
        memory.body = preserved_body;
        match preserved_encryption {
            Some(encryption) => {
                memory.frontmatter.extras.insert("encryption".to_string(), encryption);
            }
            None => {
                memory.frontmatter.extras.remove("encryption");
            }
        }
        validate_frontmatter(&memory.frontmatter).map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err.to_string())),
        })?;
        let final_hash = atomic_write(crate::markdown::AtomicWrite {
            repo: &self.roots.repo,
            memory: &memory,
            expected_base_hash: Some(&current_hash),
            mode: WriteMode::ReplaceExisting,
            operation_id: &operation_id,
            durability: self.durability,
            suppression: Some(&self.suppression),
            allow_encrypted_namespace: true,
        })?;
        let (indexed_memory, metadata_only) = encrypted_index_projection(&memory);
        self.index
            .lock()
            .map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() },
            })?
            .upsert_memory_with_file_hash(&indexed_memory, metadata_only, Some(&final_hash))
            .map_err(|_err| {
                let pending = PendingEncryptedIndexOp {
                    op_id: operation_id.clone(),
                    indexed_memory: indexed_memory.clone(),
                    metadata_only,
                    expected_ciphertext_hash: final_hash.clone(),
                    enqueued_at: Utc::now(),
                    attempts: 0,
                    last_error: None,
                };
                RepairCascade {
                    runtime: &self.roots.runtime,
                    op: IndexRepairOp::Encrypted(Box::new(pending)),
                    marker_reason: "pending encrypted metadata index enqueue failed",
                    failure_kinds: CascadeFailureKinds::Tiered,
                    durability: self.durability,
                    operation_id: operation_id.clone(),
                }
                .into_failure()
            })?;
        Ok(())
    }

    /// Tombstone a memory.
    pub async fn tombstone_memory(&self, request: TombstoneRequest) -> Result<WriteOutcome, WriteFailure> {
        let operation_id = new_operation_id();
        let outcome = WriteOutcome::not_committed(operation_id.clone(), self.durability);
        let envelope = self.read_memory_envelope(&request.id).await.map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err.to_string())),
        })?;
        let mut memory = envelope.metadata;
        let path = memory.path.clone().unwrap_or_else(|| default_memory_path(&memory));
        let prior_status = memory.frontmatter.status;
        memory.frontmatter.status = MemoryStatus::Tombstoned;
        memory.frontmatter.updated_at = Utc::now();
        memory.frontmatter.superseded_by.clear();
        // Deferred: hydrate actor and reason from caller-supplied classification.
        memory.frontmatter.tombstone_events.push(crate::model::TombstoneEvent {
            id: format!("tomb_{}", uuid::Uuid::new_v4().simple()),
            applied_at: Utc::now(),
            actor: crate::model::TombstoneActor {
                kind: crate::model::TombstoneActorKind::System,
                reference: "stream-a".to_string(),
            },
            reason: crate::model::TombstoneKind::Other,
            reason_text: Some(request.reason.clone()),
            reason_hash: None,
            prior_status,
        });
        memory.frontmatter.retrieval_policy.index_body = false;
        memory.frontmatter.retrieval_policy.index_embeddings = false;
        memory.path = Some(path.clone());
        validate_frontmatter(&memory.frontmatter).map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err.to_string())),
        })?;
        let final_hash = atomic_write(crate::markdown::AtomicWrite {
            repo: &self.roots.repo,
            memory: &memory,
            expected_base_hash: None,
            mode: WriteMode::AdminRepair,
            operation_id: &operation_id,
            durability: self.durability,
            suppression: Some(&self.suppression),
            allow_encrypted_namespace: path.as_str().starts_with("encrypted/"),
        })?;
        self.index
            .lock()
            .map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() },
            })?
            .upsert_memory_with_file_hash(&memory, true, Some(&final_hash))
            .map_err(|_err| {
                let pending = PendingIndexOp {
                    op_id: operation_id.clone(),
                    kind: PendingIndexKind::UpsertPath,
                    path: path.clone(),
                    memory_id: Some(memory.frontmatter.id.clone()),
                    expected_file_hash: Some(final_hash.clone()),
                    enqueued_at: Utc::now(),
                    attempts: 0,
                    last_error: None,
                };
                RepairCascade {
                    runtime: &self.roots.runtime,
                    op: IndexRepairOp::Plain(pending),
                    marker_reason: "pending tombstone index enqueue failed",
                    failure_kinds: CascadeFailureKinds::Tiered,
                    durability: self.durability,
                    operation_id: operation_id.clone(),
                }
                .into_failure()
            })?;
        self.commit_lifecycle_event(
            EventKind::TombstoneCommitted { id: request.id },
            &operation_id,
            "pending tombstone event enqueue failed",
            &outcome,
        )
    }

    /// Allocate next memory id.
    pub async fn next_memory_id(&self) -> Result<MemoryId, crate::error::IdError> {
        next_memory_id(&self.roots.runtime, &self.device_id, &HashSet::new())
    }

    fn validate_memory_path(&self, memory: &Memory, outcome: WriteOutcome) -> Result<(), WriteFailure> {
        let path = memory.path.clone().unwrap_or_else(|| default_memory_path(memory));
        if !path.is_safe_relative() {
            return Err(WriteFailure {
                outcome,
                kind: WriteFailureKind::ValidationTyped(ValidationError::Other(format!(
                    "invalid repo path: {}",
                    path.as_str()
                ))),
            });
        }
        if path.as_str().starts_with("encrypted/") {
            return Err(WriteFailure {
                outcome,
                kind: WriteFailureKind::ValidationTyped(ValidationError::Other(format!(
                    "plaintext writes cannot target encrypted namespace: {}",
                    path.as_str()
                ))),
            });
        }
        ensure_write_parent_contained(&self.roots.repo, &path).map_err(|err| WriteFailure {
            outcome,
            kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err)),
        })
    }

    fn enforce_best_effort_opt_in(&self, allow_best_effort: bool, outcome: WriteOutcome) -> Result<(), WriteFailure> {
        if matches!(self.durability, DurabilityTier::BestEffort) && !allow_best_effort {
            return Err(WriteFailure { outcome, kind: WriteFailureKind::DurabilityUnavailable });
        }
        Ok(())
    }

    fn enforce_plaintext_classification(
        &self,
        request: &WriteRequest,
        outcome: WriteOutcome,
    ) -> Result<(), WriteFailure> {
        match request.classification {
            ClassificationOutcome::Secret => Err(WriteFailure { outcome, kind: WriteFailureKind::SecretRefused }),
            ClassificationOutcome::RequiresEncryption => {
                Err(WriteFailure { outcome, kind: WriteFailureKind::EncryptionRequired })
            }
            ClassificationOutcome::Trusted
                if matches!(
                    request.memory.frontmatter.sensitivity,
                    Sensitivity::Confidential | Sensitivity::Personal
                ) =>
            {
                Err(WriteFailure { outcome, kind: WriteFailureKind::ClassificationSensitivityMismatch })
            }
            ClassificationOutcome::Trusted => Ok(()),
        }
    }

    /// Run a single pre-disk refusal gate against the per-write audit context.
    ///
    /// Thin wrapper over [`Self::guard_with_refusal_audit`] that sources the
    /// invariant audit fields (id, path, classification, operation id) from `ctx`
    /// so each gate at a call site reads as `self.run_gate(&ctx, <gate result>)?`.
    fn run_gate(&self, ctx: &RefusalAuditContext, result: Result<(), WriteFailure>) -> Result<(), WriteFailure> {
        self.guard_with_refusal_audit(result, ctx.id.clone(), ctx.path.clone(), ctx.classification, &ctx.operation_id)
    }

    /// Wrap a refusal-gate result so a refusal emits a `WriteRefused` event before
    /// returning to the caller (spec §8.7 step 6, §12.2 `WriteRefused`).
    ///
    /// On the success arm, returns `Ok(())` unchanged. On the refusal arm, attempts
    /// to append a `WriteRefused` event; the audit-event append failure is
    /// intentionally swallowed because the refusal itself remains the outcome the
    /// caller observes.
    #[allow(clippy::too_many_arguments)]
    fn guard_with_refusal_audit(
        &self,
        result: Result<(), WriteFailure>,
        id: MemoryId,
        path: Option<RepoPath>,
        classification: ClassificationOutcome,
        operation_id: &OperationId,
    ) -> Result<(), WriteFailure> {
        match result {
            Ok(()) => Ok(()),
            Err(failure) => {
                let event = EventKind::write_refused(Some(id), path, classification, &failure.kind);
                // The refusal is the caller-observed outcome regardless, but the audit-event
                // append failure must not be fully silent (spec §8.7 step 6 audit trail).
                if let Err(err) = self.record_event(event, operation_id) {
                    tracing::warn!("failed to append WriteRefused audit event for refused write: {err}");
                }
                Err(failure)
            }
        }
    }

    /// Append a post-commit lifecycle event, running the spec §8.7 durability
    /// ladder once for all write paths.
    ///
    /// On success returns the fully-committed `WriteOutcome`. If the event append
    /// fails, the three-tier fallback runs: enqueue a `PendingEvent` repair (still
    /// `Ok`, the canonical write is durable); else write a startup marker keyed by
    /// `marker_reason` (`FullStartupScan` + `RepairQueueFailed`); else surface
    /// `OperatorRequired` (`RepairStateNotDurable`). `not_committed` supplies the
    /// outcome reported if event assembly itself fails before any append.
    #[allow(clippy::too_many_arguments)]
    fn commit_lifecycle_event(
        &self,
        kind: EventKind,
        operation_id: &OperationId,
        marker_reason: &str,
        not_committed: &WriteOutcome,
    ) -> Result<WriteOutcome, WriteFailure> {
        let event = self.build_recorded_event(kind, operation_id).map_err(|err| WriteFailure {
            outcome: not_committed.clone(),
            kind: WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() },
        })?;
        if let Err(err) = self.append_event_and_mirror(&event, false) {
            let pending = PendingEventOp {
                op_id: operation_id.clone(),
                event_id: event.id.clone(),
                event,
                enqueued_at: Utc::now(),
                attempts: 0,
                last_error: Some(err.to_string()),
            };
            if enqueue_pending_event(&self.roots.runtime, &pending).is_ok() {
                return Ok(committed_pending_event(operation_id.clone(), self.durability));
            }
            if write_startup_marker(&self.roots.runtime, marker_reason).is_ok() {
                return Err(WriteFailure {
                    outcome: committed_event_repair(
                        operation_id.clone(),
                        self.durability,
                        RepairRequired::FullStartupScan,
                    ),
                    kind: WriteFailureKind::RepairQueueFailed,
                });
            }
            return Err(WriteFailure {
                outcome: committed_event_repair(
                    operation_id.clone(),
                    self.durability,
                    RepairRequired::OperatorRequired("repair state not durable".to_string()),
                ),
                kind: WriteFailureKind::RepairStateNotDurable,
            });
        }
        Ok(committed_indexed_recorded(operation_id.clone(), self.durability))
    }
}
