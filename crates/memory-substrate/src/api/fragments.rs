//! Stream F substrate fragments: append to the per-device JSONL series and
//! archive expired plaintext fragments.

use super::*;

impl Substrate {
    /// Append a Stream F substrate fragment to the per-device JSONL series.
    pub async fn append_substrate_fragment(
        &self,
        request: SubstrateFragmentAppendRequest,
    ) -> Result<SubstrateFragmentAppendOutcome, WriteFailure> {
        let operation_id = request.operation_id.clone().unwrap_or_else(new_operation_id);
        let outcome = WriteOutcome::not_committed(operation_id.clone(), self.durability);
        if matches!(request.classification, ClassificationOutcome::Secret) {
            return Err(WriteFailure { outcome, kind: WriteFailureKind::SecretRefused });
        }
        validate_substrate_fragment_append(&request).map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err)),
        })?;
        let id = request.id.clone().unwrap_or_else(new_substrate_fragment_id);
        validate_substrate_fragment_id(&id).map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err.to_string())),
        })?;
        let device = DeviceId::try_new(&self.device_id).map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err.to_string())),
        })?;
        let path = substrate_fragment_path(&request, device.as_str()).map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err)),
        })?;

        match &request.payload {
            SubstrateFragmentPayload::Plaintext { text } => {
                let record = SubstrateFragmentRecord {
                    id: id.clone(),
                    ts: request.at,
                    device,
                    session: request.session.clone(),
                    harness: request.harness.clone(),
                    scope: request.scope.clone(),
                    entities: request.entities.clone(),
                    kind: request.kind,
                    text: text.clone(),
                    source_ref: request.source_ref.clone(),
                    privacy_spans: request.privacy_spans.clone(),
                };
                append_jsonl_record(
                    JsonlWriteTarget::new(&self.roots.repo, &path, &operation_id, self.durability),
                    &record,
                )
                .map_err(|err| WriteFailure {
                    outcome: outcome.clone(),
                    kind: WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() },
                })?;
            }
            SubstrateFragmentPayload::Encrypted { encryption, descriptor } => {
                let record = EncryptedSubstrateFragmentRecord {
                    id: id.clone(),
                    ts: request.at,
                    device,
                    session: request.session.clone(),
                    harness: request.harness.clone(),
                    scope: request.scope.clone(),
                    entities: request.entities.clone(),
                    kind: request.kind,
                    encryption: encryption.clone(),
                    descriptor: descriptor.clone(),
                    source_ref: request.source_ref.clone(),
                    privacy_spans: request.privacy_spans.clone(),
                };
                append_jsonl_record(
                    JsonlWriteTarget::new(&self.roots.repo, &path, &operation_id, self.durability),
                    &record,
                )
                .map_err(|err| WriteFailure {
                    outcome: outcome.clone(),
                    kind: WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() },
                })?;
            }
        }

        let event_kind = EventKind::SubstrateFragmentWritten {
            id: id.clone(),
            path: path.clone(),
            classification: request.classification,
        };
        self.record_event(event_kind, &operation_id).map_err(|err| WriteFailure {
            outcome: committed_pending_event(operation_id.clone(), self.durability),
            kind: WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() },
        })?;

        Ok(SubstrateFragmentAppendOutcome { id, path, operation_id })
    }

    /// Archive expired plaintext substrate fragments for this device.
    pub async fn archive_expired_substrate_fragments(
        &self,
        now: DateTime<Utc>,
        lifetime_days: i64,
    ) -> Result<SubstrateArchiveOutcome, WriteFailure> {
        let operation_id = new_operation_id();
        let outcome = WriteOutcome::not_committed(operation_id.clone(), self.durability);
        if lifetime_days < 1 {
            return Err(WriteFailure {
                outcome,
                kind: WriteFailureKind::ValidationTyped(ValidationError::Other(
                    "lifetime_days must be positive".to_string(),
                )),
            });
        }
        let device = DeviceId::try_new(&self.device_id).map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err.to_string())),
        })?;
        let device_dir = self.roots.repo.join("substrate").join(device.as_str());
        if !device_dir.exists() {
            return Ok(SubstrateArchiveOutcome { fragments_archived: 0 });
        }

        let mut archive_batches: BTreeMap<RepoPath, Vec<SubstrateFragmentRecord>> = BTreeMap::new();
        for entry in std::fs::read_dir(&device_dir).map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() },
        })? {
            let entry = entry.map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() },
            })?;
            let file_path = entry.path();
            if file_path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let repo_path = absolute_to_repo_path(&self.roots.repo, &file_path).map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err)),
            })?;
            let records = read_substrate_records(&file_path).map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() },
            })?;
            let (expired, live): (Vec<_>, Vec<_>) =
                records.into_iter().partition(|record| record.ts + Duration::days(lifetime_days) <= now);
            if expired.is_empty() {
                continue;
            }
            write_jsonl_records(
                JsonlWriteTarget::new(&self.roots.repo, &repo_path, &operation_id, self.durability),
                &live,
            )
            .map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() },
            })?;
            for record in expired {
                let archive_path = RepoPath::try_new(format!(
                    "substrate/archive/{}/{}.jsonl",
                    device.as_str(),
                    record.ts.format("%Y-%m")
                ))
                .map_err(|err| WriteFailure {
                    outcome: outcome.clone(),
                    kind: WriteFailureKind::ValidationTyped(ValidationError::Other(err)),
                })?;
                archive_batches.entry(archive_path).or_default().push(record);
            }
        }

        let mut fragments_archived = 0;
        for (archive_path, mut new_records) in archive_batches {
            let absolute_archive = self.roots.repo.join(archive_path.as_path());
            let mut records = read_substrate_records(&absolute_archive).map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() },
            })?;
            let mut seen: BTreeSet<String> = records.iter().map(|record| record.id.clone()).collect();
            for record in new_records.drain(..) {
                if seen.insert(record.id.clone()) {
                    records.push(record);
                    fragments_archived += 1;
                }
            }
            records.sort_by(|left, right| left.id.cmp(&right.id));
            write_jsonl_records(
                JsonlWriteTarget::new(&self.roots.repo, &archive_path, &operation_id, self.durability),
                &records,
            )
            .map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::IoTyped { kind: std::io::ErrorKind::Other, context: err.to_string() },
            })?;
        }

        Ok(SubstrateArchiveOutcome { fragments_archived })
    }
}
