//! Governance write-input parsing and the `GovernanceMeta` deserialization model.
//!
//! Owns the caller-supplied meta surface (`GovernanceMeta` and its sub-types),
//! the parsed `GovernanceWriteInput` plus its `Memory`-building logic, the
//! lifecycle descriptor, and the `MetaSource` discriminator. Cross-module
//! consumers (`pipeline`, `privacy`) reach the parsed input and meta through the
//! `pub(super)` surface re-exported by `governance::mod`.

use std::collections::BTreeMap;
use std::path::Path;

use memory_governance::{
    CandidateMemory, Scope as GovernanceScope, Source as GovernanceSource, SourceKind as GovernanceSourceKind,
};
use memory_privacy::{CallerSensitivity, PrivacyDecision, PrivacyNamespace, PrivacyStorageAction};
use memory_substrate::{
    Author, AuthorKind, Entity, Evidence, Frontmatter, Memory, MemoryId, MemoryStatus, MemoryType, RepoPath,
    RetrievalPolicy, Scope, Sensitivity, Source, SourceKind, TrustLevel, WritePolicy,
};
use serde::Deserialize;
use serde_json::Value;

use crate::handlers::memory_ops::validated_claim_lock_identity_field;
use crate::handlers::{
    bounded, compute_quote_norm_hash, insert_safe_descriptor, is_safe_plaintext_for_indexing, HandlerError,
    DEFAULT_SUPERSEDE_HARNESS, DEFAULT_SUPERSEDE_SESSION_ID,
};
use crate::protocol::{GovernanceRefusalReason, GovernanceStatus, GovernanceWriteResponse};
use crate::recall::project::resolve_project_binding;
use crate::recall::ConcurrentSessionMode;

#[derive(Clone, Debug)]
pub(super) struct GovernedLifecycle {
    pub(super) status: MemoryStatus,
    pub(super) trust_level: TrustLevel,
    pub(super) policy_applied: String,
}

impl GovernedLifecycle {
    pub(super) fn new(status: MemoryStatus, trust_level: TrustLevel, policy_applied: String) -> Self {
        Self { status, trust_level, policy_applied }
    }
}

#[derive(Clone, Debug)]
pub(super) struct GovernanceWriteInput {
    body: String,
    title: Option<String>,
    tags: Vec<String>,
    pub(super) meta: GovernanceMeta,
}

pub(super) struct GovernanceWriteInputParts {
    pub(super) body: String,
    pub(super) title: Option<String>,
    pub(super) tags: Vec<String>,
    pub(super) meta: Value,
    pub(super) source: MetaSource,
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
    pub(super) session_id: String,
    #[serde(default = "default_supersede_harness")]
    pub(super) harness: String,
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
    namespace_alias: Option<String>,
    cwd: Option<String>,
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

impl GovernanceNamespace {
    /// Wire/response label for this namespace (`me` / `project` / `agent`).
    fn response_label(self) -> &'static str {
        match self {
            Self::Me => "me",
            Self::Project => "project",
            Self::Agent => "agent",
        }
    }

    fn governance_scope(self) -> GovernanceScope {
        match self {
            Self::Me => GovernanceScope::Me,
            Self::Project => GovernanceScope::Project,
            Self::Agent => GovernanceScope::Agent,
        }
    }

    fn privacy_namespace(self) -> PrivacyNamespace {
        match self {
            Self::Me => PrivacyNamespace::Me,
            Self::Project => PrivacyNamespace::Project,
            Self::Agent => PrivacyNamespace::Agent,
        }
    }

