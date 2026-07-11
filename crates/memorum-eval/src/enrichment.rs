//! Deterministic, resumable sidecar enrichment for benchmark corpus writes.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use memory_substrate::frontmatter::{normalize_abstraction_value, normalize_cue_values};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Enrichment {
    pub abstraction: String,
    pub cues: Vec<String>,
    pub source: String,
}

#[derive(Debug, Default, Serialize)]
pub struct EnrichmentReport {
    pub generated: usize,
    pub structural: usize,
    pub skipped: BTreeMap<String, usize>,
}

pub type EnrichmentSidecar = BTreeMap<String, Enrichment>;

pub fn sidecar_path(dataset: &Path) -> PathBuf {
    PathBuf::from(format!("{}.enrichment.json", dataset.display()))
}

pub fn load_sidecar(dataset: &Path) -> Result<EnrichmentSidecar, String> {
    let path = sidecar_path(dataset);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    serde_json::from_slice(&fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?)
        .map_err(|error| format!("parse {}: {error}", path.display()))
}

pub fn enrich_dataset_dir(
    dataset_dir: &Path,
    structural_only: bool,
    harness: &str,
    limit: Option<usize>,
) -> Result<EnrichmentReport, String> {
    let mut report = EnrichmentReport::default();
    for dataset in [
        dataset_dir.join("locomo/locomo10.json"),
        dataset_dir.join("longmemeval/longmemeval_oracle.json"),
        dataset_dir.join("longmemeval/longmemeval_s_cleaned.json"),
    ] {
        if !dataset.exists() {
            continue;
        }
        let items = corpus_bodies(&dataset)?;
        let mut sidecar = load_sidecar(&dataset)?;
        for body in items.into_iter().take(limit.unwrap_or(usize::MAX)) {
            let key = item_key(&body);
            if sidecar.contains_key(&key) {
                *report.skipped.entry("already_enriched".to_owned()).or_default() += 1;
                continue;
            }
            let generated = if structural_only {
                None
            } else {
                match generate(harness, &body) {
                    Ok(value) => Some(value),
                    Err(reason) => {
                        *report.skipped.entry(format!("harness:{reason}")).or_default() += 1;
                        None
                    }
                }
            };
            let (enrichment, was_generated) = match generated.and_then(|value| validate(value).ok()) {
                Some(value) => (value, true),
                None => (structural(&body), false),
            };
            if was_generated {
                report.generated += 1;
            } else {
                report.structural += 1;
            }
            sidecar.insert(key, enrichment);
        }
        let output = serde_json::to_vec_pretty(&sidecar).map_err(|error| error.to_string())?;
        fs::write(sidecar_path(&dataset), format!("{}\n", String::from_utf8_lossy(&output)))
            .map_err(|error| format!("write sidecar: {error}"))?;
    }
    Ok(report)
}

pub fn item_key(body: &str) -> String {
    hex::encode(Sha256::digest(body.as_bytes()))
}

fn structural(body: &str) -> Enrichment {
    Enrichment {
        abstraction: body.split_whitespace().take(8).collect::<Vec<_>>().join(" "),
        cues: Vec::new(),
        source: "structural".to_owned(),
    }
}

fn generate(harness: &str, body: &str) -> Result<Enrichment, String> {
    let prompt = format!("Return only JSON {{\"abstraction\":string,\"cues\":[string]}}. Abstraction: at most 8 words. Cues: 0-3 phrases, each 2-4 words, pattern [Main Entity] + [Key Aspect].\nSummary: {body}\nBody:\n{body}");
    let mut command = Command::new(harness);
    if harness == "claude" {
        command.arg("-p");
    } else if harness == "codex" {
        command.args(["exec", "-"]);
    } else {
        return Err("unsupported_cli".to_owned());
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "spawn_failed".to_owned())?;
    child
        .stdin
        .take()
        .ok_or_else(|| "stdin_unavailable".to_owned())?
        .write_all(prompt.as_bytes())
        .map_err(|_| "stdin_failed".to_owned())?;
    let output = child.wait_with_output().map_err(|_| "wait_failed".to_owned())?;
    if !output.status.success() {
        return Err(format!("exit_{}", output.status.code().unwrap_or(-1)));
    }
    let value: Enrichment = serde_json::from_slice(&output.stdout).map_err(|_| "malformed_json".to_owned())?;
    Ok(value)
}

fn validate(mut value: Enrichment) -> Result<Enrichment, String> {
    value.abstraction = normalize_abstraction_value(Some(value.abstraction))
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "empty_abstraction".to_owned())?;
    value.cues = normalize_cue_values(value.cues).map_err(|error| error.to_string())?;
    value.source = "harness".to_owned();
    Ok(value)
}

fn corpus_bodies(dataset: &Path) -> Result<Vec<String>, String> {
    let value: Value = serde_json::from_slice(&fs::read(dataset).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let mut bodies = Vec::new();
    for item in value.as_array().ok_or_else(|| "dataset must be an array".to_owned())? {
        if let Some(conversation) = item.get("conversation").and_then(Value::as_object) {
            for (key, session) in conversation {
                if let Some(date) = key.strip_suffix("_date_time") {
                    bodies
                        .push(format!("Dataset session {date} occurred at {}.", session.as_str().unwrap_or_default()));
                }
                if let Some(turns) = session.as_array() {
                    for turn in turns {
                        push_turn(&mut bodies, turn, "speaker", "text");
                    }
                }
            }
        }
        if let Some(sessions) = item.get("haystack_sessions").and_then(Value::as_array) {
            let dates = item.get("haystack_dates").and_then(Value::as_array);
            let ids = item.get("haystack_session_ids").and_then(Value::as_array);
            for (index, session) in sessions.iter().enumerate() {
                if let Some(date) = dates.and_then(|values| values.get(index)).and_then(Value::as_str) {
                    let id = ids
                        .and_then(|values| values.get(index))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("session_{index}"));
                    bodies.push(format!("Dataset session {id} occurred at {date}."));
                }
                if let Some(turns) = session.as_array() {
                    for turn in turns {
                        push_turn(&mut bodies, turn, "role", "content");
                    }
                }
            }
        }
    }
    bodies.sort();
    bodies.dedup();
    Ok(bodies)
}

fn push_turn(bodies: &mut Vec<String>, turn: &Value, speaker: &str, text: &str) {
    if let (Some(speaker), Some(text)) =
        (turn.get(speaker).and_then(Value::as_str), turn.get(text).and_then(Value::as_str))
    {
        bodies.push(format!("{speaker}: {text}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structural_enrichment_respects_caps() {
        let value = structural("one two three four five six seven eight nine");
        assert_eq!(value.abstraction.split_whitespace().count(), 8);
        assert!(value.cues.is_empty());
    }
}
