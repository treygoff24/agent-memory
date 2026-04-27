use std::process::Command;

#[test]
fn release_gate_script_runs_release_perf_and_regression_contracts() {
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
    assert!(json["metrics"]["cold_reindex"]["p95_ms"].as_f64().expect("real p95") > 0.0);
    let script = std::fs::read_to_string(repo.join("scripts/bench-gate.sh")).expect("bench script");
    assert!(script.contains("corpus=10000"), "release bench default must stay at spec scale");
    let variants = json["corpus_variants"].as_array().expect("corpus variants");
    for variant in [
        "long_bodies",
        "large_bodies",
        "aliases",
        "entity_aliases",
        "regressions",
        "prospective",
        "tombstones",
        "encrypted_metadata_only",
    ] {
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
    let repo = repo_root();
    run(&repo, &["./scripts/two-clone-convergence.sh", "--smoke"]);
}

#[test]
fn check_script_contains_release_test_doc_spec_and_convergence_gates() {
    let check = std::fs::read_to_string(repo_root().join("scripts/check.sh")).expect("check script");
    for needle in [
        "cargo test --workspace --release",
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

fn run(repo: &std::path::Path, args: &[&str]) {
    let mut command = Command::new(args.iter().find(|arg| arg.starts_with("./")).expect("script path"));
    for arg in args {
        if let Some((key, value)) = arg.split_once('=') {
            command.env(key, value);
        } else if !arg.starts_with("./") {
            command.arg(arg);
        }
    }
    let output = command.current_dir(repo).output().expect("run script");
    assert!(
        output.status.success(),
        "script failed: {}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("crates dir").parent().expect("repo").to_path_buf()
}
