#![allow(unknown_lints, file_too_long)]
//! Public DTO contract is intentionally centralized for Task 2 seam stability.
//! Public data model for Stream A.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::ValidationError;

/// Memory ID format. Mirrors spec §7.1.
#[allow(clippy::expect_used)]
static MEMORY_ID_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^mem_\d{8}_[0-9a-f]{16}_\d{6}$").expect("memory-id regex literal")
    // expect-justified: compile-time regex literal cannot fail
});

/// Device ID format. Mirrors spec §7.4: `dev_<lowercase hex/alnum>`.
#[allow(clippy::expect_used)]
static DEVICE_ID_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^dev_[a-z0-9]+$").expect("device-id regex literal") // expect-justified: compile-time regex literal cannot fail
});

/// Local and synced roots used by Stream A.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Roots {
    /// Synced git repository root.
    pub repo: PathBuf,
    /// Local per-device runtime root.
    pub runtime: PathBuf,
}

impl Roots {
    /// Build roots from explicit paths.
    pub fn new(repo: impl Into<PathBuf>, runtime: impl Into<PathBuf>) -> Self {
        Self { repo: repo.into(), runtime: runtime.into() }
    }
}

/// Initialization options.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct InitOptions {
    /// Permit unsafe durability only in tests/CI.
    pub force_unsafe_durability: bool,
    /// Optional stable device identifier.
    pub device_id: Option<String>,
}

/// Clone adoption options.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdoptOptions {
    /// Force regeneration of local device identity.
    pub force_new_device: bool,
}

/// Stream A doctor report.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DoctorReport {
    /// Resolved durability tier.
    pub durability_tier: DurabilityTier,
    /// Validation warnings.
    pub warnings: Vec<String>,
    /// Operator-required repair items.
    pub repairs_required: Vec<String>,
}

/// Caller-supplied classification result from Stream D.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationOutcome {
    /// Caller asserts plaintext is safe for public/internal sensitivity.
    Trusted,
    /// Caller asserts content must be encrypted before disk.
    RequiresEncryption,
    /// Caller asserts content is secret and must not be persisted by Stream A.
    Secret,
}

/// Frontmatter sensitivity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    /// Public memory.
    Public,
    /// Internal memory.
    Internal,
    /// Confidential memory.
    Confidential,
    /// Personal memory.
    Personal,
}

/// Memory status.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryStatus {
    /// Candidate memory.
    Candidate,
    /// Active memory.
    Active,
    /// Pinned memory.
    Pinned,
    /// Superseded memory.
    Superseded,
    /// Archived memory.
    Archived,
    /// Tombstoned memory.
    Tombstoned,
    /// Quarantined memory.
    Quarantined,
}

/// Trust level.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// Trusted.
    Trusted,
    /// Untrusted.
    Untrusted,
    /// Candidate.
    Candidate,
    /// Quarantined.
    Quarantined,
    /// Pinned.
    Pinned,
}

/// Memory type.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryType {
    /// Project memory.
    Project,
    /// Person memory.
    Person,
    /// Procedure memory.
    Procedure,
    /// Episode memory.
    Episode,
    /// Claim memory.
    Claim,
    /// Artifact memory.
    Artifact,
    /// Prospective memory.
    Prospective,
    /// Pattern memory.
    Pattern,
    /// Playbook memory.
    Playbook,
    /// Postmortem memory.
    Postmortem,
    /// Anti-pattern memory.
    #[serde(rename = "anti-pattern")]
    AntiPattern,
    /// Heuristic memory.
    Heuristic,
    /// Regression memory.
    Regression,
    /// Correction memory.
    Correction,
    /// Invariant memory.
    Invariant,
    /// Decision memory.
    Decision,
    /// Open question memory.
    #[serde(rename = "open-question")]
    OpenQuestion,
}

/// Memory scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// User scope.
    User,
    /// Project scope.
    Project,
    /// Organization scope.
    Org,
    /// Agent scope.
    Agent,
    /// Subagent scope.
    Subagent,
}

