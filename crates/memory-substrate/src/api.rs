#![allow(unknown_lints, file_too_long)]
//! Public API orchestration remains centralized until Task 10 seam split.
//! Public Stream A API.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::error::{OpenError, ReadError, SubstrateResult, VectorError, WriteFailure, WriteFailureKind};
use crate::events::{
    append_event, append_event_best_effort, read_events, reserve_event_sequence, sync_event_sequence_state, Event,
    EventKind,
};
use crate::frontmatter::validate_frontmatter;
use crate::git;
use crate::ids::next_memory_id;
use crate::index::{open_index, Index};
use crate::markdown::{atomic_write, probe_durability, read_memory_file};
use crate::model::*;
use crate::runtime::reconcile::{
    enqueue_pending_encrypted_index, enqueue_pending_event, enqueue_pending_index, reconcile_startup_pre_index,
    replay_pending_repairs, write_startup_marker, PendingEncryptedIndexOp, PendingEventOp, PendingIndexKind,
    PendingIndexOp,
};
use crate::tree::{bootstrap_repo_layout, validate_tree, TreeValidationMode};
use crate::watcher::{watch_root_with_suppression, SuppressionLedger, WatchSubscription};

/// Stream A substrate handle.
#[derive(Clone)]
pub struct Substrate {
    roots: Roots,
    device_id: String,
    durability: DurabilityTier,
    index: Arc<Mutex<Index>>,
    event_log: PathBuf,
    best_effort_event_seq: Arc<AtomicU64>,
    suppression: Arc<Mutex<SuppressionLedger>>,
}

impl Substrate {
    /// Roots backing this substrate handle.
    pub fn roots(&self) -> &Roots {
        &self.roots
    }

