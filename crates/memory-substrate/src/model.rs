#![allow(unknown_lints, file_too_long)]
//! Public DTO contract is intentionally centralized for Task 2 seam stability.
//! Public data model for Stream A.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

use crate::error::ValidationError;

/// Memory ID format. Mirrors spec §7.1.
#[allow(clippy::expect_used)]
static MEMORY_ID_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^mem_\d{8}_[0-9a-f]{16}_\d{6}$").expect("memory-id regex literal")
    // expect-justified: compile-time regex literal cannot fail
});

/// Device ID format. Mirrors spec §7.4: `dev_<lowercase hex/alnum>`.
#[allow(clippy::expect_used)]
static DEVICE_ID_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^dev_[a-z0-9]+$").expect("device-id regex literal") // expect-justified: compile-time regex literal cannot fail
});

/// Substrate fragment id format. Stream F uses `sub_<ULID>`.
#[allow(clippy::expect_used)]
static SUBSTRATE_FRAGMENT_ID_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^sub_[0-9A-HJKMNP-TV-Z]{26}$").expect("substrate-fragment-id regex literal")
    // expect-justified: compile-time regex literal cannot fail
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
    /// Explicit merge driver binary path. Ambient PATH lookup is intentionally
    /// refused for unattended clone adoption.
    pub merge_driver_path: Option<PathBuf>,
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

/// Derived SQLite events-log mirror freshness.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventsLogMirrorHealth {
    /// Highest event sequence observed in canonical JSONL logs.
    pub jsonl_max_seq: u64,
    /// Highest event sequence currently mirrored into SQLite.
    pub sqlite_max_seq: u64,
    /// Saturating `jsonl_max_seq - sqlite_max_seq`.
    pub lag: u64,
    /// Canonical JSONL event count across all device logs.
    pub jsonl_count: u64,
    /// SQLite mirror event count.
    pub sqlite_count: u64,
    /// Canonical JSONL events missing from the SQLite mirror by event id.
    pub missing_count: u64,
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

/// Substrate-observation kind accepted by `memory_observe`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObserveKind {
    /// A raw observation.
    Observation,
    /// A recurring pattern.
    Pattern,
    /// A weak signal that may need later attention.
    Signal,
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

impl Sensitivity {
    const PERSISTED_VARIANTS: [Self; 4] = [Self::Public, Self::Internal, Self::Confidential, Self::Personal];

    /// Canonical on-disk string for the SQLite index. Byte-identical to the
    /// serde representation used in frontmatter YAML; the `model` tests lock the
    /// two together. See [`Sensitivity::from_db_str`] for the inverse.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
            Self::Confidential => "confidential",
            Self::Personal => "personal",
        }
    }

    /// Parse a canonical on-disk string back into the variant, `None` if unknown.
    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "public" => Some(Self::Public),
            "internal" => Some(Self::Internal),
            "confidential" => Some(Self::Confidential),
            "personal" => Some(Self::Personal),
            _ => None,
        }
    }

    /// Whether this persisted sensitivity tier may transit an API embedding
    /// lane.
    ///
    /// This mirrors the privacy storage policy: `Public` and `Internal` store
    /// as plaintext, while `Confidential` and `Personal` are encrypted at rest
    /// and must remain local-only when an API embedding provider is active.
    pub fn api_lane_eligible(&self) -> bool {
        matches!(self, Self::Public | Self::Internal)
    }

    /// Canonical DB strings for API-eligible tiers, derived from
    /// [`Self::api_lane_eligible`] so SQL predicates and Rust checks share one
    /// source of truth.
    pub fn api_lane_eligible_db_strs() -> Vec<&'static str> {
        Self::PERSISTED_VARIANTS
            .iter()
            .filter(|sensitivity| sensitivity.api_lane_eligible())
            .map(Self::as_db_str)
            .collect()
    }
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