/// Structured author principal.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Author {
    /// Principal kind.
    pub kind: AuthorKind,
    /// Optional user handle.
    pub user_handle: Option<String>,
    /// Optional harness.
    pub harness: Option<String>,
    /// Optional harness version.
    pub harness_version: Option<String>,
    /// Optional session id.
    pub session_id: Option<String>,
    /// Optional subagent id.
    pub subagent_id: Option<String>,
    /// Optional dreaming phase.
    pub phase: Option<String>,
    /// Optional system component.
    pub component: Option<String>,
}

/// Author kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorKind {
    /// User principal.
    User,
    /// Agent principal.
    Agent,
    /// Subagent principal.
    Subagent,
    /// Dreaming principal.
    Dreaming,
    /// System principal.
    System,
}

/// Source metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Source {
    /// Source kind.
    pub kind: SourceKind,
    /// Optional reference.
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Optional harness.
    pub harness: Option<String>,
    /// Optional harness version.
    pub harness_version: Option<String>,
    /// Optional session id.
    pub session_id: Option<String>,
    /// Optional subagent id.
    pub subagent_id: Option<String>,
    /// Optional device id.
    pub device: Option<String>,
}

/// Source kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// User source.
    User,
    /// Primary agent source.
    AgentPrimary,
    /// Subagent source.
    AgentSubagent,
    /// Tool source.
    Tool,
    /// Web source.
    Web,
    /// Email source.
    Email,
    /// File source.
    File,
    /// Synthesis source.
    Synthesis,
    /// Import source.
    Import,
    /// System source.
    System,
}

/// Retrieval policy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RetrievalPolicy {
    /// Whether passive recall may use this memory.
    pub passive_recall: bool,
    /// Maximum recall scope.
    pub max_scope: Scope,
    /// Mask personal data for synthesis.
    pub mask_personal_for_synthesis: bool,
    /// Whether body FTS indexing is enabled.
    pub index_body: bool,
    /// Whether vector indexing is enabled.
    pub index_embeddings: bool,
}

/// Write policy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WritePolicy {
    /// Whether human review is required.
    pub human_review_required: bool,
    /// Policy id applied.
    pub policy_applied: String,
    /// Expected base hash.
    pub expected_base_hash: Option<Sha256>,
}

/// Structured entity entry (spec §6.2 / §6.5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    /// Stable entity id.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Optional alias surface forms.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

/// Structured evidence entry (spec §6.5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    /// Stable evidence id (`ev_<ulid>`).
    pub id: String,
    /// Quoted support text.
    pub quote: String,
    /// Normalized hash of the quote (`sha256:<hex>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_norm_hash: Option<String>,
    /// Reference to source artifact / file:line.
    #[serde(rename = "ref")]
    pub reference: String,
    /// Weight in [0.0, 1.0].
    #[serde(default = "default_evidence_weight")]
    pub weight: f64,
    /// Observation timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    /// Optional source string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

fn default_evidence_weight() -> f64 {
    1.0
}

/// Reason class for a tombstone event (spec §6.5).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TombstoneKind {
    /// Duplicate of another memory.
    Duplicate,
    /// Wrong information.
    Wrong,
    /// Out-of-date.
    Stale,
    /// Privacy concern.
    Privacy,
    /// User requested removal.
    UserRequest,
    /// Policy-driven.
    Policy,
    /// Other reason.
    Other,
}

/// Tombstone actor (spec §6.5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TombstoneActor {
    /// Actor kind.
    pub kind: TombstoneActorKind,
    /// Actor reference (user handle, agent slug, system component).
    #[serde(rename = "ref")]
    pub reference: String,
}

/// Tombstone actor kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TombstoneActorKind {
    /// User actor.
    User,
    /// Agent actor.
    Agent,
    /// System actor.
    System,
}

/// Tombstone event (spec §6.5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TombstoneEvent {
    /// Event id (`tomb_<ulid>`).
    pub id: String,
    /// Applied-at timestamp.
    pub applied_at: DateTime<Utc>,
    /// Actor that applied the tombstone.
    pub actor: TombstoneActor,
    /// Reason class.
    pub reason: TombstoneKind,
    /// Optional free-text reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_text: Option<String>,
    /// Hash of the reason text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_hash: Option<String>,
    /// Lifecycle status prior to tombstoning.
    pub prior_status: MemoryStatus,
}

