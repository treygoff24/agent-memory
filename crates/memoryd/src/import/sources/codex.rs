//! OpenAI Codex CLI memory parser (T03).
//!
//! Reads `~/.codex/memories/MEMORY.md` (one memory per `# Task Group:` block)
//! plus `~/.codex/memories/extensions/ad_hoc/notes/*.md` (one memory per note).
//! Skips `raw_memories.md`, `memory_summary.md`, `rollout_summaries/`, `skills/`
//! per the locked-decisions table — those are intermediate or orthogonal.
//!
//! Per the plan's v0.3 B2 override of the locked Q3 decision: ad-hoc notes use
//! `memory_write` (not `memory_note`) so provenance is preserved.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{extract_wiki_links, slugify};
use crate::import::candidate::{Harness, ParsedMemory};
use crate::import::{ImportError, ImportResult};

/// Output bundle from a Codex memory root parse.
#[derive(Debug, Default)]
pub struct CodexParseOutput {
    pub candidates: Vec<ParsedMemory>,
    pub errors: Vec<ImportError>,
}

/// Parse a Codex memory root. Missing root → empty output without error.
pub fn parse(root: &Path) -> ImportResult<CodexParseOutput> {
    let mut output = CodexParseOutput::default();
    let memory_md = root.join("MEMORY.md");
    if memory_md.exists() {
        parse_memory_md(&memory_md, &mut output);
    }
    let ad_hoc = root.join("extensions").join("ad_hoc").join("notes");
    if ad_hoc.exists() {
        parse_ad_hoc_notes(&ad_hoc, &mut output);
    }
    output.candidates.sort_by(|a, b| a.source_key.cmp(&b.source_key));
    Ok(output)
}

fn parse_memory_md(path: &Path, output: &mut CodexParseOutput) {
    let raw = match std::fs::read(path) {
        Ok(value) => value,
        Err(error) => {
            output.errors.push(ImportError::io(path, error));
            return;
        }
    };
    let text = match std::str::from_utf8(&raw) {
        Ok(value) => value.to_string(),
        Err(error) => {
            output.errors.push(ImportError::Encoding {
                source_key: "codex:memories/MEMORY.md".to_string(),
                reason: error.to_string(),
            });
            return;
        }
    };
    let blocks = split_task_groups(&text);
    for (index, block) in blocks.into_iter().enumerate() {
        match parse_task_group_block(&block, index, path) {
            Ok(parsed) => output.candidates.push(parsed),
            Err(error) => output.errors.push(error),
        }
    }
}

fn split_task_groups(text: &str) -> Vec<TaskGroupBlock> {
    let mut blocks = Vec::new();
    let mut current_header: Option<String> = None;
    let mut current_lines: Vec<&str> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# Task Group:") {
            if let Some(header) = current_header.take() {
                blocks.push(TaskGroupBlock { header, body: current_lines.join("\n") });
                current_lines.clear();
            }
            current_header = Some(rest.trim().to_string());
        } else if current_header.is_some() {
            current_lines.push(line);
        }
    }
    if let Some(header) = current_header.take() {
        blocks.push(TaskGroupBlock { header, body: current_lines.join("\n") });
    }
    blocks
}

#[derive(Debug)]
struct TaskGroupBlock {
    header: String,
    body: String,
}

