//! Versioned policy loading and dry-run previews for governance candidates.

use std::{collections::BTreeMap, fs, path::Path};

use serde::{Deserialize, Deserializer, Serialize};

/// Result alias for policy operations.
pub type PolicyResult<T> = Result<T, PolicyError>;

/// Fail-closed policy loading and selection errors.
#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    /// The policy directory could not be read.
    #[error("failed to read policy directory {path}: {source}")]
    ReadDir { path: String, source: std::io::Error },
    /// A policy file could not be read.
    #[error("failed to read policy file {path}: {source}")]
    ReadFile { path: String, source: std::io::Error },
    /// Policy YAML was invalid or violated the schema.
    #[error("invalid policy YAML in {path}: {source}")]
    InvalidYaml { path: String, source: serde_yaml::Error },
    /// No policy exists for a required scope.
    #[error("missing governance policy for scope {scope:?}")]
    MissingPolicyForScope { scope: Scope },
    /// No policy exists for a required name.
    #[error("missing governance policy named {name}")]
    MissingPolicyNamed { name: String },
    /// More than one policy exists for a required scope.
    #[error("duplicate governance policies for scope {scope:?}")]
    DuplicatePolicyScope { scope: Scope },
    /// More than one policy exists with the same name.
    #[error("duplicate governance policy named {name}")]
    DuplicatePolicyName { name: String },
}

/// Candidate scope used for policy selection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// Human author policy scope.
    Me,
    /// Project memory policy scope.
    Project,
    /// Agent-authored memory policy scope.
    Agent,
    /// Dreaming/scratch synthesis policy scope.
    Dreaming,
}

/// Whether a policy came from disk or from built-in fallback defaults.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicySource {
    /// Policy was loaded from a filesystem policy file.
    Disk,
    /// Policy came from the compiled fallback set.
    BuiltInFallback,
}

/// Tombstone enforcement behavior selected by policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TombstoneEnforcementMode {
    /// Refuse candidates matching tombstones.
    Refuse,
    /// Route tombstone matches to review.
    Review,
}

/// Contradiction behavior selected by policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContradictionPolicy {
    /// Route contradictions to a supersession chain.
    Supersede,
    /// Quarantine contradictions for review.
    Quarantine,
}

/// Policy schema loaded from YAML.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    name: String,
    version: u32,
    scope: Scope,
    #[serde(deserialize_with = "deserialize_confidence_floor")]
    confidence_floor: f32,
    requires_grounding: bool,
    tombstone_enforcement: TombstoneEnforcementMode,
    contradiction_policy: ContradictionPolicy,
    #[serde(default)]
    review_gates: Vec<String>,
    #[serde(skip, default = "default_policy_source")]
    source: PolicySource,
}

/// Policies keyed by scope and name.
#[derive(Clone, Debug, PartialEq)]
pub struct PolicySet {
    by_scope: BTreeMap<Scope, Policy>,
    by_name: BTreeMap<String, Policy>,
}

/// Candidate facts needed for deterministic policy selection and preview.
#[derive(Clone, Debug, PartialEq)]
pub struct CandidateContext {
    scope: Scope,
    confidence: f32,
    has_grounding: bool,
}

/// Deterministic dry-run preview. It records policy source for production auditability.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PolicyPreview {
    /// Selected policy marker, for example `agent-strict@v3`.
    pub selected_policy: String,
    /// Whether the selected policy came from disk or built-in fallback.
    pub policy_source: PolicySource,
    /// Candidate confidence supplied to the dry-run.
    pub confidence: f32,
    /// Policy confidence floor.
    pub confidence_floor: f32,
    /// Whether candidate confidence meets the floor.
    pub confidence_floor_passed: bool,
    /// Review gates triggered by candidate facts.
    pub triggered_review_gates: Vec<String>,
    /// Whether grounding is required for this policy.
    pub requires_grounding: bool,
    /// Whether grounding evidence is currently present.
    pub grounding_satisfied: bool,
    /// Tombstone handling selected by policy.
    pub tombstone_enforcement: TombstoneEnforcementMode,
}