/// Typed memory frontmatter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frontmatter {
    /// Schema version.
    pub schema_version: u32,
    /// Memory id.
    pub id: MemoryId,
    /// Memory type.
    #[serde(rename = "type")]
    pub memory_type: MemoryType,
    /// Scope.
    pub scope: Scope,
    /// Summary.
    pub summary: String,
    /// Confidence score.
    pub confidence: f64,
    /// Trust level.
    pub trust_level: TrustLevel,
    /// Sensitivity.
    pub sensitivity: Sensitivity,
    /// Lifecycle status.
    pub status: MemoryStatus,
    /// Created timestamp.
    pub created_at: DateTime<Utc>,
    /// Updated timestamp.
    pub updated_at: DateTime<Utc>,
    /// Author.
    pub author: Author,
    /// Optional namespace.
    pub namespace: Option<String>,
    /// Optional canonical namespace id.
    pub canonical_namespace_id: Option<String>,
    /// Tags.
    pub tags: Vec<String>,
    /// Entities, structured per spec §6.2/§6.5.
    pub entities: Vec<Entity>,
    /// Aliases.
    pub aliases: Vec<String>,
    /// Source metadata.
    pub source: Source,
    /// Evidence entries, structured per spec §6.5.
    pub evidence: Vec<Evidence>,
    /// Requires user confirmation.
    pub requires_user_confirmation: bool,
    /// Review state.
    pub review_state: Option<String>,
    /// Supersedes ids.
    pub supersedes: Vec<MemoryId>,
    /// Superseded-by ids.
    pub superseded_by: Vec<MemoryId>,
    /// Related ids.
    pub related: Vec<MemoryId>,
    /// Tombstone events, structured per spec §6.5.
    pub tombstone_events: Vec<TombstoneEvent>,
    /// Retrieval policy.
    pub retrieval_policy: RetrievalPolicy,
    /// Write policy.
    pub write_policy: WritePolicy,
    /// Merge diagnostics.
    #[serde(rename = "_merge_diagnostics")]
    pub merge_diagnostics: Option<serde_json::Value>,
    /// Unknown v1 fields preserved for round-trip per spec §6.2. `BTreeMap`
    /// keeps re-emission order deterministic.
    #[serde(flatten)]
    pub extras: BTreeMap<String, serde_json::Value>,
}

/// Canonical memory document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    /// Frontmatter.
    pub frontmatter: Frontmatter,
    /// LF-normalized body.
    pub body: String,
    /// Optional repository path.
    pub path: Option<RepoPath>,
}

/// Write mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum WriteMode {
    /// Create a new file.
    CreateNew,
    /// Replace an existing file with CAS.
    ReplaceExisting,
    /// Explicit admin repair.
    AdminRepair,
}

/// Event context supplied by caller.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventContext {
    /// Optional actor.
    pub actor: Option<String>,
    /// Optional reason.
    pub reason: Option<String>,
}

/// Safe index projection for encrypted records.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct IndexProjection {
    /// Optional safe body text.
    pub safe_body: Option<String>,
}

/// Plaintext write request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WriteRequest {
    /// Optional operation id.
    pub operation_id: Option<OperationId>,
    /// Memory to write.
    pub memory: Memory,
    /// Expected base hash.
    pub expected_base_hash: Option<Sha256>,
    /// Write mode.
    pub write_mode: WriteMode,
    /// Optional index projection.
    pub index_projection: Option<IndexProjection>,
    /// Event context.
    pub event_context: EventContext,
    /// Allow best-effort durability.
    pub allow_best_effort_durability: bool,
    /// Mandatory caller classification.
    pub classification: ClassificationOutcome,
}

/// Supersession lifecycle request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SupersedeRequest {
    /// Existing memory that will become superseded.
    pub old_id: MemoryId,
    /// Replacement memory to write before mutating the old memory.
    pub replacement: Memory,
    /// Operator/governance reason for the lifecycle transition.
    pub reason: String,
    /// Mandatory caller classification for both plaintext writes.
    pub classification: ClassificationOutcome,
    /// Allow best-effort durability for both writes.
    pub allow_best_effort_durability: bool,
}

