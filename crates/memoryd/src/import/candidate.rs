//! Parsed memory candidate — the uniform shape both Claude and Codex parsers
//! emit, consumed by the pipeline's dedup/plan/write stages.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
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
    /// `source.harness` field on the persisted memory. This is also the
    /// canonical descriptor `id` in the shared [`memorum_coordination::HarnessRegistry`].
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }

    /// Resolve any recognized spelling (`"claude"`, `"claude-code"`, `"codex"`,
    /// `"codex-cli"`, case-insensitive) to its importer variant via the shared
    /// harness registry. Returns `None` for harnesses without an importer here.
    pub fn from_identifier(identifier: &str) -> Option<Self> {
        let registry = memorum_coordination::HarnessRegistry::builtin();
        let descriptor = registry.resolve(identifier)?;
        match descriptor.id.as_str() {
            "claude-code" => Some(Self::ClaudeCode),
            "codex" => Some(Self::Codex),
            _ => None,
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
    /// Content-hash suffix appended to the section component when multiple
    /// sections in the same source file would otherwise share the same ordinal-
    /// free slug. Set by `disambiguate_collisions` after parsing.
    pub section_disambiguation: Option<String>,
}

impl ParsedMemory {
    /// Stable import identity. A recovered canonical id wins; otherwise use a
    /// portable tuple rather than the mutable absolute path/source key.
    pub fn import_identity(&self, canonical_project_id: Option<&str>) -> String {
        if let Some(id) = self.recovered_memory_id() {
            return format!("mem:{id}");
        }
        compute_identity(
            &self.source_key,
            &self.source_path,
            self.harness.as_str(),
            canonical_project_id.unwrap_or("me"),
            self.section_disambiguation.as_deref(),
        )
    }

    pub fn recovered_memory_id(&self) -> Option<&str> {
        self.frontmatter_hint.get("id").and_then(Value::as_str).filter(|id| id.starts_with("mem_"))
    }

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

    /// First 8 hex digits of the content hash, used as a collision disambiguator.
    pub fn short_content_hash(&self) -> &str {
        short_hash(&self.content_hash)
    }

    /// The root-relative path without the section, and the ordinal-free section
    /// base, grouped as a collision key for `disambiguate_collisions`.
    pub fn section_base_key(&self) -> (String, String) {
        let (_, relative) = self.source_key.split_once(':').unwrap_or(("", self.source_key.as_str()));
        let (path, section) = relative.split_once('#').unwrap_or((relative, ""));
        (path.to_string(), ordinal_free_section(section))
    }

    /// The section base alone (ordinal-free).
    pub fn section_base(&self) -> String {
        self.section_base_key().1
    }
}

/// Compute the stable import identity for any source record or candidate.
///
/// The identity is `tuple:<harness>:<canonical-profile-path>:<project>:<path>:<section>`.
/// The profile path is canonicalized so symlinked profiles resolve to the same
/// backing store; the section is stripped of Codex `task-group-N-` prefixes and
/// disambiguated when `section_disambiguation` is supplied.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_identity(
    source_key: &str,
    source_path: &Path,
    harness: &str,
    canonical_project_id: &str,
    section_disambiguation: Option<&str>,
) -> String {
    let (_, relative) = source_key.split_once(':').unwrap_or(("", source_key));
    let (path, section) = relative.split_once('#').unwrap_or((relative, ""));
    let section = ordinal_free_section(section);
    let section = match section_disambiguation {
        Some(suffix) if !section.is_empty() => format!("{section}-{suffix}"),
        Some(suffix) => suffix.to_string(),
        None => section,
    };
    format!("tuple:{harness}:{}:{canonical_project_id}:{path}:{section}", profile_identity_from_path(source_path))
}

/// Strip Codex `task-group-N-` prefixes from a section slug so a renumbered
/// task group still maps to the same identity.
pub(crate) fn ordinal_free_section(section: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^task-group-\d+-").expect("static regex compiles"));
    re.replace(section, "").into_owned()
}

/// Canonicalize the profile path (e.g. `~/.claude` or `~/.codex`) from a source
/// path. If the profile directory is a symlink, this resolves to the real path
/// so two profile roots pointing at the same store share an identity.
fn profile_identity_from_path(source_path: &Path) -> String {
    let Some(profile_root) = source_path.ancestors().find(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(".claude") || name.starts_with(".codex"))
    }) else {
        return "default".to_string();
    };
    std::fs::canonicalize(profile_root)
        .unwrap_or_else(|_| profile_root.to_path_buf())
        .to_string_lossy()
        .to_string()
}

/// First 8 hex digits of a `sha256:`-prefixed hash.
pub(crate) fn short_hash(content_hash: &str) -> &str {
    let hex = content_hash.strip_prefix("sha256:").unwrap_or(content_hash);
    hex.get(..8).unwrap_or(hex)
}

