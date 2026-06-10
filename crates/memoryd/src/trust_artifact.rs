use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use memorum_coordination::ClaimLockRegistry;
use memory_privacy::{
    DeterministicPrivacyClassifier, PrivacyClassifier, PrivacyLabel, PrivacyNamespace, PrivacyStorageAction,
};
use memory_source::{ArtifactStore, WebCaptureSourceRef};
use memory_substrate::{MemoryContent, MemoryEnvelope, MemoryId, Substrate};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dynamics::strength::{strength, StrengthFacts};
use crate::dynamics::usage::{distinct_sources_for_conn, UsageSummary};
use crate::dynamics::{load_dynamics_config, DynamicsConfig};
use crate::handlers::memory_ops::serialized_enum_value as enum_value;

const ENCRYPTED_REDACTION: &str = "[encrypted - use memoryd reveal <id> to decrypt]";
const NO_POLICY_VALUE: &str = "not recorded";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SafeContent {
    Plaintext(String),
    Encrypted,
}

impl SafeContent {
    pub fn display_text(&self) -> &str {
        match self {
            Self::Plaintext(value) => value,
            Self::Encrypted => ENCRYPTED_REDACTION,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecallStats {
    pub total: u32,
    pub last_30_days: u32,
    pub last_recalled_at: Option<DateTime<Utc>>,
    /// Use-driven strength in `[0, 1]`, rendered to 2 decimals when dynamics is
    /// enabled (memory-dynamics-v0.1 §3 observability). This is an approximate
    /// single-memory render-time view; exact ranking strength depends on the
    /// active recall candidate pool.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub strength: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProvenanceEvent {
    pub timestamp: DateTime<Utc>,
    pub kind: String,
    pub summary: String,
    pub evidence: String,
    pub device: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub policy_applied: String,
    pub policy_source: String,
    pub confidence_floor_pass: String,
    pub grounding_satisfied: String,
    pub contradiction_result: String,
    pub tombstone_enforced: String,
    pub sensitivity_gate_result: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PrivacyScan {
    pub labels_detected: Vec<String>,
    pub storage_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SupersessionLink {
    pub id: MemoryId,
    pub timestamp: Option<DateTime<Utc>>,
    pub title: SafeContent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncState {
    pub devices: Vec<String>,
    pub merge_status: String,
    pub claim_lock_status: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TrustArtifact {
    pub id: MemoryId,
    pub namespace: String,
    pub status: String,
    pub sensitivity: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_evidence: Option<WebSourceEvidence>,
    pub title: SafeContent,
    pub body: SafeContent,
    pub current_confidence: String,
    pub original_confidence: String,
    pub confidence_reason: Option<String>,
    pub trust_summary: String,
    pub recall: RecallStats,
    pub provenance_chain: Vec<ProvenanceEvent>,
    pub policy_decisions: Vec<PolicyDecision>,
    pub privacy_scan: PrivacyScan,
    pub supersedes: Vec<SupersessionLink>,
    pub superseded_by: Vec<SupersessionLink>,
    pub sync_state: SyncState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebSourceEvidence {
    pub kind: String,
    pub artifact_id: String,
    pub excerpt_id: String,
    pub available: bool,
    pub original_url: Option<String>,
    pub final_url: Option<String>,
    pub captured_at: Option<DateTime<Utc>>,
    pub quote: Option<String>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum TrustArtifactError {
    #[error("memory {0} was not found")]
    MemoryNotFound(MemoryId),
    #[error("read memory {id}: {source}")]
    ReadMemory { id: MemoryId, source: memory_substrate::ReadError },
    #[error("open events-log mirror: {0}")]
    OpenMirror(#[from] rusqlite::Error),
    #[error("query event mirror: {0}")]
    QueryMirror(rusqlite::Error),
    #[error("privacy scan failed: {0}")]
    PrivacyScan(String),
    #[error("git status for memory path: {0}")]
    GitStatus(std::io::Error),
}

pub struct TrustArtifactBuilder<'a> {
    substrate: &'a Substrate,
    now: DateTime<Utc>,
    claim_locks: Option<&'a ClaimLockRegistry>,
}

impl<'a> TrustArtifactBuilder<'a> {
    pub fn new(substrate: &'a Substrate) -> Self {
        Self { substrate, now: Utc::now(), claim_locks: None }
    }

    pub fn with_now(mut self, now: DateTime<Utc>) -> Self {
        self.now = now;
        self
    }

    pub fn with_claim_locks(mut self, claim_locks: &'a ClaimLockRegistry) -> Self {
        self.claim_locks = Some(claim_locks);
        self
    }

    pub async fn build(&self, id: &MemoryId) -> Result<TrustArtifact, TrustArtifactError> {
        let envelope = self
            .substrate
            .read_memory_envelope(id)
            .await
            .map_err(|source| TrustArtifactError::ReadMemory { id: id.clone(), source })?;
        let connection = Connection::open(self.substrate.roots().runtime.join("index.sqlite"))?;
        let (mut artifact, supersedes_ids, superseded_by_ids) = self.assemble(id, envelope, &connection)?;
        drop(connection);
        artifact.supersedes = read_supersession_links(self.substrate, supersedes_ids).await;
        artifact.superseded_by = read_supersession_links(self.substrate, superseded_by_ids).await;
        Ok(artifact)
    }

    fn assemble(
        &self,
        id: &MemoryId,
        envelope: MemoryEnvelope,
        connection: &Connection,
    ) -> Result<(TrustArtifact, Vec<MemoryId>, Vec<MemoryId>), TrustArtifactError> {
        let memory = envelope.metadata;
        let frontmatter = &memory.frontmatter;
        let encrypted = !matches!(envelope.content, MemoryContent::Plaintext(_));
        let title =
            if encrypted { SafeContent::Encrypted } else { SafeContent::Plaintext(frontmatter.summary.clone()) };
        let body = match envelope.content {
            MemoryContent::Plaintext(body) => SafeContent::Plaintext(body),
            MemoryContent::Ciphertext { .. } | MemoryContent::MetadataOnly => SafeContent::Encrypted,
        };

        let source = memory
            .path
            .as_ref()
            .map(|path| path.as_str().to_owned())
            .or_else(|| frontmatter.source.reference.clone())
            .unwrap_or_else(|| frontmatter.source.kind.to_string());
        let source_evidence = web_source_evidence(self.substrate, &frontmatter.source);
        let privacy_scan = build_privacy_scan(&frontmatter.extras, &body, privacy_namespace(&frontmatter.scope))?;
        let dynamics = load_dynamics_config(self.substrate.roots().repo.as_path()).unwrap_or_else(|error| {
            tracing::warn!(%error, "dynamics: failed to load config for trust artifact; using defaults");
            DynamicsConfig::default()
        });
        let supersedes_ids = query_supersession_ids(connection, id, SupersessionDirection::Supersedes)?;
        let superseded_by_ids = query_supersession_ids(connection, id, SupersessionDirection::SupersededBy)?;

        Ok((
            TrustArtifact {
                id: id.clone(),
                namespace: frontmatter.namespace.clone().unwrap_or_else(|| enum_value(&frontmatter.scope)),
                status: enum_value(&frontmatter.status),
                sensitivity: enum_value(&frontmatter.sensitivity),
                source,
                source_evidence,
                title,
                body,
                current_confidence: format_confidence(frontmatter.confidence),
                original_confidence: frontmatter
                    .original_confidence
                    .or_else(|| query_original_confidence(connection, id).ok().flatten())
                    .map(format_confidence)
                    .unwrap_or_else(|| "not recorded".to_owned()),
                confidence_reason: string_extra(&frontmatter.extras, "confidence_reason"),
                trust_summary: format!(
                    "{} / {}",
                    enum_value(&frontmatter.trust_level),
                    frontmatter.write_policy.policy_applied
                ),
                recall: query_recall_stats(connection, id, self.now, &dynamics)?,
                provenance_chain: query_provenance(connection, id)?,
                policy_decisions: query_policy_decisions(connection, id)?,
                privacy_scan,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                sync_state: SyncState {
                    devices: query_distinct_devices(connection, id)?,
                    merge_status: memory.path.as_ref().map_or_else(
                        || Ok("unknown".to_owned()),
                        |path| memory_path_git_status(self.substrate, path.as_str()),
                    )?,
                    claim_lock_status: self.claim_lock_status(id),
                },
            },
            supersedes_ids,
            superseded_by_ids,
        ))
    }

    fn claim_lock_status(&self, id: &MemoryId) -> Option<String> {
        let lock = self.claim_locks?.get(id.as_str())?;
        Some(format!(
            "held by {}:{} until {}",
            lock.holder_harness,
            lock.holder_session_id,
            lock.expires_at.to_rfc3339()
        ))
    }
}

fn query_recall_stats(
    connection: &Connection,
    id: &MemoryId,
    now: DateTime<Utc>,
    dynamics: &DynamicsConfig,
) -> Result<RecallStats, TrustArtifactError> {
    let cutoff = (now - Duration::days(30)).to_rfc3339();
    let (total, last_30_days, last_recalled_at): (i64, i64, Option<String>) = connection
        .query_row(
            "SELECT
                COUNT(*),
                SUM(CASE WHEN ts > ?2 THEN 1 ELSE 0 END),
                MAX(ts)
             FROM events_log
             WHERE kind = 'recall_hit' AND memory_id = ?1",
            (id.as_str(), cutoff),
            |row| Ok((row.get(0)?, row.get::<_, Option<i64>>(1)?.unwrap_or(0), row.get(2)?)),
        )
        .map_err(TrustArtifactError::QueryMirror)?;

    let last_recalled = last_recalled_at.as_deref().and_then(parse_time);
    let strength = render_strength(
        connection,
        StrengthRenderInput {
            id,
            usage: UsageSummary { count: last_30_days as u32, last_recalled_at: last_recalled },
            now,
            dynamics,
        },
    );

    Ok(RecallStats {
        total: total as u32,
        last_30_days: last_30_days as u32,
        last_recalled_at: last_recalled,
        strength,
    })
}

/// Render the use-driven strength (memory-dynamics-v0.1 §3) for one memory.
///
/// Single-memory pool: the `freq_norm` denominator is the memory's own
/// 30-day count, so frequency saturates to `1` whenever it has any recalls
/// (spec §2 single-memory-pool boundary). Corroboration reads the same
/// supersession-chain distinct-source query the ranking path uses. This is still
/// approximate because exact ranking strength normalizes frequency over the
/// active recall candidate pool and anchors recency to ranking time, not artifact
/// render time. On a query error this falls back to `"not recorded"` rather than
/// failing the artifact.
struct StrengthRenderInput<'a> {
    id: &'a MemoryId,
    usage: UsageSummary,
    now: DateTime<Utc>,
    dynamics: &'a DynamicsConfig,
}

fn render_strength(connection: &Connection, input: StrengthRenderInput<'_>) -> String {
    let StrengthRenderInput { id, usage, now, dynamics } = input;
    if !dynamics.enabled {
        return String::new();
    }
    let distinct_sources = match distinct_sources_for_conn(connection, &[id.as_str()]) {
        Ok(map) => map.get(id.as_str()).copied().unwrap_or(0),
        Err(_) => return "not recorded".to_owned(),
    };
    let facts = StrengthFacts {
        recall_count_30d: usage.count,
        last_recalled_at: usage.last_recalled_at,
        max_recall_30d_active: usage.count,
        distinct_sources,
    };
    let value = strength(facts, dynamics.weights, dynamics.tau_days, now);
    format!("{value:.2} (approximate; computed at render time over this memory alone)")
}

fn web_source_evidence(substrate: &Substrate, source: &memory_substrate::Source) -> Option<WebSourceEvidence> {
    if source.kind != memory_substrate::SourceKind::Web {
        return None;
    }
    let source_ref = source.reference.as_deref()?;
    let parsed = match WebCaptureSourceRef::parse(source_ref) {
        Ok(parsed) => parsed,
        Err(error) => {
            return Some(WebSourceEvidence {
                kind: "web".to_string(),
                artifact_id: String::new(),
                excerpt_id: String::new(),
                available: false,
                original_url: None,
                final_url: None,
                captured_at: None,
                quote: None,
                unavailable_reason: Some(error.to_string()),
            });
        }
    };
    let artifact_id = parsed.artifact_id().to_string();
    let excerpt_id = parsed.excerpt_id().to_string();
    match ArtifactStore::new(substrate.roots().repo.clone()).verify_artifact_id(parsed.artifact_id()) {
        Ok(artifact) => {
            let quote = artifact
                .excerpts
                .iter()
                .find(|record| record.excerpt_id == excerpt_id)
                .map(|record| bounded(&record.quote, 500));
            Some(WebSourceEvidence {
                kind: "web".to_string(),
                artifact_id,
                excerpt_id,
                available: quote.is_some(),
                original_url: Some(artifact.manifest.original_url),
                final_url: Some(artifact.manifest.final_url),
                captured_at: Some(artifact.manifest.captured_at),
                quote,
                unavailable_reason: None,
            })
        }
        Err(error) => Some(WebSourceEvidence {
            kind: "web".to_string(),
            artifact_id,
            excerpt_id,
            available: false,
            original_url: None,
            final_url: None,
            captured_at: None,
            quote: None,
            unavailable_reason: Some(error.to_string()),
        }),
    }
}

fn query_original_confidence(connection: &Connection, id: &MemoryId) -> Result<Option<f64>, TrustArtifactError> {
    connection
        .query_row("SELECT original_confidence FROM memories WHERE id = ?1", [id.as_str()], |row| row.get(0))
        .map_err(TrustArtifactError::QueryMirror)
}

fn query_provenance(connection: &Connection, id: &MemoryId) -> Result<Vec<ProvenanceEvent>, TrustArtifactError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT ts, kind, device, payload_json
             FROM events_log
             WHERE memory_id = ?1
             ORDER BY ts ASC, device ASC, seq ASC",
        )
        .map_err(TrustArtifactError::QueryMirror)?;
    let rows = statement
        .query_map([id.as_str()], |row| {
            let timestamp: String = row.get(0)?;
            let kind: String = row.get(1)?;
            let device: String = row.get(2)?;
            let payload_json: String = row.get(3)?;
            let timestamp = parse_time(&timestamp).unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
            let payload = serde_json::from_str::<Value>(&payload_json).unwrap_or(Value::Null);
            Ok(ProvenanceEvent {
                timestamp,
                kind: kind.clone(),
                summary: summarize_event(&kind, &payload),
                evidence: event_evidence(&payload),
                device,
            })
        })
        .map_err(TrustArtifactError::QueryMirror)?;

    let mut events = Vec::new();
    for row in rows {
        events.push(row.map_err(TrustArtifactError::QueryMirror)?);
    }
    events.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.device.cmp(&right.device))
            .then_with(|| left.kind.cmp(&right.kind))
    });
    Ok(events)
}

fn query_distinct_devices(connection: &Connection, id: &MemoryId) -> Result<Vec<String>, TrustArtifactError> {
    let mut statement = connection
        .prepare_cached("SELECT DISTINCT device FROM events_log WHERE memory_id = ?1 ORDER BY device")
        .map_err(TrustArtifactError::QueryMirror)?;
    let rows =
        statement.query_map([id.as_str()], |row| row.get::<_, String>(0)).map_err(TrustArtifactError::QueryMirror)?;

    let mut devices = Vec::new();
    for row in rows {
        devices.push(row.map_err(TrustArtifactError::QueryMirror)?);
    }
    Ok(devices)
}

#[derive(Clone, Copy)]
enum SupersessionDirection {
    Supersedes,
    SupersededBy,
}

fn query_supersession_ids(
    connection: &Connection,
    id: &MemoryId,
    direction: SupersessionDirection,
) -> Result<Vec<MemoryId>, TrustArtifactError> {
    let sql = match direction {
        SupersessionDirection::Supersedes => {
            "SELECT supersedes_id FROM memory_supersession WHERE memory_id = ?1 ORDER BY supersedes_id"
        }
        SupersessionDirection::SupersededBy => {
            "SELECT memory_id FROM memory_supersession WHERE supersedes_id = ?1 ORDER BY memory_id"
        }
    };
    let mut statement = connection.prepare_cached(sql).map_err(TrustArtifactError::QueryMirror)?;
    let rows =
        statement.query_map([id.as_str()], |row| row.get::<_, String>(0)).map_err(TrustArtifactError::QueryMirror)?;

    let mut ids = Vec::new();
    for row in rows {
        let linked_id = MemoryId::try_new(row.map_err(TrustArtifactError::QueryMirror)?)
            .map_err(|_| TrustArtifactError::MemoryNotFound(id.clone()))?;
        ids.push(linked_id);
    }
    Ok(ids)
}

async fn read_supersession_links(substrate: &Substrate, ids: Vec<MemoryId>) -> Vec<SupersessionLink> {
    let mut links = Vec::with_capacity(ids.len());
    for id in ids {
        links.push(read_supersession_link(substrate, id).await);
    }
    links
}

async fn read_supersession_link(substrate: &Substrate, id: MemoryId) -> SupersessionLink {
    let memory = substrate.read_memory_envelope(&id).await.ok();
    let (timestamp, title) = match memory {
        Some(envelope) if matches!(envelope.content, MemoryContent::Plaintext(_)) => (
            Some(envelope.metadata.frontmatter.created_at),
            SafeContent::Plaintext(envelope.metadata.frontmatter.summary),
        ),
        Some(envelope) => (Some(envelope.metadata.frontmatter.created_at), SafeContent::Encrypted),
        None => (None, SafeContent::Plaintext("unavailable".to_owned())),
    };
    SupersessionLink { id, timestamp, title }
}

fn query_policy_decisions(connection: &Connection, id: &MemoryId) -> Result<Vec<PolicyDecision>, TrustArtifactError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT payload_json
             FROM events_log
             WHERE memory_id = ?1 AND kind = 'governance_decision'
             ORDER BY ts ASC, device ASC, seq ASC",
        )
        .map_err(TrustArtifactError::QueryMirror)?;
    let rows = statement
        .query_map([id.as_str()], |row| {
            let payload_json: String = row.get(0)?;
            Ok(serde_json::from_str::<Value>(&payload_json).unwrap_or(Value::Null))
        })
        .map_err(TrustArtifactError::QueryMirror)?;

    let mut decisions = Vec::new();
    for row in rows {
        decisions.push(policy_from_value(&row.map_err(TrustArtifactError::QueryMirror)?));
    }
    Ok(decisions)
}

fn policy_from_value(value: &Value) -> PolicyDecision {
    value.as_object().map(policy_from_object).unwrap_or_else(|| policy_from_object(&serde_json::Map::new()))
}

fn policy_from_object(object: &serde_json::Map<String, Value>) -> PolicyDecision {
    PolicyDecision {
        policy_applied: object_string(object, "policy_applied").unwrap_or_else(|| NO_POLICY_VALUE.to_owned()),
        policy_source: object_string(object, "policy_source").unwrap_or_else(|| NO_POLICY_VALUE.to_owned()),
        confidence_floor_pass: object_string(object, "confidence_floor_pass")
            .unwrap_or_else(|| NO_POLICY_VALUE.to_owned()),
        grounding_satisfied: object_string(object, "grounding_satisfied").unwrap_or_else(|| NO_POLICY_VALUE.to_owned()),
        contradiction_result: object_string(object, "contradiction_result")
            .unwrap_or_else(|| NO_POLICY_VALUE.to_owned()),
        tombstone_enforced: object_string(object, "tombstone_enforced").unwrap_or_else(|| NO_POLICY_VALUE.to_owned()),
        sensitivity_gate_result: object_string(object, "sensitivity_gate_result")
            .unwrap_or_else(|| NO_POLICY_VALUE.to_owned()),
    }
}

fn build_privacy_scan(
    extras: &BTreeMap<String, Value>,
    body: &SafeContent,
    namespace: PrivacyNamespace,
) -> Result<PrivacyScan, TrustArtifactError> {
    let Some(scan) = extras.get("privacy_scan").and_then(Value::as_object) else {
        return match body {
            SafeContent::Plaintext(body) => classify_privacy_scan(body, namespace),
            SafeContent::Encrypted => Ok(PrivacyScan {
                labels_detected: vec!["not available without reveal".to_owned()],
                storage_action: "encrypted".to_owned(),
            }),
        };
    };

    let labels_detected = scan
        .get("labels_detected")
        .and_then(Value::as_array)
        .map(|labels| labels.iter().filter_map(value_to_string).collect())
        .unwrap_or_else(|| vec!["none".to_owned()]);
    let storage_action = match body {
        SafeContent::Plaintext(_) => object_string(scan, "storage_action").unwrap_or_else(|| "plaintext".to_owned()),
        SafeContent::Encrypted => "encrypted".to_owned(),
    };
    Ok(PrivacyScan { labels_detected, storage_action })
}

fn classify_privacy_scan(body: &str, namespace: PrivacyNamespace) -> Result<PrivacyScan, TrustArtifactError> {
    let decision = DeterministicPrivacyClassifier::new()
        .classify(body, namespace, None)
        .map_err(|error| TrustArtifactError::PrivacyScan(error.to_string()))?;
    let mut labels = decision.scan.labels.into_iter().map(privacy_label_name).collect::<Vec<_>>();
    labels.sort();
    labels.dedup();
    if labels.is_empty() {
        labels.push("none".to_owned());
    }
    Ok(PrivacyScan { labels_detected: labels, storage_action: storage_action_name(decision.storage_action).to_owned() })
}

fn memory_path_git_status(substrate: &Substrate, repo_path: &str) -> Result<String, TrustArtifactError> {
    if memory_substrate::RepoPath::try_new(repo_path.to_owned()).is_err() || repo_path.starts_with(':') {
        return Ok("unknown".to_owned());
    }
    let output = std::process::Command::new(git_binary())
        .arg("-C")
        .arg(&substrate.roots().repo)
        .arg("--literal-pathspecs")
        .arg("status")
        .arg("--porcelain")
        .arg("--")
        .arg(repo_path)
        .output()
        .map_err(TrustArtifactError::GitStatus)?;
    if output.status.success() && output.stdout.is_empty() {
        Ok("clean".to_owned())
    } else if output.status.success() {
        Ok("modified".to_owned())
    } else {
        Ok("unknown".to_owned())
    }
}

fn git_binary() -> &'static str {
    if std::path::Path::new("/usr/bin/git").exists() {
        "/usr/bin/git"
    } else {
        "git"
    }
}

