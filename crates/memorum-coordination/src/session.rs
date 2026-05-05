use std::collections::{HashMap, HashSet};
use std::path::Path;

use memory_substrate::EmbeddingTriple;
use serde::{Deserialize, Serialize};

/// Per-project override for Stream I coordination level.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConcurrentSessionMode {
    Minimal,
    Default,
    Collaborative,
}

impl ConcurrentSessionMode {
    pub fn project_value(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Default => "default",
            Self::Collaborative => "collaborative",
        }
    }

    pub fn from_project_value(value: &str) -> Option<Self> {
        match value.trim() {
            "minimal" => Some(Self::Minimal),
            "default" => Some(Self::Default),
            "collaborative" => Some(Self::Collaborative),
            _ => None,
        }
    }
}

/// Project binding data needed by coordination without depending on memoryd.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectBinding {
    pub canonical_id: String,
    pub alias: Option<String>,
    pub cwd: Option<String>,
    pub concurrent_session_mode: Option<ConcurrentSessionMode>,
}

/// Inputs Stream I may share from startup recall and Stream E's delta seed extraction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StartupRecallEntityInput<'a> {
    pub recall_block: &'a str,
    pub last_three_turn_fts5_entity_ids: &'a [&'a str],
}

/// Recent prompt embedding used for topic-similarity scoring.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryEmbedding {
    pub triple: EmbeddingTriple,
    pub vector: Vec<f32>,
}

pub type EmbeddingCache = HashMap<(String, String), (EmbeddingTriple, Vec<f32>)>;

// Invariant: this explicit allowlist contains harness names known to support
// full coordination (peer-update insertion and claim locks). Unknown harnesses
// default to observe-only to prevent silent privilege escalation. Adding a
// full-coordination harness requires updating the session_derivation tests.
const FULL_COORDINATION_HARNESSES: &[&str] = &["codex", "codex-cli", "claude-code"];

/// Working set for one active harness session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionContext {
    pub session_id: String,
    pub harness: String,
    pub project_binding: Option<ProjectBinding>,
    pub namespaces_in_scope: Vec<String>,
    pub salient_entities: HashSet<String>,
    pub salient_paths: HashSet<String>,
    pub recent_query_message_hash: Option<String>,
    pub recent_query_embedding: Option<QueryEmbedding>,
    pub embedding_cache: EmbeddingCache,
    pub surfaced_peer_writes: HashSet<String>,
}

impl SessionContext {
    pub fn from_startup_recall(
        session_id: impl Into<String>,
        harness: impl Into<String>,
        input: StartupRecallEntityInput<'_>,
    ) -> Self {
        let mut session = Self { session_id: session_id.into(), harness: harness.into(), ..Self::default() };
        session.add_entity_ids(entity_recall_attribute_ids(input.recall_block));
        session.add_entity_ids(input.last_three_turn_fts5_entity_ids.iter().copied());
        session.populate_salient_paths_from_recall(input.recall_block);
        session
    }

    pub fn from_tier3_binding(
        session_id: impl Into<String>,
        harness: impl Into<String>,
        project_binding: ProjectBinding,
    ) -> Self {
        let entity_ids = tier3_binding_entity_ids(&project_binding);
        let mut session = Self {
            session_id: session_id.into(),
            harness: harness.into(),
            project_binding: Some(project_binding),
            ..Self::default()
        };
        session.add_entity_ids(entity_ids);
        session
    }

    pub fn is_full_coordination_harness(&self) -> bool {
        let harness = self.harness.trim().to_ascii_lowercase();
        FULL_COORDINATION_HARNESSES.contains(&harness.as_str())
    }

    /// Harnesses outside `FULL_COORDINATION_HARNESSES` are observe-only.
    pub fn is_observe_only_harness(&self) -> bool {
        !self.is_full_coordination_harness()
    }

    pub fn populate_salient_paths_from_recall(&mut self, recall_block: &str) {
        self.add_memory_paths(recall_ref_paths(recall_block));
    }

    pub fn add_session_paths<I, S>(&mut self, paths: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        if self.is_full_coordination_harness() {
            self.add_memory_paths(paths);
        }
    }

    pub fn set_recent_query_message_hash(&mut self, message_hash: impl Into<String>) {
        self.recent_query_message_hash = Some(message_hash.into());
    }

    pub fn cache_query_embedding(&mut self, message_hash: impl Into<String>, embedding: QueryEmbedding) {
        self.embedding_cache
            .insert((self.session_id.clone(), message_hash.into()), (embedding.triple, embedding.vector));
    }

    pub fn try_get_embedding(
        &self,
        session_id: impl AsRef<str>,
        message_hash: impl AsRef<str>,
    ) -> Option<(EmbeddingTriple, Vec<f32>)> {
        self.embedding_cache.get(&(session_id.as_ref().to_string(), message_hash.as_ref().to_string())).cloned()
    }

