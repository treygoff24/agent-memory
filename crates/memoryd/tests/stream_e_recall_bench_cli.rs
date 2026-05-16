use std::process::Command;

#[test]
fn stream_e_recall_bench_rejects_conflicting_mode_flags() {
    let output = Command::new(env!("CARGO_BIN_EXE_stream_e_recall_bench"))
        .args(["--smoke", "--release", "--sizes", "1", "--warm-runs", "1"])
        .output()
        .expect("run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--smoke") && stderr.contains("--release"), "stderr: {stderr}");
}

#[test]
fn stream_e_recall_bench_smoke_report_uses_honest_sample_counts() {
    let output = Command::new(env!("CARGO_BIN_EXE_stream_e_recall_bench"))
        .args(["--smoke", "--sizes", "1", "--warm-runs", "1"])
        .output()
        .expect("run");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("parse report");
    assert_eq!(report["mode"], "smoke");
    let result = &report["results"][0];
    assert!(result["cold_start_ms"].as_f64().expect("cold start ms") >= 0.0);
    assert_eq!(result["cold_start_samples"], 1);
    assert_eq!(result["startup_warm_samples"], 1);
    assert_eq!(result["delta_no_match_samples"], 1);
    assert_eq!(result["delta_five_entity_match_samples"], 1);
    assert!(result.get("cold_start_p95_ms").is_none(), "single-sample cold start must not be labeled p95");
}