fn parse_task_group_block(block: &TaskGroupBlock, index: usize, source_path: &Path) -> ImportResult<ParsedMemory> {
    let source_key = format!("codex:memories/MEMORY.md#task-group-{}-{}", index + 1, slugify(&block.header));
    let scope = extract_field(&block.body, "scope:").ok_or_else(|| ImportError::Parse {
        source_key: source_key.clone(),
        reason: "Task Group missing required `scope:` line".to_string(),
    })?;
    let applies_to = extract_field(&block.body, "applies_to:");
    let cwd = applies_to.as_deref().and_then(parse_applies_to_cwd);
    let reuse_rule = applies_to.as_deref().and_then(parse_applies_to_reuse_rule);
    let keywords = collect_keywords(&block.body);
    let evidence_refs = collect_rollout_summary_files(&block.body);

    let mut frontmatter_hint: BTreeMap<String, Value> = BTreeMap::new();
    frontmatter_hint.insert("name".to_string(), Value::String(block.header.clone()));
    frontmatter_hint.insert("scope".to_string(), Value::String(scope));
    if let Some(rule) = reuse_rule {
        frontmatter_hint.insert("reuse_rule".to_string(), Value::String(rule));
    }
    if !keywords.is_empty() {
        frontmatter_hint
            .insert("tags".to_string(), Value::Array(keywords.iter().cloned().map(Value::String).collect()));
    }
    if !evidence_refs.is_empty() {
        let array: Vec<Value> = evidence_refs.iter().map(EvidenceRef::to_value).collect();
        frontmatter_hint.insert("evidence_refs".to_string(), Value::Array(array));
    }

    let body = block.body.trim().to_string();
    let wiki_links = extract_wiki_links(&body);
    let content_hash = ParsedMemory::compute_content_hash(&frontmatter_hint, &body);
    Ok(ParsedMemory {
        source_key,
        source_path: source_path.to_path_buf(),
        content_hash,
        harness: Harness::Codex,
        frontmatter_hint,
        body,
        wiki_links,
        cwd,
        title: Some(block.header.clone()),
    })
}