struct BuiltInPolicySpec<'a> {
    name: &'a str,
    version: u32,
    scope: Scope,
    confidence_floor: f32,
    requires_grounding: bool,
    tombstone_enforcement: TombstoneEnforcementMode,
    contradiction_policy: ContradictionPolicy,
    review_gates: &'a [&'a str],
}

impl PolicySet {
    /// Load every `.yaml` policy in a directory. Missing, unreadable, or invalid policies fail closed.
    pub fn load_from_dir(path: &Path) -> PolicyResult<Self> {
        let entries = fs::read_dir(path).map_err(|source| PolicyError::ReadDir { path: display_path(path), source })?;
        let mut policies = Vec::new();

        for entry in entries {
            let entry = entry.map_err(|source| PolicyError::ReadDir { path: display_path(path), source })?;
            let file_path = entry.path();
            if file_path.extension().is_some_and(|extension| extension == "yaml") {
                policies.push(read_policy_file(&file_path)?);
            }
        }

        Self::from_policies(policies)
    }

    /// Return compiled bootstrap policies. Runtime callers can distinguish this fallback by source.
    pub fn builtin() -> Self {
        Self::from_policies(vec![
            Policy::builtin(BuiltInPolicySpec {
                name: "me-strict",
                version: 1,
                scope: Scope::Me,
                confidence_floor: 0.9,
                requires_grounding: true,
                tombstone_enforcement: TombstoneEnforcementMode::Refuse,
                contradiction_policy: ContradictionPolicy::Quarantine,
                review_gates: &["low_confidence", "missing_grounding"],
            }),
            Policy::builtin(BuiltInPolicySpec {
                name: "project-standard",
                version: 2,
                scope: Scope::Project,
                confidence_floor: 0.7,
                requires_grounding: true,
                tombstone_enforcement: TombstoneEnforcementMode::Review,
                contradiction_policy: ContradictionPolicy::Supersede,
                review_gates: &["low_confidence"],
            }),
            Policy::builtin(BuiltInPolicySpec {
                name: "agent-strict",
                version: 3,
                scope: Scope::Agent,
                confidence_floor: 0.82,
                requires_grounding: true,
                tombstone_enforcement: TombstoneEnforcementMode::Refuse,
                contradiction_policy: ContradictionPolicy::Quarantine,
                review_gates: &["low_confidence", "missing_grounding"],
            }),
            Policy::builtin(BuiltInPolicySpec {
                name: "dreaming-strict",
                version: 1,
                scope: Scope::Dreaming,
                confidence_floor: 0.95,
                requires_grounding: true,
                tombstone_enforcement: TombstoneEnforcementMode::Refuse,
                contradiction_policy: ContradictionPolicy::Quarantine,
                review_gates: &["low_confidence", "missing_grounding", "dream_source"],
            }),
        ])
        .expect("built-in policies are statically valid")
    }

    /// Resolve the policy for a candidate.
    pub fn policy_for_candidate(&self, context: &CandidateContext) -> PolicyResult<&Policy> {
        self.policy_for_scope(context.scope)
    }

    /// Resolve a policy by scope.
    pub fn policy_for_scope(&self, scope: Scope) -> PolicyResult<&Policy> {
        self.by_scope.get(&scope).ok_or(PolicyError::MissingPolicyForScope { scope })
    }

    /// Resolve a policy by name.
    pub fn policy_named(&self, name: &str) -> PolicyResult<&Policy> {
        self.by_name.get(name).ok_or_else(|| PolicyError::MissingPolicyNamed { name: name.to_owned() })
    }

    fn from_policies(policies: Vec<Policy>) -> PolicyResult<Self> {
        let mut by_scope = BTreeMap::new();
        let mut by_name = BTreeMap::new();

        for policy in &policies {
            if by_name.contains_key(&policy.name) {
                return Err(PolicyError::DuplicatePolicyName { name: policy.name.clone() });
            }
            by_name.insert(policy.name.clone(), policy.clone());
        }

        for policy in policies {
            if by_scope.contains_key(&policy.scope) {
                return Err(PolicyError::DuplicatePolicyScope { scope: policy.scope });
            }

            by_scope.insert(policy.scope, policy);
        }

        for scope in [Scope::Me, Scope::Project, Scope::Agent, Scope::Dreaming] {
            if !by_scope.contains_key(&scope) {
                return Err(PolicyError::MissingPolicyForScope { scope });
            }
        }

        Ok(Self { by_scope, by_name })
    }
}

