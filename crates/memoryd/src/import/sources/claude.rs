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

use super::{extract_wiki_links, slugify, stamp_auto_memory_provenance, strip_memorum_recall_blocks};
use crate::import::candidate::{Harness, ParsedMemory};
use crate::import::{ImportError, ImportResult};

/// `##` headings that are scaffolding rather than substantive sections. The
/// multi-section heuristic ignores them when counting whether a topic file is
/// a dossier worth decomposing.
const BOILERPLATE_HEADINGS: &[&str] =
    &["why", "how to apply", "how", "when", "why this matters", "references", "links", "context"];

/// Output bundle for a Claude memory root parse. `candidates` carries the
/// successful parses; `errors` carries per-file failures so the import report
/// can surface them without aborting the whole import; `recovered` carries the
/// source keys of files whose frontmatter strict YAML rejected but the lenient
/// line-scan fallback salvaged, so the report can surface them as soft
/// recoveries rather than silently dropping real memories.
#[derive(Debug, Default)]
pub struct ClaudeParseOutput {
    pub candidates: Vec<ParsedMemory>,
    pub errors: Vec<ImportError>,
    pub recovered: Vec<String>,
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
    //
    // filter_entry prunes dream-scratch project directories and their entire
    // subtrees from the walk. Their `memory/` subdirs are always empty (zero
    // candidates), so descending into them on every import is pure wasted cost
    // on machines that dream frequently. filter_entry composes with follow_links.
    for entry in walkdir::WalkDir::new(root).follow_links(true).into_iter().filter_entry(|e| !is_dream_scratch_dir(e)) {
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
            Ok(parsed) => {
                if parsed.recovered {
                    output.recovered.push(source_key_for(path, root));
                }
                output.candidates.extend(parsed.memories);
            }
            Err(error) => output.errors.push(error),
        }
    }
    output.candidates.sort_by(|a, b| a.source_key.cmp(&b.source_key));
    output.recovered.sort();
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

/// Result of parsing one topic file: the candidate memories plus whether the
/// frontmatter had to be salvaged via the lenient line-scan fallback (because
/// strict `serde_yaml` rejected an otherwise-valid-intent mapping).
struct TopicParse {
    memories: Vec<ParsedMemory>,
    recovered: bool,
}

