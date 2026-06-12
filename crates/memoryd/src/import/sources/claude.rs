//! Claude Code auto-memory parser (T02).
//!
//! Reads `~/.claude/projects/<encoded>/memory/<topic>.md` and emits a
//! [`ParsedMemory`] per topic. Multi-section dossiers (`3+` substantive `##`
//! sections) decompose into one `ParsedMemory` per section; single-fact files
//! stay as one. `MEMORY.md` is skipped — it's an index Claude maintains for its
//! own startup loading, not a memory in its own right.
//!
//! The locked decisions from the plan that this parser implements:
//!
//! - Granularity: adaptive — single-fact vs. multi-section decomposition.
//! - Wiki-links: `[[name]]` patterns in the body get extracted to
//!   `ParsedMemory::wiki_links`; T05 resolves them into a memory-id DAG.
//! - Entity extraction: source-provided only (the topic's `name` frontmatter
//!   field). The parser does not run NLP over the body.
//!
//! Per-file errors are contained: a malformed-YAML file or a non-UTF-8 file
//! does not abort the parse — it appends to a per-file error list and parsing
//! continues with subsequent files. Empty directory → empty `Vec`, not an error.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{extract_wiki_links, slugify};
use crate::import::candidate::{Harness, ParsedMemory};
use crate::import::{ImportError, ImportResult};

/// `##` headings that are scaffolding rather than substantive sections. The
/// multi-section heuristic ignores them when counting whether a topic file is
/// a dossier worth decomposing.
const BOILERPLATE_HEADINGS: &[&str] =
    &["why", "how to apply", "how", "when", "why this matters", "references", "links", "context"];

/// Output bundle for a Claude memory root parse. `candidates` carries the
/// successful parses; `errors` carries per-file failures so the import report
/// can surface them without aborting the whole import.
#[derive(Debug, Default)]
pub struct ClaudeParseOutput {
    pub candidates: Vec<ParsedMemory>,
    pub errors: Vec<ImportError>,
}

/// Parse a Claude memory root. Walks `<root>/<encoded>/memory/*.md` recursively
/// because Claude organises auto-memory per encoded-cwd directory; missing root
/// → `Ok(empty)` so an unused harness on this machine is not an error.
pub fn parse(root: &Path) -> ImportResult<ClaudeParseOutput> {
    if !root.exists() {
        return Ok(ClaudeParseOutput::default());
    }
    let mut output = ClaudeParseOutput::default();
    // follow_links(true): shared-profile setups symlink `<encoded>/memory/` to a
    // common store (observed live: C-Mux's ~/.claude-shared migration). walkdir
    // detects symlink cycles and reports them as entry errors, which we already
    // collect per-file below.
    for entry in walkdir::WalkDir::new(root).follow_links(true) {
        let entry = match entry {
            Ok(value) => value,
            Err(error) => {
                output.errors.push(ImportError::io(
                    error.path().map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf()),
                    error.into_io_error().unwrap_or_else(|| std::io::Error::other("walkdir error")),
                ));
                continue;
            }
        };
        let path = entry.path();
        if !is_topic_file(path) {
            continue;
        }
        match parse_topic_file(path, root) {
            Ok(parsed) => output.candidates.extend(parsed),
            Err(error) => output.errors.push(error),
        }
    }
    output.candidates.sort_by(|a, b| a.source_key.cmp(&b.source_key));
    Ok(output)
}

fn is_topic_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    if path.extension().and_then(std::ffi::OsStr::to_str) != Some("md") {
        return false;
    }
    let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
        return false;
    };
    name != "MEMORY.md"
}

