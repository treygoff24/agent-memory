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

use regex::Regex;

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
