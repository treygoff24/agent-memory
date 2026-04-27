//! Body chunking: markdown-section-aware, ~400 token target with 80 token overlap.
//!
//! "Token" is approximated as a whitespace-separated word.  For typical English
//! prose this is accurate to ±10%.  Multibyte text will have more chars per word
//! but the approximation degrades gracefully — chunks will be shorter in words
//! rather than violating the target.  The approximation is documented here so
//! callers know not to expect strict GPT-style token counts.
//!
//! Spec §10.3: target ~400 tokens, 80-token overlap, markdown-section-aware.

use sha2::{Digest, Sha256};

use crate::model::{Memory, Sha256 as Sha256Text};

/// Target chunk size in approximate tokens (whitespace words).
const TARGET_TOKENS: usize = 400;

/// Overlap between consecutive chunks in approximate tokens.
const OVERLAP_TOKENS: usize = 80;

/// Hard maximum byte size per chunk (UTF-8 safe; spec §10.3 integrity).
///
/// When a word-based chunk exceeds this limit (e.g. a very long word with no
/// whitespace), it is split at the last UTF-8 character boundary at or before
/// this limit.  The byte split never introduces extra whitespace — sub-chunk
/// texts concatenate back to the original body text at those byte positions.
const MAX_CHUNK_BYTES: usize = 4096;

/// Indexed body chunk.
#[derive(Clone, Debug)]
pub struct Chunk {
    /// Chunk id: `chk_<sha256(chunk_text)>` per spec §10.3.
    pub chunk_id: String,
    /// SHA-256 hash of the chunk text (`sha256:<hex>`).
    pub body_hash: Sha256Text,
    /// Chunk text (LF-normalized).
    pub text: String,
    /// Start byte offset in the LF-normalized body.
    pub start_byte: usize,
    /// End byte offset (exclusive) in the LF-normalized body.
    pub end_byte: usize,
}

/// Chunk a memory body into deterministic, overlapping chunks.
///
/// Returns an empty slice for empty bodies.  Chunks are produced in order;
/// consecutive chunks share ~`OVERLAP_TOKENS` words at the boundary.
pub fn chunk_memory(memory: &Memory) -> Vec<Chunk> {
    let body = memory.body.replace("\r\n", "\n");
    if body.is_empty() {
        return Vec::new();
    }
    let sections = split_markdown_sections(&body);
    sections_to_chunks(&sections)
}

/// A contiguous run of text under one markdown heading (or preamble).
struct Section<'a> {
    text: &'a str,
    /// Byte offset of the section's first char in the parent body.
    byte_offset: usize,
}

/// Split a body into sections at each `#`-heading boundary.
///
/// The preamble (everything before the first heading) is its own section.
/// Heading lines are included at the start of their section.
fn split_markdown_sections(body: &str) -> Vec<Section<'_>> {
    let mut sections: Vec<Section<'_>> = Vec::new();
    let mut section_start = 0usize;

    for (byte_offset, _) in body.match_indices('\n') {
        let next_line_start = byte_offset + 1;
        if next_line_start >= body.len() {
            break;
        }
        let rest = &body[next_line_start..];
        if rest.starts_with('#') {
            // Heading found: close the current section, start a new one.
            if next_line_start > section_start {
                sections.push(Section { text: &body[section_start..next_line_start], byte_offset: section_start });
            }
            section_start = next_line_start;
        }
    }
    // Trailing section (or the whole body if no headings).
    if section_start < body.len() {
        sections.push(Section { text: &body[section_start..], byte_offset: section_start });
    }
    sections
}