/// Detect sections whose ordinal-free slugs collide within the same source file
/// and append a short content-hash suffix to their identity. Callers parse all
/// candidates from a source root, then run this once over the vector.
///
/// Collisions with identical content are collapsed to a single candidate, and
/// 8-hex suffix collisions are extended deterministically until the suffixes
/// are unique within the file.
pub(crate) fn disambiguate_collisions(candidates: &mut Vec<ParsedMemory>) {
    let taken = std::mem::take(candidates);
    let mut groups: HashMap<(String, String), Vec<ParsedMemory>> = HashMap::new();
    let mut group_order: Vec<(String, String)> = Vec::new();
    for candidate in taken {
        let key = candidate.section_base_key();
        if !groups.contains_key(&key) {
            group_order.push(key.clone());
        }
        groups.entry(key).or_default().push(candidate);
    }

    let mut result = Vec::with_capacity(groups.values().map(Vec::len).sum());
    for key in group_order {
        let group = groups.remove(&key).unwrap_or_default();
        if group.len() < 2 {
            result.extend(group);
            continue;
        }

        // Collapse identical content to a single candidate (keep first occurrence).
        let mut seen_hashes: HashSet<String> = HashSet::new();
        let mut collapsed: Vec<ParsedMemory> = Vec::new();
        for candidate in group {
            if seen_hashes.insert(candidate.content_hash.clone()) {
                collapsed.push(candidate);
            }
        }

        if collapsed.len() == 1 {
            result.extend(collapsed);
            continue;
        }

        // Build a deterministic, unique suffix for each distinct content hash.
        let hashes: Vec<String> = collapsed
            .iter()
            .map(|c| c.content_hash.strip_prefix("sha256:").unwrap_or(&c.content_hash).to_string())
            .collect();
        let suffixes = unique_hash_suffixes(&hashes);

        for mut candidate in collapsed {
            let hex = candidate.content_hash.strip_prefix("sha256:").unwrap_or(&candidate.content_hash);
            candidate.section_disambiguation = suffixes.get(hex).cloned();
            result.push(candidate);
        }
    }

    *candidates = result;
}

