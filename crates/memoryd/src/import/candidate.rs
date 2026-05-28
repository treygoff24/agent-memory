//! Parsed memory candidate — the uniform shape both Claude and Codex parsers
//! emit, consumed by the pipeline's dedup/plan/write stages.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Which harness a parsed memory came from. Surfaces in the import report and
/// in the `source.harness` field on the persisted memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Harness {
    ClaudeCode,
    Codex,
}

impl Harness {
    /// Stable wire-format token used in the state file's source-keys and in the
    /// `source.harness` field on the persisted memory.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }
}

/// A single memory candidate ready for the dedup/write pipeline. The parsers
/// produce a `Vec<ParsedMemory>` per source root; the pipeline consumes them in
/// topological order (after wiki-link dependency resolution in T05).
#[derive(Debug, Clone)]
pub struct ParsedMemory {
    /// Harness-relative source key for the state file, e.g.
    /// `claude:projects/-Users-treygoff-Code-atlasos/memory/feedback_X.md` or
    /// `codex:memories/MEMORY.md#task-group-3-atlasos-react-doctor`.
    pub source_key: String,
    /// Absolute path the parser read from. Populates `source.ref` on the
    /// persisted memory.
    pub source_path: PathBuf,
    /// SHA-256 of `(frontmatter_canonical_yaml || body)`. Drives idempotency
    /// detection and supersede-on-change.
    pub content_hash: String,
    /// Which harness sourced this candidate.
    pub harness: Harness,
    /// Frontmatter-derived hints (e.g. Claude `name`, Codex `keywords`,
    /// `applies_to`). Surfaced to T05 as candidate `entities` / `tags` /
    /// `evidence_refs`.
    pub frontmatter_hint: BTreeMap<String, Value>,
    /// LF-normalised body to be written into the memory.
    pub body: String,
    /// `[[wiki_link]]` aliases extracted from the body. T05 resolves these into
    /// a memory-id DAG; T06 attaches the resolved ids as `related`.
    pub wiki_links: Vec<String>,
    /// Working directory associated with the source, if one can be inferred
    /// (Codex `applies_to: cwd=<path>` line, or the Claude project encoded path).
    /// `None` means "no cwd hint" — the importer falls back to `me` scope.
    pub cwd: Option<PathBuf>,
    /// Optional Claude-style topic title for `memory_write { title }` if the
    /// parser was able to extract one.
    pub title: Option<String>,
}

impl ParsedMemory {
    /// Compute the canonical content hash over `(canonical-yaml of frontmatter
    /// hint || body)`. Whitespace-stable: the YAML representation is what
    /// `serde_yaml` produces, which is deterministic for a `BTreeMap`-keyed
    /// hint map.
    pub fn compute_content_hash(frontmatter_hint: &BTreeMap<String, Value>, body: &str) -> String {
        let yaml = serde_yaml::to_string(frontmatter_hint).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(yaml.as_bytes());
        hasher.update(b"\n");
        hasher.update(body.as_bytes());
        format!("sha256:{}", hex::encode(hasher.finalize()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_is_stable_for_equivalent_input() {
        let mut hint = BTreeMap::new();
        hint.insert("name".to_string(), Value::String("Topic".to_string()));
        let a = ParsedMemory::compute_content_hash(&hint, "body");
        let b = ParsedMemory::compute_content_hash(&hint, "body");
        assert_eq!(a, b);
        assert!(a.starts_with("sha256:"));
    }

    #[test]
    fn content_hash_diverges_for_distinct_body() {
        let hint = BTreeMap::new();
        let a = ParsedMemory::compute_content_hash(&hint, "first");
        let b = ParsedMemory::compute_content_hash(&hint, "second");
        assert_ne!(a, b);
    }

    #[test]
    fn harness_str_token_is_stable_for_wire_use() {
        assert_eq!(Harness::ClaudeCode.as_str(), "claude-code");
        assert_eq!(Harness::Codex.as_str(), "codex");
    }
}