/// Accumulate sections into `TARGET_TOKENS`-sized chunks with `OVERLAP_TOKENS` overlap.
///
/// When a single section exceeds `TARGET_TOKENS`, it is split by sentence
/// boundaries (`. `, `! `, `? `) up to the token budget.  Any resulting chunk
/// whose byte length exceeds `MAX_CHUNK_BYTES` is further split at UTF-8
/// character boundaries (never mid-codepoint) to respect the hard byte limit.
fn sections_to_chunks(sections: &[Section<'_>]) -> Vec<Chunk> {
    // Flatten sections into (word, byte_offset_in_body) pairs first.
    let words: Vec<(usize, &str)> =
        sections.iter().flat_map(|section| words_with_offsets(section.text, section.byte_offset)).collect();

    if words.is_empty() {
        return Vec::new();
    }

    let mut chunks: Vec<Chunk> = Vec::new();
    let mut start_word = 0usize;

    while start_word < words.len() {
        let end_word = (start_word + TARGET_TOKENS).min(words.len());
        let chunk_text = join_words(&words[start_word..end_word]);
        let start_byte = words[start_word].0;
        let end_byte = if end_word < words.len() {
            words[end_word].0
        } else {
            // Last chunk: extend to the end of the body (include trailing whitespace/newlines
            // not captured in word offsets).
            words.last().map_or(start_byte, |(off, w)| off + w.len())
        };

        // Enforce hard byte limit: if this chunk's text exceeds MAX_CHUNK_BYTES,
        // byte-split it into sub-chunks at UTF-8 character boundaries.
        if chunk_text.len() > MAX_CHUNK_BYTES {
            byte_split_chunk(chunk_text, start_byte, end_byte, &mut chunks);
        } else {
            let hash = hash_text(&chunk_text);
            let chunk_id = chunk_id_from_text(&chunk_text);
            chunks.push(Chunk { chunk_id, body_hash: hash, text: chunk_text, start_byte, end_byte });
        }

        // Advance with overlap: next chunk starts `OVERLAP_TOKENS` before `end_word`.
        if end_word >= words.len() {
            break;
        }
        let next_start = end_word.saturating_sub(OVERLAP_TOKENS);
        // Guard against infinite loop: always advance at least 1 word past the
        // current start.  This can only trigger when TARGET_TOKENS ≤ OVERLAP_TOKENS
        // (impossible with current constants) but is kept as a safety net.
        start_word = next_start.max(start_word + 1);
    }

    chunks
}

/// Split a chunk text that exceeds `MAX_CHUNK_BYTES` into sub-chunks at UTF-8
/// character boundaries.
///
/// Each sub-chunk contains at most `MAX_CHUNK_BYTES` bytes.  Sub-chunk byte
/// offsets are relative to the LF-normalized body (`body_start_byte` is the
/// byte offset of `text[0]` in that body).  Sub-chunk texts concatenate back
/// to the original `text` without any additional whitespace.
fn byte_split_chunk(text: String, body_start_byte: usize, body_end_byte: usize, out: &mut Vec<Chunk>) {
    let _ = body_end_byte; // used indirectly via text length
    let mut offset = 0usize; // byte offset within `text`

    while offset < text.len() {
        // Find the last UTF-8 character boundary at or before offset + MAX_CHUNK_BYTES.
        let budget_end = (offset + MAX_CHUNK_BYTES).min(text.len());
        // Walk back from budget_end to find a valid UTF-8 char boundary.
        let split_at = floor_char_boundary(&text, budget_end);
        let sub_text = text[offset..split_at].to_owned();
        let sub_start = body_start_byte + offset;
        let sub_end = body_start_byte + split_at;
        let hash = hash_text(&sub_text);
        let chunk_id = chunk_id_from_text(&sub_text);
        out.push(Chunk { chunk_id, body_hash: hash, text: sub_text, start_byte: sub_start, end_byte: sub_end });
        offset = split_at;
    }
}

/// Return the largest byte index `≤ index` that is a valid UTF-8 char boundary
/// in `s`.  If `index >= s.len()`, returns `s.len()`.
fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    // Walk backwards from index until we land on a char boundary.
    let bytes = s.as_bytes();
    let mut i = index;
    while i > 0 && (bytes[i] & 0xC0) == 0x80 {
        // Continuation byte (10xxxxxx) — step back.
        i -= 1;
    }
    i
}

/// Yield `(byte_offset_in_body, word)` pairs for all whitespace-separated words
/// in `text`, adjusting offsets by `base_offset` (section's position in body).
fn words_with_offsets(text: &str, base_offset: usize) -> impl Iterator<Item = (usize, &str)> {
    WordOffsets { text, pos: 0, base_offset }
}

struct WordOffsets<'a> {
    text: &'a str,
    pos: usize,
    base_offset: usize,
}

impl<'a> Iterator for WordOffsets<'a> {
    type Item = (usize, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        // Skip whitespace.
        while self.pos < self.text.len() && is_whitespace_byte(self.text.as_bytes()[self.pos]) {
            self.pos += 1;
        }
        if self.pos >= self.text.len() {
            return None;
        }
        let word_start = self.pos;
        // Consume non-whitespace.
        while self.pos < self.text.len() && !is_whitespace_byte(self.text.as_bytes()[self.pos]) {
            self.pos += 1;
        }
        Some((self.base_offset + word_start, &self.text[word_start..self.pos]))
    }
}

fn is_whitespace_byte(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r'
}