    pub fn scoring_query_embedding(&self) -> Option<QueryEmbedding> {
        if let Some(message_hash) = self.recent_query_message_hash.as_deref() {
            return self
                .try_get_embedding(&self.session_id, message_hash)
                .map(|(triple, vector)| QueryEmbedding { triple, vector });
        }

        self.recent_query_embedding.clone()
    }

    pub fn has_surfaced_peer_write(&self, peer_write_id: impl AsRef<str>) -> bool {
        self.surfaced_peer_writes.contains(peer_write_id.as_ref())
    }

    pub fn record_surfaced_peer_write(&mut self, peer_write_id: impl Into<String>) {
        self.surfaced_peer_writes.insert(peer_write_id.into());
    }

    fn add_entity_ids<I, S>(&mut self, entity_ids: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.salient_entities.extend(entity_ids.into_iter().filter_map(trimmed_non_empty));
    }

    fn add_memory_paths<I, S>(&mut self, paths: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.salient_paths.extend(paths.into_iter().filter_map(non_empty_exact_string));
    }
}

fn entity_recall_attribute_ids(recall_block: &str) -> Vec<String> {
    let mut entity_ids = Vec::new();
    let mut remaining = recall_block;

    while let Some(tag_start) = remaining.find("<entity-recall") {
        let tag = &remaining[tag_start..];
        let tag_end = tag.find('>').unwrap_or(tag.len());
        if let Some(entities) = attribute_value(&tag[..tag_end], "entities") {
            entity_ids.extend(entities.split(',').filter_map(trimmed_non_empty));
        }
        remaining = &tag[tag_end..];
    }

    entity_ids
}

fn attribute_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let bytes = tag.as_bytes();
    let mut cursor = 0;

    while cursor < bytes.len() {
        while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || matches!(bytes[cursor], b'/' | b'>') {
            return None;
        }

        let attribute_name_start = cursor;
        while cursor < bytes.len() && is_attribute_name_byte(bytes[cursor]) {
            cursor += 1;
        }
        let attribute_name = &tag[attribute_name_start..cursor];

        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != b'=' {
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            return None;
        }

        let quote = bytes[cursor];
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        let value_start = cursor + 1;
        let value_end = tag[value_start..].find(char::from(quote)).map(|offset| value_start + offset)?;
        if attribute_name == name {
            return Some(&tag[value_start..value_end]);
        }
        cursor = value_end + 1;
    }

    None
}

fn is_attribute_name_byte(byte: u8) -> bool {
    !byte.is_ascii_whitespace() && !matches!(byte, b'=' | b'/' | b'>')
}

fn recall_ref_paths(recall_block: &str) -> Vec<String> {
    let mut paths = Vec::new();
    paths.extend(section_ref_paths(recall_block, "entity-recall"));
    paths.extend(section_ref_paths(recall_block, "project-state"));
    paths
}

fn section_ref_paths(recall_block: &str, section_name: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut remaining = recall_block;
    let opening = format!("<{section_name}");
    let closing = format!("</{section_name}>");

    while let Some(section_start) = remaining.find(&opening) {
        let section = &remaining[section_start..];
        let Some(section_end) = section.find(&closing) else {
            break;
        };
        let section_end = section_end + closing.len();
        paths.extend(ref_attribute_paths(&section[..section_end]));
        remaining = &section[section_end..];
    }

    paths
}

fn ref_attribute_paths(xml: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut remaining = xml;

    while let Some(tag_start) = remaining.find('<') {
        let tag = &remaining[tag_start..];
        let tag_end = tag.find('>').unwrap_or(tag.len());
        paths.extend(attribute_value(&tag[..tag_end], "ref").and_then(non_empty_exact_string));
        if tag_end == tag.len() {
            break;
        }
        remaining = &tag[tag_end + 1..];
    }

    paths
}

fn tier3_binding_entity_ids(project_binding: &ProjectBinding) -> Vec<String> {
    let mut entity_ids = Vec::new();
    entity_ids.extend(trimmed_non_empty(&project_binding.canonical_id));
    entity_ids.extend(project_binding.alias.as_deref().and_then(trimmed_non_empty));

    if let Some(cwd) = project_binding.cwd.as_deref() {
        entity_ids.extend(path_basename(cwd));
        entity_ids.extend(parent_basename(cwd));
    }

    entity_ids
}

fn path_basename(path: &str) -> Option<String> {
    Path::new(path).file_name().and_then(|name| name.to_str()).and_then(trimmed_non_empty)
}

fn parent_basename(path: &str) -> Option<String> {
    Path::new(path).parent().and_then(Path::file_name).and_then(|name| name.to_str()).and_then(trimmed_non_empty)
}

fn trimmed_non_empty(value: impl AsRef<str>) -> Option<String> {
    let value = value.as_ref().trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn non_empty_exact_string(value: impl AsRef<str>) -> Option<String> {
    let value = value.as_ref();
    (!value.is_empty()).then(|| value.to_string())
}
