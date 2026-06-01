use std::path::{Path, PathBuf};

use memoryd::setup::{
    DaemonDetection, HarnessDetection, SetupDecisions, SetupDetection, SetupDetectionOptions, SetupDiscoverySource,
    SetupReport, SetupSocketState,
};

#[test]
fn setup_report_round_trips_restart_required() {
    let report = SetupReport::new(
        SetupDetection {
            claude: HarnessDetection {
                root: Some(PathBuf::from("/tmp/claude")),
                source: Some(SetupDiscoverySource::FlagOverride),
                candidates: 2,
                parse_errors: 0,
            },
            codex: HarnessDetection {
                root: Some(PathBuf::from("/tmp/codex")),
                source: Some(SetupDiscoverySource::FlagOverride),
                candidates: 3,
                parse_errors: 0,
            },
            daemon: DaemonDetection {
                socket_path: PathBuf::from("/tmp/memoryd.sock"),
                socket_state: SetupSocketState::Absent,
            },
        },
        SetupDecisions::default(),
    )
    .with_restart_required(true);

    let encoded = serde_json::to_string(&report).expect("report serializes");
    let decoded: SetupReport = serde_json::from_str(&encoded).expect("report deserializes");

    assert!(decoded.restart_required);
    assert_eq!(decoded.detection.claude.candidates, 2);
    assert_eq!(decoded.detection.codex.candidates, 3);
}

#[test]
fn setup_detection_counts_fixture_candidates() {
    let temp = tempfile::tempdir().expect("tempdir");
    let claude_root = temp.path().join("claude");
    let codex_root = temp.path().join("codex");
    seed_claude_fixture(&claude_root);
    seed_codex_fixture(&codex_root);

    let detection = SetupDetection::run_with_options(SetupDetectionOptions {
        claude_root_override: Some(claude_root.clone()),
        codex_root_override: Some(codex_root.clone()),
        socket_path: Some(temp.path().join("missing.sock")),
    })
    .expect("setup detection succeeds");

    assert_eq!(detection.claude.root.as_deref(), Some(claude_root.as_path()));
    assert_eq!(detection.claude.source, Some(SetupDiscoverySource::FlagOverride));
    assert_eq!(detection.claude.candidates, 2);
    assert_eq!(detection.claude.parse_errors, 0);

    assert_eq!(detection.codex.root.as_deref(), Some(codex_root.as_path()));
    assert_eq!(detection.codex.source, Some(SetupDiscoverySource::FlagOverride));
    assert_eq!(detection.codex.candidates, 3);
    assert_eq!(detection.codex.parse_errors, 0);

    assert_eq!(detection.daemon.socket_state, SetupSocketState::Absent);
}

fn seed_claude_fixture(root: &Path) {
    write_file(root, "project/memory/build.md", b"---\nname: Build\n---\nRun cargo test before shipping.\n");
    write_file(root, "project/memory/style.md", b"---\nname: Style\n---\nPrefer small modules with explicit errors.\n");
    write_file(root, "project/memory/MEMORY.md", b"# Claude index\n");
}

fn seed_codex_fixture(root: &Path) {
    write_file(
        root,
        "MEMORY.md",
        b"\
# Task Group: agent-memory setup

scope: onboarding setup notes
applies_to: cwd=/Users/u/Code/agent-memory; reuse_rule=cwd-scoped

## Task 1: setup
Keep setup idempotent.

# Task Group: workflow

scope: cross-repo workflow
applies_to: cwd=unknown; reuse_rule=workflow-scoped

## Task 1: review
Run clean-code reviews after implementation waves.
",
    );
    write_file(
        root,
        "extensions/ad_hoc/notes/preference.md",
        b"Prefer on-demand daemon startup unless the user asks for persistence.\n",
    );
}

fn write_file(root: &Path, relative: &str, body: &[u8]) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("fixture directory created");
    }
    std::fs::write(path, body).expect("fixture file written");
}
