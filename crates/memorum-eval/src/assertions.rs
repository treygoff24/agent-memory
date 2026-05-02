use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallBlock {
    pub memories: Vec<RecallMemory>,
    pub omitted_count: Option<usize>,
    pub pending_attention_items: Vec<PendingAttentionItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallMemory {
    pub ref_id: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAttentionItem {
    pub kind: Option<String>,
    pub count: Option<usize>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssertionError {
    EmptyXml,
    MalformedRecallBlock { step: &'static str, reason: String },
    MemoryMissing { step: &'static str, expected_ref: String, found_refs: Vec<String> },
    UnexpectedMemory { step: &'static str, unexpected_ref: String, found_refs: Vec<String> },
    PiiFoundOnDisk { step: &'static str, pii_string: String, path: PathBuf },
    Io { step: &'static str, path: PathBuf, message: String },
    StatusMismatch { step: &'static str, expected: String, found: String },
    GovernanceOutcomeMismatch { step: &'static str, expected: String, found: String },
}

impl fmt::Display for AssertionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyXml => formatter.write_str("XML input is empty"),
            Self::MalformedRecallBlock { step, reason } => {
                write!(formatter, "{step}: memory recall XML is malformed: {reason}")
            }
            Self::MemoryMissing { step, expected_ref, found_refs } => write!(
                formatter,
                "{step}: expected memory ref `{expected_ref}` in recall block; found refs [{}]",
                found_refs.join(", ")
            ),
            Self::UnexpectedMemory { step, unexpected_ref, found_refs } => write!(
                formatter,
                "{step}: expected no memory ref `{unexpected_ref}` in recall block; found refs [{}]",
                found_refs.join(", ")
            ),
            Self::PiiFoundOnDisk { step, pii_string, path } => write!(
                formatter,
                "{step}: expected PII string `{pii_string}` to be absent from disk; found in {}",
                path.display()
            ),
            Self::Io { step, path, message } => {
                write!(formatter, "{step}: failed to inspect {}: {message}", path.display())
            }
            Self::StatusMismatch { step, expected, found } => {
                write!(formatter, "{step}: expected status `{expected}`, found `{found}`")
            }
            Self::GovernanceOutcomeMismatch { step, expected, found } => {
                write!(formatter, "{step}: expected governance outcome `{expected}`, found `{found}`")
            }
        }
    }
}

impl std::error::Error for AssertionError {}

pub fn parse_recall_block(xml: &str) -> Result<RecallBlock, AssertionError> {
    assert_xml_valid(xml)?;
    let root = first_tag(xml, "memory-recall")?;
    let root_attrs = parse_attributes(root.opening_tag)?;
    let omitted_count = root_attrs
        .get("omitted_count")
        .and_then(|value| value.parse::<usize>().ok())
        .or_else(|| element_text(xml, "omitted_count").and_then(|value| value.trim().parse().ok()));

    Ok(RecallBlock {
        memories: parse_memories(xml)?,
        omitted_count,
        pending_attention_items: parse_pending_attention_items(xml)?,
    })
}

pub fn assert_memory_in_recall(block: &RecallBlock, ref_id: &str) -> Result<(), AssertionError> {
    if block.memories.iter().any(|memory| memory.ref_id == ref_id) {
        return Ok(());
    }

    Err(AssertionError::MemoryMissing {
        step: "assert_memory_in_recall",
        expected_ref: ref_id.to_string(),
        found_refs: memory_refs(block),
    })
}

pub fn assert_no_memory_in_recall(block: &RecallBlock, ref_id: &str) -> Result<(), AssertionError> {
    if block.memories.iter().all(|memory| memory.ref_id != ref_id) {
        return Ok(());
    }

    Err(AssertionError::UnexpectedMemory {
        step: "assert_no_memory_in_recall",
        unexpected_ref: ref_id.to_string(),
        found_refs: memory_refs(block),
    })
}

pub fn assert_xml_valid(xml: &str) -> Result<(), AssertionError> {
    if xml.trim().is_empty() {
        return Err(AssertionError::EmptyXml);
    }

    validate_xml(xml)
}

pub fn assert_status_eq(found: &str, expected: &str) -> Result<(), AssertionError> {
    if found == expected {
        Ok(())
    } else {
        Err(AssertionError::StatusMismatch {
            step: "assert_status_eq",
            expected: expected.to_string(),
            found: found.to_string(),
        })
    }
}

pub fn assert_governance_outcome(found: &str, expected: &str) -> Result<(), AssertionError> {
    if found == expected {
        Ok(())
    } else {
        Err(AssertionError::GovernanceOutcomeMismatch {
            step: "assert_governance_outcome",
            expected: expected.to_string(),
            found: found.to_string(),
        })
    }
}

pub fn assert_no_pii_on_disk(tree_dir: &Path, pii_string: &str) -> Result<(), AssertionError> {
    inspect_tree_for_pii(tree_dir, pii_string)
}

fn parse_memories(xml: &str) -> Result<Vec<RecallMemory>, AssertionError> {
    let mut memories = Vec::new();
    let mut rest = xml;

    while let Some(start) = rest.find("<memory") {
        let candidate = &rest[start..];
        if let Some(stripped) = candidate.strip_prefix("<memory-recall") {
            rest = stripped;
            continue;
        }

        let tag_end = candidate.find('>').ok_or_else(|| malformed("unterminated memory tag"))?;
        let opening_tag = &candidate[..=tag_end];
        let attrs = parse_attributes(opening_tag)?;
        let ref_id = attrs.get("ref").cloned().ok_or_else(|| malformed("memory tag missing ref attribute"))?;

        let body = if opening_tag.trim_end().ends_with("/>") {
            String::new()
        } else {
            let close = candidate[tag_end + 1..]
                .find("</memory>")
                .ok_or_else(|| malformed("memory tag missing closing tag"))?;
            candidate[tag_end + 1..tag_end + 1 + close].trim().to_string()
        };

        memories.push(RecallMemory { ref_id, body });
        rest = &candidate[tag_end + 1..];
    }

    Ok(memories)
}

fn parse_pending_attention_items(xml: &str) -> Result<Vec<PendingAttentionItem>, AssertionError> {
    let mut items = Vec::new();
    let Some(section) = element_text(xml, "pending-attention") else {
        return Ok(items);
    };
    let mut rest = section.as_str();

    while let Some(start) = rest.find("<item") {
        let candidate = &rest[start..];
        let tag_end = candidate.find('>').ok_or_else(|| malformed("unterminated item tag"))?;
        let opening_tag = &candidate[..=tag_end];
        let attrs = parse_attributes(opening_tag)?;
        let close =
            candidate[tag_end + 1..].find("</item>").ok_or_else(|| malformed("item tag missing closing tag"))?;
        let text = candidate[tag_end + 1..tag_end + 1 + close].trim().to_string();
        let count = attrs.get("count").and_then(|value| value.parse::<usize>().ok());

        items.push(PendingAttentionItem { kind: attrs.get("kind").cloned(), count, text });
        rest = &candidate[tag_end + 1 + close + "</item>".len()..];
    }

    Ok(items)
}

fn inspect_tree_for_pii(path: &Path, pii_string: &str) -> Result<(), AssertionError> {
    let metadata = fs::metadata(path).map_err(|error| io_error(path, error))?;
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|error| io_error(path, error))? {
            let entry = entry.map_err(|error| io_error(path, error))?;
            inspect_tree_for_pii(&entry.path(), pii_string)?;
        }
        return Ok(());
    }

    if metadata.is_file() {
        let bytes = fs::read(path).map_err(|error| io_error(path, error))?;
        if String::from_utf8_lossy(&bytes).contains(pii_string) {
            return Err(AssertionError::PiiFoundOnDisk {
                step: "assert_no_pii_on_disk",
                pii_string: pii_string.to_string(),
                path: path.to_path_buf(),
            });
        }
    }

    Ok(())
}

fn validate_xml(xml: &str) -> Result<(), AssertionError> {
    let mut stack: Vec<String> = Vec::new();
    let mut rest = xml;

    while let Some(start) = rest.find('<') {
        let after_open = &rest[start + 1..];
        let end = after_open.find('>').ok_or_else(|| malformed("unterminated tag"))?;
        let raw = after_open[..end].trim();

        if raw.is_empty() {
            return Err(malformed("empty tag"));
        }
        if raw.starts_with('!') || raw.starts_with('?') {
            rest = &after_open[end + 1..];
            continue;
        }
        if let Some(closing_name) = raw.strip_prefix('/') {
            let closing_name = closing_name.trim();
            let opening_name = stack.pop().ok_or_else(|| malformed("closing tag without opener"))?;
            if opening_name != closing_name {
                return Err(malformed(format!(
                    "mismatched closing tag: expected </{opening_name}>, found </{closing_name}>"
                )));
            }
        } else if !raw.ends_with('/') {
            stack.push(tag_name(raw).to_string());
        }

        rest = &after_open[end + 1..];
    }

    if let Some(unclosed) = stack.pop() {
        return Err(malformed(format!("unclosed tag <{unclosed}>")));
    }

    if !xml.trim_start().starts_with("<memory-recall") {
        return Err(malformed("root tag must be memory-recall"));
    }

    Ok(())
}

fn first_tag<'a>(xml: &'a str, name: &str) -> Result<TagSlice<'a>, AssertionError> {
    let marker = format!("<{name}");
    let start = xml.find(&marker).ok_or_else(|| malformed(format!("missing <{name}> tag")))?;
    let end = xml[start..].find('>').ok_or_else(|| malformed(format!("unterminated <{name}> tag")))?;
    Ok(TagSlice { opening_tag: &xml[start..=start + end] })
}

fn element_text(xml: &str, tag: &str) -> Option<String> {
    let open_marker = format!("<{tag}");
    let start = xml.find(&open_marker)?;
    let after_open = &xml[start..];
    let tag_end = after_open.find('>')?;
    let close_marker = format!("</{tag}>");
    let text_start = start + tag_end + 1;
    let close = xml[text_start..].find(&close_marker)?;
    Some(xml[text_start..text_start + close].to_string())
}

fn parse_attributes(tag: &str) -> Result<std::collections::BTreeMap<String, String>, AssertionError> {
    let mut attrs = std::collections::BTreeMap::new();
    let inner = tag.trim().trim_start_matches('<').trim_end_matches('>').trim_end_matches('/').trim();
    let mut rest = inner[tag_name(inner).len()..].trim();

    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else { break };
        let name = rest[..eq].trim();
        let after_eq = rest[eq + 1..].trim_start();
        let Some(after_quote) = after_eq.strip_prefix('"') else {
            return Err(malformed(format!("attribute `{name}` must use double quotes")));
        };
        let quote_end =
            after_quote.find('"').ok_or_else(|| malformed(format!("attribute `{name}` missing closing quote")))?;
        attrs.insert(name.to_string(), after_quote[..quote_end].to_string());
        rest = after_quote[quote_end + 1..].trim_start();
    }

    Ok(attrs)
}

fn tag_name(raw: &str) -> &str {
    raw.split_whitespace().next().unwrap_or(raw).trim_end_matches('/')
}

fn memory_refs(block: &RecallBlock) -> Vec<String> {
    block.memories.iter().map(|memory| memory.ref_id.clone()).collect()
}

fn malformed(reason: impl Into<String>) -> AssertionError {
    AssertionError::MalformedRecallBlock { step: "assert_xml_valid", reason: reason.into() }
}

fn io_error(path: &Path, error: std::io::Error) -> AssertionError {
    AssertionError::Io { step: "assert_no_pii_on_disk", path: path.to_path_buf(), message: error.to_string() }
}

struct TagSlice<'a> {
    opening_tag: &'a str,
}