fn parse_topic_file(path: &Path, root: &Path) -> ImportResult<Vec<ParsedMemory>> {
    let source_key = source_key_for(path, root);
    let raw = std::fs::read(path).map_err(|error| ImportError::io(path, error))?;
    let text = std::str::from_utf8(&raw)
        .map_err(|error| ImportError::Encoding { source_key: source_key.clone(), reason: error.to_string() })?
        .to_string();
    let (frontmatter, body) = split_frontmatter_and_body(&text)
        .map_err(|reason| ImportError::Parse { source_key: source_key.clone(), reason })?;
    let frontmatter_hint = parse_frontmatter_hint(&frontmatter, &source_key)?;
    let body = body.trim_end_matches('\n').to_string();
    let cwd = cwd_from_encoded_path(path, root);
    let title = frontmatter_hint.get("name").and_then(Value::as_str).map(str::to_string);
    let sections = collect_substantive_sections(&body);
    if sections.len() >= 3 {
        Ok(sections
            .into_iter()
            .map(|section| {
                build_memory(ClaudeCandidateInput {
                    source_key: format!("{source_key}#{}", slugify(&section.heading)),
                    path,
                    cwd: cwd.clone(),
                    title: Some(combine_title(title.as_deref(), &section.heading)),
                    frontmatter_hint: extend_hint(&frontmatter_hint, &section.heading),
                    body: section.body,
                })
            })
            .collect())
    } else {
        Ok(vec![build_memory(ClaudeCandidateInput { source_key, path, cwd, title, frontmatter_hint, body })])
    }
}

struct ClaudeCandidateInput<'a> {
    source_key: String,
    path: &'a Path,
    cwd: Option<PathBuf>,
    title: Option<String>,
    frontmatter_hint: BTreeMap<String, Value>,
    body: String,
}

fn build_memory(input: ClaudeCandidateInput<'_>) -> ParsedMemory {
    let ClaudeCandidateInput { source_key, path, cwd, title, frontmatter_hint, body } = input;
    let wiki_links = extract_wiki_links(&body);
    let content_hash = ParsedMemory::compute_content_hash(&frontmatter_hint, &body);
    ParsedMemory {
        source_key,
        source_path: path.to_path_buf(),
        content_hash,
        harness: Harness::ClaudeCode,
        frontmatter_hint,
        body,
        wiki_links,
        cwd,
        title,
    }
}

fn combine_title(parent: Option<&str>, section_heading: &str) -> String {
    match parent {
        Some(parent) => format!("{parent} — {section_heading}"),
        None => section_heading.to_string(),
    }
}

fn extend_hint(parent: &BTreeMap<String, Value>, section_heading: &str) -> BTreeMap<String, Value> {
    let mut hint = parent.clone();
    if let Some(Value::String(parent_name)) = hint.get("name").cloned() {
        hint.insert("name".to_string(), Value::String(format!("{parent_name} — {section_heading}")));
    } else {
        hint.insert("name".to_string(), Value::String(section_heading.to_string()));
    }
    hint
}

fn source_key_for(path: &Path, root: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    format!("claude:{}", relative.display().to_string().replace('\\', "/"))
}

fn split_frontmatter_and_body(text: &str) -> Result<(String, String), String> {
    if !text.starts_with("---") {
        return Ok((String::new(), text.to_string()));
    }
    let after_first = &text[3..];
    let after_first = after_first.strip_prefix('\n').unwrap_or(after_first);
    let Some(end) = after_first.find("\n---") else {
        return Err("frontmatter delimiter `---` not closed".to_string());
    };
    let frontmatter = after_first[..end].to_string();
    let mut body_start = end + 4;
    if after_first.as_bytes().get(body_start).copied() == Some(b'\n') {
        body_start += 1;
    }
    let body = after_first.get(body_start..).unwrap_or("").to_string();
    Ok((frontmatter, body))
}

fn parse_frontmatter_hint(yaml: &str, source_key: &str) -> ImportResult<BTreeMap<String, Value>> {
    if yaml.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let value: serde_yaml::Value = serde_yaml::from_str(yaml).map_err(|error| ImportError::Parse {
        source_key: source_key.to_string(),
        reason: format!("frontmatter YAML: {error}"),
    })?;
    let Some(mapping) = value.as_mapping() else {
        return Err(ImportError::Parse {
            source_key: source_key.to_string(),
            reason: "frontmatter must be a YAML mapping".to_string(),
        });
    };
    let mut hint = BTreeMap::new();
    for (key, val) in mapping {
        let key = key.as_str().ok_or_else(|| ImportError::Parse {
            source_key: source_key.to_string(),
            reason: "frontmatter keys must be strings".to_string(),
        })?;
        let json = serde_json::to_value(val).map_err(|error| ImportError::Parse {
            source_key: source_key.to_string(),
            reason: format!("frontmatter value coercion: {error}"),
        })?;
        hint.insert(key.to_string(), json);
    }
    Ok(hint)
}

