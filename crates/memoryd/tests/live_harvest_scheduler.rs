use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use memory_privacy::FileKeyProvider;
use memory_substrate::{InitOptions, MemoryId, Roots, SourceKind, Substrate};
use serial_test::serial;

mod common;
use common::{shutdown, spawn_daemon, unique_socket_path, wait_for_socket};

struct EnvGuard {
    name: &'static str,
    previous: Option<OsString>,
}

impl EnvGuard {
    fn set(name: &'static str, value: &Path) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

#[tokio::test]
#[serial]
async fn scheduler_imports_then_noops_then_supersedes_after_source_edit() {
    let temp = tempfile::tempdir().expect("temp");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let home = temp.path().join("home");
    let claude = home.join(".claude");
    let codex = home.join(".codex");
    let source = claude.join("projects/live-harvest/memory/fixture.md");
    std::fs::create_dir_all(source.parent().expect("source parent")).expect("claude dirs");
    std::fs::create_dir_all(codex.join("memories")).expect("codex dirs");
    write_source(&source, "live harvest fixture version one");

    let _home = EnvGuard::set("HOME", &home);
    let _claude = EnvGuard::set("CLAUDE_CONFIG_DIR", &claude);
    let _codex = EnvGuard::set("CODEX_HOME", &codex);

    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_liveharvest".to_string()) },
    )
    .await
    .expect("substrate init");
    FileKeyProvider::runtime_default(&roots.runtime).onboard_local_file().expect("privacy key");
    memory_substrate::config::store_harvest_config(
        &roots.runtime,
        memory_substrate::config::HarvestConfig { enabled: true, interval_minutes: 5 },
    )
    .expect("enable harvest");

    let first = run_one_due_tick("first", substrate, &roots.runtime, None).await;
    assert!(first["last_success_at"].is_string(), "first tick succeeds: {first}");
    assert!(first["harnesses"]["claude-code"]["written"].as_u64().unwrap_or(0) > 0, "first tick writes: {first}");
    let substrate = Substrate::open(roots.clone()).await.expect("reopen for no-op");
    assert_import_provenance(&substrate, &roots.repo, &source).await;
    let overdue_baseline = make_state_overdue(&roots.runtime);
    let second = run_one_due_tick("second", substrate, &roots.runtime, Some(overdue_baseline)).await;
    assert_eq!(second["harnesses"]["claude-code"]["written"], 0);
    assert!(second["harnesses"]["claude-code"]["skipped"].as_u64().unwrap_or(0) > 0, "second tick no-ops: {second}");

    write_source(&source, "live harvest fixture version two");
    let overdue_baseline = make_state_overdue(&roots.runtime);
    let substrate = Substrate::open(roots.clone()).await.expect("reopen for edit");
    let third = run_one_due_tick("third", substrate, &roots.runtime, Some(overdue_baseline)).await;
    assert!(third["harnesses"]["claude-code"]["written"].as_u64().unwrap_or(0) > 0, "edited source writes: {third}");
    let substrate = Substrate::open(roots.clone()).await.expect("reopen after edit");
    assert_import_provenance(&substrate, &roots.repo, &source).await;
}

#[tokio::test]
#[serial]
async fn restart_with_recent_attempt_does_not_reharvest() {
    let temp = tempfile::tempdir().expect("temp");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let home = temp.path().join("home");
    let claude = home.join(".claude");
    let codex = home.join(".codex");
    std::fs::create_dir_all(claude.join("projects")).expect("claude dirs");
    std::fs::create_dir_all(codex.join("memories")).expect("codex dirs");
    let _home = EnvGuard::set("HOME", &home);
    let _claude = EnvGuard::set("CLAUDE_CONFIG_DIR", &claude);
    let _codex = EnvGuard::set("CODEX_HOME", &codex);

    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_harvestfresh".to_string()) },
    )
    .await
    .expect("substrate init");
    FileKeyProvider::runtime_default(&roots.runtime).onboard_local_file().expect("privacy key");
    memory_substrate::config::store_harvest_config(
        &roots.runtime,
        memory_substrate::config::HarvestConfig { enabled: true, interval_minutes: 30 },
    )
    .expect("enable harvest");

