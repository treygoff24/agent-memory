use std::process::Command;

const RELEASE_GATE_CONTRACTS_ENV: &str = "MEMORUM_RUN_RELEASE_GATE_CONTRACTS";

#[test]
fn release_gate_script_runs_release_perf_and_regression_contracts() {
    if skip_shell_release_gate("release_gate_script_runs_release_perf_and_regression_contracts") {
        return;
    }

    let repo = repo_root();
    let temp = tempfile::tempdir().expect("tempdir");
    let results = temp.path().join("results.linux-x86_64.json");
    run(
        &repo,
        &[
            "BENCH_CORPUS_OVERRIDE=20",
            "./scripts/bench-gate.sh",
            "--tier",
            "release",
            "--profile",
            "linux-x86_64",
            "--output",
            results.to_str().expect("results path"),
        ],
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&results).expect("results json")).expect("parse results json");
    assert_eq!(json["runs"], 9);
    assert_eq!(json["corpus_size"], 20);
    assert_eq!(json["vectorized_chunks"], 20);
    assert!(json["metrics"]["cold_reindex"]["p95_ms"].as_f64().expect("real p95") > 0.0);
    let script = std::fs::read_to_string(repo.join("scripts/bench-gate.sh")).expect("bench script");
    assert!(script.contains("corpus=10000"), "release bench default must stay at spec scale");
    let variants = json["corpus_variants"].as_array().expect("corpus variants");
    for variant in ["active_plaintext_internal", "aliases", "tag_buckets", "variable_body_lengths"] {
        assert!(variants.iter().any(|value| value == variant), "missing corpus variant {variant}");
    }

    let baseline = temp.path().join("baseline.linux-x86_64.json");
    std::fs::write(&baseline, std::fs::read_to_string(&results).expect("copy results")).expect("baseline");
    run(
        &repo,
        &[
            "./scripts/bench-regression-check.sh",
            "--profile",
            "linux-x86_64",
            "--results",
            results.to_str().expect("results path"),
            "--baseline",
            baseline.to_str().expect("baseline path"),
        ],
    );
}

#[test]
fn durability_probe_gate_exercises_full_refused_and_best_effort_matrix() {
    if skip_shell_release_gate("durability_probe_gate_exercises_full_refused_and_best_effort_matrix") {
        return;
    }

    let repo = repo_root();
    let temp = tempfile::tempdir().expect("tempdir");
    let output = temp.path().join("durability.json");
    run(
        &repo,
        &[
            "./scripts/durability-probe-gate.sh",
            "--matrix",
            "apfs,tmpfs,ext4,einval,best-effort",
            "--output",
            output.to_str().expect("output path"),
        ],
    );
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(output).expect("durability json"))
        .expect("parse durability json");
    let entries = json["entries"].as_array().expect("entries");
    assert!(entries.iter().any(|entry| entry["name"] == "apfs"));
    assert!(entries.iter().any(|entry| entry["name"] == "tmpfs"));
    assert!(entries.iter().any(|entry| entry["name"] == "ext4"));
    assert!(entries.iter().any(|entry| entry["name"] == "einval" && entry["status"] == "passed"));
    assert!(entries.iter().any(|entry| entry["name"] == "best-effort" && entry["status"] == "passed"));
}

#[test]
fn two_clone_convergence_script_reaches_fixed_point() {
    if skip_shell_release_gate("two_clone_convergence_script_reaches_fixed_point") {
        return;
    }

    let repo = repo_root();
    run(&repo, &["./scripts/two-clone-convergence.sh", "--smoke"]);
}

#[test]
fn check_script_contains_release_test_doc_spec_and_convergence_gates() {
    let check = std::fs::read_to_string(repo_root().join("scripts/check.sh")).expect("check script");
    for needle in [
        "cargo test --workspace --release",
        "cargo test -p memoryd --features dev-fixtures --test dream_cli",
        "RUSTDOCFLAGS=\"-D warnings\" cargo doc --workspace --no-deps",
        "./scripts/two-clone-convergence.sh --full",
        "./scripts/durability-probe-gate.sh",
        "./scripts/bench-gate.sh --tier smoke",
        "./scripts/bench-gate.sh --tier release",
        "./scripts/bench-regression-check.sh",
    ] {
        assert!(check.contains(needle), "check.sh missing {needle}");
    }
}