impl MemoryStatus {
    /// Canonical on-disk string for the SQLite index. Byte-identical to the
    /// serde representation; the `model` tests lock the two together. See
    /// [`MemoryStatus::from_db_str`] for the inverse.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::Active => "active",
            Self::Pinned => "pinned",
            Self::Superseded => "superseded",
            Self::Archived => "archived",
            Self::Tombstoned => "tombstoned",
            Self::Quarantined => "quarantined",
        }
    }

    /// Parse a canonical on-disk string back into the variant, `None` if unknown.
    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "candidate" => Some(Self::Candidate),
            "active" => Some(Self::Active),
            "pinned" => Some(Self::Pinned),
            "superseded" => Some(Self::Superseded),
            "archived" => Some(Self::Archived),
            "tombstoned" => Some(Self::Tombstoned),
            "quarantined" => Some(Self::Quarantined),
            _ => None,
        }
    }
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

impl TrustLevel {
    /// Canonical on-disk string for the SQLite index. Byte-identical to the
    /// serde representation; the `model` tests lock the two together.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::Untrusted => "untrusted",
            Self::Candidate => "candidate",
            Self::Quarantined => "quarantined",
            Self::Pinned => "pinned",
        }
    }
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

impl MemoryType {
    /// Canonical on-disk string for the SQLite index. Byte-identical to the
    /// serde representation (note `anti-pattern`/`open-question` overrides); the
    /// `model` tests lock the two together.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Person => "person",
            Self::Procedure => "procedure",
            Self::Episode => "episode",
            Self::Claim => "claim",
            Self::Artifact => "artifact",
            Self::Prospective => "prospective",
            Self::Pattern => "pattern",
            Self::Playbook => "playbook",
            Self::Postmortem => "postmortem",
            Self::AntiPattern => "anti-pattern",
            Self::Heuristic => "heuristic",
            Self::Regression => "regression",
            Self::Correction => "correction",
            Self::Invariant => "invariant",
            Self::Decision => "decision",
            Self::OpenQuestion => "open-question",
        }
    }
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

impl Scope {
    /// Canonical on-disk string for the SQLite index. Byte-identical to the
    /// serde representation; the `model` tests lock the two together. See
    /// [`Scope::from_db_str`] for the inverse.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
            Self::Org => "org",
            Self::Agent => "agent",
            Self::Subagent => "subagent",
        }
    }

    /// Parse a canonical on-disk string back into the variant, `None` if unknown.
    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "project" => Some(Self::Project),
            "org" => Some(Self::Org),
            "agent" => Some(Self::Agent),
            "subagent" => Some(Self::Subagent),
            _ => None,
        }
    }
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

impl AuthorKind {
    /// Canonical on-disk string for the SQLite index. Byte-identical to the
    /// serde representation; the `model` tests lock the two together.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::Subagent => "subagent",
            Self::Dreaming => "dreaming",
            Self::System => "system",
        }
    }
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

impl SourceKind {
    /// Canonical on-disk string for the SQLite index. Byte-identical to the
    /// serde representation (note `agent-primary`/`agent-subagent`); the `model`
    /// tests lock the two together. `Display` delegates here. See
    /// [`SourceKind::from_db_str`] for the inverse.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::AgentPrimary => "agent-primary",
            Self::AgentSubagent => "agent-subagent",
            Self::Tool => "tool",
            Self::Web => "web",
            Self::Email => "email",
            Self::File => "file",
            Self::Synthesis => "synthesis",
            Self::Import => "import",
            Self::System => "system",
        }
    }

    /// Parse a canonical on-disk string back into the variant, `None` if unknown.
    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "agent-primary" => Some(Self::AgentPrimary),
            "agent-subagent" => Some(Self::AgentSubagent),
            "tool" => Some(Self::Tool),
            "web" => Some(Self::Web),
            "email" => Some(Self::Email),
            "file" => Some(Self::File),
            "synthesis" => Some(Self::Synthesis),
            "import" => Some(Self::Import),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

impl std::fmt::Display for SourceKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_db_str())
    }
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
    /// Confidence at first observation, used by drift scoring as the baseline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_confidence: Option<f64>,
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
    /// Timestamp when the memory's claim was last observed or confirmed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
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
    /// Compact semantic abstraction used by derived vector lanes.
    #[serde(default)]
    pub abstraction: Option<String>,
    /// Short retrieval cues used by derived vector lanes.
    #[serde(default)]
    pub cues: Vec<String>,
    /// Unknown v1 fields preserved for round-trip per spec §6.2. `BTreeMap`
    /// keeps re-emission order deterministic.
    #[serde(flatten)]
    pub extras: BTreeMap<String, serde_json::Value>,
}