    fn substrate_scope(self) -> Scope {
        match self {
            Self::Me => Scope::User,
            Self::Project => Scope::Project,
            Self::Agent => Scope::Agent,
        }
    }
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
            namespace_alias: None,
            cwd: None,
            requires_user_confirmation: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MetaSource {
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

pub(super) const PROJECT_NAMESPACE_IDENTITY_REQUIRED_MESSAGE: &str = "project-namespace write requires project identity: pass meta.canonical_namespace_id, or call from a project directory (git remote or .memory-project.yaml), or use namespace \"me\"/\"agent\"";

impl GovernanceWriteInput {
    pub(super) fn parse(parts: GovernanceWriteInputParts) -> Result<Self, HandlerError> {
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

    pub(super) async fn resolve_project_namespace(&mut self) -> Result<(), HandlerError> {
        if !matches!(self.meta.namespace, GovernanceNamespace::Project) {
            return Ok(());
        }
        if self.meta.canonical_namespace_id.is_some() {
            return Ok(());
        }

        let Some(cwd) = self.meta.cwd.as_deref() else {
            return Err(project_namespace_identity_error());
        };

        let binding = resolve_project_binding(Path::new(cwd)).await.map_err(|error| {
            HandlerError::invalid_request(format!(
                "{PROJECT_NAMESPACE_IDENTITY_REQUIRED_MESSAGE}; project resolution error: {error}"
            ))
        })?;
        let Some(binding) = binding else {
            return Err(project_namespace_identity_error());
        };

        self.meta.canonical_namespace_id = Some(binding.canonical_id);
        self.meta.namespace_alias = binding.alias;
        Ok(())
    }

    pub(super) fn privacy_scan_text(&self) -> String {
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

    pub(super) fn privacy_refusal(&self, privacy: &PrivacyDecision) -> Option<GovernanceWriteResponse> {
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
                similarity_degraded: None,
            }),
            PrivacyStorageAction::Plaintext | PrivacyStorageAction::EncryptAtRest => None,
        }
    }

    pub(super) fn candidate(&self, id: &str) -> CandidateMemory {
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
    pub(super) fn to_memory(
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
        let namespace = self.substrate_namespace()?;
        let canonical_namespace_id = self.meta.canonical_namespace_id.clone().or_else(|| namespace.clone());
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
                namespace,
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
            path: Some(self.repo_path(id.as_str())?),
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

    pub(super) fn response_namespace(&self) -> String {
        self.meta.namespace.response_label().to_string()
    }

    fn governance_scope(&self) -> GovernanceScope {
        self.meta.namespace.governance_scope()
    }

    pub(super) fn privacy_namespace(&self) -> PrivacyNamespace {
        self.meta.namespace.privacy_namespace()
    }

    pub(super) fn caller_sensitivity(&self) -> Option<CallerSensitivity> {
        self.meta.sensitivity.map(|sensitivity| match sensitivity {
            GovernanceSensitivity::Public => CallerSensitivity::Public,
            GovernanceSensitivity::Internal => CallerSensitivity::Internal,
            GovernanceSensitivity::Confidential => CallerSensitivity::Confidential,
            GovernanceSensitivity::Personal => CallerSensitivity::Personal,
            GovernanceSensitivity::Sensitive => CallerSensitivity::Sensitive,
            GovernanceSensitivity::Secret => CallerSensitivity::Secret,
        })
    }

    pub(super) fn is_same_body_bucket_repair(&self, old: &Frontmatter, old_body: &str) -> bool {
        if self.body != old_body {
            return false;
        }
        let Ok(namespace) = self.substrate_namespace() else {
            return false;
        };
        let canonical_namespace_id = self.meta.canonical_namespace_id.clone().or_else(|| namespace.clone());
        old.scope != self.substrate_scope()
            || old.namespace != namespace
            || old.canonical_namespace_id != canonical_namespace_id
    }

    fn substrate_scope(&self) -> Scope {
        self.meta.namespace.substrate_scope()
    }

    fn substrate_namespace(&self) -> Result<Option<String>, HandlerError> {
        if matches!(self.meta.namespace, GovernanceNamespace::Project) {
            return self.project_namespace_alias().map(Some);
        }
        Ok(None)
    }

    fn project_namespace_alias(&self) -> Result<String, HandlerError> {
        let raw = self
            .meta
            .namespace_alias
            .clone()
            .or_else(|| self.meta.canonical_namespace_id.clone())
            .ok_or_else(project_namespace_identity_error)?;
        Ok(sanitize_namespace_alias(&raw))
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

    fn repo_path(&self, id: &str) -> Result<RepoPath, HandlerError> {
        match self.meta.namespace {
            GovernanceNamespace::Me => Ok(RepoPath::new(format!("me/knowledge/{id}.md"))),
            GovernanceNamespace::Project => {
                let namespace = self.project_namespace_alias()?;
                Ok(RepoPath::new(format!("projects/{namespace}/decisions/{id}.md")))
            }
            GovernanceNamespace::Agent => Ok(RepoPath::new(format!("agent/patterns/{id}.md"))),
        }
    }
}

fn project_namespace_identity_error() -> HandlerError {
    HandlerError::invalid_request(PROJECT_NAMESPACE_IDENTITY_REQUIRED_MESSAGE)
}

/// Sanitize a namespace alias to the path-safe charset `[A-Za-z0-9._-]`.
///
/// Characters outside the charset are dropped. If the result is empty (the
/// alias was entirely path-hostile) a stable placeholder `"unnamed"` is
/// returned so the store always gets a valid directory name.
///
/// This is a **conservative no-op for clean aliases** — any alias already
/// composed entirely of `[A-Za-z0-9._-]` passes through byte-identical,
/// so no existing store is repathed by this sanitizer.
///
/// This sanitizer lives in the memoryd handler layer. Do not call it from
/// within `crates/memory-substrate/` — the substrate path validator has its
/// own contract.
fn sanitize_namespace_alias(alias: &str) -> String {
    let sanitized: String =
        alias.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')).collect();
    if sanitized.is_empty() {
        "unnamed".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use memory_substrate::{AuthorKind, MemoryId, MemoryStatus, SourceKind, TrustLevel};
    use serde_json::Value;

    use super::{
        parse_governance_meta, GovernanceMeta, GovernanceSourceKindMeta, GovernanceWriteInput,
        GovernanceWriteInputParts, GovernedLifecycle, MetaSource, PROJECT_NAMESPACE_IDENTITY_REQUIRED_MESSAGE,
    };
    use crate::handlers::compute_quote_norm_hash;
    use crate::handlers::governance::privacy::classify_input_privacy;
    use memory_privacy::PrivacyStorageAction;

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

        // Empty project-scoped meta no longer silently falls back to a development
        // placeholder namespace; callers must provide identity or cwd before persistence.
        let input = write_input(Value::Null);
        let err = input
            .to_memory(
                MemoryId::new("mem_20260527_a1b2c3d4e5f60718_000001"),
                promoted_lifecycle(),
                &plaintext_privacy_decision(),
            )
            .expect_err("empty project meta must refuse before persistence");
        assert!(err.message.contains(PROJECT_NAMESPACE_IDENTITY_REQUIRED_MESSAGE), "actionable error: {}", err.message);
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
            "namespace_alias": "policy",
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
        assert_eq!(memory.frontmatter.namespace.as_deref(), Some("policy"));
        assert_eq!(memory.frontmatter.canonical_namespace_id.as_deref(), Some("proj_0123456789abcdef"));
        assert_eq!(
            memory.path.as_ref().map(|path| path.as_str()),
            Some("projects/policy/decisions/mem_20260527_a1b2c3d4e5f60718_000042.md")
        );

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
            "canonical_namespace_id": "proj_import_source",
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
    fn same_body_bucket_repair_detects_changed_project_bucket() {
        let old = write_input(serde_json::json!({
            "namespace": "project",
            "namespace_alias": "legacy",
            "canonical_namespace_id": "proj_legacy",
            "source_kind": "import"
        }))
        .to_memory(
            MemoryId::new("mem_20260527_a1b2c3d4e5f60718_000008"),
            promoted_lifecycle(),
            &plaintext_privacy_decision(),
        )
        .expect("old memory converts");

        let repair = write_input(serde_json::json!({
            "namespace": "project",
            "namespace_alias": "policy",
            "canonical_namespace_id": "proj_policy-c6698817853503be",
            "source_kind": "import"
        }));

        assert!(
            repair.is_same_body_bucket_repair(&old.frontmatter, "Body text"),
            "same body with different project namespace/canonical id is an allowed bucket repair"
        );
        assert!(
            !repair.is_same_body_bucket_repair(&old.frontmatter, "Different body"),
            "changed content must still use normal supersession governance"
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

    // ── sanitize_namespace_alias chokepoint tests ──────────────────────────

    #[test]
    fn sanitize_namespace_alias_is_noop_for_clean_aliases() {
        // All legit aliases used in production must pass through byte-identical
        // so no existing store is repathed.
        for alias in &["b4a-plan-site", "atlasos", "policy", "agent-memory", "proj_abc123-deadbeef"] {
            assert_eq!(super::sanitize_namespace_alias(alias), *alias, "clean alias {alias:?} must be unchanged",);
        }
    }

    #[test]
    fn sanitize_namespace_alias_strips_hostile_characters() {
        // A Codex prose-leak alias like "`cmux` on PATH)" must become path-safe.
        let hostile = "`cmux` on PATH)";
        let result = super::sanitize_namespace_alias(hostile);
        assert!(
            result.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')),
            "sanitized alias must only contain path-safe characters: {result:?}",
        );
        assert!(!result.is_empty(), "sanitized alias must not be empty");
    }

    #[test]
    fn sanitize_namespace_alias_falls_back_to_unnamed_when_all_chars_stripped() {
        // If every character is hostile the fallback placeholder is returned.
        assert_eq!(super::sanitize_namespace_alias("``()"), "unnamed");
        assert_eq!(super::sanitize_namespace_alias(""), "unnamed");
    }
}