/// Supersession lifecycle outcome.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SupersedeOutcome {
    /// Existing memory that was superseded.
    pub old_id: MemoryId,
    /// Replacement memory that supersedes `old_id`.
    pub new_id: MemoryId,
    /// Outcome for the old-memory status mutation.
    pub old_outcome: WriteOutcome,
    /// Outcome for the replacement write.
    pub new_outcome: WriteOutcome,
}

/// Encrypted write request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EncryptedWriteRequest {
    /// Optional operation id.
    pub operation_id: Option<OperationId>,
    /// Metadata-only memory.
    pub metadata_memory: Memory,
    /// Ciphertext bytes.
    pub ciphertext: Vec<u8>,
    /// Optional safe index projection.
    pub safe_index_projection: Option<IndexProjection>,
    /// Event context.
    pub event_context: EventContext,
    /// Allow best-effort durability.
    pub allow_best_effort_durability: bool,
    /// Mandatory caller classification.
    pub classification: ClassificationOutcome,
}

/// Tombstone request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TombstoneRequest {
    /// Memory id.
    pub id: MemoryId,
    /// Reason.
    pub reason: String,
}

/// Durability tier.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DurabilityTier {
    /// Parent-directory fsync is supported.
    Full,
    /// Best effort only.
    BestEffort,
    /// Refused by default.
    Refused,
}

/// Repair state required for a committed write.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RepairRequired {
    /// Pending index repair.
    PendingIndex,
    /// Pending event repair.
    PendingEvent,
    /// Full startup scan.
    FullStartupScan,
    /// Operator action required.
    OperatorRequired(String),
}

/// Write outcome semantics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WriteOutcome {
    /// Whether the canonical file/ciphertext was committed.
    pub committed: bool,
    /// Whether the derived index was updated.
    pub indexed: bool,
    /// Whether the audit event was recorded.
    pub event_recorded: bool,
    /// Durability tier used.
    pub durability: DurabilityTier,
    /// Optional repair requirement.
    pub repair_required: Option<RepairRequired>,
    /// Operation id.
    pub operation_id: OperationId,
}

impl WriteOutcome {
    /// Build a not-committed outcome.
    pub fn not_committed(operation_id: OperationId, durability: DurabilityTier) -> Self {
        Self {
            committed: false,
            indexed: false,
            event_recorded: false,
            durability,
            repair_required: None,
            operation_id,
        }
    }
}

