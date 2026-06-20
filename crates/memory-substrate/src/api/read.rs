//! Read paths: legacy `Memory` reads, `MemoryEnvelope` reads, ciphertext
//! envelopes, and id→path resolution.

use super::*;

impl Substrate {
    /// Read a memory by id (legacy `Memory` shape; prefer
    /// [`Self::read_memory_envelope`] for the spec §16.2 shape).
    ///
    /// Resolution is index-first (B-API-7): the id is resolved to its path via a
    /// PK lookup on `memories.id`, and only that single file is read. The full
    /// tree-walk is retained solely as the empty-index fallback so a fresh open
    /// before any index hydration still finds plaintext memories on disk.
    pub async fn read_memory(&self, id: &MemoryId) -> Result<Memory, ReadError> {
        self.read_memory_with_hash(id).await.map(|(memory, _hash)| memory)
    }

    pub(super) async fn read_memory_with_hash(&self, id: &MemoryId) -> Result<(Memory, Sha256), ReadError> {
        // Index-first: resolve the id to a single path via PK lookup and read
        // only that file. Encrypted records are not returned by the legacy
        // `Memory` shape, so skip an index hit that resolves under `encrypted/`
        // and fall through to the disk-walk's plaintext-only semantics.
        //
        // A stale index must not change the answer: if the resolved path is gone
        // (`NotFound`) or holds a *different* id than requested (the index row is
        // stale relative to disk), fall through to the disk-walk so the id is
        // still found at its current path and a truly absent id yields
        // `NotFound` — identical to the pre-index-first disk-walk semantics. Only
        // a genuine read error (not a stale-index signal) propagates.
        if let Some(path) = self.resolve_memory_id_to_path_opt(id) {
            if !path.as_str().starts_with("encrypted/") {
                match read_memory_file(&self.roots.repo, &path) {
                    Ok((memory, hash)) if &memory.frontmatter.id == id => return Ok((memory, hash)),
                    Ok(_) => {} // stale index: path holds a different id — fall through to the disk-walk
                    Err(ReadError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {} // stale index: file gone
                    Err(other) => return Err(other),
                }
            }
        }
        // Disk-walk fallback for the empty/unindexed case. Preserves the legacy
        // plaintext-only "found-on-disk" semantics: encrypted paths are skipped
        // and a missing id yields `NotFound`.
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

    /// Synchronous, blocking-pool variant of [`Self::read_memory_envelope`].
    ///
    /// Resolves the id to a path and reads the canonical envelope entirely
    /// synchronously (`std::fs` + Markdown/frontmatter parse, plus the index-lock
    /// resolution), so callers that read many memories can run it on the blocking
    /// pool via `tokio::task::spawn_blocking` to keep the disk work off the async
    /// worker threads — see `attach_search_bodies` and the governance
    /// active-memory candidate path. Produces an identical `MemoryEnvelope` to the
    /// async method above.
    pub fn read_memory_envelope_blocking(&self, id: &MemoryId) -> Result<MemoryEnvelope, ReadError> {
        let path = self.resolve_memory_id_to_path(id)?;
        self.read_path_envelope_blocking(&path)
    }

    /// Read by repository path; returns the spec §16.2 `MemoryEnvelope` (B-API-1).
    ///
    /// Blocking note: despite the `async` signature this performs a synchronous
    /// `std::fs` read + Markdown/frontmatter parse inline (no `spawn_blocking`),
    /// so it occupies the calling worker thread for the duration. Callers that
    /// read many memories should either fan these out concurrently
    /// (`JoinSet`/`join_all`) or, to keep the disk work off the async worker
    /// pool entirely, call [`Self::read_path_envelope_blocking`] from
    /// `spawn_blocking` — see `attach_search_bodies` and the governance
    /// active-memory candidate path.
    pub async fn read_path_envelope(&self, path: &RepoPath) -> Result<MemoryEnvelope, ReadError> {
        self.read_path_envelope_blocking(path)
    }

    /// Synchronous core of [`Self::read_path_envelope`].
    ///
    /// The read is entirely synchronous (`std::fs` + Markdown/frontmatter parse,
    /// no `.await`), so this exposes it as a plain `fn` for callers that want to
    /// run it on a blocking thread (`tokio::task::spawn_blocking`) without
    /// re-entering the async runtime. The async method above is a thin wrapper
    /// over this, so both paths produce an identical `MemoryEnvelope`.
    pub fn read_path_envelope_blocking(&self, path: &RepoPath) -> Result<MemoryEnvelope, ReadError> {
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

    /// Resolve an id to its path via a single PK lookup on `memories.id`.
    /// Returns `None` when the index is empty/unavailable or the id is absent,
    /// so callers with their own disk-walk fallback can branch on it.
    fn resolve_memory_id_to_path_opt(&self, id: &MemoryId) -> Option<RepoPath> {
        let query = MemoryQuery { id: Some(id.clone()), include_metadata_only: true, ..MemoryQuery::default() };
        // A failed lookup is index *unavailability*, not "id absent": it degrades
        // the read to the O(n) disk-walk fallback. That is correct but a silent
        // perf cliff, so log it. A successful lookup that returns no row is the
        // normal "not indexed yet" case and stays quiet. (`lock_index` recovers a
        // poisoned mutex, so poison no longer forces the degraded path.)
        let guard = lock_index(&self.index);
        match guard.query_memory(&query) {
            Ok(rows) => rows.into_iter().next().map(|hit| hit.path),
            Err(err) => {
                tracing::warn!(memory_id = id.as_str(), error = %err, "index lookup failed; degrading read to disk-walk");
                None
            }
        }
    }

    fn resolve_memory_id_to_path(&self, id: &MemoryId) -> Result<RepoPath, ReadError> {
        // Prefer index lookup; fall back to disk walk if the index is empty
        // (e.g. fresh open before any read paths through it).
        if let Some(path) = self.resolve_memory_id_to_path_opt(id) {
            return Ok(path);
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
        original_confidence: None,
        trust_level: TrustLevel::Trusted,
        sensitivity: Sensitivity::Confidential,
        status: MemoryStatus::Active,
        created_at: epoch,
        updated_at: epoch,
        observed_at: None,
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