fn parse_topic_file(path: &Path, root: &Path) -> ImportResult<TopicParse> {
    let source_key = source_key_for(path, root);
    let raw = std::fs::read(path).map_err(|error| ImportError::io(path, error))?;
    let text = std::str::from_utf8(&raw)
        .map_err(|error| ImportError::Encoding { source_key: source_key.clone(), reason: error.to_string() })?
        .to_string();
    // An unterminated `---` frontmatter block is a genuine structural error and
    // stays hard — we cannot trust where the body begins.
    let (frontmatter, body) = split_frontmatter_and_body(&text)
        .map_err(|reason| ImportError::Parse { source_key: source_key.clone(), reason })?;
    // Strict YAML first; on a *mapping-parse* failure fall back to a tolerant
    // line scan rather than dropping a real memory whose frontmatter merely
    // trips a reserved YAML indicator (`: `, leading backtick, dangling quote).
    let (frontmatter_hint, recovered) = match parse_frontmatter_hint(&frontmatter, &source_key) {
        Ok(hint) => (hint, false),
        Err(ImportError::Parse { .. }) => (lenient_frontmatter_hint(&frontmatter), true),
        Err(other) => return Err(other),
    };
    // Drop any Memorum recall blocks a harness may have written back into its
    // native store before parsing — they are our own injected text, not a fresh
    // user memory, and re-ingesting them would amplify a re-ingestion loop.
    let body = strip_memorum_recall_blocks(&body);
    let body = body.trim_end_matches('\n').to_string();
    // A topic file that was *entirely* a pasted Memorum block leaves nothing
    // behind; emit no candidate rather than a hollow one.
    if body.trim().is_empty() {
        return Ok(TopicParse { memories: Vec::new(), recovered });
    }
    let cwd = cwd_from_encoded_path(path, root);
    let title = frontmatter_hint.get("name").and_then(Value::as_str).map(str::to_string);
    let sections = collect_substantive_sections(&body);
    let memories = if sections.len() >= 3 {
        sections
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
            .collect()
    } else {
        vec![build_memory(ClaudeCandidateInput { source_key, path, cwd, title, frontmatter_hint, body })]
    };
    Ok(TopicParse { memories, recovered })
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
    let ClaudeCandidateInput { source_key, path, cwd, title, mut frontmatter_hint, body } = input;
    stamp_auto_memory_provenance(&mut frontmatter_hint);
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

/// Lenient frontmatter recovery for files that `serde_yaml` rejects but whose
/// intent is plainly a flat `key: scalar` mapping. Strict YAML rejects values
/// that happen to contain a reserved indicator — an unquoted `: ` (read as a
/// nested mapping), a leading backtick or quote, a trailing unquoted run after
/// a quote. We salvage these by line-scanning: for each `key: rest-of-line`
/// line we take the rest of the line verbatim as a JSON string, stripping a
/// surrounding matched quote/backtick pair only when it wraps the *entire*
/// value. Multi-line / block YAML values are ignored — this is a best-effort
/// floor that at minimum recovers `name`, `description`, and `type`.
fn lenient_frontmatter_hint(yaml: &str) -> BTreeMap<String, Value> {
    let mut hint = BTreeMap::new();
    for line in yaml.lines() {
        let Some((key, value)) = split_simple_key_value(line) else {
            continue;
        };
        let value = value.trim();
        // A block-scalar header (`|`, `>`, with optional chomping/indent
        // indicators) carries its content on the following indented lines, which
        // this flat line-scan deliberately skips. Recording the sentinel as the
        // literal value would store a wrong `"|"`; skip the field instead.
        if is_block_scalar_header(value) {
            continue;
        }
        let value = unwrap_matched_quotes(value);
        hint.insert(key.to_string(), Value::String(value.to_string()));
    }
    hint
}

/// True for a YAML block-scalar header: `|` or `>` optionally followed by a
/// chomping indicator (`+`/`-`) and/or an explicit indent digit, with nothing
/// else on the line. Such a value's content lives on subsequent indented lines.
fn is_block_scalar_header(value: &str) -> bool {
    let mut chars = value.chars();
    if !matches!(chars.next(), Some('|' | '>')) {
        return false;
    }
    chars.all(|c| c == '+' || c == '-' || c.is_ascii_digit())
}

/// Match a `^(\w[\w-]*):\s*(.*)$` line and return `(key, rest_of_line)`.
/// Returns `None` for indented lines, comments, list items, or lines whose key
/// is not a bare word — those are not simple top-level scalar assignments.
fn split_simple_key_value(line: &str) -> Option<(&str, &str)> {
    // Reject indentation: a leading space/tab means this is a nested/block value.
    if line.starts_with([' ', '\t']) {
        return None;
    }
    let colon = line.find(':')?;
    let key = &line[..colon];
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return None;
    }
    // The key must start with a word character, mirroring `\w[\w-]*`.
    let first = key.chars().next()?;
    if !(first.is_ascii_alphanumeric() || first == '_') {
        return None;
    }
    let rest = line[colon + 1..].trim_start_matches([' ', '\t']);
    Some((key, rest))
}

/// Strip a surrounding matched quote or backtick pair only when it wraps the
/// whole value (length ≥ 2, identical first/last delimiter). Anything else —
/// including a value that merely *opens* a quote then trails unquoted — is kept
/// verbatim, since the raw text is the closest thing to the author's intent.
fn unwrap_matched_quotes(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if first == last && matches!(first, b'"' | b'\'' | b'`') {
            return &value[1..value.len() - 1];
        }
    }
    value
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