fn parse_ad_hoc_notes(dir: &Path, output: &mut CodexParseOutput) {
    let entries = match std::fs::read_dir(dir) {
        Ok(value) => value,
        Err(error) => {
            output.errors.push(ImportError::io(dir, error));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(value) => value,
            Err(error) => {
                output.errors.push(ImportError::io(dir, error));
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("md") {
            continue;
        }
        match parse_ad_hoc_note(&path) {
            Ok(parsed) => output.candidates.push(parsed),
            Err(error) => output.errors.push(error),
        }
    }
}

fn parse_ad_hoc_note(path: &Path) -> ImportResult<ParsedMemory> {
    let filename = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("note");
    let source_key = format!("codex:memories/extensions/ad_hoc/notes/{filename}");
    let raw = std::fs::read(path).map_err(|error| ImportError::io(path, error))?;
    let body = std::str::from_utf8(&raw)
        .map_err(|error| ImportError::Encoding { source_key: source_key.clone(), reason: error.to_string() })?
        .trim()
        .to_string();
    if body.is_empty() {
        return Err(ImportError::Parse { source_key: source_key.clone(), reason: "ad-hoc note is empty".to_string() });
    }
    let title = path.file_stem().and_then(std::ffi::OsStr::to_str).map(str::to_string);
    let mut frontmatter_hint: BTreeMap<String, Value> = BTreeMap::new();
    if let Some(name) = title.clone() {
        frontmatter_hint.insert("name".to_string(), Value::String(name));
    }
    let wiki_links = extract_wiki_links(&body);
    let content_hash = ParsedMemory::compute_content_hash(&frontmatter_hint, &body);
    Ok(ParsedMemory {
        source_key,
        source_path: path.to_path_buf(),
        content_hash,
        harness: Harness::Codex,
        frontmatter_hint,
        body,
        wiki_links,
        cwd: None,
        title,
    })
}

fn extract_field(body: &str, key: &str) -> Option<String> {
    for line in body.lines() {
        if let Some(value) = line.trim_start().strip_prefix(key) {
            return Some(value.trim().to_string());
        }
    }
    None
}

/// First `<key>=<value>` entry in a `;`-separated `applies_to` line whose
/// trimmed value passes `accept`. Scans past entries that fail the predicate so
/// callers can reject sentinels (e.g. `cwd=unknown`) without short-circuiting.
fn applies_to_field(applies_to: &str, key: &str, accept: impl Fn(&str) -> bool) -> Option<String> {
    for part in applies_to.split(';') {
        if let Some(value) = part.trim().strip_prefix(key) {
            let value = value.trim();
            if accept(value) {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Extract a leading path-shaped token from a raw `cwd=` field value.
///
/// A path-shaped token begins with `/` or `~/` and ends at the first
/// whitespace, backtick, or comma character. Values that don't begin with
/// those prefixes (including the sentinel `"unknown"`) produce `None`.
///
/// Deliberate trade-off: a real path containing a space (e.g.
/// `/Users/u/Google Drive/x`) is truncated at the space. This is acceptable
/// for Codex `applies_to` cwd fields because such paths are rare and the
/// alternative — admitting malformed prose as cwd values — causes on-disk
/// `projects/<alias>/` directories with hostile names.
fn path_shaped_prefix(value: &str) -> Option<&str> {
    if !value.starts_with('/') && !value.starts_with("~/") {
        return None;
    }
    let end = value.find(|c: char| c.is_ascii_whitespace() || c == '`' || c == ',').unwrap_or(value.len());
    let token = &value[..end];
    if token.starts_with('/') || token.starts_with("~/") {
        Some(token)
    } else {
        None
    }
}

fn parse_applies_to_cwd(applies_to: &str) -> Option<PathBuf> {
    applies_to_field(applies_to, "cwd=", |value| value != "unknown" && path_shaped_prefix(value).is_some())
        .and_then(|value| path_shaped_prefix(&value).map(PathBuf::from))
}

fn parse_applies_to_reuse_rule(applies_to: &str) -> Option<String> {
    applies_to_field(applies_to, "reuse_rule=", |value| !value.is_empty())
}

fn collect_keywords(body: &str) -> Vec<String> {
    let mut keywords = Vec::new();
    let mut in_section = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("### keywords") {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("###") || trimmed.starts_with("##") || trimmed.starts_with('#') {
                in_section = false;
                continue;
            }
            // The template allows either `- k1, k2, k3` or one bullet per keyword.
            let line = trimmed.strip_prefix('-').unwrap_or(trimmed);
            if line.trim().is_empty() {
                continue;
            }
            for chunk in line.split(',') {
                let chunk = chunk.trim();
                if !chunk.is_empty() && !keywords.iter().any(|k| k == chunk) {
                    keywords.push(chunk.to_string());
                }
            }
        }
    }
    keywords
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EvidenceRef {
    rollout_path: String,
    thread_id: Option<String>,
    updated_at: Option<String>,
    cwd: Option<String>,
    outcome: Option<String>,
}

impl EvidenceRef {
    fn to_value(&self) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("rollout_path".to_string(), Value::String(self.rollout_path.clone()));
        if let Some(thread_id) = &self.thread_id {
            obj.insert("thread_id".to_string(), Value::String(thread_id.clone()));
        }
        if let Some(updated_at) = &self.updated_at {
            obj.insert("updated_at".to_string(), Value::String(updated_at.clone()));
        }
        if let Some(cwd) = &self.cwd {
            obj.insert("cwd".to_string(), Value::String(cwd.clone()));
        }
        if let Some(outcome) = &self.outcome {
            obj.insert("outcome".to_string(), Value::String(outcome.clone()));
        }
        // file:// URI is computed by T06 when it lifts these into Evidence entries;
        // the parser carries the raw rollout_path so T06 can do the right thing.
        Value::Object(obj)
    }
}

fn collect_rollout_summary_files(body: &str) -> Vec<EvidenceRef> {
    let mut refs = Vec::new();
    let mut in_section = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("### rollout_summary_files") {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("###") || trimmed.starts_with("##") || trimmed.starts_with('#') {
                in_section = false;
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix('-') {
                let parsed = parse_rollout_summary_bullet(rest.trim());
                if let Some(reference) = parsed {
                    refs.push(reference);
                }
            }
        }
    }
    refs
}

fn parse_rollout_summary_bullet(text: &str) -> Option<EvidenceRef> {
    // Bullet shape per the Codex Phase-2 prompt:
    //   `cwd=<path>, rollout_path=<path>, updated_at=<ts>, thread_id=<id>[, outcome=<o>]`
    // Order may vary; we pick on key= prefixes so the parser is resilient.
    let mut cwd = None;
    let mut rollout_path = None;
    let mut updated_at = None;
    let mut thread_id = None;
    let mut outcome = None;
    for part in text.split(',') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("cwd=") {
            cwd = Some(value.trim().to_string());
        } else if let Some(value) = part.strip_prefix("rollout_path=") {
            rollout_path = Some(value.trim().to_string());
        } else if let Some(value) = part.strip_prefix("updated_at=") {
            updated_at = Some(value.trim().to_string());
        } else if let Some(value) = part.strip_prefix("thread_id=") {
            thread_id = Some(value.trim().to_string());
        } else if let Some(value) = part.strip_prefix("outcome=") {
            outcome = Some(value.trim().to_string());
        }
    }
    let rollout_path = rollout_path?;
    Some(EvidenceRef { rollout_path, thread_id, updated_at, cwd, outcome })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&path, body).expect("write");
        path
    }

    #[test]
    fn empty_root_returns_empty_output() {
        let tmp = tempfile::tempdir().expect("tmp");
        let out = parse(tmp.path()).expect("parse ok");
        assert!(out.candidates.is_empty());
        assert!(out.errors.is_empty());
    }

    #[test]
    fn parses_three_task_groups_with_distinct_cwd_shapes() {
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"\
# Task Group: AtlasOS React doctor

scope: how to diagnose react-doctor failures in atlasos
applies_to: cwd=/Users/u/Code/atlasos; reuse_rule=cwd-scoped

## Task 1: react-doctor flake
react-doctor flakes on cold start.

### rollout_summary_files
- cwd=/Users/u/Code/atlasos, rollout_path=/Users/u/.codex/memories/rollout_summaries/abc.md, updated_at=2026-05-20T10:00:00Z, thread_id=t-1, outcome=success

### keywords
- react-doctor, flake, atlasos

# Task Group: workflow notes

scope: notes about how the team works
applies_to: cwd=unknown; reuse_rule=workflow-scoped

## Task 1: PR template
Use the PR template.

### keywords
- workflow, pr-template

# Task Group: codeshare conventions

scope: conventions in the codeshare repo
applies_to: cwd=/Users/u/Code/codeshare; reuse_rule=cwd-scoped

## Task 1: lint settings
Lint settings live in eslintrc.

### keywords
- lint, eslint
";
        write_file(tmp.path(), "MEMORY.md", body);
        let out = parse(tmp.path()).expect("parse ok");
        assert_eq!(out.candidates.len(), 3, "three task groups parsed");
        assert!(out.errors.is_empty());
        let atlasos = out
            .candidates
            .iter()
            .find(|c| c.title.as_deref().is_some_and(|t| t.contains("AtlasOS")))
            .expect("AtlasOS group present");
        assert_eq!(atlasos.cwd.as_deref(), Some(Path::new("/Users/u/Code/atlasos")));
        // applies_to cwd=unknown should leave cwd as None
        let workflow = out
            .candidates
            .iter()
            .find(|c| c.title.as_deref().is_some_and(|t| t.contains("workflow")))
            .expect("workflow group present");
        assert_eq!(workflow.cwd, None);
    }

    #[test]
    fn rollout_summary_files_get_extracted_as_evidence_refs() {
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"\
# Task Group: with evidence

scope: tests rollout-summary extraction
applies_to: cwd=/work; reuse_rule=cwd

## Task 1: t1
work

### rollout_summary_files
- cwd=/work, rollout_path=/r/a.md, updated_at=2026-05-20T10:00:00Z, thread_id=t-1, outcome=success
- cwd=/work, rollout_path=/r/b.md, updated_at=2026-05-21T10:00:00Z, thread_id=t-2, outcome=partial
- cwd=/work, rollout_path=/r/c.md, updated_at=2026-05-22T10:00:00Z, thread_id=t-3, outcome=fail

### keywords
- evidence
";
        write_file(tmp.path(), "MEMORY.md", body);
        let out = parse(tmp.path()).expect("parse ok");
        assert_eq!(out.candidates.len(), 1);
        let refs = out.candidates[0].frontmatter_hint.get("evidence_refs").expect("evidence_refs present");
        let Value::Array(array) = refs else { panic!("evidence_refs is array") };
        assert_eq!(array.len(), 3);
        assert!(array.iter().any(|v| v["thread_id"] == "t-1"));
        assert!(array.iter().any(|v| v["thread_id"] == "t-2"));
        assert!(array.iter().any(|v| v["thread_id"] == "t-3"));
    }

    #[test]
    fn ad_hoc_note_produces_separate_candidate() {
        let tmp = tempfile::tempdir().expect("tmp");
        write_file(
            tmp.path(),
            "extensions/ad_hoc/notes/preference.md",
            b"Prefer rustls over openssl for TLS dependencies.\n",
        );
        let out = parse(tmp.path()).expect("parse ok");
        assert_eq!(out.candidates.len(), 1);
        let note = &out.candidates[0];
        assert!(note.source_key.contains("ad_hoc/notes/preference.md"));
        assert_eq!(note.title.as_deref(), Some("preference"));
        assert_eq!(note.cwd, None);
        assert_eq!(note.harness, Harness::Codex);
    }

    #[test]
    fn skips_raw_memories_and_memory_summary_and_skills_and_rollout_summaries() {
        let tmp = tempfile::tempdir().expect("tmp");
        write_file(tmp.path(), "raw_memories.md", b"raw stuff");
        write_file(tmp.path(), "memory_summary.md", b"summary stuff");
        write_file(tmp.path(), "skills/skill1.md", b"skill body");
        write_file(tmp.path(), "rollout_summaries/r1.md", b"rollout body");
        // No MEMORY.md, no ad_hoc notes → zero candidates.
        let out = parse(tmp.path()).expect("parse ok");
        assert!(out.candidates.is_empty());
        assert!(out.errors.is_empty(), "skipped files don't produce errors");
    }

    #[test]
    fn task_group_missing_scope_produces_error_but_others_continue() {
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"\
# Task Group: missing scope

applies_to: cwd=/x; reuse_rule=cwd

## Task 1: t1
body

# Task Group: with scope

scope: real scope
applies_to: cwd=/y; reuse_rule=cwd

## Task 1: t2
body
";
        write_file(tmp.path(), "MEMORY.md", body);
        let out = parse(tmp.path()).expect("parse ok");
        assert_eq!(out.candidates.len(), 1, "one parses, one errors");
        assert_eq!(out.candidates[0].title.as_deref(), Some("with scope"));
        assert!(out
            .errors
            .iter()
            .any(|e| matches!(e, ImportError::Parse { source_key, .. } if source_key.contains("missing-scope"))));
    }

    // ── parse_applies_to_cwd path-shape tests ──────────────────────────────

    #[test]
    fn parse_applies_to_cwd_accepts_absolute_path() {
        assert_eq!(parse_applies_to_cwd("cwd=/Users/u/Code/atlasos"), Some(PathBuf::from("/Users/u/Code/atlasos")),);
    }

    #[test]
    fn parse_applies_to_cwd_accepts_tilde_path() {
        assert_eq!(parse_applies_to_cwd("cwd=~/Code/x"), Some(PathBuf::from("~/Code/x")),);
    }

    #[test]
    fn parse_applies_to_cwd_rejects_unknown() {
        assert_eq!(parse_applies_to_cwd("cwd=unknown"), None);
    }

    #[test]
    fn parse_applies_to_cwd_rejects_prose_with_backtick() {
        // Real dogfood case: "`droid`, cmux` on PATH)"
        assert_eq!(parse_applies_to_cwd("cwd=`droid`, cmux` on PATH)"), None);
    }

    #[test]
    fn parse_applies_to_cwd_rejects_prose_with_factory_config() {
        // Real dogfood case: ".factory` config in this environment"
        assert_eq!(parse_applies_to_cwd("cwd=.factory` config in this environment"), None);
    }

    #[test]
    fn parse_applies_to_cwd_semicolon_split_isolates_cwd_from_reuse_rule() {
        // The `;`-split in applies_to_field already isolates the `cwd=` value from
        // subsequent fields, so trailing "; reuse_rule=manual" must not leak into
        // the cwd — and parse_applies_to_reuse_rule must still extract "manual".
        let applies_to = "cwd=/work; reuse_rule=manual";
        assert_eq!(parse_applies_to_cwd(applies_to), Some(PathBuf::from("/work")));
        assert_eq!(parse_applies_to_reuse_rule(applies_to), Some("manual".to_string()));
    }

    #[test]
    fn wiki_links_in_task_group_body_get_extracted() {
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"\
# Task Group: linked

scope: cross-reference
applies_to: cwd=/w; reuse_rule=cwd

## Task 1: t1
See [[Other Topic]] for details.
";
        write_file(tmp.path(), "MEMORY.md", body);
        let out = parse(tmp.path()).expect("parse ok");
        assert_eq!(out.candidates.len(), 1);
        assert_eq!(out.candidates[0].wiki_links, vec!["Other Topic".to_string()]);
    }
}