    /// Initialize a new memory repository and open it.
    ///
    /// Q4: `git::adopt_clone` is the sole authority that mints
    /// `local-device.yaml`; `init` drives that path so a fresh repo's first
    /// open has a valid device identity in place. Tests / daemons that want to
    /// supply their own device id thread it through `InitOptions::device_id`,
    /// which is forwarded to `git::adopt_clone_explicit`.
    pub async fn init(roots: Roots, options: InitOptions) -> Result<Self, OpenError> {
        let merge_driver = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("memory-merge-driver"));
        git::init_git_repo(&roots.repo, &merge_driver).map_err(|err| OpenError::InvalidRoots(err.to_string()))?;
        std::fs::create_dir_all(&roots.runtime)?;
        // Mint device identity via git::adopt_clone_explicit (Q4 authority).
        crate::git::adopt_clone_explicit(&roots.repo, &roots.runtime, &merge_driver, options.device_id, false)
            .map_err(|err| OpenError::InvalidRoots(err.to_string()))?;
        // Seed a minimal config.yaml so `open_with_options` can load the active
        // embedding triple.  Deferred: `InitOptions` should carry an explicit
        // `active_embedding` field so callers control the triple.
        write_initial_config_if_absent(&roots.repo)?;
        Self::open_with_options(roots, options.force_unsafe_durability).await
    }

    /// Open an existing substrate.
    pub async fn open(roots: Roots) -> Result<Self, OpenError> {
        Self::open_with_options(roots, false).await
    }

    /// Adopt a cloned repo and open it.
    ///
    /// Q4: `git::adopt_clone` mints `local-device.yaml`. When
    /// `force_new_device` is set, the prior identity file is removed first so
    /// `adopt_clone`'s skip-if-exists guard mints a fresh one.
    pub async fn adopt_clone(roots: Roots, options: AdoptOptions) -> Result<Self, OpenError> {
        if options.force_new_device {
            let local_device = roots.runtime.join("local-device.yaml");
            if local_device.exists() {
                std::fs::remove_file(local_device)?;
            }
        }
        git::adopt_clone(&roots.repo, &roots.runtime).map_err(|err| OpenError::InvalidRoots(err.to_string()))?;
        Self::open(roots).await
    }

    /// Doctor report.
    pub async fn doctor(&self) -> DoctorReport {
        let validation = validate_tree(&self.roots.repo, TreeValidationMode::PartialSync);
        let mut report =
            DoctorReport { durability_tier: self.durability, warnings: Vec::new(), repairs_required: Vec::new() };
        if let Err(err) = validation {
            report.repairs_required.push(err.to_string());
        }
        report
    }

    /// Read a memory by id (legacy `Memory` shape; prefer
    /// [`Self::read_memory_envelope`] for the spec §16.2 shape).
    ///
    /// B-API-7 (resolve via index) is staged behind the envelope API; the legacy
    /// path keeps its O(n) walk to avoid breaking existing callers in this pass.
    pub async fn read_memory(&self, id: &MemoryId) -> Result<Memory, ReadError> {
        self.read_memory_with_hash(id).await.map(|(memory, _hash)| memory)
    }

    async fn read_memory_with_hash(&self, id: &MemoryId) -> Result<(Memory, Sha256), ReadError> {
        let paths = crate::tree::relative_memory_paths(&self.roots.repo);
        for path in paths {
            let repo_path = RepoPath::new(path.to_string_lossy().replace('\\', "/"));
            if repo_path.as_str().starts_with("encrypted/") {
                continue;
            }
            let (memory, hash) = read_memory_file(&self.roots.repo, &repo_path)?;
            if &memory.frontmatter.id == id {
                return Ok((memory, hash));
            }
        }
        // `from_unchecked`: id-shaped string used only for the NotFound diagnostic path.
        Err(ReadError::NotFound(RepoPath::from_unchecked(id.as_str())))
    }

    /// Read a memory by id and return the spec §16.2 `MemoryEnvelope` (B-API-1).
    ///
    /// Routes plaintext, encrypted-ciphertext, and metadata-only encrypted
    /// reads through the typed `MemoryContent` discriminator so Stream E can
    /// dispatch without inspecting paths or extras.
    ///
    /// Resolution: index lookup first; falls back to filesystem walk when the
    /// memory is not yet indexed (B-API-7 fast path is index-first; the walk
    /// preserves legacy "found-on-disk" semantics).
    pub async fn read_memory_envelope(&self, id: &MemoryId) -> Result<MemoryEnvelope, ReadError> {
        let path = self.resolve_memory_id_to_path(id)?;
        self.read_path_envelope(&path).await
    }

    /// Read by repository path; returns the spec §16.2 `MemoryEnvelope` (B-API-1).
    pub async fn read_path_envelope(&self, path: &RepoPath) -> Result<MemoryEnvelope, ReadError> {
        if is_noncanonical_stream_f_repo_path(path.as_str()) {
            return Err(ReadError::NotACanonicalMemory { path: path.clone() });
        }
        if path.as_str().starts_with("encrypted/") {
            return self.read_ciphertext_envelope(path);
        }
        let memory = read_memory_file(&self.roots.repo, path).map(|(memory, _)| memory)?;
        let body = memory.body.clone();
        Ok(MemoryEnvelope { metadata: memory, content: MemoryContent::Plaintext(body) })
    }

    /// Read by repository path (legacy `Memory` shape).
    pub async fn read_path(&self, path: &RepoPath) -> Result<Memory, ReadError> {
        read_memory_file(&self.roots.repo, path).map(|(memory, _)| memory)
    }

    fn read_ciphertext_envelope(&self, path: &RepoPath) -> Result<MemoryEnvelope, ReadError> {
        let absolute = self.roots.repo.join(path.as_path());
        let bytes = std::fs::read(&absolute)?;
        // Try Markdown parse first — encrypted records now persist a parseable
        // metadata projection with base64-encoded ciphertext in the body. If
        // parsing fails, fall back to raw ciphertext bytes for legacy files.
        if let Ok(text) = String::from_utf8(bytes.clone()) {
            if let Ok(parsed) = crate::frontmatter::parse_document(&text, Some(path.clone())) {
                let metadata = parsed.memory.clone();
                let envelope_meta =
                    metadata.frontmatter.extras.get("encryption").cloned().map(|value| EncryptionEnvelope {
                        scheme: value.get("scheme").and_then(|v| v.as_str()).unwrap_or("unspecified").to_string(),
                        recipient: value.get("recipient").and_then(|v| v.as_str()).unwrap_or("unspecified").to_string(),
                        metadata: Some(value),
                    });
                let content = match envelope_meta {
                    Some(envelope) if !metadata.body.is_empty() => MemoryContent::Ciphertext {
                        bytes: BASE64_STANDARD.decode(metadata.body.as_bytes()).map_err(|err| ReadError::Parse {
                            path: path.clone(),
                            message: format!("invalid encrypted body encoding: {err}"),
                        })?,
                        encryption: envelope,
                    },
                    Some(_) => MemoryContent::MetadataOnly,
                    None => MemoryContent::MetadataOnly,
                };
                return Ok(MemoryEnvelope { metadata, content });
            }
        }
        // Pure ciphertext: build a placeholder metadata from the path; Stream D
        // owns translating this into a richer Memory after decrypt.
        let placeholder_id = MemoryId::try_new(format!(
            "mem_{}",
            // Best-effort: derive from path stem when it resembles an id.
            path.as_path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.trim_start_matches("mem_").to_string())
                .unwrap_or_else(|| "00000000_0000000000000000_000000".to_string())
        ))
        .unwrap_or_else(|_| MemoryId::new("mem_20260424_0000000000000000_000000"));
        let metadata = Memory {
            frontmatter: placeholder_frontmatter(&placeholder_id),
            body: String::new(),
            path: Some(path.clone()),
        };
        let envelope = EncryptionEnvelope {
            scheme: "unspecified".to_string(),
            recipient: "unspecified".to_string(),
            metadata: None,
        };
        Ok(MemoryEnvelope { metadata, content: MemoryContent::Ciphertext { bytes, encryption: envelope } })
    }

    fn resolve_memory_id_to_path(&self, id: &MemoryId) -> Result<RepoPath, ReadError> {
        // Prefer index lookup; fall back to disk walk if the index is empty
        // (e.g. fresh open before any read paths through it).
        let query = MemoryQuery { id: Some(id.clone()), include_metadata_only: true, ..MemoryQuery::default() };
        let from_index = self.index.lock().ok().and_then(|guard| guard.query_memory(&query).ok());
        if let Some(rows) = from_index {
            if let Some(hit) = rows.into_iter().next() {
                return Ok(hit.path);
            }
        }
        // Disk-walk fallback (Phase 5 retains it pending B-API-7's index
        // hydration of `frontmatter_json`).
        for path in crate::tree::relative_memory_paths(&self.roots.repo) {
            let repo_path = RepoPath::new(path.to_string_lossy().replace('\\', "/"));
            if let Ok((memory, _)) = read_memory_file(&self.roots.repo, &repo_path) {
                if &memory.frontmatter.id == id {
                    return Ok(repo_path);
                }
            }
        }
        // The id is well-formed but not present in the tree. Use
        // `from_unchecked` to embed the id-shaped string in `NotFound`'s
        // `RepoPath` slot for diagnostics — the path validator would reject it.
        Err(ReadError::NotFound(RepoPath::from_unchecked(id.as_str())))
    }

    /// Write plaintext memory.
    pub async fn write_memory(&self, request: WriteRequest) -> Result<WriteOutcome, WriteFailure> {
        let operation_id = request.operation_id.clone().unwrap_or_else(new_operation_id);
        let outcome = WriteOutcome::not_committed(operation_id.clone(), self.durability);
        // Pre-disk refusal gates emit `WriteRefused` audit events per spec §8.7 step 6.
        self.guard_with_refusal_audit(
            self.enforce_best_effort_opt_in(request.allow_best_effort_durability, outcome.clone()),
            request.memory.frontmatter.id.clone(),
            request.memory.path.clone(),
            request.classification,
            &operation_id,
        )?;
        self.guard_with_refusal_audit(
            self.enforce_plaintext_classification(&request, outcome.clone()),
            request.memory.frontmatter.id.clone(),
            request.memory.path.clone(),
            request.classification,
            &operation_id,
        )?;
        self.guard_with_refusal_audit(
            self.validate_memory_path(&request.memory, outcome.clone()),
            request.memory.frontmatter.id.clone(),
            request.memory.path.clone(),
            request.classification,
            &operation_id,
        )?;
        self.guard_with_refusal_audit(
            enforce_no_dream_prose_sources(&request.memory, outcome.clone()),
            request.memory.frontmatter.id.clone(),
            request.memory.path.clone(),
            request.classification,
            &operation_id,
        )?;
        self.guard_with_refusal_audit(
            validate_frontmatter(&request.memory.frontmatter).map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::Validation(err.to_string()),
            }),
            request.memory.frontmatter.id.clone(),
            request.memory.path.clone(),
            request.classification,
            &operation_id,
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
        let upsert_res = {
            let mut index_guard = self.index.lock().map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::Io(err.to_string()),
            })?;
            index_guard.upsert_memory(&request.memory, false)
        };
        if let Err(_idx_err) = upsert_res {
            let pending = PendingIndexOp {
                op_id: operation_id.clone(),
                kind: PendingIndexKind::UpsertPath,
                path: request.memory.path.clone().unwrap_or_else(|| {
                    RepoPath::new(format!("agent/patterns/{}.md", request.memory.frontmatter.id.as_str()))
                }),
                memory_id: Some(request.memory.frontmatter.id.clone()),
                expected_file_hash: Some(final_hash.clone()),
                enqueued_at: Utc::now(),
                attempts: 0,
                last_error: None,
            };
            let repair_kind = if enqueue_pending_index(&self.roots.runtime, &pending).is_ok() {
                Some(RepairRequired::PendingIndex)
            } else if write_startup_marker(&self.roots.runtime, "pending index enqueue failed").is_ok() {
                Some(RepairRequired::FullStartupScan)
            } else {
                Some(RepairRequired::OperatorRequired("repair state not durable".to_string()))
            };
            return Err(WriteFailure {
                outcome: WriteOutcome {
                    committed: true,
                    indexed: false,
                    event_recorded: false,
                    durability: self.durability,
                    repair_required: repair_kind,
                    operation_id: operation_id.clone(),
                },
                kind: WriteFailureKind::IndexAfterCommitFailed,
            });
        }
        let write_event_kind = EventKind::WriteCommitted {
            id: request.memory.frontmatter.id.clone(),
            path: request.memory.path.clone().unwrap_or_else(|| {
                RepoPath::new(format!("agent/patterns/{}.md", request.memory.frontmatter.id.as_str()))
            }),
            classification: request.classification,
        };
        let device = DeviceId::try_new(&self.device_id)
            .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Io(err.to_string()) })?;
        let seq = reserve_event_sequence(&self.roots.runtime, &self.event_log, &device)
            .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Io(err.to_string()) })?;
        let event = Event {
            schema: crate::SUBSTRATE_SCHEMA_VERSION,
            id: EventId::new(format!("evt_{}", uuid::Uuid::new_v4())),
            at: Utc::now(),
            device,
            seq,
            operation_id: Some(operation_id.clone()),
            kind: write_event_kind,
            crc32c: 0,
        };
        if let Err(err) = append_event(&self.event_log, &event) {
            let pending = PendingEventOp {
                op_id: operation_id.clone(),
                event_id: event.id.clone(),
                event,
                enqueued_at: Utc::now(),
                attempts: 0,
                last_error: Some(err.to_string()),
            };
            if enqueue_pending_event(&self.roots.runtime, &pending).is_ok() {
                return Ok(WriteOutcome {
                    committed: true,
                    indexed: true,
                    event_recorded: false,
                    durability: self.durability,
                    repair_required: Some(RepairRequired::PendingEvent),
                    operation_id,
                });
            }
            if write_startup_marker(&self.roots.runtime, "pending event enqueue failed").is_ok() {
                return Err(WriteFailure {
                    outcome: WriteOutcome {
                        committed: true,
                        indexed: true,
                        event_recorded: false,
                        durability: self.durability,
                        repair_required: Some(RepairRequired::FullStartupScan),
                        operation_id: operation_id.clone(),
                    },
                    kind: WriteFailureKind::RepairQueueFailed,
                });
            }
            return Err(WriteFailure {
                outcome: WriteOutcome {
                    committed: true,
                    indexed: true,
                    event_recorded: false,
                    durability: self.durability,
                    repair_required: Some(RepairRequired::OperatorRequired("repair state not durable".to_string())),
                    operation_id: operation_id.clone(),
                },
                kind: WriteFailureKind::RepairStateNotDurable,
            });
        }
        Ok(WriteOutcome {
            committed: true,
            indexed: true,
            event_recorded: true,
            durability: self.durability,
            repair_required: None,
            operation_id,
        })
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
                kind: WriteFailureKind::Validation(err.to_string()),
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
        let mem_id = request.metadata_memory.frontmatter.id.clone();
        let mem_path = request.metadata_memory.path.clone();
        self.guard_with_refusal_audit(
            self.enforce_best_effort_opt_in(request.allow_best_effort_durability, outcome.clone()),
            mem_id.clone(),
            mem_path.clone(),
            request.classification,
            &operation_id,
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
        self.guard_with_refusal_audit(
            classification_check,
            mem_id.clone(),
            mem_path.clone(),
            request.classification,
            &operation_id,
        )?;
        self.guard_with_refusal_audit(
            self.validate_memory_path(&request.metadata_memory, outcome.clone()),
            mem_id.clone(),
            mem_path.clone(),
            request.classification,
            &operation_id,
        )?;
        self.guard_with_refusal_audit(
            enforce_no_dream_prose_sources(&request.metadata_memory, outcome.clone()),
            mem_id.clone(),
            mem_path.clone(),
            request.classification,
            &operation_id,
        )?;
        validate_frontmatter(&request.metadata_memory.frontmatter).map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::Validation(err.to_string()),
        })?;
        let path = encrypted_ciphertext_path(&request.metadata_memory)
            .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Validation(err) })?;
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
        let encrypted_document = crate::frontmatter::serialize_document(&stored_memory).map_err(|err| {
            WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Validation(err.to_string()) }
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
                WriteFailureKind::Io(err.to_string())
            },
        })?;
        let mut indexed_memory = request.metadata_memory.clone();
        indexed_memory.path = Some(path.clone());
        let metadata_only =
            match request.safe_index_projection.as_ref().and_then(|projection| projection.safe_body.as_ref()) {
                Some(safe_body) => {
                    indexed_memory.body = safe_body.clone();
                    indexed_memory.frontmatter.retrieval_policy.index_body = true;
                    false
                }
                None => {
                    indexed_memory.body.clear();
                    true
                }
            };
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
                kind: WriteFailureKind::Io(err.to_string()),
            })?
            .upsert_memory(&indexed_memory, metadata_only)
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
                let (repair_kind, kind) = if enqueue_pending_encrypted_index(&self.roots.runtime, &pending).is_ok() {
                    (Some(RepairRequired::PendingIndex), WriteFailureKind::IndexAfterCommitFailed)
                } else if write_startup_marker(&self.roots.runtime, "pending encrypted index enqueue failed").is_ok() {
                    (Some(RepairRequired::FullStartupScan), WriteFailureKind::RepairQueueFailed)
                } else {
                    (
                        Some(RepairRequired::OperatorRequired("repair state not durable".to_string())),
                        WriteFailureKind::RepairStateNotDurable,
                    )
                };
                WriteFailure {
                    outcome: WriteOutcome {
                        committed: true,
                        indexed: false,
                        event_recorded: false,
                        durability: self.durability,
                        repair_required: repair_kind,
                        operation_id: operation_id.clone(),
                    },
                    kind,
                }
            })?;
        let encrypted_event_kind = EventKind::EncryptedWriteCommitted {
            id: request.metadata_memory.frontmatter.id.clone(),
            path,
            classification: request.classification,
        };
        let device = DeviceId::try_new(&self.device_id)
            .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Io(err.to_string()) })?;
        let seq = reserve_event_sequence(&self.roots.runtime, &self.event_log, &device)
            .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Io(err.to_string()) })?;
        let event = Event {
            schema: crate::SUBSTRATE_SCHEMA_VERSION,
            id: EventId::new(format!("evt_{}", uuid::Uuid::new_v4())),
            at: Utc::now(),
            device,
            seq,
            operation_id: Some(operation_id.clone()),
            kind: encrypted_event_kind,
            crc32c: 0,
        };
        if let Err(err) = append_event(&self.event_log, &event) {
            let pending = PendingEventOp {
                op_id: operation_id.clone(),
                event_id: event.id.clone(),
                event,
                enqueued_at: Utc::now(),
                attempts: 0,
                last_error: Some(err.to_string()),
            };
            if enqueue_pending_event(&self.roots.runtime, &pending).is_ok() {
                return Ok(WriteOutcome {
                    committed: true,
                    indexed: true,
                    event_recorded: false,
                    durability: self.durability,
                    repair_required: Some(RepairRequired::PendingEvent),
                    operation_id,
                });
            }
            if write_startup_marker(&self.roots.runtime, "pending encrypted event enqueue failed").is_ok() {
                return Err(WriteFailure {
                    outcome: WriteOutcome {
                        committed: true,
                        indexed: true,
                        event_recorded: false,
                        durability: self.durability,
                        repair_required: Some(RepairRequired::FullStartupScan),
                        operation_id: operation_id.clone(),
                    },
                    kind: WriteFailureKind::RepairQueueFailed,
                });
            }
            return Err(WriteFailure {
                outcome: WriteOutcome {
                    committed: true,
                    indexed: true,
                    event_recorded: false,
                    durability: self.durability,
                    repair_required: Some(RepairRequired::OperatorRequired("repair state not durable".to_string())),
                    operation_id: operation_id.clone(),
                },
                kind: WriteFailureKind::RepairStateNotDurable,
            });
        }
        Ok(WriteOutcome {
            committed: true,
            indexed: true,
            event_recorded: true,
            durability: self.durability,
            repair_required: None,
            operation_id,
        })
    }

    /// Append a Stream F substrate fragment to the per-device JSONL series.
    pub async fn append_substrate_fragment(
        &self,
        request: SubstrateFragmentAppendRequest,
    ) -> Result<SubstrateFragmentAppendOutcome, WriteFailure> {
        let operation_id = request.operation_id.clone().unwrap_or_else(new_operation_id);
        let outcome = WriteOutcome::not_committed(operation_id.clone(), self.durability);
        validate_substrate_fragment_append(&request)
            .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Validation(err) })?;
        let id = request.id.clone().unwrap_or_else(new_substrate_fragment_id);
        validate_substrate_fragment_id(&id).map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::Validation(err.to_string()),
        })?;
        let device = DeviceId::try_new(&self.device_id).map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::Validation(err.to_string()),
        })?;
        let path = substrate_fragment_path(&request, device.as_str())
            .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Validation(err) })?;

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
                    kind: WriteFailureKind::Io(err.to_string()),
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
                    kind: WriteFailureKind::Io(err.to_string()),
                })?;
            }
        }

        let event_kind = EventKind::SubstrateFragmentWritten {
            id: id.clone(),
            path: path.clone(),
            classification: request.classification,
        };
        self.record_event(event_kind, &operation_id).map_err(|err| WriteFailure {
            outcome: WriteOutcome {
                committed: true,
                indexed: true,
                event_recorded: false,
                durability: self.durability,
                repair_required: Some(RepairRequired::PendingEvent),
                operation_id: operation_id.clone(),
            },
            kind: WriteFailureKind::Io(err.to_string()),
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
                kind: WriteFailureKind::Validation("lifetime_days must be positive".to_string()),
            });
        }
        let device = DeviceId::try_new(&self.device_id).map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::Validation(err.to_string()),
        })?;
        let device_dir = self.roots.repo.join("substrate").join(device.as_str());
        if !device_dir.exists() {
            return Ok(SubstrateArchiveOutcome { fragments_archived: 0 });
        }

        let mut archive_batches: BTreeMap<RepoPath, Vec<SubstrateFragmentRecord>> = BTreeMap::new();
        for entry in std::fs::read_dir(&device_dir)
            .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Io(err.to_string()) })?
        {
            let entry = entry.map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::Io(err.to_string()),
            })?;
            let file_path = entry.path();
            if file_path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let repo_path = absolute_to_repo_path(&self.roots.repo, &file_path)
                .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Validation(err) })?;
            let records = read_substrate_records(&file_path).map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::Io(err.to_string()),
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
            .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Io(err.to_string()) })?;
            for record in expired {
                let archive_path =
                    RepoPath::new(format!("substrate/archive/{}/{}.jsonl", device.as_str(), record.ts.format("%Y-%m")));
                archive_batches.entry(archive_path).or_default().push(record);
            }
        }

        let mut fragments_archived = 0;
        for (archive_path, mut new_records) in archive_batches {
            let absolute_archive = self.roots.repo.join(archive_path.as_path());
            let mut records = read_substrate_records(&absolute_archive).map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::Io(err.to_string()),
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
            .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Io(err.to_string()) })?;
        }

        Ok(SubstrateArchiveOutcome { fragments_archived })
    }

    /// Tombstone a memory.
    pub async fn tombstone_memory(&self, request: TombstoneRequest) -> Result<WriteOutcome, WriteFailure> {
        let operation_id = new_operation_id();
        let outcome = WriteOutcome::not_committed(operation_id.clone(), self.durability);
        let envelope = self.read_memory_envelope(&request.id).await.map_err(|err| WriteFailure {
            outcome: outcome.clone(),
            kind: WriteFailureKind::Validation(err.to_string()),
        })?;
        let mut memory = envelope.metadata;
        let path = memory
            .path
            .clone()
            .unwrap_or_else(|| RepoPath::new(format!("agent/patterns/{}.md", memory.frontmatter.id.as_str())));
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
            kind: WriteFailureKind::Validation(err.to_string()),
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
            .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Io(err.to_string()) })?
            .upsert_memory(&memory, true)
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
                let (repair_required, kind) = if enqueue_pending_index(&self.roots.runtime, &pending).is_ok() {
                    (Some(RepairRequired::PendingIndex), WriteFailureKind::IndexAfterCommitFailed)
                } else if write_startup_marker(&self.roots.runtime, "pending tombstone index enqueue failed").is_ok() {
                    (Some(RepairRequired::FullStartupScan), WriteFailureKind::RepairQueueFailed)
                } else {
                    (
                        Some(RepairRequired::OperatorRequired("repair state not durable".to_string())),
                        WriteFailureKind::RepairStateNotDurable,
                    )
                };
                WriteFailure {
                    outcome: WriteOutcome {
                        committed: true,
                        indexed: false,
                        event_recorded: false,
                        durability: self.durability,
                        repair_required,
                        operation_id: operation_id.clone(),
                    },
                    kind,
                }
            })?;
        let device = DeviceId::try_new(&self.device_id)
            .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Io(err.to_string()) })?;
        let seq = reserve_event_sequence(&self.roots.runtime, &self.event_log, &device)
            .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Io(err.to_string()) })?;
        let event = Event {
            schema: crate::SUBSTRATE_SCHEMA_VERSION,
            id: EventId::new(format!("evt_{}", uuid::Uuid::new_v4())),
            at: Utc::now(),
            device,
            seq,
            operation_id: Some(operation_id.clone()),
            kind: EventKind::TombstoneCommitted { id: request.id },
            crc32c: 0,
        };
        if let Err(err) = append_event(&self.event_log, &event) {
            let pending = PendingEventOp {
                op_id: operation_id.clone(),
                event_id: event.id.clone(),
                event,
                enqueued_at: Utc::now(),
                attempts: 0,
                last_error: Some(err.to_string()),
            };
            if enqueue_pending_event(&self.roots.runtime, &pending).is_ok() {
                return Ok(WriteOutcome {
                    committed: true,
                    indexed: true,
                    event_recorded: false,
                    durability: self.durability,
                    repair_required: Some(RepairRequired::PendingEvent),
                    operation_id,
                });
            }
            if write_startup_marker(&self.roots.runtime, "pending tombstone event enqueue failed").is_ok() {
                return Err(WriteFailure {
                    outcome: WriteOutcome {
                        committed: true,
                        indexed: true,
                        event_recorded: false,
                        durability: self.durability,
                        repair_required: Some(RepairRequired::FullStartupScan),
                        operation_id: operation_id.clone(),
                    },
                    kind: WriteFailureKind::RepairQueueFailed,
                });
            }
            return Err(WriteFailure {
                outcome: WriteOutcome {
                    committed: true,
                    indexed: true,
                    event_recorded: false,
                    durability: self.durability,
                    repair_required: Some(RepairRequired::OperatorRequired("repair state not durable".to_string())),
                    operation_id: operation_id.clone(),
                },
                kind: WriteFailureKind::RepairStateNotDurable,
            });
        }
        Ok(WriteOutcome {
            committed: true,
            indexed: true,
            event_recorded: true,
            durability: self.durability,
            repair_required: None,
            operation_id,
        })
    }

    /// Allocate next memory id.
    pub async fn next_memory_id(&self) -> Result<MemoryId, crate::error::IdError> {
        next_memory_id(&self.roots.runtime, &self.device_id, &HashSet::new())
    }

    /// Rebuild derived index from files.
    pub async fn reindex(&self) -> SubstrateResult<usize> {
        let mut count = 0usize;
        let repo_paths = collect_reindex_paths(&self.roots.repo).map_err(OpenError::OperatorRepairRequired)?;
        self.index.lock().map_err(|err| OpenError::InvalidRoots(err.to_string()))?.clear_plaintext_memory_index()?;
        for (_repo_path, memory, metadata_only) in repo_paths {
            self.index
                .lock()
                .map_err(|err| OpenError::InvalidRoots(err.to_string()))?
                .upsert_memory(&memory, metadata_only)?;
            count += 1;
        }
        self.index.lock().map_err(|err| OpenError::InvalidRoots(err.to_string()))?.reconcile_active_embedding_jobs()?;
        Ok(count)
    }

    /// Query memories.
    pub async fn query_memory(&self, query: MemoryQuery) -> SubstrateResult<Vec<QueryResult>> {
        self.index.lock().map_err(|err| OpenError::InvalidRoots(err.to_string()))?.query_memory(&query)
    }

    /// Query recall-index rows without hydrating memory envelopes.
    pub async fn query_recall_index(&self, query: RecallIndexQuery) -> SubstrateResult<Vec<RecallIndexRow>> {
        self.index.lock().map_err(|err| OpenError::InvalidRoots(err.to_string()))?.query_recall_index(&query)
    }

    /// Query chunks.
    pub async fn query_chunks(&self, query: ChunkQuery) -> SubstrateResult<Vec<ChunkResult>> {
        if let (Some(triple), Some(vector)) = (query.triple.as_ref(), query.vector.as_ref()) {
            let index = self.index.lock().map_err(|err| OpenError::InvalidRoots(err.to_string()))?;
            return Ok(index.query_vector_chunks(triple, vector, 20)?);
        }
        let Some(text) = query.text else {
            return Ok(Vec::new());
        };
        let index = self.index.lock().map_err(|err| OpenError::InvalidRoots(err.to_string()))?;
        Ok(index.query_chunks(&text)?)
    }

    /// Update embedding for a chunk.
    pub async fn update_embedding(&self, update: EmbeddingUpdate) -> Result<(), VectorError> {
        self.index
            .lock()
            .map_err(|err| VectorError::IndexUnavailable(format!("index mutex poisoned: {err}")))?
            .update_embedding(&update)
    }

    /// Drop embedding model (legacy: returns vector count for backward compat).
    ///
    /// New code should call [`Self::drop_embedding_model_report`] (B-API-4) for
    /// the spec §16.4 `DropTripleReport` shape.
    pub async fn drop_embedding_model(&self, triple: EmbeddingTriple) -> Result<usize, VectorError> {
        self.index
            .lock()
            .map_err(|err| VectorError::IndexUnavailable(format!("index mutex poisoned: {err}")))?
            .drop_embedding_model(&triple)
    }

    /// Drop embedding model and return the structured report (spec §16.4, B-API-4).
    ///
    /// Phase 5 surface: returns counts for each derived table affected so callers
    /// can confirm the drop matched their expectation. The legacy `usize` return
    /// from [`Self::drop_embedding_model`] only carried `vectors_removed`.
    pub async fn drop_embedding_model_report(&self, triple: EmbeddingTriple) -> Result<DropTripleReport, VectorError> {
        let mut index =
            self.index.lock().map_err(|err| VectorError::IndexUnavailable(format!("index mutex poisoned: {err}")))?;
        let meta_rows_removed = index.connection().query_row(
            "SELECT COUNT(*) FROM chunk_embedding_meta WHERE provider=?1 AND model_ref=?2 AND dimension=?3",
            (&triple.provider, &triple.model_ref, i64::from(triple.dimension)),
            |row| row.get::<_, i64>(0),
        )? as u64;
        let pending_jobs_dropped = index.connection().query_row(
            "SELECT COUNT(*) FROM pending_embedding_jobs WHERE provider=?1 AND model_ref=?2 AND dimension=?3",
            (&triple.provider, &triple.model_ref, i64::from(triple.dimension)),
            |row| row.get::<_, i64>(0),
        )? as u64;
        let vectors_removed = index.drop_embedding_model(&triple)? as u64;
        Ok(DropTripleReport {
            vectors_removed,
            meta_rows_removed,
            pending_jobs_dropped,
            table_dropped: vectors_removed > 0,
        })
    }

    /// Count vectors for a triple.
    pub async fn vector_count(&self, triple: EmbeddingTriple) -> Result<usize, VectorError> {
        self.index
            .lock()
            .map_err(|err| VectorError::IndexUnavailable(format!("index mutex poisoned: {err}")))?
            .vector_count(&triple)
    }

    /// Git preflight.
    pub async fn git_preflight(&self, merge_driver_binary: PathBuf) -> Result<(), crate::error::GitError> {
        git::git_preflight(&self.roots.repo, &merge_driver_binary)
    }

    /// Inspect fetch without merge.
    pub async fn fetch_inspect(&self) -> Result<String, crate::error::GitError> {
        git::fetch_inspect(&self.roots.repo)
    }

    /// Auto commit.
    ///
    /// Deferred: return `CommitOutcome` so callers know whether a commit was made.
    pub async fn auto_commit(&self) -> Result<(), crate::error::GitError> {
        git::auto_commit(&self.roots.repo, "Stream A auto-commit\n\nStream-A: true").map(|_| ())
    }

    /// Fetch and merge.
    pub async fn fetch_and_merge(&self) -> Result<(), crate::error::GitError> {
        git::fetch_and_merge(&self.roots.repo)
    }

    /// Push.
    pub async fn push(&self) -> Result<(), crate::error::GitError> {
        git::push(&self.roots.repo)
    }

    /// Resolved durability tier.
    pub fn durability_tier(&self) -> DurabilityTier {
        self.durability
    }

    /// Synchronous watch subscription setup.
    pub fn watch(&self) -> Result<WatchSubscription, crate::error::WatchError> {
        watch_root_with_suppression(&self.roots.repo, Some(Arc::clone(&self.suppression)))
    }

    /// Read event log.
    pub fn events(&self) -> std::io::Result<Vec<Event>> {
        read_events(&self.event_log)
    }

    /// Record that encrypted content was intentionally revealed without
    /// persisting the revealed plaintext.
    pub fn record_encrypted_content_revealed(&self, id: MemoryId, reason: String) -> std::io::Result<()> {
        self.record_event(EventKind::EncryptedContentRevealed { id, reason }, &new_operation_id())
    }

    async fn open_with_options(roots: Roots, force_unsafe_durability: bool) -> Result<Self, OpenError> {
        bootstrap_repo_layout(&roots.repo)?;
        std::fs::create_dir_all(&roots.runtime)?;
        let durability = probe_durability(&roots.repo, force_unsafe_durability);
        if matches!(durability, DurabilityTier::Refused) && !force_unsafe_durability {
            return Err(OpenError::DurabilityUnsupported { tier: durability });
        }
        let device_id = load_device_id(&roots.runtime)?;
        let event_log = roots.repo.join("events").join(format!("{device_id}.jsonl"));
        reconcile_startup_pre_index(&roots.runtime, &event_log, &roots.repo)
            .map_err(|err| OpenError::OperatorRepairRequired(err.to_string()))?;
        let device = DeviceId::try_new(&device_id)
            .map_err(|err| OpenError::InvalidRoots(format!("invalid device id in local-device.yaml: {err}")))?;
        sync_event_sequence_state(&roots.runtime, &event_log, &device)
            .map_err(|err| OpenError::OperatorRepairRequired(err.to_string()))?;
        // `load_active_embedding` returns Err when config.yaml is absent or has
        // no `active_embedding` field.  Spec §10.2.2 #5: no silent fallback.
        // Deferred: introduce typed `OpenError::ActiveEmbeddingTripleRequired` variant.
        let active_embedding = crate::config::load_active_embedding(&roots.repo)
            .map_err(|err| OpenError::InvalidRoots(err.to_string()))?;
        let connection =
            open_index(&roots.runtime.join("index.sqlite")).map_err(|err| OpenError::InvalidRoots(err.to_string()))?;
        let mut index = Index::with_active_embedding(connection, active_embedding);
        replay_pending_repairs(&roots.repo, &roots.runtime, &event_log, &device, &mut index)
            .map_err(|err| OpenError::OperatorRepairRequired(err.to_string()))?;
        full_reindex_from_repo(&roots.repo, &mut index)
            .map_err(|err| OpenError::OperatorRepairRequired(err.to_string()))?;
        Ok(Self {
            roots,
            device_id,
            durability,
            index: Arc::new(Mutex::new(index)),
            best_effort_event_seq: Arc::new(AtomicU64::new(best_effort_event_seq_start(&event_log, &device))),
            event_log,
            suppression: Arc::new(Mutex::new(SuppressionLedger::default())),
        })
    }

    fn validate_memory_path(&self, memory: &Memory, outcome: WriteOutcome) -> Result<(), WriteFailure> {
        let path = memory
            .path
            .clone()
            .unwrap_or_else(|| RepoPath::new(format!("agent/patterns/{}.md", memory.frontmatter.id.as_str())));
        if !path.is_safe_relative() {
            return Err(WriteFailure {
                outcome,
                kind: WriteFailureKind::Validation(format!("invalid repo path: {}", path.as_str())),
            });
        }
        if path.as_str().starts_with("encrypted/") {
            return Err(WriteFailure {
                outcome,
                kind: WriteFailureKind::Validation(format!(
                    "plaintext writes cannot target encrypted namespace: {}",
                    path.as_str()
                )),
            });
        }
        ensure_write_parent_contained(&self.roots.repo, &path)
            .map_err(|err| WriteFailure { outcome, kind: WriteFailureKind::Validation(err) })
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
                let _ = self.record_event(event, operation_id);
                Err(failure)
            }
        }
    }

    fn build_recorded_event(&self, kind: EventKind, operation_id: &OperationId) -> std::io::Result<Event> {
        let device = DeviceId::try_new(&self.device_id)
            .map_err(|err| std::io::Error::other(format!("invalid device_id in Substrate: {err}")))?;
        let seq = reserve_event_sequence(&self.roots.runtime, &self.event_log, &device)?;
        Ok(Event {
            schema: crate::SUBSTRATE_SCHEMA_VERSION,
            id: EventId::new(format!("evt_{}", uuid::Uuid::new_v4())),
            at: Utc::now(),
            device,
            seq,
            operation_id: Some(operation_id.clone()),
            kind,
            crc32c: 0,
        })
    }

    fn record_event(&self, kind: EventKind, operation_id: &OperationId) -> std::io::Result<()> {
        if matches!(self.durability, DurabilityTier::BestEffort) {
            let device = DeviceId::try_new(&self.device_id).map_err(std::io::Error::other)?;
            let event = Event {
                schema: crate::SUBSTRATE_SCHEMA_VERSION,
                id: EventId::new(format!("evt_{}", uuid::Uuid::new_v4())),
                at: Utc::now(),
                device,
                seq: self.best_effort_event_seq.fetch_add(1, Ordering::Relaxed),
                operation_id: Some(operation_id.clone()),
                kind,
                crc32c: 0,
            };
            return append_event_best_effort(&self.event_log, &event);
        }
        let event = self.build_recorded_event(kind, operation_id)?;
        append_event(&self.event_log, &event)
    }
}