/// Unvalidated newtype: any UTF-8 string is accepted. Used for opaque
/// identifiers we do not constrain (operation ids, event ids, hash digests).
macro_rules! opaque_id_type {
    ($name:ident) => {
        #[doc = concat!(stringify!($name), " newtype (opaque, no format constraints).")]
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        pub struct $name(pub String);
        impl $name {
            #[doc = concat!("Wrap a string as ", stringify!($name), ".")]
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }
            #[doc = concat!("Borrow ", stringify!($name), " as str.")]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

opaque_id_type!(OperationId);
opaque_id_type!(EventId);
opaque_id_type!(Sha256);

/// Validated memory id (spec §7.1).
///
/// `try_new` validates against the spec regex; `new` panics on invalid input
/// and is intended only for fixture/test construction. Removed:
/// `From<&str>` / `From<String>` impls — they bypassed validation.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MemoryId(String);

impl MemoryId {
    /// Validate and construct a memory id.
    pub fn try_new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        if MEMORY_ID_REGEX.is_match(&value) {
            Ok(Self(value))
        } else {
            Err(ValidationError::InvalidMemoryId(value))
        }
    }

    /// Test/fixture constructor: panics on invalid input. Production code uses [`Self::try_new`].
    #[allow(clippy::expect_used)]
    pub fn new(value: impl Into<String>) -> Self {
        Self::try_new(value).expect("test memory id passed to MemoryId::new must validate")
        // expect-justified: test/fixture-only constructor
    }

    /// Borrow inner string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for MemoryId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl TryFrom<String> for MemoryId {
    type Error = ValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<MemoryId> for String {
    fn from(value: MemoryId) -> Self {
        value.0
    }
}

/// Validated device id (spec §7.4).
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DeviceId(String);

impl DeviceId {
    /// Validate and construct a device id.
    pub fn try_new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        if DEVICE_ID_REGEX.is_match(&value) {
            Ok(Self(value))
        } else {
            Err(ValidationError::BadShape(format!("invalid device id: {value}")))
        }
    }

    /// Test/fixture constructor: panics on invalid input.
    #[allow(clippy::expect_used)]
    pub fn new(value: impl Into<String>) -> Self {
        Self::try_new(value).expect("test device id passed to DeviceId::new must validate")
        // expect-justified: test/fixture-only constructor
    }

    /// Borrow inner string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for DeviceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl TryFrom<String> for DeviceId {
    type Error = ValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<DeviceId> for String {
    fn from(value: DeviceId) -> Self {
        value.0
    }
}

/// Validated repository-relative path. `try_new` enforces the Stream A
/// allow-list (spec §5.1); `new` panics on invalid input and is intended for
/// fixture/test code. Removed: unchecked `From<&str>` / `From<String>` impls.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RepoPath(String);

impl RepoPath {
    /// Try to create a validated repository-relative path.
    pub fn try_new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        validate_repo_relative_path(&value)?;
        Ok(Self(value))
    }

    /// Test/fixture constructor: panics on invalid input. Production code uses [`Self::try_new`].
    #[allow(clippy::expect_used)]
    pub fn new(value: impl Into<String>) -> Self {
        Self::try_new(value).expect("test repo path passed to RepoPath::new must validate")
        // expect-justified: test/fixture-only constructor
    }

    /// Wrap an arbitrary string without validation.
    ///
    /// Intended for two non-typed cases that must not call `try_new`:
    ///
    /// 1. **Index row hydration.** Paths loaded from SQLite were validated on
    ///    write; re-validating on read is wasteful and would refuse
    ///    historically-valid paths if the allow-list shrinks.
    /// 2. **Refusal-test fixtures.** Tests for path-escape / encrypted-tier
    ///    refusal need to *construct* invalid paths to feed downstream
    ///    validators. They cannot use `try_new`, by definition.
    ///
    /// Production write paths must use [`Self::try_new`] and propagate the error.
    pub fn from_unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow inner string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Return true when this path is safe to join under the repo root.
    pub fn is_safe_relative(&self) -> bool {
        validate_repo_relative_path(&self.0).is_ok()
    }

    /// Convert to a relative path.
    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl Display for RepoPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl TryFrom<String> for RepoPath {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<RepoPath> for String {
    fn from(value: RepoPath) -> Self {
        value.0
    }
}

/// Top-level prefixes where Markdown memory files (`.md`) are valid (spec §5.1).
const MEMORY_PREFIXES: &[&str] = &["me/", "projects/", "agent/", "dreams/", "encrypted/"];

/// Top-level prefixes that are JSONL-only (spec §5.1). Markdown is rejected here.
const JSONL_PREFIXES: &[&str] = &["substrate/", "events/", "tombstones/", "policies/", "leases/"];

/// Repository-root files we allow alongside the namespaced trees.
const ROOT_FILES: &[&str] = &[".gitattributes", ".gitignore", "config.yaml"];

fn validate_repo_relative_path(value: &str) -> Result<(), String> {
    if value.is_empty() || value.contains('\0') {
        return Err("empty or nul path".to_string());
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err("absolute paths are not allowed".to_string());
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir | Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("forbidden path component in {value}"));
            }
        }
    }
    if ROOT_FILES.contains(&value) {
        return Ok(());
    }
    if let Some(prefix) = MEMORY_PREFIXES.iter().find(|prefix| value.starts_with(*prefix)) {
        return validate_memory_tier_extension(value, prefix);
    }
    if let Some(prefix) = JSONL_PREFIXES.iter().find(|prefix| value.starts_with(*prefix)) {
        return validate_jsonl_tier_extension(value, prefix);
    }
    Err(format!("path is outside Stream A tree: {value}"))
}