    // Persist a state whose last attempt is fresh; a restarting daemon must
    // treat it as not-due instead of harvesting unconditionally (review m4/F7).
    let now = chrono::Utc::now().to_rfc3339();
    std::fs::create_dir_all(&roots.runtime).expect("runtime dir");
    std::fs::write(
        roots.runtime.join("harvest-state.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "last_attempt_at": now,
            "last_success_at": now,
            "next_due": null,
            "harnesses": {},
            "last_error": null,
            "active_embedding_lane": null,
        }))
        .expect("state JSON"),
    )
    .expect("seed fresh state");

    let socket = unique_socket_path("harvest", "fresh");
    let (shutdown_tx, server) = spawn_daemon(&socket, substrate);
    wait_for_socket(&socket).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    shutdown(shutdown_tx, server, &socket).await;

    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(roots.runtime.join("harvest-state.json")).expect("read state"))
            .expect("state JSON");
    assert_eq!(state["last_attempt_at"].as_str(), Some(now.as_str()), "restart must not attempt while not due");
}

async fn run_one_due_tick(
    label: &str,
    substrate: Substrate,
    runtime: &Path,
    previous_attempt: Option<String>,
) -> serde_json::Value {
    let socket = unique_socket_path("harvest", label);
    let (shutdown_tx, server) = spawn_daemon(&socket, substrate);
    wait_for_socket(&socket).await;
    let state = wait_for_new_attempt(runtime, previous_attempt.as_deref()).await;
    shutdown(shutdown_tx, server, &socket).await;
    state
}

async fn wait_for_new_attempt(runtime: &Path, previous_attempt: Option<&str>) -> serde_json::Value {
    let path = runtime.join("harvest-state.json");
    for _ in 0..400 {
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str::<serde_json::Value>(&raw) {
                if state["last_attempt_at"].as_str() != previous_attempt {
                    return state;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("harvest state did not record a new attempt at {}", path.display());
}

/// Backdate the persisted state so the next daemon start is immediately due.
/// Pacing runs off `last_attempt_at` (review M1), so it must be backdated too —
/// which also means callers must use the RETURNED sentinel as their
/// `previous_attempt` baseline: `wait_for_new_attempt` keys on
/// `last_attempt_at` changing, and this rewrite already changed it once.
fn make_state_overdue(runtime: &Path) -> String {
    let sentinel = "2020-01-01T00:00:00Z".to_string();
    let path = runtime.join("harvest-state.json");
    let mut state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read state")).expect("state JSON");
    state["last_attempt_at"] = serde_json::Value::String(sentinel.clone());
    state["last_success_at"] = serde_json::Value::String(sentinel.clone());
    state["next_due"] = serde_json::Value::String("2020-01-01T00:05:00Z".to_string());
    std::fs::write(&path, serde_json::to_vec_pretty(&state).expect("serialize state")).expect("write overdue state");
    sentinel
}

fn write_source(path: &Path, body: &str) {
    std::fs::write(path, format!("---\nname: Live harvest fixture\n---\n{body}\n")).expect("write source");
}

async fn assert_import_provenance(substrate: &Substrate, repo: &Path, source: &Path) {
    let state =
        memoryd::import::state::ImportState::load(&repo.join(".memorum/import-state.json")).expect("import state");
    let record = state.imports.values().find(|record| record.harness == "claude-code").expect("claude import record");
    assert_eq!(record.source_path_at_import, source);
    let envelope =
        substrate.read_memory_envelope(&MemoryId::new(&record.memory_id)).await.expect("imported memory envelope");
    assert_eq!(envelope.metadata.frontmatter.source.kind, SourceKind::File);
    assert_eq!(envelope.metadata.frontmatter.source.harness.as_deref(), Some("memoryd-import"));
    assert_eq!(envelope.metadata.frontmatter.author.harness.as_deref(), Some("memoryd-import"));
}