fn summarize_event(kind: &str, payload: &Value) -> String {
    match kind {
        "write_committed" => "written as plaintext memory".to_owned(),
        "encrypted_write_committed" => "written as encrypted memory".to_owned(),
        "recall_hit" => "recalled in response".to_owned(),
        "reality_check_confirmed" => "user confirmed in Reality Check".to_owned(),
        "reality_check_forgotten" => "user forgot in Reality Check".to_owned(),
        "reality_check_not_relevant" => "user marked not relevant in Reality Check".to_owned(),
        "claim_lock_contention" => "claim-lock contention warning".to_owned(),
        other => payload.get("kind").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| other.to_owned()),
    }
}

fn event_evidence(payload: &Value) -> String {
    payload
        .get("operation_id")
        .and_then(Value::as_str)
        .map(|operation| format!("operation:{operation}"))
        .unwrap_or_else(|| "event-log".to_owned())
}

fn format_confidence(value: f64) -> String {
    format!("{value:.2}")
}

fn string_extra(extras: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    extras.get(key).and_then(value_to_string)
}

fn object_string(object: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(value_to_string)
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok().map(|time| time.with_timezone(&Utc))
}

fn privacy_namespace(scope: &memory_substrate::Scope) -> PrivacyNamespace {
    match scope {
        memory_substrate::Scope::User => PrivacyNamespace::Me,
        memory_substrate::Scope::Project | memory_substrate::Scope::Org => PrivacyNamespace::Project,
        memory_substrate::Scope::Agent | memory_substrate::Scope::Subagent => PrivacyNamespace::Agent,
    }
}

fn privacy_label_name(label: PrivacyLabel) -> String {
    enum_value(&label)
}

fn storage_action_name(action: PrivacyStorageAction) -> &'static str {
    match action {
        PrivacyStorageAction::Plaintext => "plaintext",
        PrivacyStorageAction::EncryptAtRest => "encrypted",
        PrivacyStorageAction::Refuse => "refuse",
    }
}

fn bounded(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    let mut end = max_chars;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}
