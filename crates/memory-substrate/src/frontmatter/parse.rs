//! Markdown/frontmatter parsing.

use serde_json::{Map, Value};

use crate::error::{ValidationError, ValidationWarning};
use crate::frontmatter::defaults::{default_retrieval_policy, default_source, default_write_policy};
use crate::frontmatter::schema::CANONICAL_KEYS;
use crate::frontmatter::validate::validate_frontmatter;
use crate::model::{Frontmatter, Memory, RepoPath};

/// Parsed memory plus warnings.
#[derive(Clone, Debug)]
pub struct ParsedMemory {
    /// Memory document.
    pub memory: Memory,
    /// Parser/validator warnings.
    pub warnings: Vec<ValidationWarning>,
}

/// Parse a Markdown document with YAML frontmatter.
pub fn parse_document(input: &str, path: Option<RepoPath>) -> Result<ParsedMemory, ValidationError> {
    let (yaml, body) = split_document(input)?;
    let (frontmatter, warnings) = parse_frontmatter_yaml(yaml)?;
    validate_frontmatter(&frontmatter)?;
    Ok(ParsedMemory { memory: Memory { frontmatter, body: body.replace("\r\n", "\n"), path }, warnings })
}

/// Parse only frontmatter YAML.
///
/// `Frontmatter::extras` is `#[serde(flatten)]`, so unknown top-level keys are
/// absorbed by serde during deserialization. We still emit
/// `UnknownFieldPreserved` warnings here so downstream callers can audit them.
pub fn parse_frontmatter_yaml(yaml: &str) -> Result<(Frontmatter, Vec<ValidationWarning>), ValidationError> {
    let mut value: Value = yaml_serde::from_str(yaml).map_err(|err| ValidationError::Other(err.to_string()))?;
    let map = value.as_object_mut().ok_or_else(|| ValidationError::BadShape("frontmatter root".to_string()))?;
    let mut warnings = Vec::new();
    materialize_defaults(map, &mut warnings)?;
    note_unknown_fields(map, &mut warnings);
    let frontmatter: Frontmatter = serde_json::from_value(Value::Object(map.clone()))
        .map_err(|err| ValidationError::BadShape(format!("frontmatter: {err}")))?;
    Ok((frontmatter, warnings))
}

fn split_document(input: &str) -> Result<(&str, &str), ValidationError> {
    let normalized =
        input.strip_prefix("---\n").ok_or_else(|| ValidationError::BadShape("frontmatter delimiters".to_string()))?;
    let end =
        normalized.find("\n---\n").ok_or_else(|| ValidationError::BadShape("frontmatter delimiters".to_string()))?;
    let yaml = &normalized[..end];
    let body = &normalized[(end + "\n---\n".len())..];
    Ok((yaml, body))
}

fn materialize_defaults(
    map: &mut Map<String, Value>,
    warnings: &mut Vec<ValidationWarning>,
) -> Result<(), ValidationError> {
    let scope = read_required::<crate::model::Scope>(map, "scope")?;
    let sensitivity = read_required::<crate::model::Sensitivity>(map, "sensitivity")?;
    set_default(map, warnings, "namespace", Value::Null);
    set_default(map, warnings, "canonical_namespace_id", Value::Null);
    for field in
        ["tags", "entities", "aliases", "evidence", "supersedes", "superseded_by", "related", "tombstone_events"]
    {
        set_default(map, warnings, field, Value::Array(Vec::new()));
    }
    set_default(
        map,
        warnings,
        "source",
        serde_json::to_value(default_source()).map_err(|err| ValidationError::Other(err.to_string()))?,
    );
    set_default(map, warnings, "requires_user_confirmation", Value::Bool(false));
    set_default(map, warnings, "grounding_rehydration_required", Value::Bool(false));
    set_default(map, warnings, "review_state", Value::Null);
    set_default(
        map,
        warnings,
        "retrieval_policy",
        serde_json::to_value(default_retrieval_policy(scope, sensitivity))
            .map_err(|err| ValidationError::Other(err.to_string()))?,
    );
    set_default(
        map,
        warnings,
        "write_policy",
        serde_json::to_value(default_write_policy()).map_err(|err| ValidationError::Other(err.to_string()))?,
    );
    set_default(map, warnings, "_merge_diagnostics", Value::Null);
    Ok(())
}

fn read_required<T>(map: &Map<String, Value>, field: &str) -> Result<T, ValidationError>
where
    T: serde::de::DeserializeOwned,
{
    let value = map.get(field).ok_or_else(|| ValidationError::MissingRequiredField(field.to_string()))?;
    serde_json::from_value(value.clone()).map_err(|_| ValidationError::BadShape(field.to_string()))
}

fn set_default(map: &mut Map<String, Value>, warnings: &mut Vec<ValidationWarning>, field: &str, value: Value) {
    if !map.contains_key(field) {
        map.insert(field.to_string(), value);
        warnings.push(ValidationWarning::AutoPopulatedNullableField { field: field.to_string() });
    }
}

fn note_unknown_fields(map: &Map<String, Value>, warnings: &mut Vec<ValidationWarning>) {
    for key in map.keys() {
        if !CANONICAL_KEYS.contains(&key.as_str()) {
            warnings.push(ValidationWarning::UnknownFieldPreserved { field: key.clone() });
        }
    }
}
