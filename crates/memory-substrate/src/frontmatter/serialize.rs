//! Canonical frontmatter serialization.

use serde_json::Value;

use crate::error::ValidationError;
use crate::frontmatter::schema::CANONICAL_KEYS;
use crate::frontmatter::validate::validate_frontmatter;
use crate::model::{Frontmatter, Memory};

/// Serialize a Markdown memory document with canonical frontmatter ordering.
pub fn serialize_document(memory: &Memory) -> Result<String, ValidationError> {
    validate_frontmatter(&memory.frontmatter)?;
    let yaml = serialize_frontmatter(&memory.frontmatter)?;
    Ok(format!("---\n{yaml}---\n{}", memory.body.replace("\r\n", "\n")))
}

/// Serialize frontmatter with canonical key order.
///
/// `Frontmatter::extras` is now `#[serde(flatten)]`, so `serde_json::to_value`
/// merges unknown fields into the same root object. We re-split on the way
/// out: known keys are emitted in spec §6.2 canonical order, then extras
/// follow in deterministic (`BTreeMap`) order. Unknown fields survive a full
/// round-trip without bleed across canonical keys.
pub fn serialize_frontmatter(frontmatter: &Frontmatter) -> Result<String, ValidationError> {
    let value = serde_json::to_value(frontmatter).map_err(|err| ValidationError::Other(err.to_string()))?;
    let source = value.as_object().ok_or_else(|| ValidationError::BadShape("frontmatter".to_string()))?;
    let mut out = String::new();
    for key in CANONICAL_KEYS {
        if let Some(value) = source.get(*key) {
            emit_key_value(&mut out, key, &sorted_value(key, value.clone()), 0);
        } else if *key == "grounding_rehydration_required" {
            emit_key_value(&mut out, key, &Value::Bool(false), 0);
        }
    }
    for (key, value) in &frontmatter.extras {
        if CANONICAL_KEYS.contains(&key.as_str()) {
            continue;
        }
        emit_key_value(&mut out, key, value, 0);
    }
    Ok(out)
}

fn emit_key_value(out: &mut String, key: &str, value: &Value, indent: usize) {
    let padding = " ".repeat(indent);
    match value {
        Value::Object(map) => {
            out.push_str(&format!("{padding}{key}:\n"));
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by_key(|(entry_key, _)| *entry_key);
            for (entry_key, entry_value) in entries {
                emit_key_value(out, entry_key, entry_value, indent + 2);
            }
        }
        Value::Array(values) if values.is_empty() => out.push_str(&format!("{padding}{key}: []\n")),
        Value::Array(values) => {
            out.push_str(&format!("{padding}{key}:\n"));
            for value in values {
                emit_array_value(out, value, indent + 2);
            }
        }
        scalar => out.push_str(&format!("{padding}{key}: {}\n", scalar_to_yaml(scalar))),
    }
}

fn emit_array_value(out: &mut String, value: &Value, indent: usize) {
    let padding = " ".repeat(indent);
    match value {
        Value::Object(map) => {
            out.push_str(&format!("{padding}-\n"));
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by_key(|(entry_key, _)| *entry_key);
            for (entry_key, entry_value) in entries {
                emit_key_value(out, entry_key, entry_value, indent + 2);
            }
        }
        scalar => out.push_str(&format!("{padding}- {}\n", scalar_to_yaml(scalar))),
    }
}

fn scalar_to_yaml(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) if plain_yaml_string(value) => value.clone(),
        Value::String(value) => serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string()),
        Value::Array(_) | Value::Object(_) => "null".to_string(),
    }
}

/// YAML reserved literals that must be quoted to prevent misinterpretation.
/// Without quoting, a YAML parser reads these as Boolean/null scalars (YAML 1.1)
/// or null/bool (YAML 1.2). We produce YAML 1.1-compatible output that yaml_serde
/// and the merge driver both consume.
const YAML_RESERVED: &[&str] = &[
    "null", "Null", "NULL", "~", "true", "True", "TRUE", "false", "False", "FALSE", "yes", "Yes", "YES", "no", "No",
    "NO", "on", "On", "ON", "off", "Off", "OFF",
];

