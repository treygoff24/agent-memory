use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use memory_substrate::config::load_config;
use serde_json::Value;

use crate::dream::registry::HarnessCliRegistry;
use crate::protocol::{
    DreamStatusCounters, DreamStatusReport, HarnessCliStatus, LeaseRecord, PassStatus, ScopeRunSummary,
};

pub const PRIVACY_DISCLOSURE: &str = "Dreaming uses whichever agent-harness CLI you have installed and authenticated on this device (Claude Code, Codex CLI, Gemini, etc.). Dream prompts are masked through the agent-memory privacy filter before they leave the daemon, but the masked text is processed by the harness CLI's upstream model provider. The data, retention, and training policies of that provider apply. Where this device's selected harness CLI accepts prompts on stdin, the prompt is not visible to other local processes; where it does not, the prompt may be visible via process listing tools (`ps`, `top`, `/proc/<pid>/cmdline`). `memoryd dream status` shows the prompt-transport mode for each installed harness adapter. Substrate fragments written via `memory_observe` are git-synced as low-level durable telemetry; this means the private git repo's raw-observation surface is larger than its canonical-memory surface, even though substrate is not searchable as memory. If you don't want dream content sent to a particular provider, set the per-scope CLI priority to exclude it, or run `memoryd dream disable` on this device.";

pub fn disabled_sentinel_path(runtime: &Path) -> PathBuf {
    runtime.join("dream-disabled")
}

pub fn dreaming_enabled(repo: &Path, runtime: &Path) -> Result<bool, String> {
    let config = load_config(repo, runtime, None)?;
    Ok(config.synced.dreams.enabled && !disabled_sentinel_path(runtime).exists())
}

pub fn enable_device(runtime: &Path) -> Result<(), std::io::Error> {
    let sentinel = disabled_sentinel_path(runtime);
    if sentinel.exists() {
        fs::remove_file(sentinel)?;
    }
    Ok(())
}

pub fn disable_device(runtime: &Path) -> Result<PathBuf, std::io::Error> {
    fs::create_dir_all(runtime)?;
    let sentinel = disabled_sentinel_path(runtime);
    fs::write(&sentinel, "disabled\n")?;
    Ok(sentinel)
}

pub async fn build_dream_status_report(repo: &Path, runtime: &Path) -> Result<DreamStatusReport, String> {
    let enabled = dreaming_enabled(repo, runtime)?;
    let registry = HarnessCliRegistry::builtin_v0_2();

    Ok(DreamStatusReport {
        enabled,
        last_runs: collect_last_runs(repo)?,
        active_leases: collect_active_leases(repo, Utc::now())?,
        cli_inventory: cli_inventory(&registry).await,
        counters: collect_counters(repo)?,
        privacy_disclosure: PRIVACY_DISCLOSURE.to_string(),
    })
}

pub fn render_human_status(report: &DreamStatusReport) -> String {
    let mut out = String::new();
    out.push_str(&report.privacy_disclosure);
    out.push('\n');
    out.push_str(&format!("enabled: {}\n", report.enabled));
    out.push_str("cli_inventory:\n");
    for cli in &report.cli_inventory {
        out.push_str(&format!(
            "  - name={} installed={} authenticated={:?} prompt_transport={:?}\n",
            cli.name, cli.is_installed, cli.is_authenticated, cli.prompt_transport
        ));
    }
    out.push_str("active_leases:\n");
    for lease in &report.active_leases {
        out.push_str(&format!(
            "  - scope={} device={} run_id={} expires_at={}\n",
            lease.scope, lease.device, lease.run_id, lease.expires_at
        ));
    }
    out.push_str("last_runs:\n");
    for run in &report.last_runs {
        out.push_str(&format!(
            "  - scope={} at={:?} outcome={:?} cli={:?} missed={}\n",
            run.scope, run.last_run_at, run.last_run_outcome, run.last_run_cli, run.consecutive_missed_runs
        ));
    }
    out.push_str(&format!(
        "counters: dream_runs_invoked_total={} cleanup_runs_invoked_total={}\n",
        report.counters.dream_runs_invoked_total, report.counters.cleanup_runs_invoked_total
    ));
    out
}