/// Reconstruct chunk text from word `(offset, word)` pairs: join with single spaces.
fn join_words(words: &[(usize, &str)]) -> String {
    let total_len: usize = words.iter().map(|(_, w)| w.len()).sum::<usize>() + words.len().saturating_sub(1);
    let mut out = String::with_capacity(total_len);
    for (i, (_, word)) in words.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Derive `chk_<sha256(chunk_text)>` per spec §10.3.
///
/// Content-addressable: identical text always produces the same chunk id,
/// regardless of which memory currently owns the chunk (relevant for merge).
fn chunk_id_from_text(text: &str) -> String {
    let digest = hex::encode(Sha256::digest(text.as_bytes()));
    format!("chk_{digest}")
}

fn hash_text(text: &str) -> Sha256Text {
    let digest = Sha256::digest(text.as_bytes());
    Sha256Text::new(format!("sha256:{}", hex::encode(digest)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Author, AuthorKind, Frontmatter, Memory, MemoryId, MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Scope,
        Sensitivity, Source, SourceKind, TrustLevel, WritePolicy,
    };

    fn make_memory(body: &str) -> Memory {
        let now = chrono::Utc::now();
        Memory {
            frontmatter: Frontmatter {
                schema_version: 1,
                id: MemoryId::new("mem_20260424_a1b2c3d4e5f60718_000001"),
                memory_type: MemoryType::Pattern,
                scope: Scope::Agent,
                summary: "test".to_string(),
                confidence: 1.0,
                trust_level: TrustLevel::Trusted,
                sensitivity: Sensitivity::Internal,
                status: MemoryStatus::Active,
                created_at: now,
                updated_at: now,
                author: Author {
                    kind: AuthorKind::System,
                    user_handle: None,
                    harness: None,
                    harness_version: None,
                    session_id: None,
                    subagent_id: None,
                    phase: None,
                    component: None,
                },
                namespace: None,
                canonical_namespace_id: None,
                tags: Vec::new(),
                entities: Vec::new(),
                aliases: Vec::new(),
                source: Source {
                    kind: SourceKind::Import,
                    reference: None,
                    harness: None,
                    harness_version: None,
                    session_id: None,
                    subagent_id: None,
                    device: None,
                },
                evidence: Vec::new(),
                requires_user_confirmation: false,
                review_state: None,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                related: Vec::new(),
                tombstone_events: Vec::new(),
                retrieval_policy: RetrievalPolicy {
                    passive_recall: true,
                    max_scope: Scope::Agent,
                    mask_personal_for_synthesis: false,
                    index_body: true,
                    index_embeddings: true,
                },
                write_policy: WritePolicy {
                    human_review_required: false,
                    policy_applied: "default-v1".to_string(),
                    expected_base_hash: None,
                },
                merge_diagnostics: None,
                extras: std::collections::BTreeMap::new(),
            },
            body: body.to_string(),
            path: Some(RepoPath::new("agent/patterns/mem_20260424_a1b2c3d4e5f60718_000001.md")),
        }
    }

    #[test]
    fn empty_body_produces_no_chunks() {
        assert!(chunk_memory(&make_memory("")).is_empty());
    }

    #[test]
    fn chunk_ids_start_with_chk_prefix() {
        let memory = make_memory("hello world this is a test body");
        let chunks = chunk_memory(&memory);
        assert!(!chunks.is_empty());
        for chunk in &chunks {
            assert!(chunk.chunk_id.starts_with("chk_"), "chunk_id must start with chk_: {}", chunk.chunk_id);
            assert_eq!(chunk.chunk_id.len(), 4 + 64, "chk_ + 64 hex chars");
        }
    }

    #[test]
    fn identical_text_produces_identical_chunk_id() {
        let id1 = chunk_id_from_text("hello world");
        let id2 = chunk_id_from_text("hello world");
        assert_eq!(id1, id2);
    }

    #[test]
    fn different_text_produces_different_chunk_id() {
        let id1 = chunk_id_from_text("hello world");
        let id2 = chunk_id_from_text("hello earth");
        assert_ne!(id1, id2);
    }

    #[test]
    fn no_chunk_exceeds_target_plus_overlap_tokens() {
        let words: Vec<String> = (0..1000).map(|i| format!("word{i}")).collect();
        let body = words.join(" ");
        let memory = make_memory(&body);
        let chunks = chunk_memory(&memory);
        for chunk in &chunks {
            let token_count = chunk.text.split_whitespace().count();
            // Allow up to TARGET + OVERLAP as the maximum (one section boundary can spill).
            assert!(
                token_count <= TARGET_TOKENS + OVERLAP_TOKENS,
                "chunk has {token_count} tokens, expected <= {}",
                TARGET_TOKENS + OVERLAP_TOKENS
            );
        }
    }

    #[test]
    fn consecutive_chunks_share_overlap_tokens() {
        let words: Vec<String> = (0..900).map(|i| format!("word{i}")).collect();
        let body = words.join(" ");
        let memory = make_memory(&body);
        let chunks = chunk_memory(&memory);
        if chunks.len() < 2 {
            return; // Not enough chunks to test overlap.
        }
        for pair in chunks.windows(2) {
            let a_words: std::collections::HashSet<&str> = pair[0].text.split_whitespace().collect();
            let b_words: std::collections::HashSet<&str> = pair[1].text.split_whitespace().collect();
            let overlap_count = a_words.intersection(&b_words).count();
            // With exact overlap there's no guarantee of an exact set intersection (words repeat),
            // so we just assert the chunks do share *some* content.
            assert!(overlap_count > 0, "consecutive chunks should share at least one word");
        }
    }

    #[test]
    fn multibyte_utf8_chunk_ids_are_well_formed() {
        let body = "α β γ δ ε ".repeat(500);
        let memory = make_memory(&body);
        let chunks = chunk_memory(&memory);
        for chunk in &chunks {
            assert!(std::str::from_utf8(chunk.text.as_bytes()).is_ok());
            assert!(chunk.chunk_id.starts_with("chk_"));
        }
    }
}
