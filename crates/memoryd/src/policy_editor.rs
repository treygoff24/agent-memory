use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use memory_governance::{CandidateContext, Policy, PolicySet, Scope};
use memory_substrate::{events::EventKind, Substrate};

use crate::protocol::{GovernancePolicySnapshot, GovernancePolicySummary, PolicyEditorMutationResponse};

pub fn snapshot(repo: &Path) -> Result<GovernancePolicySnapshot, String> {
    let policy_dir = repo.join("policies");
    if !policy_dir_has_yaml(&policy_dir)? {
        if policy_dir_is_writable(&policy_dir) {
            materialize_builtin_policies(&policy_dir, None)?;
        } else {
            return builtin_snapshot(false);
        }
    }
    let policies = PolicySet::load_from_dir(&policy_dir).map_err(|error| error.to_string())?;
    let files = policy_files(&policy_dir)?;
    let current_file = files.first().cloned();
    let raw_yaml = current_file
        .as_ref()
        .map(|file| fs::read_to_string(policy_dir.join(file)).map_err(|error| error.to_string()))
        .transpose()?;

    Ok(GovernancePolicySnapshot {
        source: "disk".to_owned(),
        raw_yaml,
        policies: summarize_policy_set(&policies, "disk"),
        writable: policy_dir_is_writable(&policy_dir),
        files,
        current_file,
    })
}

pub fn validate(repo: &Path, raw_yaml: &str, file_name: Option<&str>) -> Result<PolicyEditorMutationResponse, String> {
    let policy_dir = repo.join("policies");
    let file_name = target_file_name(raw_yaml, file_name)?;
    let policies = validate_full_policy_set(&policy_dir, &file_name, raw_yaml)?;
    Ok(PolicyEditorMutationResponse { accepted: true, file_name, policies: summarize_policy_set(&policies, "disk") })
}

pub fn write(
    substrate: &Substrate,
    raw_yaml: &str,
    file_name: Option<&str>,
) -> Result<PolicyEditorMutationResponse, String> {
    let repo = substrate.roots().repo.as_path();
    let policy_dir = repo.join("policies");
    let file_name = target_file_name(raw_yaml, file_name)?;
    let needs_bootstrap = !policy_dir_has_yaml(&policy_dir)?;
    let policies = validate_full_policy_set(&policy_dir, &file_name, raw_yaml)?;
    if needs_bootstrap {
        materialize_builtin_policies(&policy_dir, Some(&file_name))?;
    }
    atomic_write(policy_dir.join(&file_name), raw_yaml)?;
    substrate
        .record_event_best_effort(EventKind::PolicyChanged { file_name: file_name.clone() })
        .map_err(|error| error.to_string())?;
    Ok(PolicyEditorMutationResponse { accepted: true, file_name, policies: summarize_policy_set(&policies, "disk") })
}

pub fn write_to_dir(
    policy_dir: &Path,
    raw_yaml: &str,
    file_name: Option<&str>,
) -> Result<PolicyEditorMutationResponse, String> {
    let file_name = target_file_name(raw_yaml, file_name)?;
    let needs_bootstrap = !policy_dir_has_yaml(policy_dir)?;
    let policies = validate_full_policy_set(policy_dir, &file_name, raw_yaml)?;
    if needs_bootstrap {
        materialize_builtin_policies(policy_dir, Some(&file_name))?;
    }
    atomic_write(policy_dir.join(&file_name), raw_yaml)?;
    Ok(PolicyEditorMutationResponse { accepted: true, file_name, policies: summarize_policy_set(&policies, "disk") })
}

pub fn snapshot_from_dir(policy_dir: &Path) -> Result<GovernancePolicySnapshot, String> {
    if !policy_dir_has_yaml(policy_dir)? {
        if policy_dir_is_writable(policy_dir) {
            materialize_builtin_policies(policy_dir, None)?;
        } else {
            return builtin_snapshot(false);
        }
    }
    let policies = PolicySet::load_from_dir(policy_dir).map_err(|error| error.to_string())?;
    let files = if policy_dir.exists() { policy_files(policy_dir)? } else { Vec::new() };
    let current_file = files.first().cloned();
    let raw_yaml = current_file
        .as_ref()
        .map(|file| fs::read_to_string(policy_dir.join(file)).map_err(|error| error.to_string()))
        .transpose()?;

    Ok(GovernancePolicySnapshot {
        source: "disk".to_owned(),
        raw_yaml,
        policies: summarize_policy_set(&policies, "disk"),
        writable: policy_dir_is_writable(policy_dir),
        files,
        current_file,
    })
}

fn validate_full_policy_set(policy_dir: &Path, file_name: &str, raw_yaml: &str) -> Result<PolicySet, String> {
    parse_single_policy(raw_yaml)?;
    let validation_dir = validation_dir(policy_dir)?;
    copy_policy_dir_for_validation(policy_dir, &validation_dir, file_name)?;
    fs::write(validation_dir.join(file_name), raw_yaml).map_err(|error| error.to_string())?;
    let policies = PolicySet::load_from_dir(&validation_dir).map_err(|error| {
        let _ = fs::remove_dir_all(&validation_dir);
        error.to_string()
    })?;
    fs::remove_dir_all(&validation_dir).map_err(|error| error.to_string())?;
    Ok(policies)
}

fn parse_single_policy(raw_yaml: &str) -> Result<(), String> {
    serde_yaml::from_str::<Policy>(raw_yaml).map(|_| ()).map_err(|error| error.to_string())
}

