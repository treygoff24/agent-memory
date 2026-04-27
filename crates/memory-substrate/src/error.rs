//! Public error taxonomy for Stream A.

use std::path::PathBuf;

use crate::model::{DurabilityTier, EmbeddingTriple, MemoryId, RepoPath, Sha256, WriteOutcome};

/// Operator action that resolves an [`OpenError`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepairAction {
    /// Run `git::adopt_clone` to mint device identity for this clone.
    AdoptClone,
}

/// Side of a three-way merge that produced a parse error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MergeSide {
    /// Common ancestor.
    Base,
    /// Local side.
    Ours,
    /// Incoming side.
    Theirs,
}

/// Result alias for substrate operations.
pub type SubstrateResult<T> = Result<T, SubstrateError>;

/// Top-level substrate error.
#[derive(Debug, thiserror::Error)]
pub enum SubstrateError {
    /// Open failed.
    #[error(transparent)]
    Open(#[from] OpenError),
    /// Read failed.
    #[error(transparent)]
    Read(#[from] ReadError),
    /// Write failed.
    #[error(transparent)]
    Write(#[from] WriteFailure),
    /// Validation failed.
    #[error(transparent)]
    Validation(#[from] ValidationError),
    /// ID allocation failed.
    #[error(transparent)]
    Id(#[from] IdError),
    /// Vector operation failed.
    #[error(transparent)]
    Vector(#[from] VectorError),
    /// Git operation failed.
    #[error(transparent)]
    Git(#[from] GitError),
    /// Watcher operation failed.
    #[error(transparent)]
    Watch(#[from] WatchError),
    /// Merge operation failed.
    #[error(transparent)]
    Merge(#[from] MergeError),
    /// IO failed.
    #[error("io error at {path}: {source}")]
    Io { path: String, source: std::io::Error },
    /// SQLite failed.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Open errors.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// Durability unsupported.
    #[error("durability unsupported: {tier:?}")]
    DurabilityUnsupported { tier: DurabilityTier },
    /// Operator repair is required before writes can proceed.
    #[error("operator repair required: {0}")]
    OperatorRepairRequired(String),
    /// Invalid roots.
    #[error("invalid roots: {0}")]
    InvalidRoots(String),
    /// Local device identity is missing; the named repair must run first.
    #[error("device identity missing; required repair: {repair:?}")]
    DeviceIdentityMissing {
        /// Operator-facing repair action that resolves this error.
        repair: RepairAction,
    },
    /// Index database schema version exceeds what this build supports.
    #[error("index schema version {found} unsupported; supported up to {supported}")]
    IndexSchemaVersionUnsupported {
        /// Highest version found in `schema_migrations`.
        found: u32,
        /// Highest version this build understands.
        supported: u32,
    },
    /// IO failed.
    #[error("open io error: {0}")]
    Io(#[from] std::io::Error),
    /// Validation failed.
    #[error(transparent)]
    Validation(#[from] ValidationError),
}

/// Read errors.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// Path not found.
    #[error("memory path not found: {0}")]
    NotFound(RepoPath),
    /// Parse failed.
    #[error("parse failed for {path}: {message}")]
    Parse { path: RepoPath, message: String },
    /// IO failed.
    #[error("read io error: {0}")]
    Io(#[from] std::io::Error),
    /// Validation failed.
    #[error(transparent)]
    Validation(#[from] ValidationError),
}

/// Write failure.
#[derive(Debug, thiserror::Error)]
#[error("write failed: {kind}")]
pub struct WriteFailure {
    /// Outcome, including committed state when applicable.
    pub outcome: WriteOutcome,
    /// Failure kind.
    pub kind: WriteFailureKind,
}

/// Write failure kind.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum WriteFailureKind {
    /// Secret content was refused before disk effects.
    #[error("secret refused")]
    SecretRefused,
    /// Plaintext path refused because encryption is required.
    #[error("encryption required")]
    EncryptionRequired,
    /// Trusted classification conflicts with sensitive frontmatter.
    #[error("classification sensitivity mismatch")]
    ClassificationSensitivityMismatch,
    /// Stale base hash.
    #[error("stale base")]
    StaleBase,
    /// Target already exists.
    #[error("target already exists")]
    AlreadyExists,
    /// Durability unavailable.
    #[error("durability unavailable")]
    DurabilityUnavailable,
    /// Index failed after durable commit.
    #[error("index failed after commit")]
    IndexAfterCommitFailed,
    /// Repair queue failed.
    #[error("repair queue failed")]
    RepairQueueFailed,
    /// Repair state could not be made durable.
    #[error("repair state not durable")]
    RepairStateNotDurable,
    /// Validation failed (legacy stringly-typed variant).
    ///
    /// Deferred: callers should switch to `ValidationTyped(ValidationError)` below;
    /// tests still match on `Validation(String)` so both variants remain until call
    /// sites are migrated.
    #[error("validation failed: {0}")]
    Validation(String),
    /// Validation failed (typed source).
    #[error("validation failed: {0}")]
    ValidationTyped(ValidationError),
    /// IO failed (legacy stringly-typed variant).
    ///
    /// Deferred: replace with `IoTyped { kind, context }` below.
    #[error("io failed: {0}")]
    Io(String),
    /// IO failed; `kind` is the typed signal, `context` is operator description.
    #[error("io failed [{kind:?}]: {context}")]
    IoTyped {
        /// Standard library IO error kind for programmatic dispatch.
        kind: std::io::ErrorKind,
        /// Operator-facing context string.
        context: String,
    },
}

/// Validation errors.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ValidationError {
    /// Required field missing.
    #[error("missing required field: {0}")]
    MissingRequiredField(String),
    /// Bad enum.
    #[error("bad enum for {field}: {value}")]
    BadEnum { field: String, value: String },
    /// Bad shape.
    #[error("bad shape for {0}")]
    BadShape(String),
    /// Unsupported schema version.
    #[error("unsupported schema version {found}; supported {supported}")]
    UnsupportedSchemaVersion { found: u32, supported: u32 },
    /// Invalid lifecycle pair.
    #[error("invalid lifecycle pair")]
    InvalidLifecyclePair,
    /// Secret sensitivity persisted on disk at a specific path.
    #[error("secret sensitivity persisted on disk at {}", path.display())]
    SecretSensitivityOnDiskAt {
        /// File where the disallowed value was found.
        path: PathBuf,
    },
    /// Plaintext file under `encrypted/` lacks an encryption envelope.
    #[error("plaintext under encrypted tier at {}", path.display())]
    PlaintextUnderEncryptedTier {
        /// Offending plaintext path.
        path: PathBuf,
    },
    /// Invalid memory id.
    #[error("invalid memory id: {0}")]
    InvalidMemoryId(String),
    /// Duplicate memory id.
    #[error("duplicate memory id: {0}")]
    DuplicateMemoryId(MemoryId),
    /// Case-folded path collision.
    #[error("case-folded path collision: {0}")]
    CaseFoldCollision(String),
    /// Supersession cycle.
    #[error("supersession cycle involving {0}")]
    SupersessionCycle(MemoryId),
    /// Missing reference.
    #[error("missing reference: {0}")]
    MissingReference(MemoryId),
    /// Other validation error.
    #[error("{0}")]
    Other(String),
}

/// Validation warning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationWarning {
    /// Nullable/collection field was materialized with a default.
    AutoPopulatedNullableField { field: String },
    /// Unknown field preserved.
    UnknownFieldPreserved { field: String },
    /// Partial sync missing reference.
    PartialSyncMissingReference { id: MemoryId },
    /// Partial sync inverse supersession mismatch.
    InverseSupersessionMismatch { message: String },
}

/// ID errors.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IdError {
    /// Device id mismatch.
    #[error("device mismatch")]
    DeviceMismatch,
    /// Sequence exhausted.
    #[error("sequence exhausted for {date}")]
    SequenceExhausted { date: String },
    /// Invalid sequence state.
    #[error("invalid sequence state: {0}")]
    InvalidState(String),
    /// Clock regression: the last allocated date is in the future relative to now.
    ///
    /// This prevents the allocator from silently issuing IDs with a stale date
    /// (e.g. after an NTP correction or timezone misconfiguration). The caller
    /// must wait until the wall clock advances past `last_allocated`, or
    /// intervene by clearing the sequence state.
    #[error("clock regression: last allocated date {last_allocated} is ahead of current date {now}")]
    ClockRegression {
        /// The date stored in the sequence state file.
        last_allocated: String,
        /// The current wall-clock date.
        now: String,
    },
}

/// Vector errors.
#[derive(Debug, thiserror::Error)]
pub enum VectorError {
    /// Dimension mismatch.
    #[error("dimension mismatch: expected {expected}, found {found}")]
    DimensionMismatch {
        /// Active triple's dimension.
        expected: u32,
        /// Vector length supplied by caller.
        found: u32,
    },
    /// Unknown embedding triple.
    #[error("unknown embedding triple: {0:?}")]
    UnknownEmbeddingTriple(EmbeddingTriple),
    /// Stale chunk hash.
    #[error("stale chunk hash: expected {expected}, found {found}")]
    StaleChunk {
        /// Hash supplied by caller.
        expected: Sha256,
        /// Hash currently in the index.
        found: Sha256,
    },
    /// Index handle is unavailable (e.g. poisoned mutex, single-thread channel
    /// closed). Distinct from a real SQLite or storage failure.
    #[error("vector index unavailable: {0}")]
    IndexUnavailable(String),
    /// SQLite operation failed.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// Vector storage failure (legacy stringly-typed variant).
    ///
    /// Deferred: remaining call sites should migrate to the `Sqlite` variant above.
    #[error("vector storage failure: {0}")]
    Storage(String),
}

/// Git errors.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// Repo root is invalid.
    #[error("invalid git repo root: {0}")]
    InvalidRepoRoot(String),
    /// Git command failed.
    #[error("git command failed: {program} {args:?}: {stderr}")]
    CommandFailed { program: String, args: Vec<String>, stderr: String },
    /// Merge driver missing.
    #[error("merge driver missing: {0}")]
    MergeDriverMissing(String),
    /// Push failed.
    #[error("git push failed: {0}")]
    GitPushFailed(String),
    /// IO failed.
    #[error("git io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Watch errors.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    /// Watcher setup failed.
    #[error("watch setup failed: {0}")]
    Setup(String),
    /// `recv_timeout` deadline elapsed; the subscription is still open.
    #[error("watch recv timed out")]
    Timeout,
    /// Subscription closed (channel disconnected).
    #[error("watch subscription closed")]
    Closed,
}

/// Merge errors.
#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    /// Frontmatter delimiters are absent.
    #[error("frontmatter delimiters absent")]
    MissingDelimiters,
    /// Schema is unsupported.
    #[error("schema_version={found} exceeds supported={supported}; upgrade required")]
    UnsupportedSchema { found: u32, supported: u32 },
    /// Parse failed (legacy stringly-typed variant).
    ///
    /// Deferred: callers should switch to `ParseSide` below.
    #[error("merge parse failed: {0}")]
    Parse(String),
    /// Parse failed for a specific side of a three-way merge.
    ///
    /// The underlying YAML parser type is not stable across the workspace
    /// (we use `yaml_serde` today, may swap to `serde_yaml` in Phase 4), so
    /// the source is captured as a typed message rather than a typed error.
    /// The discriminant — [`MergeSide`] — is the load-bearing signal callers
    /// dispatch on.
    #[error("merge parse failed on {side:?}: {message}")]
    ParseSide {
        /// Which side of the merge failed to parse.
        side: MergeSide,
        /// Rendered YAML parse error.
        message: String,
    },
    /// Conflict cannot be represented.
    #[error("unrepresentable conflict: {0}")]
    UnrepresentableConflict(String),
    /// Quarantine retry produced a file that would not validate.
    ///
    /// Spec §14.2 #7: when a clean merge fails validation, the driver retries
    /// with `status: quarantined` + diagnostics. If even that retry will not
    /// validate, the driver exits 1 and surfaces this variant.
    #[error("quarantine retry will not validate: {message}")]
    QuarantineWillNotValidate {
        /// Underlying validation error rendered for stderr/logs.
        message: String,
    },
    /// Serialization or post-merge revalidation failed.
    ///
    /// Distinct from [`MergeError::Parse`] (input-side YAML failure) and from
    /// [`MergeError::ParseSide`] (one-side typed parse error). Carries the
    /// rendered validator error so callers can decide whether to quarantine.
    #[error("merge serialize failed: {message}")]
    Serialize {
        /// Rendered validator/serializer error.
        message: String,
    },
    /// Refusal: a side carries `sensitivity: secret`, which is not persisted.
    ///
    /// Spec §14.4 sensitivity row: `secret` is a runtime
    /// [`crate::model::ClassificationOutcome`] only; the merge driver detects
    /// it via textual prefilter and exits 1 without writing a merged file.
    #[error("merge-driver: secret sensitivity refused")]
    SecretSensitivityRefused {
        /// Side where the disallowed value was found.
        side: MergeSide,
    },
}