#[test]
fn task_integration_script_refuses_untracked_dirty_main_before_merge() {
    let script =
        std::fs::read_to_string(repo_root().join("scripts/integrate-task-worktree.sh")).expect("integrate task script");
    assert!(
        script.contains("git status --porcelain=v1 --untracked-files=all"),
        "integration guard must include untracked files regardless of git config"
    );
    assert!(script.contains("main has uncommitted changes"));
}

#[test]
fn fuzz_workflow_runs_merge_driver_for_ten_minutes() {
    let workflow =
        std::fs::read_to_string(repo_root().join(".github/workflows/stream-a-fuzz.yml")).expect("fuzz workflow");

    assert!(workflow.contains("fuzz run merge_driver"));
    assert!(workflow.contains("-max_total_time=600"));
}

#[test]
fn shell_release_gate_contracts_are_opt_in_by_default() {
    assert!(!release_gate_contracts_enabled_value(None));
    assert!(!release_gate_contracts_enabled_value(Some("0")));
    assert!(release_gate_contracts_enabled_value(Some("1")));
}

#[test]
fn command_spec_parses_leading_env_and_preserves_equals_in_arguments() {
    let spec =
        command_spec(&["BENCH_CORPUS_OVERRIDE=20", "./scripts/bench-gate.sh", "--profile=linux-x86_64", "path=a=b"]);

    assert_eq!(spec.env, vec![("BENCH_CORPUS_OVERRIDE", "20")]);
    assert_eq!(spec.script, "./scripts/bench-gate.sh");
    assert_eq!(spec.args, vec!["--profile=linux-x86_64", "path=a=b"]);
}

fn run(repo: &std::path::Path, args: &[&str]) {
    let spec = command_spec(args);
    let mut command = Command::new(spec.script);
    for (key, value) in spec.env {
        command.env(key, value);
    }
    command.args(spec.args);
    let output = command.current_dir(repo).output().expect("run script");
    assert!(
        output.status.success(),
        "script failed: {}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[derive(Debug, Eq, PartialEq)]
struct CommandSpec<'a> {
    env: Vec<(&'a str, &'a str)>,
    script: &'a str,
    args: Vec<&'a str>,
}

fn command_spec<'a>(args: &'a [&'a str]) -> CommandSpec<'a> {
    let script_index = args.iter().position(|arg| arg.starts_with("./")).expect("script path");
    let (env_args, script_and_args) = args.split_at(script_index);
    let script = script_and_args.first().expect("script path");
    let env = env_args
        .iter()
        .map(|arg| parse_env_assignment(arg).unwrap_or_else(|| panic!("invalid env assignment before script: {arg}")))
        .collect();

    CommandSpec { env, script, args: script_and_args[1..].to_vec() }
}

fn parse_env_assignment(arg: &str) -> Option<(&str, &str)> {
    let (key, value) = arg.split_once('=')?;
    is_env_key(key).then_some((key, value))
}

fn is_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic()) && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn skip_shell_release_gate(test_name: &str) -> bool {
    if release_gate_contracts_enabled() {
        return false;
    }

    eprintln!("skipping {test_name}; set {RELEASE_GATE_CONTRACTS_ENV}=1 to run shell release-gate contracts");
    true
}

fn release_gate_contracts_enabled() -> bool {
    release_gate_contracts_enabled_value(std::env::var(RELEASE_GATE_CONTRACTS_ENV).ok().as_deref())
}

fn release_gate_contracts_enabled_value(value: Option<&str>) -> bool {
    value == Some("1")
}

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("crates dir").parent().expect("repo").to_path_buf()
}