fn validate_memory_tier_extension(value: &str, prefix: &str) -> Result<(), String> {
    let extension = Path::new(value).extension().and_then(|ext| ext.to_str());
    match extension {
        Some("md") => Ok(()),
        Some(other) => Err(format!("memory tier {prefix} accepts only .md, got .{other}: {value}")),
        None => Err(format!("memory tier {prefix} accepts only .md: {value}")),
    }
}

fn validate_jsonl_tier_extension(value: &str, prefix: &str) -> Result<(), String> {
    let extension = Path::new(value).extension().and_then(|ext| ext.to_str());
    match extension {
        Some("jsonl") => Ok(()),
        Some("yaml") if prefix == "policies/" => Ok(()),
        Some("md") => Err(format!("JSONL-only tier {prefix} rejects markdown: {value}")),
        Some(other) => Err(format!("JSONL-only tier {prefix} rejects .{other}: {value}")),
        None => Err(format!("JSONL-only tier {prefix} requires an extension: {value}")),
    }
}

/// Embedding model identity.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct EmbeddingTriple {
    /// Provider.
    pub provider: String,
    /// Model reference.
    pub model_ref: String,
    /// Dimension.
    pub dimension: u32,
}

/// Embedding update request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingUpdate {
    /// Chunk id to update.
    pub chunk_id: String,
    /// Expected chunk hash for stale-write protection.
    pub expected_chunk_hash: Sha256,
    /// Embedding triple identity.
    pub triple: EmbeddingTriple,
    /// Vector values.
    pub vector: Vec<f32>,
}

/// Memory query.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MemoryQuery {
    /// Optional id filter.
    pub id: Option<MemoryId>,
    /// Optional tag filter.
    pub tag: Option<String>,
    /// Include metadata-only encrypted records.
    pub include_metadata_only: bool,
    /// Optional lifecycle status filter.
    pub status: Option<MemoryStatus>,
    /// Optional synthetic namespace filter (`me`, `agent`, `project:<id>`, `org:<id>`).
    pub namespace_prefix: Option<String>,
    /// Return only rows whose retrieval policy permits passive recall.
    pub passive_recall_only: bool,
    /// Optional inclusive updated-at lower bound.
    pub updated_since: Option<DateTime<Utc>>,
}

/// Query result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueryResult {
    /// Memory id.
    pub id: MemoryId,
    /// Repository path.
    pub path: RepoPath,
    /// Summary.
    pub summary: String,
}

/// Read-only query over Stream A's derived recall index.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecallIndexQuery {
    /// Optional synthetic namespace filter (`me`, `agent`, `project:<id>`, `org:<id>`).
    pub namespace_prefix: Option<String>,
    /// Optional lifecycle status allow-list. Empty means no status predicate.
    pub statuses: Vec<MemoryStatus>,
    /// Return only rows whose retrieval policy permits passive recall.
    pub passive_recall_only: bool,
    /// Optional inclusive updated-at lower bound.
    pub updated_since: Option<DateTime<Utc>>,
    /// Optional exact/case-insensitive terms matched against tags, aliases, entities, and entity aliases.
    pub match_terms: Vec<String>,
}

/// Stream E recall-index row projected from SQLite index and auxiliary tables.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecallIndexRow {
    /// Memory id.
    pub id: MemoryId,
    /// Repository path.
    pub path: RepoPath,
    /// Frontmatter summary.
    pub summary: String,
    /// Lifecycle status.
    pub status: MemoryStatus,
    /// Namespace scope.
    pub scope: Scope,
    /// Canonical namespace id when scope is project/org.
    pub canonical_namespace_id: Option<String>,
    /// Updated-at timestamp.
    pub updated_at: DateTime<Utc>,
    /// Confidence score.
    pub confidence: f64,
    /// Source kind.
    pub source_kind: SourceKind,
    /// Sensitivity classification.
    pub sensitivity: Sensitivity,
    /// Indexed retrieval_policy.passive_recall value.
    pub passive_recall: bool,
    /// Indexed retrieval_policy.index_body value.
    pub index_body: bool,
    /// Indexed frontmatter.requires_user_confirmation value.
    pub requires_user_confirmation: bool,
    /// Indexed frontmatter.review_state value.
    pub review_state: Option<String>,
    /// Indexed write_policy.human_review_required value.
    pub human_review_required: bool,
    /// Indexed retrieval_policy.max_scope value.
    pub max_scope: Scope,
    /// Tags from `memory_tags`, sorted deterministically.
    pub tags: Vec<String>,
    /// Memory aliases from `memory_aliases`, sorted deterministically.
    pub aliases: Vec<String>,
    /// Entities with aliases from `memory_entities` / `memory_entity_aliases`, sorted deterministically by id.
    pub entities: Vec<Entity>,
}