fn best_effort_event_seq_start(event_log: &std::path::Path, device: &DeviceId) -> u64 {
    read_events(event_log)
        .map(|events| {
            events
                .into_iter()
                .filter(|event| &event.device == device)
                .map(|event| event.seq)
                .max()
                .map_or(1, |seq| seq.saturating_add(1))
        })
        .unwrap_or(1)
}

fn committed_lifecycle_failure(failure: WriteFailure, committed_outcome: &WriteOutcome) -> WriteFailure {
    if failure.outcome.committed {
        failure
    } else {
        let mut outcome = committed_outcome.clone();
        outcome.repair_required.get_or_insert(RepairRequired::FullStartupScan);
        WriteFailure { outcome, kind: failure.kind }
    }
}

fn lifecycle_updated_at(frontmatter: &Frontmatter) -> chrono::DateTime<Utc> {
    Utc::now().max(frontmatter.created_at)
}

fn validate_substrate_fragment_append(request: &SubstrateFragmentAppendRequest) -> Result<(), String> {
    if request.scope.trim().is_empty() {
        return Err("substrate fragment scope is required".to_string());
    }
    if request.entities.len() > 32 {
        return Err("substrate fragment entities exceeds 32 entries".to_string());
    }
    for entity in &request.entities {
        if entity.len() > 128 {
            return Err(format!("substrate fragment entity exceeds 128 bytes: {entity}"));
        }
    }
    match (&request.payload, request.classification) {
        (SubstrateFragmentPayload::Plaintext { text }, ClassificationOutcome::Trusted) if text.trim().is_empty() => {
            Err("plaintext substrate fragment text is required".to_string())
        }
        (SubstrateFragmentPayload::Plaintext { .. }, ClassificationOutcome::Trusted) => Ok(()),
        (SubstrateFragmentPayload::Encrypted { encryption, descriptor }, ClassificationOutcome::RequiresEncryption) => {
            if encryption.recipient.trim().is_empty() || encryption.ciphertext_b64.trim().is_empty() {
                return Err("encrypted substrate fragment requires recipient and ciphertext_b64".to_string());
            }
            if descriptor.summary_safe.trim().is_empty() {
                return Err("encrypted substrate fragment requires descriptor.summary_safe".to_string());
            }
            Ok(())
        }
        (_, ClassificationOutcome::Secret) => Err("secret substrate fragments are refused".to_string()),
        (SubstrateFragmentPayload::Plaintext { .. }, ClassificationOutcome::RequiresEncryption) => {
            Err("requires_encryption classification must use encrypted substrate payload".to_string())
        }
        (SubstrateFragmentPayload::Encrypted { .. }, ClassificationOutcome::Trusted) => {
            Err("trusted classification must use plaintext substrate payload".to_string())
        }
    }
}