fn target_file_name(raw_yaml: &str, file_name: Option<&str>) -> Result<String, String> {
    let file_name = match file_name {
        Some(file_name) => file_name.to_owned(),
        None => {
            let policy: Policy = serde_yaml::from_str(raw_yaml).map_err(|error| error.to_string())?;
            format!("{}.yaml", policy.name())
        }
    };
    if is_safe_yaml_file_name(&file_name) {
        Ok(file_name)
    } else {
        Err("file_name must be a plain .yaml filename".to_owned())
    }
}

/// Whether `file_name` is a safe policy YAML filename: non-empty, `.yaml`
/// extension, no path separators or dotfile/traversal tricks, alphanumerics
/// plus `-_.` only. Shared with the web policy-editor route.
pub fn is_safe_yaml_file_name(file_name: &str) -> bool {
    !file_name.is_empty()
        && file_name.ends_with(".yaml")
        && !file_name.contains('/')
        && !file_name.contains('\\')
        && !file_name.starts_with('.')
        && file_name.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
}

fn policy_files(policy_dir: &Path) -> Result<Vec<String>, String> {
    let mut files = fs::read_dir(policy_dir)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "yaml") {
                path.file_name().and_then(|file_name| file_name.to_str()).map(str::to_owned)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn copy_policy_dir_for_validation(policy_dir: &Path, validation_dir: &Path, target_file: &str) -> Result<(), String> {
    fs::create_dir_all(validation_dir).map_err(|error| error.to_string())?;
    let files = policy_files(policy_dir)?;
    if files.is_empty() {
        materialize_builtin_policies(validation_dir, Some(target_file))?;
        return Ok(());
    }
    for file in files {
        if file != target_file {
            fs::copy(policy_dir.join(&file), validation_dir.join(&file)).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn validation_dir(policy_dir: &Path) -> Result<PathBuf, String> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_nanos();
    Ok(policy_dir.join(format!(".policy-editor-validate-{}-{nonce}", std::process::id())))
}

fn atomic_write(path: PathBuf, raw_yaml: &str) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "policy file has no parent directory".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temp_path = path.with_extension("yaml.tmp");
    fs::write(&temp_path, raw_yaml).map_err(|error| error.to_string())?;
    fs::File::open(&temp_path).and_then(|file| file.sync_all()).map_err(|error| error.to_string())?;
    fs::rename(&temp_path, &path).map_err(|error| error.to_string())?;
    fs::File::open(parent).and_then(|file| file.sync_all()).map_err(|error| error.to_string())?;
    Ok(())
}

fn policy_dir_is_writable(policy_dir: &Path) -> bool {
    if fs::create_dir_all(policy_dir).is_err() {
        return false;
    }
    let probe = policy_dir.join(format!(".policy-editor-write-probe-{}", std::process::id()));
    match fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(file) => {
            let _ = file.sync_all();
            let _ = fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

fn policy_dir_has_yaml(policy_dir: &Path) -> Result<bool, String> {
    if !policy_dir.exists() {
        return Ok(false);
    }
    Ok(!policy_files(policy_dir)?.is_empty())
}

fn materialize_builtin_policies(policy_dir: &Path, skip_file: Option<&str>) -> Result<(), String> {
    fs::create_dir_all(policy_dir).map_err(|error| error.to_string())?;
    for (file_name, raw_yaml) in builtin_policy_files()? {
        if Some(file_name.as_str()) == skip_file {
            continue;
        }
        let path = policy_dir.join(file_name);
        if !path.exists() {
            atomic_write(path, &raw_yaml)?;
        }
    }
    Ok(())
}

fn builtin_policy_files() -> Result<Vec<(String, String)>, String> {
    let policies = PolicySet::builtin();
    [Scope::Me, Scope::Project, Scope::Agent, Scope::Dreaming]
        .into_iter()
        .map(|scope| {
            let policy = policies.policy_for_scope(scope).map_err(|error| error.to_string())?;
            let file_name = format!("{}.yaml", policy.name());
            let raw_yaml = serde_yaml::to_string(policy).map_err(|error| error.to_string())?;
            Ok((file_name, raw_yaml))
        })
        .collect()
}

fn builtin_snapshot(writable: bool) -> Result<GovernancePolicySnapshot, String> {
    let files = builtin_policy_files()?;
    let current_file = files.first().map(|(file, _)| file.clone());
    let raw_yaml = files.first().map(|(_, raw_yaml)| raw_yaml.clone());
    let policies = PolicySet::builtin();
    Ok(GovernancePolicySnapshot {
        source: "built_in_fallback".to_owned(),
        raw_yaml,
        policies: summarize_policy_set(&policies, "built_in_fallback"),
        writable,
        files: files.into_iter().map(|(file, _)| file).collect(),
        current_file,
    })
}

/// Summarize a [`PolicySet`] into the per-scope [`GovernancePolicySummary`]
/// rows used by the governance snapshot API. `source` tags where the policies
/// came from (e.g. `"disk"` or `"built_in_fallback"`). Shared with the web
/// policy-editor route.
pub fn summarize_policy_set(policies: &PolicySet, source: &str) -> Vec<GovernancePolicySummary> {
    [Scope::Me, Scope::Project, Scope::Agent, Scope::Dreaming]
        .into_iter()
        .filter_map(|scope| {
            let policy = policies.policy_for_scope(scope).ok()?;
            let preview = policy.dry_run(&CandidateContext::new(scope).with_confidence(0.0).with_grounding(false));
            Some(GovernancePolicySummary {
                scope: format!("{scope:?}").to_ascii_lowercase(),
                selected_policy: preview.selected_policy,
                policy_source: source.to_owned(),
                confidence_floor: preview.confidence_floor,
                review_gates: preview.triggered_review_gates,
                requires_grounding: preview.requires_grounding,
            })
        })
        .collect()
}