/// Compute the shortest unique hex prefix (at least 8 chars) for each hash in
/// the group. Prefixes are extended only when another hash collides on the
/// first N characters, so the suffix is deterministic and minimal.
fn unique_hash_suffixes(hashes: &[String]) -> HashMap<String, String> {
    let mut suffixes = HashMap::new();
    for (i, hash) in hashes.iter().enumerate() {
        let mut required = 8;
        for (j, other) in hashes.iter().enumerate() {
            if i == j {
                continue;
            }
            let common = hash.bytes().zip(other.bytes()).take_while(|(a, b)| a == b).count();
            required = required.max(common + 1);
        }
        let suffix = hash.get(..required.min(hash.len())).unwrap_or(hash);
        suffixes.insert(hash.clone(), suffix.to_string());
    }
    suffixes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn candidate(path: &str, source_key: &str) -> ParsedMemory {
        ParsedMemory {
            source_key: source_key.to_string(),
            source_path: PathBuf::from(path),
            content_hash: "sha256:00000000000000000000000000000000000000000000000000000000deadbeef".to_string(),
            harness: Harness::ClaudeCode,
            frontmatter_hint: BTreeMap::new(),
            body: "body".to_string(),
            wiki_links: Vec::new(),
            cwd: None,
            title: None,
            section_disambiguation: None,
        }
    }

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

    #[test]
    #[cfg(unix)]
    fn import_identity_uses_canonical_profile_root_and_survives_symlinks() {
        let tmp = tempfile::tempdir().expect("tmp");
        let real_profile = tmp.path().join(".claude-real");
        let other_profile = tmp.path().join(".claude-other");
        std::fs::create_dir_all(real_profile.join("projects/proj/memory")).expect("mkdir");
        std::fs::create_dir_all(other_profile.join("projects/proj/memory")).expect("mkdir");
        std::fs::write(real_profile.join("projects/proj/memory/topic.md"), b"body").expect("write");
        std::fs::write(other_profile.join("projects/proj/memory/topic.md"), b"body").expect("write");

        let link_profile = tmp.path().join(".claude-shared");
        std::os::unix::fs::symlink(&real_profile, &link_profile).expect("symlink");

        let same_via_link = link_profile.join("projects/proj/memory/topic.md");
        let same_real = real_profile.join("projects/proj/memory/topic.md");
        let other = other_profile.join("projects/proj/memory/topic.md");
        let renamed = real_profile.join("projects/proj/memory/renamed.md");

        let c1 = candidate(same_via_link.to_str().unwrap(), "claude:projects/proj/memory/topic.md");
        let c2 = candidate(same_real.to_str().unwrap(), "claude:projects/proj/memory/topic.md");
        let c3 = candidate(other.to_str().unwrap(), "claude:projects/proj/memory/topic.md");
        let c4 = candidate(renamed.to_str().unwrap(), "claude:projects/proj/memory/renamed.md");

        assert_eq!(c1.import_identity(Some("proj_x")), c2.import_identity(Some("proj_x")));
        assert_ne!(c2.import_identity(Some("proj_x")), c3.import_identity(Some("proj_x")));
        assert_ne!(c2.import_identity(Some("proj_x")), c4.import_identity(Some("proj_x")));
        assert_ne!(c2.import_identity(Some("proj_x")), c2.import_identity(Some("proj_y")));
    }

    #[test]
    fn recovered_canonical_id_survives_rename_and_content_change() {
        let mut original = candidate("/a/.claude/projects/p/memory/old.md", "claude:projects/p/memory/old.md");
        original.frontmatter_hint.insert("id".to_string(), Value::String("mem_20260710_test_000001".to_string()));
        let mut renamed = candidate("/a/.claude/projects/p/memory/new.md", "claude:projects/p/memory/new.md");
        renamed.frontmatter_hint = original.frontmatter_hint.clone();
        assert_eq!(original.import_identity(Some("proj_p")), renamed.import_identity(Some("proj_p")));
    }

    #[test]
    fn ordinal_free_section_strips_task_group_prefix() {
        assert_eq!(ordinal_free_section("task-group-1-atlasos-react-doctor"), "atlasos-react-doctor");
        assert_eq!(ordinal_free_section("atlasos-react-doctor"), "atlasos-react-doctor");
        assert_eq!(ordinal_free_section(""), "");
    }

    #[test]
    fn section_disambiguation_survives_into_identity() {
        let mut c = candidate(
            "/u/.claude/projects/p/memory/topic.md",
            "claude:projects/p/memory/topic.md#section-a",
        );
        c.section_disambiguation = Some("a1b2c3d4".to_string());
        let id = c.import_identity(Some("proj_p"));
        assert!(id.ends_with(":section-a-a1b2c3d4"), "identity: {id}");
    }

    #[test]
    fn disambiguate_collisions_appends_short_hash_suffix() {
        let mut c1 = candidate("/u/.codex/memories/MEMORY.md", "codex:memories/MEMORY.md#task-group-1-foo");
        c1.content_hash = "sha256:aaa1110000000000000000000000000000000000000000000000000000000000".to_string();
        let mut c2 = candidate("/u/.codex/memories/MEMORY.md", "codex:memories/MEMORY.md#task-group-2-foo");
        c2.content_hash = "sha256:bbb2220000000000000000000000000000000000000000000000000000000000".to_string();
        let mut c3 = candidate("/u/.codex/memories/MEMORY.md", "codex:memories/MEMORY.md#task-group-3-bar");
        c3.content_hash = "sha256:ccc3330000000000000000000000000000000000000000000000000000000000".to_string();

        let mut candidates = vec![c1, c2, c3];
        disambiguate_collisions(&mut candidates);

        assert!(candidates[0].section_disambiguation.is_some());
        assert!(candidates[1].section_disambiguation.is_some());
        assert!(candidates[0].section_disambiguation != candidates[1].section_disambiguation);
        assert!(candidates[2].section_disambiguation.is_none());
    }

    #[test]
    fn disambiguate_collisions_collapses_identical_content_to_one_candidate() {
        let mut c1 = candidate("/u/.codex/memories/MEMORY.md", "codex:memories/MEMORY.md#task-group-1-foo");
        c1.content_hash = "sha256:aaa1110000000000000000000000000000000000000000000000000000000000".to_string();
        let mut c2 = candidate("/u/.codex/memories/MEMORY.md", "codex:memories/MEMORY.md#task-group-2-foo");
        c2.content_hash = "sha256:aaa1110000000000000000000000000000000000000000000000000000000000".to_string();
        let mut c3 = candidate("/u/.codex/memories/MEMORY.md", "codex:memories/MEMORY.md#task-group-3-bar");
        c3.content_hash = "sha256:ccc3330000000000000000000000000000000000000000000000000000000000".to_string();

        let mut candidates = vec![c1, c2, c3];
        disambiguate_collisions(&mut candidates);

        assert_eq!(candidates.len(), 2, "identical content collapsed to one candidate");
        assert!(candidates.iter().filter(|c| c.section_base() == "foo").count() == 1);
        assert!(candidates.iter().any(|c| c.section_base() == "bar" && c.section_disambiguation.is_none()));
    }

    #[test]
    fn disambiguate_collisions_extends_suffix_until_prefix_unique() {
        let mut c1 = candidate("/u/.codex/memories/MEMORY.md", "codex:memories/MEMORY.md#task-group-1-foo");
        c1.content_hash = "sha256:aaa1110000000000000000000000000000000000000000000000000000000000".to_string();
        let mut c2 = candidate("/u/.codex/memories/MEMORY.md", "codex:memories/MEMORY.md#task-group-2-foo");
        // Same 8-hex prefix as c1, diverging at the 9th hex character.
        c2.content_hash = "sha256:aaa1110010000000000000000000000000000000000000000000000000000000".to_string();

        let mut candidates = vec![c1, c2];
        disambiguate_collisions(&mut candidates);

        assert_eq!(candidates.len(), 2);
        let s1 = candidates[0].section_disambiguation.as_deref().unwrap();
        let s2 = candidates[1].section_disambiguation.as_deref().unwrap();
        assert_ne!(s1, s2, "extended suffixes must differ");
        assert!(s1.starts_with("aaa11100"));
        assert!(s2.starts_with("aaa11100"));
        assert!(s1.len() > 8 || s2.len() > 8, "suffix extended beyond 8-hex prefix");
    }
}