impl Frontmatter {
    /// Whether a dream-authored candidate must re-resolve cited grounding refs
    /// immediately before review approval.
    pub fn grounding_rehydration_required(&self) -> bool {
        self.extras.get("grounding_rehydration_required").and_then(serde_json::Value::as_bool).unwrap_or(false)
    }

    /// Set the explicit Stream F grounding rehydration marker.
    pub fn set_grounding_rehydration_required(&mut self, required: bool) {
        self.extras.insert("grounding_rehydration_required".to_string(), serde_json::Value::Bool(required));
    }
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

/// Privacy span persisted for substrate audit records.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PrivacySpanRecord {
    /// Privacy label supplied by Stream D.
    pub label: String,
    /// UTF-8 byte start offset.
    pub start: usize,
    /// UTF-8 byte end offset.
    pub end: usize,
}

/// Plaintext substrate fragment record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SubstrateFragmentRecord {
    /// Fragment id (`sub_<ulid>`).
    pub id: String,
    /// Observation timestamp.
    #[serde(rename = "ts")]
    pub ts: DateTime<Utc>,
    /// Writing device.
    pub device: DeviceId,
    /// Optional caller session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Optional harness name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// Synthetic scope string (`me`, `agent`, `project:<id>`, `org:<id>`).
    pub scope: String,
    /// Referenced entities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<String>,
    /// Observation kind.
    pub kind: ObserveKind,
    /// Plaintext body. Only legal for plaintext substrate records.
    pub text: String,
    /// Source reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    /// Privacy spans from Stream D classifier.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub privacy_spans: Vec<PrivacySpanRecord>,
}

/// Encryption envelope stored in encrypted substrate JSONL records.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SubstrateFragmentEncryption {
    /// Recipient key id.
    pub recipient: String,
    /// Base64-encoded ciphertext.
    pub ciphertext_b64: String,
}

/// Safe descriptor projection for encrypted substrate fragments.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EncryptedSubstrateDescriptor {
    /// Safe summary supplied by Stream D.
    pub summary_safe: String,
    /// Safe tag projection supplied by Stream D.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tag_safe: Vec<String>,
}

/// Encrypted substrate fragment record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EncryptedSubstrateFragmentRecord {
    /// Fragment id (`sub_<ulid>`).
    pub id: String,
    /// Observation timestamp.
    #[serde(rename = "ts")]
    pub ts: DateTime<Utc>,
    /// Writing device.
    pub device: DeviceId,
    /// Optional caller session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Optional harness name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// Synthetic scope string.
    pub scope: String,
    /// Referenced entities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<String>,
    /// Observation kind.
    pub kind: ObserveKind,
    /// Encryption payload. No plaintext `text` field is present in this record.
    pub encryption: SubstrateFragmentEncryption,
    /// Safe descriptor projection.
    pub descriptor: EncryptedSubstrateDescriptor,
    /// Source reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    /// Privacy spans from Stream D classifier.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub privacy_spans: Vec<PrivacySpanRecord>,
}

/// Payload routed by Stream D classification.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum SubstrateFragmentPayload {
    /// Plaintext-safe substrate record.
    Plaintext {
        /// Plaintext body.
        text: String,
    },
    /// Encrypted substrate record with safe descriptor projection.
    Encrypted {
        /// Encryption envelope.
        encryption: SubstrateFragmentEncryption,
        /// Descriptor projection.
        descriptor: EncryptedSubstrateDescriptor,
    },
}

/// Append request for Stream F substrate fragments.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SubstrateFragmentAppendRequest {
    /// Optional caller-supplied id, used by deterministic tests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Observation timestamp.
    pub at: DateTime<Utc>,
    /// Optional caller session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Optional harness name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// Synthetic scope string.
    pub scope: String,
    /// Entity ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<String>,
    /// Observation kind.
    pub kind: ObserveKind,
    /// Source reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    /// Privacy spans.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub privacy_spans: Vec<PrivacySpanRecord>,
    /// Storage payload.
    pub payload: SubstrateFragmentPayload,
    /// Classification supplied by Stream D.
    pub classification: ClassificationOutcome,
    /// Optional operation id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<OperationId>,
}