#[derive(Debug)]
struct Section {
    heading: String,
    body: String,
}

fn collect_substantive_sections(body: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_lines: Vec<&str> = Vec::new();
    for line in body.lines() {
        if let Some(stripped) = line.strip_prefix("## ") {
            if let Some(heading) = current_heading.take() {
                flush_section(&mut sections, heading, std::mem::take(&mut current_lines));
            }
            current_heading = Some(stripped.trim().trim_end_matches(':').to_string());
        } else if current_heading.is_some() {
            current_lines.push(line);
        }
    }
    if let Some(heading) = current_heading.take() {
        flush_section(&mut sections, heading, current_lines);
    }
    sections
}

fn flush_section(sections: &mut Vec<Section>, heading: String, lines: Vec<&str>) {
    let body = lines.join("\n");
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return;
    }
    if BOILERPLATE_HEADINGS.iter().any(|boiler| heading.eq_ignore_ascii_case(boiler)) {
        return;
    }
    if trimmed.lines().filter(|line| !line.trim().is_empty()).count() < 3 {
        return;
    }
    sections.push(Section { heading, body: trimmed.to_string() });
}

/// Claude's per-project directories are named like
/// `-Users-treygoff-Code-atlasos` for `/Users/treygoff/Code/atlasos`. The
/// transformation is: leading `-` plus separator-`-` for `/`. The encoding is
/// lossy — a literal `-` inside a path segment (`/Users/x/agent-memory`) is
/// indistinguishable from a separator — so we resolve against the live
/// filesystem first and only fall back to the naive every-`-`-is-`/` decode
/// when no on-disk directory matches (e.g. the path no longer exists on this
/// machine). The fallback keeps cwd hinting best-effort; the project-mapper
/// may prompt or skip when handed a phantom path.
fn cwd_from_encoded_path(path: &Path, root: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(root).ok()?;
    let mut components = relative.components();
    let encoded = components.next()?.as_os_str().to_str()?;
    let encoded = encoded.strip_prefix('-')?;
    let segments: Vec<&str> = encoded.split('-').collect();
    if let Some(existing) = resolve_existing_path(Path::new("/"), &segments) {
        return Some(existing);
    }
    Some(PathBuf::from(format!("/{}", segments.join("/"))))
}