struct JsonlWriteTarget<'a> {
    repo: &'a std::path::Path,
    path: &'a RepoPath,
    operation_id: &'a OperationId,
    durability: DurabilityTier,
}

impl<'a> JsonlWriteTarget<'a> {
    fn new(
        repo: &'a std::path::Path,
        path: &'a RepoPath,
        operation_id: &'a OperationId,
        durability: DurabilityTier,
    ) -> Self {
        Self { repo, path, operation_id, durability }
    }
}

fn substrate_fragment_path(request: &SubstrateFragmentAppendRequest, device_id: &str) -> Result<RepoPath, String> {
    let prefix = match &request.payload {
        SubstrateFragmentPayload::Plaintext { .. } => "substrate",
        SubstrateFragmentPayload::Encrypted { .. } => "encrypted/substrate",
    };
    RepoPath::try_new(format!("{prefix}/{}/{}.jsonl", device_id, request.at.format("%Y-%m-%d")))
}

fn append_jsonl_record<T: Serialize>(target: JsonlWriteTarget<'_>, record: &T) -> std::io::Result<()> {
    ensure_write_parent_contained(target.repo, target.path).map_err(std::io::Error::other)?;
    let final_path = target.repo.join(target.path.as_path());
    let parent = final_path.parent().ok_or_else(|| std::io::Error::other("missing parent"))?;
    std::fs::create_dir_all(parent)?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&final_path)?;
    serde_json::to_writer(&mut file, record).map_err(std::io::Error::other)?;
    file.write_all(b"\n")?;
    if matches!(target.durability, DurabilityTier::Full) {
        file.sync_all()?;
    }
    if matches!(target.durability, DurabilityTier::Full) {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn write_jsonl_records<T: Serialize>(target: JsonlWriteTarget<'_>, records: &[T]) -> std::io::Result<()> {
    ensure_write_parent_contained(target.repo, target.path).map_err(std::io::Error::other)?;
    let final_path = target.repo.join(target.path.as_path());
    let parent = final_path.parent().ok_or_else(|| std::io::Error::other("missing parent"))?;
    std::fs::create_dir_all(parent)?;
    let file_name = final_path.file_name().and_then(|name| name.to_str()).unwrap_or("substrate.jsonl");
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", target.operation_id.as_str()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp_path)?;
    for record in records {
        serde_json::to_writer(&mut file, record).map_err(std::io::Error::other)?;
        file.write_all(b"\n")?;
    }
    if matches!(target.durability, DurabilityTier::Full) {
        file.sync_all()?;
    }
    std::fs::rename(&temp_path, &final_path)?;
    if matches!(target.durability, DurabilityTier::Full) {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn read_substrate_records(path: &std::path::Path) -> std::io::Result<Vec<SubstrateFragmentRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path)?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(std::io::Error::other))
        .collect()
}