/// Append outcome for substrate fragments.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SubstrateFragmentAppendOutcome {
    /// Fragment id.
    pub id: String,
    /// Repo-relative JSONL path.
    pub path: RepoPath,
    /// Operation id used for audit event emission.
    pub operation_id: OperationId,
}

/// Archival outcome for expired plaintext substrate fragments.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SubstrateArchiveOutcome {
    /// Number of newly archived fragments.
    pub fragments_archived: usize,
}

/// Validate a Stream F substrate fragment id.
pub fn validate_substrate_fragment_id(id: &str) -> Result<(), ValidationError> {
    if SUBSTRATE_FRAGMENT_ID_REGEX.is_match(id) {
        Ok(())
    } else {
        Err(ValidationError::BadShape(format!("invalid substrate fragment id: {id}")))
    }
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
        crate::path_validation::validate_repo_relative_path(&value)?;
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
        crate::path_validation::validate_repo_relative_path(&self.0).is_ok()
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

/// Which sensitivity tiers may be handed to the active embedding lane.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EmbeddingLaneEligibility {
    /// Local lane: every persisted sensitivity tier may be embedded locally.
    AllTiers,
    /// API lane: only tiers stored as plaintext may transit the provider.
    PlaintextOnly,
}

impl EmbeddingLaneEligibility {
    /// `true` when SQL should add the plaintext-only sensitivity predicate.
    pub fn requires_plaintext_filter(self) -> bool {
        matches!(self, Self::PlaintextOnly)
    }

    /// DB strings allowed by this eligibility mode.
    ///
    /// `AllTiers` returns an empty list because no SQL sensitivity predicate is
    /// applied. `PlaintextOnly` derives its allowlist from
    /// [`Sensitivity::api_lane_eligible`].
    pub fn allowed_sensitivity_db_strs(self) -> Vec<&'static str> {
        match self {
            Self::AllTiers => Vec::new(),
            Self::PlaintextOnly => {
                let allowed = Sensitivity::api_lane_eligible_db_strs();
                // A PlaintextOnly allowlist must never be empty: callers splice it
                // into `sensitivity IN (...)`, and `IN ()` is a SQL syntax error,
                // not a fail-closed empty match. Public|Internal keep this at
                // length 2 today; this guards a future tier-table edit.
                debug_assert!(!allowed.is_empty(), "PlaintextOnly sensitivity allowlist must be non-empty");
                allowed
            }
        }
    }
}

/// A pending embedding job paired with the chunk text the worker must embed.
///
/// Produced by [`crate::index::Index::pending_embedding_jobs`] / the
/// [`crate::Substrate::pending_embedding_jobs`] wrapper for the background
/// embedding worker. `content_hash` is the chunk `body_hash` captured at
/// enqueue time; it must be passed back as the `expected_chunk_hash` of the
/// resulting [`EmbeddingUpdate`] so a stale job (the chunk changed since
/// enqueue) is rejected by the vector store rather than writing a vector for
/// content that no longer exists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingEmbeddingJob {
    /// Chunk id to embed.
    pub chunk_id: String,
    /// Chunk text the worker embeds.
    pub text: String,
    /// Content hash captured at enqueue time (the chunk `body_hash`).
    pub content_hash: Sha256,
}

/// Derived semantic embedding row kind.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuxRowKind {
    /// One abstraction per memory.
    Abstraction,
    /// Up to three cues per memory.
    Cue,
}

impl AuxRowKind {
    /// Stable SQLite representation.
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Abstraction => "abstraction",
            Self::Cue => "cue",
        }
    }
}

/// Pending abstraction/cue embedding work.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuxPendingEmbeddingJob {
    /// Semantic row kind.
    pub row_kind: AuxRowKind,
    /// Memory id, or `memory_id:ordinal` for cues.
    pub target_id: String,
    /// Text to embed.
    pub text: String,
    /// Hash captured when the job was enqueued.
    pub content_hash: Sha256,
}