/// Chunk query.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChunkQuery {
    /// Search text.
    pub text: Option<String>,
    /// Optional embedding triple.
    pub triple: Option<EmbeddingTriple>,
    /// Optional query vector for sqlite-vec KNN search.
    pub vector: Option<Vec<f32>>,
}

/// Chunk result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChunkResult {
    /// Memory id.
    pub memory_id: MemoryId,
    /// Chunk text.
    pub text: String,
    /// Score.
    pub score: f64,
}

/// Whether a memory's body is indexable in chunk-level search (spec §10.4).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyIndexability {
    /// Body is fully indexed for FTS and embeddings.
    Full,
    /// Only metadata is indexed; body chunks/vectors are absent.
    MetadataOnly,
    /// Body indexing is disabled entirely (e.g. `index_body = false`).
    None,
}

/// Component scores for a hybrid hit (spec §10.4 / §16.4).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    /// FTS bm25 score, when the FTS branch matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fts: Option<f32>,
    /// Vector similarity score, when the vector branch matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<f32>,
    /// Vector distance (lower = closer), when the vector branch matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<f32>,
}

/// Memory query hit for spec §16.4 `query_memory`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemoryHit {
    /// Hydrated memory metadata (frontmatter + path).
    pub memory: Memory,
    /// Body indexing class, controls Stream E's recall routing.
    pub body_indexability: BodyIndexability,
    /// Per-component scores for explainability.
    pub score_breakdown: ScoreBreakdown,
}

/// Chunk query hit for spec §16.4 `query_chunks`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChunkHit {
    /// Chunk id (`chk_<sha256>`).
    pub chunk_id: String,
    /// Owning memory id.
    pub memory_id: MemoryId,
    /// Chunk text snippet.
    pub text: String,
    /// Body indexing class for the parent memory.
    pub body_indexability: BodyIndexability,
    /// Per-component scores.
    pub score_breakdown: ScoreBreakdown,
}

/// Encryption envelope describing how Stream D produced the ciphertext.
///
/// Encryption envelope describing how Stream D produced the ciphertext.
///
/// Deferred: expand with KMS provider, key id, nonce, AAD, etc. when
/// `EncryptedWriteRequest` is wired through.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EncryptionEnvelope {
    /// Stream D scheme identifier (e.g. `age-x25519`).
    pub scheme: String,
    /// Recipient key reference (e.g. `age1...`).
    pub recipient: String,
    /// Optional opaque metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Memory content variant returned by `read_memory` / `read_path` (spec §16.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryContent {
    /// Plaintext body, LF-normalized.
    Plaintext(String),
    /// Encrypted bytes plus envelope describing how to decrypt.
    Ciphertext {
        /// Ciphertext bytes.
        bytes: Vec<u8>,
        /// Encryption envelope.
        encryption: EncryptionEnvelope,
    },
    /// Encrypted record without an authorized projection; only metadata is visible.
    MetadataOnly,
}

/// Envelope returned by `read_memory` / `read_path` per spec §16.2.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemoryEnvelope {
    /// Hydrated memory metadata.
    pub metadata: Memory,
    /// Content variant.
    pub content: MemoryContent,
}