fn enforce_no_dream_prose_sources(memory: &Memory, outcome: WriteOutcome) -> Result<(), WriteFailure> {
    let source_ref = memory.frontmatter.source.reference.as_deref();
    let evidence_refs = memory.frontmatter.evidence.iter().map(|evidence| evidence.reference.as_str());

    if source_ref.into_iter().chain(evidence_refs).any(is_dream_prose_ref) {
        Err(WriteFailure { outcome, kind: WriteFailureKind::DreamProseAsSource })
    } else {
        Ok(())
    }
}

fn is_dream_prose_ref(reference: &str) -> bool {
    let without_file_prefix = reference.strip_prefix("file:").unwrap_or(reference);
    without_file_prefix
        .split_once('#')
        .map_or(without_file_prefix, |(path, _fragment)| path)
        .split('/')
        .collect::<Vec<_>>()
        .windows(3)
        .any(|window| window[0] == "dreams" && matches!(window[1], "journal" | "questions"))
}

fn absolute_to_repo_path(repo: &std::path::Path, absolute: &std::path::Path) -> Result<RepoPath, String> {
    let relative = absolute.strip_prefix(repo).map_err(|err| err.to_string())?;
    RepoPath::try_new(relative.to_string_lossy().replace('\\', "/"))
}

fn new_substrate_fragment_id() -> String {
    format!("sub_{}", ulid::Ulid::new())
}