/// Resolve an ambiguous hyphen-encoded path against the filesystem: each
/// boundary between segments is either a `/` or a literal `-`. Boundaries are
/// tried as separators first, matching the historical decode for paths with no
/// hyphenated segments.
fn resolve_existing_path(base: &Path, segments: &[&str]) -> Option<PathBuf> {
    if segments.is_empty() {
        return Some(base.to_path_buf());
    }
    for end in 1..=segments.len() {
        let candidate = base.join(segments[..end].join("-"));
        if candidate.is_dir() {
            if let Some(resolved) = resolve_existing_path(&candidate, &segments[end..]) {
                return Some(resolved);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fixture(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&path, body).expect("write");
        path
    }

    fn run(root: &Path) -> ClaudeParseOutput {
        parse(root).expect("parse ok")
    }

    #[test]
    fn empty_root_returns_empty_output_without_error() {
        let tmp = tempfile::tempdir().expect("tmp");
        let out = run(tmp.path());
        assert!(out.candidates.is_empty());
        assert!(out.errors.is_empty());
    }

    #[test]
    fn missing_root_returns_empty_without_error() {
        let path = PathBuf::from("/does/not/exist");
        let out = parse(&path).expect("missing root is ok");
        assert!(out.candidates.is_empty());
    }

    #[test]
    fn single_fact_topic_produces_one_parsed_memory() {
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"---\nname: Build commands\ntype: reference\n---\nUse `cargo build --release` for prod builds.\n";
        write_fixture(tmp.path(), "-Users-u-x/memory/build-commands.md", body);
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1);
        assert_eq!(out.candidates[0].title.as_deref(), Some("Build commands"));
        assert!(out.candidates[0].body.contains("cargo build --release"));
        assert_eq!(out.candidates[0].harness, Harness::ClaudeCode);
    }

    #[test]
    fn multi_section_dossier_decomposes_into_one_memory_per_substantive_section() {
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"---\nname: AtlasOS notes\ntype: reference\n---\n\
## Build setup\n\
Run `make build`.\n\
This sets up the toolchain.\n\
Toolchain is pinned to 1.82.\n\
\n\
## Why:\n\
Reproducibility for CI builds.\n\
\n\
## Test policy\n\
We use nextest for parallelism.\n\
Coverage threshold is 80%.\n\
Failures block merge.\n\
\n\
## Release flow\n\
Tag, run release script, push.\n\
Verify staging.\n\
Promote to prod.\n";
        write_fixture(tmp.path(), "-Users-u-atlasos/memory/atlasos.md", body);
        let out = run(tmp.path());
        // Three substantive sections; `## Why:` is boilerplate and filtered out.
        assert_eq!(out.candidates.len(), 3);
        let headings: Vec<&str> = out.candidates.iter().filter_map(|c| c.title.as_deref()).collect();
        assert!(headings.iter().any(|h| h.contains("Build setup")), "headings: {headings:?}");
        assert!(headings.iter().any(|h| h.contains("Test policy")));
        assert!(headings.iter().any(|h| h.contains("Release flow")));
        // Parent name extension contract.
        for heading in &headings {
            assert!(heading.starts_with("AtlasOS notes — "), "got: {heading}");
        }
    }

    #[test]
    fn memory_md_index_files_are_skipped() {
        let tmp = tempfile::tempdir().expect("tmp");
        write_fixture(tmp.path(), "-Users-u-x/memory/MEMORY.md", b"# Index\n- foo: ./foo.md\n");
        write_fixture(tmp.path(), "-Users-u-x/memory/foo.md", b"---\nname: Foo\n---\nA single fact.\n");
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1, "MEMORY.md not parsed; foo.md parsed");
        assert_eq!(out.candidates[0].title.as_deref(), Some("Foo"));
    }

    #[test]
    fn user_profile_md_is_parsed_like_any_topic_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        write_fixture(
            tmp.path(),
            "-Users-u-x/memory/user_profile.md",
            b"---\nname: User profile\ntype: profile\n---\nPrefers monospace fonts.\n",
        );
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1);
        assert_eq!(out.candidates[0].title.as_deref(), Some("User profile"));
    }

    #[test]
    fn wiki_links_in_body_are_extracted_into_candidate() {
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"---\nname: Linked\n---\nSee [[Other Topic]] and [[ build commands ]] for context.\n";
        write_fixture(tmp.path(), "-Users-u-x/memory/linked.md", body);
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1);
        assert_eq!(out.candidates[0].wiki_links, vec!["Other Topic".to_string(), "build commands".to_string()]);
    }

    #[test]
    fn malformed_frontmatter_yields_per_file_error_and_does_not_abort_others() {
        let tmp = tempfile::tempdir().expect("tmp");
        write_fixture(
            tmp.path(),
            "-Users-u-x/memory/bad.md",
            b"---\n: this is not valid yaml ::\n  :: blip\n---\nBody after broken YAML\n",
        );
        write_fixture(tmp.path(), "-Users-u-x/memory/good.md", b"---\nname: Good\n---\nA fine memory.\n");
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1, "good.md still imported");
        assert_eq!(out.candidates[0].title.as_deref(), Some("Good"));
        assert!(out
            .errors
            .iter()
            .any(|e| matches!(e, ImportError::Parse { source_key, .. } if source_key.contains("bad.md"))));
    }

    #[test]
    fn non_utf8_file_yields_encoding_error_and_does_not_abort_others() {
        let tmp = tempfile::tempdir().expect("tmp");
        write_fixture(tmp.path(), "-Users-u-x/memory/binary.md", &[0xff, 0xfe, 0xfd, 0xfc]);
        write_fixture(tmp.path(), "-Users-u-x/memory/good.md", b"---\nname: Good\n---\nA fine memory.\n");
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1, "good.md still imported");
        assert!(out
            .errors
            .iter()
            .any(|e| matches!(e, ImportError::Encoding { source_key, .. } if source_key.contains("binary.md"))));
    }

    #[test]
    fn multi_section_boilerplate_headings_do_not_count_toward_threshold() {
        // Only one substantive section after `## Why:` / `## How:` / `## When:` are
        // filtered, so the file stays as a single ParsedMemory.
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"---\nname: Tip\n---\n## Why:\nBecause it helps.\n\n## How:\nLike this.\n\n## When:\nAlways.\n\n## Real section\nLine 1\nLine 2\nLine 3\n";
        write_fixture(tmp.path(), "-Users-u-x/memory/tip.md", body);
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1, "no decomposition when only one non-boilerplate section");
    }

    #[test]
    fn cwd_from_encoded_directory_recovers_original_absolute_path() {
        let tmp = tempfile::tempdir().expect("tmp");
        write_fixture(tmp.path(), "-Users-treygoff-Code-atlasos/memory/x.md", b"---\nname: X\n---\nbody\n");
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1);
        assert_eq!(out.candidates[0].cwd.as_deref(), Some(Path::new("/Users/treygoff/Code/atlasos")),);
    }

    #[test]
    fn resolve_existing_path_recovers_hyphenated_segment() {
        let tmp = tempfile::tempdir().expect("tmp");
        let real = tmp.path().join("Code").join("agent-memory");
        std::fs::create_dir_all(&real).expect("mkdir");
        // Encode the real path the way Claude does: '/' -> '-'.
        let encoded_root = tmp.path().to_str().expect("utf8").replace('/', "-");
        let encoded = format!("{encoded_root}-Code-agent-memory");
        let segments: Vec<&str> = encoded.trim_start_matches('-').split('-').collect();
        let resolved = resolve_existing_path(Path::new("/"), &segments).expect("resolves");
        assert_eq!(resolved, real);
    }

    #[test]
    fn resolve_existing_path_prefers_separator_when_unambiguous() {
        let tmp = tempfile::tempdir().expect("tmp");
        let real = tmp.path().join("Code").join("atlasos");
        std::fs::create_dir_all(&real).expect("mkdir");
        let encoded_root = tmp.path().to_str().expect("utf8").replace('/', "-");
        let encoded = format!("{encoded_root}-Code-atlasos");
        let segments: Vec<&str> = encoded.trim_start_matches('-').split('-').collect();
        let resolved = resolve_existing_path(Path::new("/"), &segments).expect("resolves");
        assert_eq!(resolved, real);
    }

    /// Shared-profile setups (e.g. C-Mux's ~/.claude-shared migration) replace
    /// `<encoded>/memory/` with a symlink to a common store; discovery must
    /// follow it or the whole project's memories silently vanish.
    #[cfg(unix)]
    #[test]
    fn symlinked_memory_dir_is_followed() {
        let tmp = tempfile::tempdir().expect("tmp");
        let shared = tmp.path().join("shared-store");
        std::fs::create_dir_all(&shared).expect("mkdir shared");
        std::fs::write(shared.join("fact.md"), b"---\nname: Fact\n---\nbody\n").expect("write");
        let root = tmp.path().join("projects");
        let project = root.join("-Users-u-x");
        std::fs::create_dir_all(&project).expect("mkdir project");
        std::os::unix::fs::symlink(&shared, project.join("memory")).expect("symlink");

        let out = run(&root);
        assert_eq!(out.candidates.len(), 1, "memories behind a symlinked memory dir are discovered");
    }

    #[test]
    fn cwd_from_encoded_directory_falls_back_to_naive_decode_for_missing_paths() {
        let tmp = tempfile::tempdir().expect("tmp");
        write_fixture(tmp.path(), "-no-such-root-anywhere-xyz/memory/x.md", b"---\nname: X\n---\nbody\n");
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1);
        assert_eq!(out.candidates[0].cwd.as_deref(), Some(Path::new("/no/such/root/anywhere/xyz")),);
    }
}