impl Policy {
    /// Stable marker included in governance decisions.
    pub fn policy_applied(&self) -> String {
        format!("{}@v{}", self.name, self.version)
    }

    /// Preview deterministic policy effects without mutating the substrate.
    pub fn dry_run(&self, context: &CandidateContext) -> PolicyPreview {
        let confidence_floor_passed = context.confidence >= self.confidence_floor;
        let grounding_satisfied = !self.requires_grounding || context.has_grounding;

        PolicyPreview {
            selected_policy: self.policy_applied(),
            policy_source: self.source,
            confidence: context.confidence,
            confidence_floor: self.confidence_floor,
            confidence_floor_passed,
            triggered_review_gates: self.triggered_review_gates(confidence_floor_passed, grounding_satisfied),
            requires_grounding: self.requires_grounding,
            grounding_satisfied,
            tombstone_enforcement: self.tombstone_enforcement,
        }
    }

    /// Policy name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Policy source.
    pub fn source(&self) -> PolicySource {
        self.source
    }

    /// Contradiction behavior selected by this policy.
    pub fn contradiction_policy(&self) -> ContradictionPolicy {
        self.contradiction_policy
    }

    /// Tombstone behavior selected by this policy.
    pub fn tombstone_enforcement(&self) -> TombstoneEnforcementMode {
        self.tombstone_enforcement
    }

    fn builtin(spec: BuiltInPolicySpec<'_>) -> Self {
        Self {
            name: spec.name.to_owned(),
            version: spec.version,
            scope: spec.scope,
            confidence_floor: spec.confidence_floor,
            requires_grounding: spec.requires_grounding,
            tombstone_enforcement: spec.tombstone_enforcement,
            contradiction_policy: spec.contradiction_policy,
            review_gates: spec.review_gates.iter().map(ToString::to_string).collect(),
            source: PolicySource::BuiltInFallback,
        }
    }

    fn triggered_review_gates(&self, confidence_floor_passed: bool, grounding_satisfied: bool) -> Vec<String> {
        self.review_gates
            .iter()
            .filter(|gate| should_trigger_gate(gate, confidence_floor_passed, grounding_satisfied))
            .cloned()
            .collect()
    }
}

impl CandidateContext {
    /// Create a candidate context for a scope.
    pub fn new(scope: Scope) -> Self {
        Self { scope, confidence: 1.0, has_grounding: false }
    }

    /// Attach a confidence score.
    #[must_use]
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    /// Attach whether grounding evidence is present.
    #[must_use]
    pub fn with_grounding(mut self, has_grounding: bool) -> Self {
        self.has_grounding = has_grounding;
        self
    }
}

fn read_policy_file(path: &Path) -> PolicyResult<Policy> {
    let yaml = fs::read_to_string(path).map_err(|source| PolicyError::ReadFile { path: display_path(path), source })?;
    let mut policy = serde_yaml::from_str::<Policy>(&yaml)
        .map_err(|source| PolicyError::InvalidYaml { path: display_path(path), source })?;
    policy.source = PolicySource::Disk;
    Ok(policy)
}

fn deserialize_confidence_floor<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: Deserializer<'de>,
{
    let confidence_floor = f32::deserialize(deserializer)?;
    if (0.0..=1.0).contains(&confidence_floor) {
        Ok(confidence_floor)
    } else {
        Err(serde::de::Error::custom("confidence_floor must be between 0.0 and 1.0"))
    }
}

fn should_trigger_gate(gate: &str, confidence_floor_passed: bool, grounding_satisfied: bool) -> bool {
    matches!(
        (gate, confidence_floor_passed, grounding_satisfied),
        ("low_confidence", false, _) | ("missing_grounding", _, false) | ("dream_source", _, _)
    )
}

fn default_policy_source() -> PolicySource {
    PolicySource::Disk
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}
