//! Deterministic tombstone rule parsing and candidate matching.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::hash::{canonical_claim_hash, canonical_text, stable_hash};
use crate::{GovernanceDecision, GovernanceRefusalReason, NextAction};
use serde::{Deserialize, Serialize};

/// Memory id string targeted by a tombstone rule.
pub type MemoryId = String;

/// Candidate key used for tombstone matching.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateTombstoneKey {
    /// Optional exact memory id match.
    pub target_memory_id: Option<MemoryId>,
    /// Canonical claim hash.
    pub content_hash: String,
    /// Canonical entity set hash.
    pub entity_hash: String,
}

impl CandidateTombstoneKey {
    /// Build a candidate key from claim text and entity identifiers.
    pub fn from_claim<I, S>(claim: &str, entities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self {
            target_memory_id: None,
            content_hash: canonical_claim_hash(claim),
            entity_hash: CanonicalEntities::from(entities).entity_hash(),
        }
    }

    /// Attach a target memory id to the candidate key.
    #[must_use]
    pub fn with_target_memory_id(mut self, memory_id: impl Into<MemoryId>) -> Self {
        self.target_memory_id = Some(memory_id.into());
        self
    }
}

/// Canonicalized, order-insensitive entity set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalEntities {
    entities: BTreeSet<String>,
}

impl CanonicalEntities {
    /// Return the stable hash for this canonical entity set.
    pub fn entity_hash(&self) -> String {
        stable_hash(&self.entities.iter().cloned().collect::<Vec<_>>().join("\n"))
    }
}

impl<I, S> From<I> for CanonicalEntities
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    fn from(entities: I) -> Self {
        let entities = entities
            .into_iter()
            .map(|entity| canonical_text(entity.as_ref()))
            .filter(|entity| !entity.is_empty())
            .collect();

        Self { entities }
    }
}

/// v0.1 tombstone rule loaded from JSONL.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TombstoneRule {
    /// Stable tombstone id.
    pub id: String,
    /// Optional exact memory id blocked by this rule.
    pub target_memory_id: Option<MemoryId>,
    /// Canonical content hash blocked by this rule.
    pub content_hash: String,
    /// Canonical entity-set hash blocked by this rule.
    pub entity_hash: String,
    /// Tombstone reason category.
    pub reason: TombstoneKind,
    /// Optional operator-facing reason text.
    pub reason_text: Option<String>,
    /// Inactive tombstones are ignored during matching.
    pub active: bool,
}

/// Tombstone reason categories.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TombstoneKind {
    /// User requested forgetting this memory or claim.
    UserForget,
    /// Policy retention rule tombstoned the memory or claim.
    PolicyRetention,
    /// Operator or reviewer tombstoned the memory or claim.
    OperatorRemoval,
}

/// Tombstone reference returned with a match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TombstoneRef {
    /// Matched tombstone id.
    pub id: String,
    /// Matched tombstone reason category.
    pub reason: TombstoneKind,
    /// Optional operator-facing reason text.
    pub reason_text: Option<String>,
}

/// Match result carrying both the governance decision and tombstone details.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TombstoneMatch {
    /// Fail-closed refusal decision for the candidate.
    pub decision: GovernanceDecision,
    /// Details of the matched tombstone rule.
    pub tombstone_ref: TombstoneRef,
}

/// Deterministic in-memory tombstone index.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TombstoneIndex {
    rules: Vec<TombstoneRule>,
}

impl TombstoneIndex {
    /// Load all `.jsonl` tombstone rule files in a directory.
    pub fn load_jsonl_dir(path: impl AsRef<Path>) -> Result<Self, TombstoneLoadError> {
        let mut jsonl_paths = jsonl_paths(path.as_ref())?;
        jsonl_paths.sort();

        let mut rules = Vec::new();
        for jsonl_path in jsonl_paths {
            rules.extend(load_jsonl_file(&jsonl_path)?);
        }

        Ok(Self { rules })
    }