fn plain_yaml_string(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    // Reject YAML reserved literals.
    if YAML_RESERVED.contains(&value) {
        return false;
    }
    // Reject strings that look like YAML numeric scalars:
    // integers, floats, hex (0x...), octal (0o...), binary (0b...),
    // and the special float literals (.inf, .Inf, .INF, .nan, .NaN, .NAN).
    if looks_like_yaml_numeric(value) {
        return false;
    }
    // YAML plain-scalar terminators. The allow-set below admits ':', which is
    // safe when adjacent to another plain character (e.g. `mem_...:abc` or the
    // colons inside an RFC 3339 timestamp), but YAML treats ": " (colon followed
    // by whitespace) as the start of a nested mapping value and a trailing ":"
    // as the end of a key with a null value. A leading "- " starts a block
    // sequence. Any of those would round-trip as a different shape, so force
    // double-quoted output for them.
    if value.contains(": ") || value.ends_with(':') || value.starts_with("- ") {
        return false;
    }
    // YAML plain scalars are stripped of leading/trailing whitespace at parse
    // time, so emitting `summary:   hello` round-trips as `hello`. Force quoting
    // to preserve the original value byte-for-byte.
    if value.starts_with(' ') || value.ends_with(' ') {
        return false;
    }
    value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | '@' | ' '))
}

/// Return true when a string would be parsed as a YAML numeric scalar without quoting.
fn looks_like_yaml_numeric(value: &str) -> bool {
    // Special float literals.
    if matches!(value, ".inf" | ".Inf" | ".INF" | ".nan" | ".NaN" | ".NAN" | "+.inf" | "+.Inf" | "+.INF") {
        return true;
    }
    // Hex / octal / binary integers.
    if value.starts_with("0x") || value.starts_with("0X") {
        return value[2..].chars().all(|c| c.is_ascii_hexdigit());
    }
    if value.starts_with("0o") || value.starts_with("0O") {
        return value[2..].chars().all(|c| matches!(c, '0'..='7'));
    }
    if value.starts_with("0b") || value.starts_with("0B") {
        return value[2..].chars().all(|c| c == '0' || c == '1');
    }
    // Decimal integer or float (optional leading sign, digits, optional dot + digits,
    // optional exponent). A quick heuristic: parse as f64 succeeds and value has
    // no letters that aren't `e`/`E`.
    let stripped = value.strip_prefix(['+', '-']).unwrap_or(value);
    let has_only_numeric_chars = !stripped.is_empty()
        && stripped
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '_' || c == 'e' || c == 'E' || c == '+' || c == '-');
    has_only_numeric_chars && stripped.parse::<f64>().is_ok()
}

fn sorted_value(key: &str, value: Value) -> Value {
    match (key, value) {
        ("tags" | "aliases" | "supersedes" | "superseded_by" | "related", Value::Array(mut values)) => {
            values.sort_by_key(|left| left.to_string());
            Value::Array(values)
        }
        (_, value) => value,
    }
}

#[cfg(test)]
mod tests {
    use super::{plain_yaml_string, scalar_to_yaml};
    use serde_json::Value;

    /// Strings the writer must wrap in double quotes because YAML would otherwise
    /// reparse them as a different shape (nested mapping / null key / sequence).
    /// The repro case for the write-note bug is `"Useful: memoryd doctor ..."`,
    /// which previously round-tripped as `summary: Useful: memoryd doctor ...`
    /// and made the substrate refuse to start on reindex.
    #[test]
    fn quotes_strings_that_look_like_yaml_indicators() {
        for value in [
            "Useful: memoryd doctor --reindex rebuilds the SQLite events_log mirror from JSONL",
            "TODO followup: revisit handbook",
            "foo:",
            "- listed thing",
            " leading space",
            "trailing space ",
            "  both sides  ",
        ] {
            assert!(!plain_yaml_string(value), "{value:?} must not be a plain scalar");
            let yaml = scalar_to_yaml(&Value::String(value.to_string()));
            assert!(yaml.starts_with('"') && yaml.ends_with('"'), "{value:?} should be quoted, got {yaml}");
        }
    }

    /// Existing valid plain scalars must keep emitting unquoted so we don't
    /// churn every memory file on the first save after upgrade.
    #[test]
    fn leaves_safe_plain_strings_unquoted() {
        for value in [
            "Cargo lockfile workflow",
            "mem_20260512_3cb85ebb22bf3577_000001",
            "2026-05-12T15:18:51.945136Z",
            "file:/Users/treygoff/.memorum-dev/grounding/lockfile-policy.md",
            "agent_primary",
            "stream-a-test",
        ] {
            assert!(plain_yaml_string(value), "{value:?} should remain a plain scalar");
            let yaml = scalar_to_yaml(&Value::String(value.to_string()));
            assert_eq!(yaml, value);
        }
    }

    /// YAML reserved literals and numeric look-alikes must still be quoted.
    /// Sanity check that the new ": " / trailing-":" / leading-"- " gate
    /// did not accidentally let those through.
    #[test]
    fn still_quotes_yaml_reserved_and_numeric_lookalikes() {
        for value in ["yes", "no", "true", "False", "null", "Off", "42", "3.14", "0xff", "1e9"] {
            assert!(!plain_yaml_string(value), "{value:?} must not be a plain scalar");
        }
    }
}