/// Stale-fenced vector update for a semantic row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuxEmbeddingUpdate {
    /// Semantic row kind.
    pub row_kind: AuxRowKind,
    /// Memory id, or `memory_id:ordinal` for cues.
    pub target_id: String,
    /// Expected current semantic text hash.
    pub expected_content_hash: Sha256,
    /// Embedding triple identity.
    pub triple: EmbeddingTriple,
    /// Vector values.
    pub vector: Vec<f32>,
}

/// Abstraction vector query hit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AbstractionVectorHit {
    /// Owning memory.
    pub memory_id: MemoryId,
    /// sqlite-vec L2 distance.
    pub distance: f32,
}

/// Cue vector query hit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CueVectorHit {
    /// Owning memory.
    pub memory_id: MemoryId,
    /// Canonical cue ordinal.
    pub ordinal: u8,
    /// sqlite-vec L2 distance.
    pub distance: f32,
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

/// Which auxiliary-table fields a recall-index query hydrates onto each row.
///
/// Hydration costs one batched `IN (...)` scan per category (tags, aliases,
/// entities, entity-aliases). Most callers read only the scalar projection from
/// the `memories` row and pay for hydration they discard, so the query carries
/// an explicit scope. Variants are cumulative in cost: `Entities` reads the two
/// entity tables, `All` additionally reads tags and aliases. `All` is the
/// default so unset queries keep the historical fully-hydrated behavior.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuxScope {
    /// Hydrate no auxiliary tables. `tags`/`aliases`/`entities` stay empty.
    None,
    /// Hydrate only `entities` (with their aliases); leave tags/aliases empty.
    Entities,
    /// Hydrate only `tags`; leave aliases/entities empty.
    Tags,
    /// Hydrate tags, aliases, and entities (the historical default).
    #[default]
    All,
}

/// Read-only query over Stream A's derived recall index.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
    /// Which auxiliary-table fields to hydrate onto each returned row.
    pub hydrate: AuxScope,
    /// Project the peer source/author identity and merge-diagnostics fields
    /// (`source_session_id`, `author_harness`, `author_session_id`,
    /// `merge_diagnostics_json`) onto each row via per-row `json_extract`.
    ///
    /// Defaults to `true` for public API compatibility: historical default recall
    /// rows projected these fields whenever present. Hot internal readers that do
    /// not consume identity/diagnostics set this to `false` explicitly.
    /// `source_harness` is always projected from its materialized column
    /// regardless of this flag.
    pub source_identity: bool,
}

impl Default for RecallIndexQuery {
    fn default() -> Self {
        Self {
            namespace_prefix: None,
            statuses: Vec::new(),
            passive_recall_only: false,
            updated_since: None,
            match_terms: Vec::new(),
            hydrate: AuxScope::All,
            source_identity: true,
        }
    }
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
    /// Timestamp this device's index ingested this memory.
    ///
    /// Stream I uses this as `local_observed_at` for peer-update recency.
    pub indexed_at: DateTime<Utc>,
    /// Confidence score.
    pub confidence: f64,
    /// Source kind.
    pub source_kind: SourceKind,
    /// Device id that authored the most recent write, when known.
    pub source_device: Option<String>,
    /// `source.harness` projected from indexed frontmatter, when present.
    ///
    /// Stream I peer-write attribution reads harness/session identity directly
    /// from the recall index instead of re-reading the canonical file.
    pub source_harness: Option<String>,
    /// `source.session_id` projected from indexed frontmatter, when present.
    pub source_session_id: Option<String>,
    /// `author.harness` projected from indexed frontmatter, when present.
    pub author_harness: Option<String>,
    /// `author.session_id` projected from indexed frontmatter, when present.
    pub author_session_id: Option<String>,
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
    /// `_merge_diagnostics` projected from indexed frontmatter, when present.
    ///
    /// Carried as the raw stored JSON string so conflict-list callers can serve
    /// the merge-diagnostics field straight from the index without re-reading
    /// and re-parsing the canonical file.
    pub merge_diagnostics_json: Option<String>,
    /// Tags from `memory_tags`, sorted deterministically.
    pub tags: Vec<String>,
    /// Memory aliases from `memory_aliases`, sorted deterministically.
    pub aliases: Vec<String>,
    /// Entities with aliases from `memory_entities` / `memory_entity_aliases`, sorted deterministically by id.
    pub entities: Vec<Entity>,
}