    /// Match a candidate against active tombstone rules.
    pub fn match_candidate(&self, candidate: &CandidateTombstoneKey) -> Option<TombstoneMatch> {
        self.rules.iter().find(|rule| rule.matches(candidate)).map(TombstoneMatch::from_rule)
    }

    /// Build the fail-closed decision used when tombstone loading failed.
    pub fn fail_closed_decision(_error: &TombstoneLoadError) -> GovernanceDecision {
        GovernanceDecision::Refused {
            reason: GovernanceRefusalReason::Tombstone,
            message: "tombstone index failed to load; refusing candidate".to_owned(),
            next_action: NextAction::NoWrite,
        }
    }
}

impl TombstoneRule {
    fn matches(&self, candidate: &CandidateTombstoneKey) -> bool {
        self.active
            && (self.matches_target_memory_id(candidate)
                || (self.content_hash == candidate.content_hash && self.entity_hash == candidate.entity_hash))
    }

    fn matches_target_memory_id(&self, candidate: &CandidateTombstoneKey) -> bool {
        self.target_memory_id.is_some() && self.target_memory_id == candidate.target_memory_id
    }
}

impl TombstoneMatch {
    fn from_rule(rule: &TombstoneRule) -> Self {
        Self {
            decision: GovernanceDecision::Refused {
                reason: GovernanceRefusalReason::Tombstone,
                message: format!("candidate matches tombstone {}", rule.id),
                next_action: NextAction::NoWrite,
            },
            tombstone_ref: TombstoneRef {
                id: rule.id.clone(),
                reason: rule.reason,
                reason_text: rule.reason_text.clone(),
            },
        }
    }
}

/// Typed tombstone loading errors.
#[derive(Debug, thiserror::Error)]
pub enum TombstoneLoadError {
    /// The tombstone directory could not be read.
    #[error("failed to read tombstone directory {path}: {source}")]
    ReadDir {
        /// Directory path.
        path: PathBuf,
        /// I/O source error.
        source: std::io::Error,
    },
    /// A tombstone file could not be read.
    #[error("failed to read tombstone file {path}: {source}")]
    ReadFile {
        /// File path.
        path: PathBuf,
        /// I/O source error.
        source: std::io::Error,
    },
    /// A JSONL line could not be parsed as a tombstone rule.
    #[error("malformed tombstone JSONL in {path} at line {line}: {source}")]
    MalformedJsonl {
        /// File path.
        path: PathBuf,
        /// 1-based JSONL line number.
        line: usize,
        /// JSON source error.
        source: serde_json::Error,
    },
}

fn load_jsonl_file(path: &Path) -> Result<Vec<TombstoneRule>, TombstoneLoadError> {
    let content =
        fs::read_to_string(path).map_err(|source| TombstoneLoadError::ReadFile { path: path.to_path_buf(), source })?;

    content
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| parse_rule_line(path, index + 1, line))
        .collect()
}

fn parse_rule_line(path: &Path, line_number: usize, line: &str) -> Result<TombstoneRule, TombstoneLoadError> {
    serde_json::from_str(line).map_err(|source| TombstoneLoadError::MalformedJsonl {
        path: path.to_path_buf(),
        line: line_number,
        source,
    })
}

fn jsonl_paths(path: &Path) -> Result<Vec<PathBuf>, TombstoneLoadError> {
    let entries =
        fs::read_dir(path).map_err(|source| TombstoneLoadError::ReadDir { path: path.to_path_buf(), source })?;

    entries
        .map(|entry| entry.map(|entry| entry.path()))
        .filter_map(|entry| match entry {
            Ok(path) if path.extension().is_some_and(|extension| extension == "jsonl") => Some(Ok(path)),
            Ok(_) => None,
            Err(source) => Some(Err(TombstoneLoadError::ReadDir { path: path.to_path_buf(), source })),
        })
        .collect()
}