async fn cli_inventory(registry: &HarnessCliRegistry) -> Vec<HarnessCliStatus> {
    let mut statuses = Vec::new();
    for (name, adapter) in registry.adapters() {
        let probe = adapter.auth_probe().await;
        statuses.push(HarnessCliStatus {
            name: name.to_string(),
            is_installed: adapter.is_installed(),
            is_authenticated: Some(probe.is_ok()),
            prompt_transport: adapter.prompt_transport(),
            last_probe_at: Some(Utc::now()),
            last_probe_error: (!probe.is_ok()).then(|| probe.operator_message(name)),
        });
    }
    statuses.extend(registry.disabled_adapters().cloned());
    statuses.sort_by(|left, right| left.name.cmp(&right.name));
    statuses
}

fn collect_active_leases(repo: &Path, now: DateTime<Utc>) -> Result<Vec<LeaseRecord>, String> {
    let path = repo.join("leases/journal.lease");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(&path).map_err(|err| err.to_string())?;
    let mut newest_by_scope = BTreeMap::<String, LeaseRecord>::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|err| err.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let lease = serde_json::from_str::<LeaseRecord>(&line).map_err(|err| err.to_string())?;
        newest_by_scope.insert(lease.scope.clone(), lease);
    }
    let mut leases = newest_by_scope.into_values().filter(|lease| lease.expires_at > now).collect::<Vec<_>>();
    leases.sort_by(|left, right| left.scope.cmp(&right.scope).then(left.expires_at.cmp(&right.expires_at)));
    Ok(leases)
}

fn collect_last_runs(repo: &Path) -> Result<Vec<ScopeRunSummary>, String> {
    let journal_root = repo.join("dreams/journal");
    let mut by_scope: BTreeMap<String, ScopeRunSummary> = BTreeMap::new();
    for path in files_under(&journal_root)? {
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let Some(scope) = scope_from_dream_path(&journal_root, &path) else {
            continue;
        };
        let modified = fs::metadata(&path).and_then(|metadata| metadata.modified()).ok().map(DateTime::<Utc>::from);
        let entry = ScopeRunSummary {
            scope: scope.clone(),
            last_run_at: modified,
            last_run_outcome: Some(PassStatus::Success),
            last_run_cli: None,
            consecutive_missed_runs: 0,
        };
        let should_replace = by_scope
            .get(&scope)
            .and_then(|existing| existing.last_run_at)
            .zip(entry.last_run_at)
            .is_none_or(|(existing, new)| new > existing);
        if should_replace {
            by_scope.insert(scope, entry);
        }
    }
    Ok(by_scope.into_values().collect())
}

fn collect_counters(repo: &Path) -> Result<DreamStatusCounters, String> {
    let mut counters = DreamStatusCounters {
        dream_runs_invoked_total: count_files(&repo.join("dreams/journal"), "md")? as u64,
        ..Default::default()
    };
    let cleanup_files = files_under(&repo.join("dreams/cleanup"))?;
    counters.cleanup_runs_invoked_total =
        cleanup_files.iter().filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json")).count()
            as u64;
    for path in cleanup_files {
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if let Ok(text) = fs::read_to_string(path) {
            if let Ok(json) = serde_json::from_str::<Value>(&text) {
                if let Some(findings) = json.get("findings").and_then(Value::as_array) {
                    for finding in findings {
                        let kind = finding.get("kind").and_then(Value::as_str).unwrap_or("unknown").to_string();
                        *counters.cleanup_findings_total.entry(kind).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    Ok(counters)
}

fn count_files(root: &Path, extension: &str) -> Result<usize, String> {
    Ok(files_under(root)?
        .into_iter()
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some(extension))
        .count())
}

fn files_under(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    collect_files(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn scope_from_dream_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let pieces = relative.iter().map(|piece| piece.to_str()).collect::<Option<Vec<_>>>()?;
    match pieces.as_slice() {
        ["me", _file] => Some("me".to_string()),
        ["agent", _file] => Some("agent".to_string()),
        ["project", id, _file] => Some(format!("project:{id}")),
        ["org", id, _file] => Some(format!("org:{id}")),
        _ => None,
    }
}