/// A single review-queue candidate projected entirely from the derived index.
///
/// Carries exactly the fields the review-queue response needs so the daemon can
/// build the queue without reading and parsing every canonical memory file.
/// `policy_applied` and `governance_reason` are projected from `frontmatter_json`
/// via `json_extract`, mirroring `RecallIndexRow::merge_diagnostics_json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewQueueRow {
    /// Memory id.
    pub id: String,
    /// Frontmatter summary.
    pub summary: String,
    /// Lifecycle status, serialized as the canonical lowercase string.
    pub status: String,
    /// Indexed `frontmatter.requires_user_confirmation` value.
    pub requires_user_confirmation: bool,
    /// Indexed `frontmatter.review_state` value.
    pub review_state: Option<String>,
    /// `write_policy.policy_applied` projected from indexed frontmatter.
    pub policy_applied: String,
    /// `extras.governance_reason` projected from indexed frontmatter, when present.
    pub governance_reason: Option<String>,
}

/// Result of an index-served review-queue query.
///
/// `total` counts every memory matching the review-queue membership predicate
/// (used for the over-threshold notification), while `rows` is the bounded,
/// deterministically-ordered slice the response actually renders.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewQueuePage {
    /// Total number of memories matching the review-queue predicate.
    pub total: usize,
    /// Bounded set of candidate rows, ordered newest-first by `updated_at`.
    pub rows: Vec<ReviewQueueRow>,
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

/// Structurally valid vector half of a hybrid recall query.
///
/// `triple` and `vector` travel together so callers cannot supply a vector
/// without the exact embedding triple identity. The absence of this struct means
/// "BM25-only"; its presence means "BM25 plus this exact vector lane".
#[derive(Clone, Copy, Debug)]
pub struct HybridVectorQuery<'a> {
    /// Embedding triple identity for the vector table.
    pub triple: &'a EmbeddingTriple,
    /// Query vector for sqlite-vec KNN.
    pub vector: &'a [f32],
}

/// Per-lane explainability/rank inputs for a hybrid recall candidate.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HybridScoreBreakdown {
    /// One-based memory rank from the BM25 lane after chunk→memory collapse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_rank: Option<usize>,
    /// Cosine similarity from the vector lane after chunk→memory collapse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cosine_similarity: Option<f32>,
}

/// Per-memory hybrid recall candidate.
///
/// This is intentionally not a fused score. Memoryd owns RRF/fusion; the
/// substrate only returns the lane-local evidence needed to fuse later.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HybridMemoryCandidate {
    /// Memory id.
    pub memory_id: MemoryId,
    /// Representative chunk text selected by the lane-local chunk→memory
    /// collapse. BM25 hits use the best matching chunk; vector-only hits use
    /// the nearest embedded chunk.
    #[serde(default)]
    pub text: String,
    /// Lane-local rank/similarity evidence.
    pub score_breakdown: HybridScoreBreakdown,
    /// Freshness signal for downstream RRF recency prior: `max(observed_at, updated_at)`
    /// from the memories index row. `None` when the index row lacks usable timestamps.
    #[serde(default)]
    pub recency_at: Option<DateTime<Utc>>,
}

/// A single active-memory neighbour returned by a governance KNN similarity
/// query ([`crate::Substrate::knn_active_memories`]).
///
/// One row per *memory* (chunk hits are collapsed to the nearest chunk), already
/// filtered to active, non-encrypted, in-scope rows. `similarity` is a cosine
/// similarity in `[-1, 1]` (higher = more similar), derived from the stored L2
/// distance under the unit-vector assumption documented on the query method.
#[derive(Clone, Debug, PartialEq)]
pub struct SimilarMemory {
    /// Memory id of the neighbour.
    pub memory_id: MemoryId,
    /// Lifecycle scope of the neighbour (drives the governance namespace label).
    pub scope: Scope,
    /// Cosine similarity to the query vector (higher is nearer).
    pub similarity: f32,
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