/// Report returned by `drop_embedding_model` (spec §16.4).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DropTripleReport {
    /// Vectors removed from the dropped triple's table.
    pub vectors_removed: u64,
    /// `chunk_embedding_meta` rows removed.
    pub meta_rows_removed: u64,
    /// Pending embedding jobs dropped.
    pub pending_jobs_dropped: u64,
    /// Whether the per-triple vector table itself was dropped.
    pub table_dropped: bool,
}

#[cfg(test)]
mod tests {
    // expect-justified: this module is test-only; assertions are explicit by design.
    use super::*;

    #[test]
    fn memory_id_try_new_accepts_spec_format() {
        let id = MemoryId::try_new("mem_20260424_a1b2c3d4e5f60718_000001").expect("spec-format id validates"); // expect-justified: test
        assert_eq!(id.as_str(), "mem_20260424_a1b2c3d4e5f60718_000001");
    }

    #[test]
    fn memory_id_try_new_rejects_invalid_format() {
        let err = MemoryId::try_new("not_a_memory_id").expect_err("invalid id rejected");
        assert!(matches!(err, ValidationError::InvalidMemoryId(_)));
    }

    #[test]
    fn device_id_try_new_accepts_lowercase_alnum() {
        let id = DeviceId::try_new("dev_a1b2c3d4e5f60718").expect("valid device id"); // expect-justified: test
        assert_eq!(id.as_str(), "dev_a1b2c3d4e5f60718");
    }

    #[test]
    fn device_id_try_new_rejects_uppercase() {
        let err = DeviceId::try_new("dev_A1B2C3").expect_err("uppercase rejected");
        assert!(matches!(err, ValidationError::BadShape(_)));
    }

    #[test]
    fn device_id_try_new_rejects_missing_prefix() {
        let err = DeviceId::try_new("a1b2c3d4").expect_err("missing dev_ prefix");
        assert!(matches!(err, ValidationError::BadShape(_)));
    }

    #[test]
    fn accepts_md_under_me() {
        RepoPath::try_new("me/identity/who-i-am.md").expect("memory tier accepts md");
        // expect-justified: test
        // expect-justified: test
    }

    #[test]
    fn accepts_md_under_agent() {
        RepoPath::try_new("agent/patterns/example.md").expect("agent tier accepts md");
        // expect-justified: test
        // expect-justified: test
    }

    #[test]
    fn accepts_md_under_encrypted() {
        RepoPath::try_new("encrypted/agent/patterns/example.md").expect("encrypted tier accepts md");
        // expect-justified: test
        // expect-justified: test
    }

    #[test]
    fn rejects_md_under_substrate() {
        RepoPath::try_new("substrate/notes/example.md").expect_err("substrate is JSONL-only");
    }

    #[test]
    fn rejects_md_under_events() {
        RepoPath::try_new("events/example.md").expect_err("events is JSONL-only");
    }

    #[test]
    fn rejects_md_under_tombstones() {
        RepoPath::try_new("tombstones/example.md").expect_err("tombstones is JSONL-only");
    }

    #[test]
    fn rejects_md_under_policies() {
        RepoPath::try_new("policies/example.md").expect_err("policies is JSONL-only");
    }

    #[test]
    fn accepts_jsonl_under_events() {
        RepoPath::try_new("events/dev_a1b2c3.jsonl").expect("events accepts jsonl");
        // expect-justified: test
        // expect-justified: test
    }

    #[test]
    fn accepts_jsonl_under_substrate() {
        RepoPath::try_new("substrate/state.jsonl").expect("substrate accepts jsonl");
        // expect-justified: test
        // expect-justified: test
    }

    #[test]
    fn rejects_path_traversal() {
        RepoPath::try_new("agent/patterns/../../etc/passwd").expect_err("traversal rejected");
    }

    #[test]
    fn rejects_outside_tree() {
        RepoPath::try_new("random/path.md").expect_err("unknown tier rejected");
    }

    #[test]
    fn accepts_root_files() {
        RepoPath::try_new(".gitattributes").expect("root file"); // expect-justified: test
        RepoPath::try_new(".gitignore").expect("root file"); // expect-justified: test
        RepoPath::try_new("config.yaml").expect("root file"); // expect-justified: test
    }
}