struct BinaryWrite<'a> {
    repo: &'a std::path::Path,
    path: &'a RepoPath,
    bytes: &'a [u8],
    operation_id: &'a OperationId,
    durability: DurabilityTier,
    suppression: Option<&'a Arc<Mutex<SuppressionLedger>>>,
}

fn atomic_write_bytes(args: BinaryWrite<'_>) -> std::io::Result<()> {
    let final_path = args.repo.join(args.path.as_path());
    ensure_write_parent_contained(args.repo, args.path).map_err(std::io::Error::other)?;
    if final_path.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "encrypted target already exists"));
    }
    let parent = final_path.parent().ok_or_else(|| std::io::Error::other("missing parent"))?;
    std::fs::create_dir_all(parent)?;
    let file_name = final_path.file_name().and_then(|name| name.to_str()).unwrap_or("encrypted.bin");
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", args.operation_id.as_str()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp_path)?;
    use std::io::Write;
    file.write_all(args.bytes)?;
    file.sync_all()?;
    let final_hash = crate::markdown::hash_bytes(args.bytes);
    if let Some(suppression) = args.suppression {
        if let Ok(mut ledger) = suppression.lock() {
            ledger.insert_in_flight(args.path.clone(), args.operation_id.clone(), final_hash.clone());
        }
    }
    let write_result = (|| {
        std::fs::hard_link(&temp_path, &final_path)?;
        std::fs::remove_file(&temp_path)?;
        if matches!(args.durability, DurabilityTier::Full) {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    match write_result {
        Ok(()) => {
            if let Some(suppression) = args.suppression {
                if let Ok(mut ledger) = suppression.lock() {
                    ledger.promote_committed(args.path.clone(), final_hash);
                }
            }
            Ok(())
        }
        Err(err) => {
            let _ = std::fs::remove_file(&temp_path);
            if let Some(suppression) = args.suppression {
                if let Ok(mut ledger) = suppression.lock() {
                    ledger.remove(args.path);
                }
            }
            Err(err)
        }
    }
}

fn encrypted_ciphertext_path(memory: &Memory) -> Result<RepoPath, String> {
    let original = memory
        .path
        .clone()
        .unwrap_or_else(|| RepoPath::new(format!("agent/patterns/{}.md", memory.frontmatter.id.as_str())));
    if !original.is_safe_relative() {
        return Err(format!("invalid repo path: {}", original.as_str()));
    }
    let memory_prefix = ["me/", "projects/", "agent/", "dreams/"];
    if !memory_prefix.iter().any(|prefix| original.as_str().starts_with(prefix))
        || original.as_str().starts_with("encrypted/")
        || !crate::watcher::is_memory_path(original.as_path())
    {
        return Err(format!("encrypted writes require an original memory markdown path: {}", original.as_str()));
    }
    // Spec §5.1 / §8.4: ciphertext is stored under `encrypted/<original-relative-path>`,
    // preserving the `.md` extension. The body inside the file is base64/armor; the
    // file itself is still a Markdown file from the tree allow-list's perspective.
    let encrypted = PathBuf::from("encrypted").join(original.as_path());
    RepoPath::try_new(encrypted.to_string_lossy().replace('\\', "/"))
}

/// Load the device id from `local-device.yaml`.
///
/// Per Q4, `git::adopt_clone` is the sole authority for minting
/// `local-device.yaml`. Returns `DeviceIdentityMissing` when absent.
fn load_device_id(runtime: &std::path::Path) -> Result<String, OpenError> {
    let local = crate::config::load_local_device_config(runtime).map_err(OpenError::InvalidRoots)?;
    match local {
        Some(cfg) => Ok(cfg.device.id),
        None => Err(OpenError::DeviceIdentityMissing { repair: crate::error::RepairAction::AdoptClone }),
    }
}

fn new_operation_id() -> OperationId {
    OperationId::new(format!("op_{}", uuid::Uuid::new_v4()))
}

/// Build a synthetic `Frontmatter` for ciphertext-only `MemoryEnvelope`s.
///
/// Used when `read_path_envelope` reads a pure-ciphertext file under
/// `encrypted/` that doesn't parse as Markdown. Stream D owns the real
/// metadata after decrypt; this lets callers pattern-match on
/// `MemoryContent::Ciphertext` without panicking. Deferred: replace with
/// `frontmatter_json` hydration from the index once B-IX-4 schema lands.
fn placeholder_frontmatter(id: &MemoryId) -> Frontmatter {
    use chrono::TimeZone;
    let epoch = chrono::Utc.timestamp_opt(0, 0).single().unwrap_or_else(Utc::now); // unwrap-justified: chrono epoch
    Frontmatter {
        schema_version: 1,
        id: id.clone(),
        memory_type: MemoryType::Pattern,
        scope: Scope::Agent,
        summary: String::new(),
        confidence: 1.0,
        trust_level: TrustLevel::Trusted,
        sensitivity: Sensitivity::Confidential,
        status: MemoryStatus::Active,
        created_at: epoch,
        updated_at: epoch,
        author: Author {
            kind: AuthorKind::System,
            user_handle: None,
            harness: None,
            harness_version: None,
            session_id: None,
            subagent_id: None,
            phase: None,
            component: Some("encrypted-placeholder".to_string()),
        },
        namespace: None,
        canonical_namespace_id: None,
        tags: Vec::new(),
        entities: Vec::new(),
        aliases: Vec::new(),
        source: Source {
            kind: SourceKind::System,
            reference: None,
            harness: None,
            harness_version: None,
            session_id: None,
            subagent_id: None,
            device: None,
        },
        evidence: Vec::new(),
        requires_user_confirmation: false,
        review_state: None,
        supersedes: Vec::new(),
        superseded_by: Vec::new(),
        related: Vec::new(),
        tombstone_events: Vec::new(),
        retrieval_policy: RetrievalPolicy {
            passive_recall: false,
            max_scope: Scope::Agent,
            mask_personal_for_synthesis: true,
            index_body: false,
            index_embeddings: false,
        },
        write_policy: WritePolicy {
            human_review_required: false,
            policy_applied: "encrypted-default".to_string(),
            expected_base_hash: None,
        },
        merge_diagnostics: None,
        extras: std::collections::BTreeMap::new(),
    }
}

fn full_reindex_from_repo(repo: &std::path::Path, index: &mut Index) -> std::io::Result<usize> {
    let repo_paths = collect_reindex_paths(repo).map_err(std::io::Error::other)?;
    index.clear_plaintext_memory_index().map_err(|err| std::io::Error::other(err.to_string()))?;
    let mut count = 0usize;
    for (_repo_path, memory, metadata_only) in repo_paths {
        index.upsert_memory(&memory, metadata_only).map_err(|err| std::io::Error::other(err.to_string()))?;
        count += 1;
    }
    index.reconcile_active_embedding_jobs().map_err(|err| std::io::Error::other(err.to_string()))?;
    Ok(count)
}

fn collect_reindex_paths(repo: &std::path::Path) -> Result<Vec<(RepoPath, Memory, bool)>, String> {
    let mut acc = Vec::new();
    for raw in crate::tree::relative_memory_paths(repo) {
        let rel = raw.to_string_lossy().replace('\\', "/");
        let path = RepoPath::new(rel.clone());
        if rel.starts_with("encrypted/") {
            match read_memory_file(repo, &path) {
                Ok((memory, _hash)) => {
                    if memory.frontmatter.extras.contains_key("encryption") {
                        let mut indexed_memory = memory;
                        let metadata_only = if let Some(safe_body) = indexed_memory
                            .frontmatter
                            .extras
                            .get("index_projection")
                            .and_then(|projection| projection.get("safe_body"))
                            .and_then(|value| value.as_str())
                        {
                            indexed_memory.body = safe_body.to_string();
                            indexed_memory.frontmatter.retrieval_policy.index_body = true;
                            false
                        } else {
                            true
                        };
                        acc.push((path, indexed_memory, metadata_only));
                    } else {
                        return Err(format!(
                            "plaintext markdown under encrypted namespace requires operator repair: {}",
                            path.as_str()
                        ));
                    }
                }
                Err(_) => continue, // legacy raw ciphertext: not a Markdown file; skip from plaintext reindex
            }
        } else {
            acc.push(
                read_memory_file(repo, &path)
                    .map(|(memory, _hash)| (path, memory, false))
                    .map_err(|err| err.to_string())?,
            );
        }
    }
    Ok(acc)
}

/// Write a minimal `config.yaml` if none exists yet.
///
/// Seeds the synthetic triple for development/test environments. Production
/// operators replace `config.yaml` with an operator-authored file.
/// Deferred: `InitOptions` should carry an explicit `active_embedding` field.
fn write_initial_config_if_absent(repo: &std::path::Path) -> Result<(), OpenError> {
    let config_path = repo.join("config.yaml");
    if config_path.exists() {
        return Ok(());
    }
    let content =
        "schema_version: 1\nactive_embedding:\n  provider: synthetic\n  model_ref: stream-a-test\n  dimension: 32\n";
    std::fs::write(&config_path, content)?;
    Ok(())
}

fn ensure_write_parent_contained(repo: &std::path::Path, path: &RepoPath) -> Result<(), String> {
    let canonical_repo = repo.canonicalize().map_err(|err| err.to_string())?;
    let mut current = repo.to_path_buf();
    let mut components = path.as_path().components().peekable();
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            break;
        }
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!("write parent contains symlink: {}", path.as_str()));
            }
            Ok(metadata) if metadata.is_dir() => {
                let canonical = current.canonicalize().map_err(|err| err.to_string())?;
                if !canonical.starts_with(&canonical_repo) {
                    return Err(format!("write parent resolves outside repository: {}", path.as_str()));
                }
            }
            Ok(_) => return Err(format!("write parent is not a directory: {}", path.as_str())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => break,
            Err(err) => return Err(err.to_string()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_lifecycle_failure_marks_stale_old_mutation_as_repair_required() {
        let operation_id = OperationId::new("op_supersede_replacement_committed");
        let replacement_outcome = WriteOutcome {
            committed: true,
            indexed: true,
            event_recorded: true,
            durability: DurabilityTier::BestEffort,
            repair_required: None,
            operation_id: operation_id.clone(),
        };
        let stale_old_mutation = WriteFailure {
            outcome: WriteOutcome::not_committed(operation_id.clone(), DurabilityTier::BestEffort),
            kind: WriteFailureKind::StaleBase,
        };

        let failure = committed_lifecycle_failure(stale_old_mutation, &replacement_outcome);

        assert_eq!(failure.kind, WriteFailureKind::StaleBase);
        assert!(failure.outcome.committed);
        assert_eq!(failure.outcome.repair_required, Some(RepairRequired::FullStartupScan));
        assert_eq!(failure.outcome.operation_id, operation_id);
    }
}