    // On-disk string contract.
    //
    // The SQLite index stores these enums as strings via `as_db_str`. Those
    // spellings MUST equal the serde representation used in frontmatter YAML —
    // a divergence silently desyncs the index from canonical files. Each test
    // enumerates every variant explicitly (so adding a variant without updating
    // the method is caught) and asserts `as_db_str` equals the serde form. The
    // four enums with a parser also assert `from_db_str` round-trips.

    /// The serde string for a unit enum variant, via JSON (always a bare string).
    fn serde_db_str<T: Serialize>(value: &T) -> String {
        serde_json::to_value(value)
            .expect("enum serializes") // expect-justified: test
            .as_str()
            .expect("unit variant serializes to a string") // expect-justified: test
            .to_string()
    }

    #[test]
    fn sensitivity_db_str_matches_serde() {
        for variant in [Sensitivity::Public, Sensitivity::Internal, Sensitivity::Confidential, Sensitivity::Personal] {
            assert_eq!(variant.as_db_str(), serde_db_str(&variant));
            assert_eq!(Sensitivity::from_db_str(variant.as_db_str()), Some(variant));
        }
    }

    #[test]
    fn memory_status_db_str_matches_serde() {
        for variant in [
            MemoryStatus::Candidate,
            MemoryStatus::Active,
            MemoryStatus::Pinned,
            MemoryStatus::Superseded,
            MemoryStatus::Archived,
            MemoryStatus::Tombstoned,
            MemoryStatus::Quarantined,
        ] {
            assert_eq!(variant.as_db_str(), serde_db_str(&variant));
            assert_eq!(MemoryStatus::from_db_str(variant.as_db_str()), Some(variant));
        }
    }

    #[test]
    fn trust_level_db_str_matches_serde() {
        for variant in [
            TrustLevel::Trusted,
            TrustLevel::Untrusted,
            TrustLevel::Candidate,
            TrustLevel::Quarantined,
            TrustLevel::Pinned,
        ] {
            assert_eq!(variant.as_db_str(), serde_db_str(&variant));
        }
    }

    #[test]
    fn memory_type_db_str_matches_serde() {
        for variant in [
            MemoryType::Project,
            MemoryType::Person,
            MemoryType::Procedure,
            MemoryType::Episode,
            MemoryType::Claim,
            MemoryType::Artifact,
            MemoryType::Prospective,
            MemoryType::Pattern,
            MemoryType::Playbook,
            MemoryType::Postmortem,
            MemoryType::AntiPattern,
            MemoryType::Heuristic,
            MemoryType::Regression,
            MemoryType::Correction,
            MemoryType::Invariant,
            MemoryType::Decision,
            MemoryType::OpenQuestion,
        ] {
            assert_eq!(variant.as_db_str(), serde_db_str(&variant));
        }
    }

    #[test]
    fn scope_db_str_matches_serde() {
        for variant in [Scope::User, Scope::Project, Scope::Org, Scope::Agent, Scope::Subagent] {
            assert_eq!(variant.as_db_str(), serde_db_str(&variant));
            assert_eq!(Scope::from_db_str(variant.as_db_str()), Some(variant));
        }
    }

    #[test]
    fn author_kind_db_str_matches_serde() {
        for variant in
            [AuthorKind::User, AuthorKind::Agent, AuthorKind::Subagent, AuthorKind::Dreaming, AuthorKind::System]
        {
            assert_eq!(variant.as_db_str(), serde_db_str(&variant));
        }
    }

    #[test]
    fn source_kind_db_str_matches_serde() {
        for variant in [
            SourceKind::User,
            SourceKind::AgentPrimary,
            SourceKind::AgentSubagent,
            SourceKind::Tool,
            SourceKind::Web,
            SourceKind::Email,
            SourceKind::File,
            SourceKind::Synthesis,
            SourceKind::Import,
            SourceKind::System,
        ] {
            assert_eq!(variant.as_db_str(), serde_db_str(&variant));
            assert_eq!(SourceKind::from_db_str(variant.as_db_str()), Some(variant));
            // `Display` delegates to `as_db_str`; lock that too.
            assert_eq!(variant.to_string(), variant.as_db_str());
        }
    }
}