/// Returns `true` when `entry` is a directory whose file name contains the
/// substring `memoryd-dream-scratch-run`. These are ephemeral scratch
/// directories created by the dreaming subsystem; their `memory/` subdirs are
/// always empty, so descending into them yields zero candidates and is pure
/// walk overhead. Returning `true` here causes `filter_entry` to skip the
/// entire subtree.
///
/// The predicate is safe to apply to every entry including the walk root and
/// ordinary project dirs: the substring `memoryd-dream-scratch-run` cannot
/// appear in a normal project directory name (which encodes a file-system path
/// using only hyphens as separators), so no legitimate directory is ever pruned.
fn is_dream_scratch_dir(entry: &walkdir::DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }
    entry.file_name().to_str().map(|name| name.contains("memoryd-dream-scratch-run")).unwrap_or(false)
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
///
/// Claude also encodes a leading `.` in a directory name as `-`, so a dotfile
/// dir like `.config` arrives indistinguishable from a separator + `config`.
/// For each candidate directory component we therefore probe both the bare name
/// and a `.`-prefixed variant, letting a real `.config`/`.codex`-style dir
/// resolve where the bare name would not stat.
fn resolve_existing_path(base: &Path, segments: &[&str]) -> Option<PathBuf> {
    if segments.is_empty() {
        return Some(base.to_path_buf());
    }
    for end in 1..=segments.len() {
        let joined = segments[..end].join("-");
        for component in [joined.clone(), format!(".{joined}")] {
            let candidate = base.join(&component);
            if candidate.is_dir() {
                if let Some(resolved) = resolve_existing_path(&candidate, &segments[end..]) {
                    return Some(resolved);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::AUTO_MEMORY_PROVENANCE;
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
    fn malformed_frontmatter_is_recovered_via_lenient_fallback_not_dropped() {
        // The old behavior dropped this file with a per-file Parse error; the
        // lenient line-scan now salvages whatever flat `key: value` lines exist
        // (here none) but still produces a candidate with the body intact rather
        // than losing the memory.
        let tmp = tempfile::tempdir().expect("tmp");
        write_fixture(
            tmp.path(),
            "-Users-u-x/memory/bad.md",
            b"---\n: this is not valid yaml ::\n  :: blip\n---\nBody after broken YAML\n",
        );
        write_fixture(tmp.path(), "-Users-u-x/memory/good.md", b"---\nname: Good\n---\nA fine memory.\n");
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 2, "both files imported; bad.md recovered");
        assert!(out.errors.is_empty(), "lenient recovery does not push a parse error: {:?}", out.errors);
        let bad = out.candidates.iter().find(|c| c.source_key.contains("bad.md")).expect("bad.md candidate");
        assert!(bad.body.contains("Body after broken YAML"), "body preserved: {:?}", bad.body);
        assert!(out.recovered.iter().any(|k| k.contains("bad.md")), "recovered: {:?}", out.recovered);
    }

    #[test]
    fn lenient_recovery_handles_colon_space_in_unquoted_value() {
        // `name: ... — agency: disagree ...` — the inner `: ` makes strict YAML
        // read a nested mapping. Lenient scan keeps the whole rest-of-line.
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"---\nname: Feedback \xe2\x80\x94 agency: disagree with skill guidance and build anyway\ntype: feedback\n---\nThe agent should push back.\n";
        write_fixture(tmp.path(), "-Users-u-x/memory/agency.md", body);
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1);
        let c = &out.candidates[0];
        assert!(c.body.contains("The agent should push back."), "body: {:?}", c.body);
        assert_eq!(c.title.as_deref(), Some("Feedback — agency: disagree with skill guidance and build anyway"),);
        assert_eq!(c.frontmatter_hint.get("type").and_then(Value::as_str), Some("feedback"));
        assert!(out.recovered.iter().any(|k| k.contains("agency.md")), "recovered: {:?}", out.recovered);
    }

    #[test]
    fn lenient_recovery_handles_leading_backtick_value() {
        // A value starting with a backtick is a reserved YAML indicator and is
        // rejected by strict parsing; the lenient scan keeps it verbatim.
        let tmp = tempfile::tempdir().expect("tmp");
        let body =
            b"---\nname: Browser tooling\ndescription: `agent-browser click` drives the page\n---\nUse the CLI.\n";
        write_fixture(tmp.path(), "-Users-u-x/memory/backtick.md", body);
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1);
        let c = &out.candidates[0];
        assert!(c.body.contains("Use the CLI."), "body: {:?}", c.body);
        assert_eq!(c.title.as_deref(), Some("Browser tooling"));
        assert_eq!(
            c.frontmatter_hint.get("description").and_then(Value::as_str),
            Some("`agent-browser click` drives the page"),
        );
        assert!(out.recovered.iter().any(|k| k.contains("backtick.md")), "recovered: {:?}", out.recovered);
    }

    #[test]
    fn lenient_recovery_handles_quote_open_then_trailing_unquoted() {
        // A value that opens a quote then trails unquoted text is malformed YAML;
        // the unmatched leading quote is kept verbatim (only a *matched* wrap is
        // stripped) so the author's text survives.
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"---\nname: \"Shape of policy\" doc altitude \xe2\x80\x94 voice calibration\ntype: note\n---\nAltitude matters.\n";
        write_fixture(tmp.path(), "-Users-u-x/memory/altitude.md", body);
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1);
        let c = &out.candidates[0];
        assert!(c.body.contains("Altitude matters."), "body: {:?}", c.body);
        // Leading quote is unmatched (no closing quote at end-of-value) → kept.
        assert_eq!(c.title.as_deref(), Some("\"Shape of policy\" doc altitude — voice calibration"),);
        assert_eq!(c.frontmatter_hint.get("type").and_then(Value::as_str), Some("note"));
        assert!(out.recovered.iter().any(|k| k.contains("altitude.md")), "recovered: {:?}", out.recovered);
    }

    #[test]
    fn is_block_scalar_header_matches_pipe_and_fold_with_indicators() {
        assert!(is_block_scalar_header("|"));
        assert!(is_block_scalar_header(">"));
        assert!(is_block_scalar_header("|-"));
        assert!(is_block_scalar_header(">+"));
        assert!(is_block_scalar_header("|2"));
        // Plain values that merely start with these bytes but carry inline text
        // are not headers — they're kept verbatim.
        assert!(!is_block_scalar_header("| not a header"));
        assert!(!is_block_scalar_header("plain"));
        assert!(!is_block_scalar_header(""));
    }

    #[test]
    fn lenient_recovery_skips_block_scalar_value_instead_of_storing_sentinel() {
        // The `name:` line fails strict YAML (colon in value), forcing the lenient
        // scan; a `description: |` block header must be skipped, not recorded as
        // the literal "|" (its content lives on indented lines this scan ignores).
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"---\nname: agency: disagree and build\ndescription: |\ntype: feedback\n---\nThe body.\n";
        write_fixture(tmp.path(), "-Users-u-x/memory/block.md", body);
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1);
        let c = &out.candidates[0];
        assert!(c.body.contains("The body."), "body: {:?}", c.body);
        assert_eq!(c.frontmatter_hint.get("type").and_then(Value::as_str), Some("feedback"));
        assert!(
            !c.frontmatter_hint.contains_key("description"),
            "block-scalar header must be skipped, not stored: {:?}",
            c.frontmatter_hint.get("description"),
        );
        assert!(out.recovered.iter().any(|k| k.contains("block.md")), "recovered: {:?}", out.recovered);
    }

    #[test]
    fn strict_yaml_success_is_not_marked_recovered() {
        // Control: a clean frontmatter parses strictly and must not appear in
        // `recovered`.
        let tmp = tempfile::tempdir().expect("tmp");
        write_fixture(
            tmp.path(),
            "-Users-u-x/memory/clean.md",
            b"---\nname: Clean\ndescription: nothing weird here\ntype: reference\n---\nAll good.\n",
        );
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1);
        assert_eq!(out.candidates[0].title.as_deref(), Some("Clean"));
        assert!(out.recovered.is_empty(), "clean file not recovered: {:?}", out.recovered);
    }

    #[test]
    fn unterminated_frontmatter_still_errors_and_is_not_recovered() {
        // An unclosed `---` block is a genuine structural error: we cannot trust
        // where the body begins, so it stays hard and skips.
        let tmp = tempfile::tempdir().expect("tmp");
        write_fixture(
            tmp.path(),
            "-Users-u-x/memory/unterminated.md",
            b"---\nname: Truncated\ntype: note\nbody with no closing delimiter\n",
        );
        write_fixture(tmp.path(), "-Users-u-x/memory/good.md", b"---\nname: Good\n---\nA fine memory.\n");
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1, "only good.md imported");
        assert_eq!(out.candidates[0].title.as_deref(), Some("Good"));
        assert!(out
            .errors
            .iter()
            .any(|e| matches!(e, ImportError::Parse { source_key, .. } if source_key.contains("unterminated.md"))));
        assert!(out.recovered.is_empty(), "structural error is not a soft recovery: {:?}", out.recovered);
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
        assert!(out.recovered.is_empty(), "non-utf8 is a hard error, not a soft recovery: {:?}", out.recovered);
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
    fn resolve_existing_path_recovers_dotfile_directory() {
        // Claude encodes both `/` and a leading `.` as `-`, so `~/.config/opencode`
        // arrives as `<root>--config-opencode`. The bare `config` dir does not
        // exist; only the real `.config` dotfile dir stats.
        let tmp = tempfile::tempdir().expect("tmp");
        let real = tmp.path().join(".config").join("opencode");
        std::fs::create_dir_all(&real).expect("mkdir");
        let encoded_root = tmp.path().to_str().expect("utf8").replace('/', "-");
        // The doubled hyphen reflects the `.` that became `-` right after the
        // separator `-`; split on `-` yields an empty segment we must tolerate.
        let encoded = format!("{encoded_root}--config-opencode");
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

    #[test]
    fn dream_scratch_project_dirs_are_pruned_from_walk() {
        // A normal project dir's memory is imported; a memoryd-dream-scratch-run-*
        // dir's memory is skipped entirely (subtree pruned, not just post-hoc
        // filtered), even though the file under it would otherwise be a valid topic.
        let tmp = tempfile::tempdir().expect("tmp");
        write_fixture(
            tmp.path(),
            "-Users-u-real-project/memory/topic.md",
            b"---\nname: Real memory\ntype: reference\n---\nThis should be imported.\n",
        );
        write_fixture(
            tmp.path(),
            "memoryd-dream-scratch-run-abc123/memory/scratch.md",
            b"---\nname: Dream scratch\ntype: reference\n---\nThis must never be imported.\n",
        );
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1, "only the real project memory is imported; dream-scratch is pruned");
        assert_eq!(out.candidates[0].title.as_deref(), Some("Real memory"));
        assert!(
            out.candidates.iter().all(|c| !c.source_key.contains("scratch")),
            "no scratch memory in candidates: {:?}",
            out.candidates.iter().map(|c| &c.source_key).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn pasted_memorum_recall_block_is_skipped_user_note_survives() {
        // Passive recall injects a `<memory-recall>` base block into the session;
        // a harness can write that injected text back into its native store
        // alongside a genuine user note. The importer must drop the recall block
        // (our own emitted content) but keep the user note.
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"---\nname: Project notes\ntype: reference\n---\n\
The deploy script lives in scripts/deploy.sh and needs the prod token.\n\
\n\
<memory-recall version=\"stream-e/v1\" harness=\"claude-code\" session=\"abc\">\n\
  <project-state>\n\
    <memory ref=\"mem_1\" updated=\"2026-06-19\" source=\"import\" confidence=\"0.7\">\n\
      <summary>injected recall summary</summary>\n\
      <snippet>injected recall snippet that must not be re-imported</snippet>\n\
    </memory>\n\
  </project-state>\n\
  <recall-explanation policy=\"stream-e/v1\" budget-tokens=\"1800\" used-tokens=\"42\">\n\
    explanation text\n\
  </recall-explanation>\n\
</memory-recall>\n\
\n\
Always run the migration before deploying.\n";
        write_fixture(tmp.path(), "-Users-u-x/memory/notes.md", body);
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1, "one candidate from the file");
        let candidate = &out.candidates[0];
        assert!(
            candidate.body.contains("scripts/deploy.sh"),
            "user note before the block survives: {:?}",
            candidate.body
        );
        assert!(
            candidate.body.contains("Always run the migration before deploying."),
            "user note after the block survives: {:?}",
            candidate.body,
        );
        assert!(
            !candidate.body.contains("injected recall snippet"),
            "Memorum recall snippet must be stripped: {:?}",
            candidate.body,
        );
        assert!(!candidate.body.contains("<memory-recall"), "no recall wrapper survives: {:?}", candidate.body);
        assert!(
            !candidate.body.contains("<recall-explanation"),
            "no explanation fragment survives: {:?}",
            candidate.body
        );
    }

    #[test]
    fn file_that_is_entirely_a_memorum_block_yields_no_candidate() {
        // A topic file that is *only* a pasted recall block (no real user note)
        // strips down to nothing and must not produce a hollow candidate.
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"---\nname: Pasted\ntype: reference\n---\n\
<memory-delta>\n\
  <item id=\"d1\">delta recall text that was written back</item>\n\
</memory-delta>\n";
        write_fixture(tmp.path(), "-Users-u-x/memory/pasted.md", body);
        let out = run(tmp.path());
        assert!(out.candidates.is_empty(), "pure-recall-block file yields no candidate: {:?}", out.candidates);
        assert!(out.errors.is_empty(), "stripping a recall block is not an error: {:?}", out.errors);
    }

    #[test]
    fn empty_delta_sentinel_is_stripped() {
        // The self-closing `<memory-delta empty="true" />` sentinel is a single
        // line drop — it must not leave a stray fragment in the body.
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"---\nname: With sentinel\ntype: reference\n---\n\
A real fact worth keeping.\n\
<memory-delta empty=\"true\" />\n\
Another real fact.\n";
        write_fixture(tmp.path(), "-Users-u-x/memory/sentinel.md", body);
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1);
        let candidate = &out.candidates[0];
        assert!(candidate.body.contains("A real fact worth keeping."), "body: {:?}", candidate.body);
        assert!(candidate.body.contains("Another real fact."), "body: {:?}", candidate.body);
        assert!(!candidate.body.contains("memory-delta"), "sentinel stripped: {:?}", candidate.body);
    }

    #[test]
    fn prose_mentioning_memory_is_not_stripped() {
        // Conservatism: a user note that merely discusses memory in prose — not
        // opening a Memorum wrapper tag — must survive untouched.
        let tmp = tempfile::tempdir().expect("tmp");
        let body = b"---\nname: Prose\ntype: reference\n---\n\
The memory subsystem records a recall block on each turn.\n\
We discussed memory-delta semantics in the design review.\n";
        write_fixture(tmp.path(), "-Users-u-x/memory/prose.md", body);
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1);
        let candidate = &out.candidates[0];
        assert!(candidate.body.contains("records a recall block"), "prose survives: {:?}", candidate.body);
        assert!(candidate.body.contains("memory-delta semantics"), "inline mention survives: {:?}", candidate.body);
    }

    #[test]
    fn imported_candidate_carries_auto_memory_provenance_and_confidence() {
        // The import dir is the native auto-memory store, so every imported
        // candidate is harness-authored auto-memory; it must carry the
        // provenance tag and a confidence below the hand-written baseline so
        // recall ranking favors human-authored memory.
        let tmp = tempfile::tempdir().expect("tmp");
        write_fixture(
            tmp.path(),
            "-Users-u-x/memory/fact.md",
            b"---\nname: A fact\ntype: reference\n---\nAuto-memory body.\n",
        );
        let out = run(tmp.path());
        assert_eq!(out.candidates.len(), 1);
        let hint = &out.candidates[0].frontmatter_hint;
        assert_eq!(
            hint.get("source_provenance").and_then(Value::as_str),
            Some(AUTO_MEMORY_PROVENANCE),
            "auto-memory provenance stamped: {hint:?}",
        );
        let confidence = hint.get("confidence").and_then(Value::as_f64).expect("confidence present");
        assert!(
            confidence < HUMAN_AUTHORED_CONFIDENCE_BASELINE,
            "auto-memory confidence {confidence} must rank below the hand-written baseline {HUMAN_AUTHORED_CONFIDENCE_BASELINE}",
        );
    }

    /// The hand-written memory confidence baseline the pipeline encodes
    /// (`pipeline.rs` comment: "hand-written `0.85` memories"). Used here only
    /// to assert the relative ordering of auto-memory vs. human-authored.
    const HUMAN_AUTHORED_CONFIDENCE_BASELINE: f64 = 0.85;
}
