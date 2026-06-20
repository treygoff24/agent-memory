//! Per-harness source parsers. Each parser reads a discovered memory root and
//! emits a `Vec<ParsedMemory>` for the pipeline to dedup, plan, and write.
//!
//! - `claude` (T02): parses `~/.claude/projects/<encoded>/memory/<topic>.md`
//!   files plus `user_profile.md`. Handles the single-fact vs. multi-section
//!   dossier split.
//! - `codex` (T03): parses `~/.codex/memories/MEMORY.md` Task Groups plus
//!   `extensions/ad_hoc/notes/*.md`.

pub mod claude;
pub mod codex;

use std::collections::BTreeMap;

use regex::Regex;
use serde_json::Value;

use crate::import::candidate::ParsedMemory;

/// Lowercased alias forms a candidate answers to when resolving `[[wiki-link]]`
/// targets: its title, the short (final path segment) of its `source_key`, and
/// that segment's filename stem. Yielded in title → short → stem order so the
/// first-write-wins `HashMap::entry` semantics at every call site (planning's
/// id index and the topo-sort key index) agree on which alias claims a key.
pub(super) fn candidate_aliases(candidate: &ParsedMemory) -> Vec<String> {
    let mut aliases = Vec::new();
    if let Some(title) = &candidate.title {
        aliases.push(title.to_ascii_lowercase());
    }
    if let Some(short) = candidate.source_key.rsplit('/').next() {
        aliases.push(short.to_ascii_lowercase());
        if let Some(stem) = short.rsplit_once('.').map(|(s, _)| s) {
            aliases.push(stem.to_ascii_lowercase());
        }
    }
    aliases
}

/// Provenance tag stamped into every imported candidate's `frontmatter_hint`.
/// The import dir *is* the harness's native auto-memory store, so everything
/// the importer reads is harness-authored auto-memory rather than a
/// hand-curated note. Downstream ranking uses this to keep imported candidates
/// below human-authored memory (see [`AUTO_MEMORY_CONFIDENCE`]).
pub(super) const AUTO_MEMORY_PROVENANCE: &str = "harness-auto-memory";

/// Confidence stamped into imported candidates so recall ranking favors
/// human-authored content. Below the hand-written baseline (`0.85`) yet above
/// the Reality Check review floor — same intent the pipeline already encodes,
/// surfaced here per-candidate so the provenance signal is self-describing.
pub(super) const AUTO_MEMORY_CONFIDENCE: f64 = 0.7;

/// Memorum recall-block opening markers. The import dir doubles as the
/// harness's native auto-memory store, and passive recall injects these blocks
/// into sessions; a harness can write that injected text back into its store,
/// which the importer would otherwise re-ingest as a fresh user memory — a
/// re-ingestion loop. These XML-ish wrappers are emitted only by Stream E's
/// renderer (`recall/render.rs`), never by a hand-written note, so matching a
/// line that opens one of them is a robust, conservative re-import signal.
const MEMORUM_BLOCK_OPENERS: &[&str] = &["<memory-recall", "<memory-delta", "<recall-explanation"];

/// Strip Memorum recall blocks out of `body`, returning the surviving content.
///
/// Memorum-emitted blocks are bounded XML-ish regions: an opening marker line
/// (`<memory-recall …>`, `<memory-delta …>`, `<recall-explanation …>`, or the
/// self-closing `<memory-delta empty="true" />`) through the matching closing
/// tag on its own line. We drop every line from an opener to its closer
/// inclusive; a self-closing opener drops just that line. Conservatism: only
/// lines that *begin* (after trimming leading whitespace) with one of the
/// known wrapper markers start a skip, so ordinary prose that merely mentions
/// the word "memory" survives untouched. An unterminated opener (truncated
/// paste) drops to end-of-input, since the remainder is Memorum tail, not a
/// user note.
pub(super) fn strip_memorum_recall_blocks(body: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut skipping_until: Option<&'static str> = None;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(closer) = skipping_until {
            if line.trim_start().starts_with(closer) {
                skipping_until = None;
            }
            continue;
        }
        if let Some(closer) = memorum_block_closer(trimmed) {
            // A self-closing opener (`closer` empty) is a single dropped line;
            // a paired opener begins a skip run until its closing tag.
            if !closer.is_empty() {
                skipping_until = Some(closer);
            }
            continue;
        }
        kept.push(line);
    }
    kept.join("\n")
}

/// If `trimmed` opens a Memorum recall block, return the closing tag to skip to
/// (`""` for a self-closing opener that needs no closer). Returns `None` for an
/// ordinary line.
fn memorum_block_closer(trimmed: &str) -> Option<&'static str> {
    if !MEMORUM_BLOCK_OPENERS.iter().any(|opener| trimmed.starts_with(opener)) {
        return None;
    }
    // A self-closing tag (`… />`) carries its own terminator; nothing to skip to.
    if trimmed.ends_with("/>") {
        return Some("");
    }
    if trimmed.starts_with("<memory-recall") {
        Some("</memory-recall>")
    } else if trimmed.starts_with("<memory-delta") {
        Some("</memory-delta>")
    } else {
        Some("</recall-explanation>")
    }
}

/// Stamp the auto-memory provenance + confidence hints onto a candidate's
/// `frontmatter_hint`. Both Claude and Codex sources read from native
/// auto-memory stores, so both call this. Existing author-supplied keys win:
/// if a memory already declares its own `confidence`/`source_provenance`, we
/// don't clobber it. The downstream pipeline reads these to keep imported
/// candidates below human-authored memory in recall ranking.
pub(super) fn stamp_auto_memory_provenance(frontmatter_hint: &mut BTreeMap<String, Value>) {
    frontmatter_hint
        .entry("source_provenance".to_string())
        .or_insert_with(|| Value::String(AUTO_MEMORY_PROVENANCE.to_string()));
    frontmatter_hint.entry("confidence".to_string()).or_insert_with(|| Value::from(AUTO_MEMORY_CONFIDENCE));
}

/// Extract `[[wiki-link]]` aliases from a memory body, de-duplicated and in
/// first-seen order. Shared by the per-harness parsers, which embed identical
/// wiki-link syntax in their source documents.
fn extract_wiki_links(body: &str) -> Vec<String> {
    let pattern = Regex::new(r"\[\[([^\]\n]+?)\]\]").expect("static regex compiles");
    let mut seen = std::collections::BTreeSet::new();
    let mut links = Vec::new();
    for capture in pattern.captures_iter(body) {
        let alias = capture[1].trim().to_string();
        if alias.is_empty() {
            continue;
        }
        if seen.insert(alias.clone()) {
            links.push(alias);
        }
    }
    links
}

/// Lowercase, hyphen-separated slug used to build stable `source_key` anchors
/// from section headings / task-group names.
fn slugify(value: &str) -> String {
    let mut slug = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_string()
}
