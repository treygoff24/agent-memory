#[cfg(feature = "live-harness")]
mod live {
    use memorum_eval::harness_runner::{HarnessRunner, RealHarness};
    use std::process::Command;

    const CLAUDE_KEY_ENV: &str = "MEMORUM_EVAL_CLAUDE_KEY";
    const CODEX_KEY_ENV: &str = "MEMORUM_EVAL_CODEX_KEY";
    const ASSERTION_MARKER: &str = "MEMORUM_EVAL_ASSERTIONS=";
    const SKIP_MARKERS: &[&str] = &["SKIP_NO_AUTH", "SKIP_MISSING_CLI", "MEMORUM_EVAL_SKIP"];

    #[test]
    fn claude_smoke() {
        if !has_env(CLAUDE_KEY_ENV) || !has_env(CODEX_KEY_ENV) {
            eprintln!("MEMORUM_EVAL_SKIP:SKIP_NO_AUTH:{CLAUDE_KEY_ENV},{CODEX_KEY_ENV}");
            return;
        }
        if !has_cli(RealHarness::Claude) || !has_cli(RealHarness::Codex) {
            return;
        }
        run_domain_filter("t13_cross_harness_substrate_sharing");
    }

    #[test]
    fn codex_smoke() {
        if !has_env(CODEX_KEY_ENV) {
            eprintln!("MEMORUM_EVAL_SKIP:SKIP_NO_AUTH:{CODEX_KEY_ENV}");
            return;
        }
        if !has_cli(RealHarness::Codex) {
            return;
        }
        run_domain_filter("t15_privacy_filter_refusal_and_retry_codex");
    }

    fn has_env(name: &str) -> bool {
        std::env::var_os(name).is_some()
    }

    fn has_cli(harness: RealHarness) -> bool {
        match HarnessRunner::detect_cli(harness) {
            Ok(Some(_)) => true,
            Ok(None) => {
                // stderr matches the convention used by `has_env`; cargo test suppresses
                // stdout on passing tests, so a `println!` skip marker would silently
                // green-light a CI environment that has the API key but not the CLI.
                eprintln!("MEMORUM_EVAL_SKIP:SKIP_MISSING_CLI:{}", harness.binary_name());
                false
            }
            Err(error) => panic!("{error}"),
        }
    }

    fn run_domain_filter(filter: &str) {
        let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .args(["test", "-p", "memorum-eval", "--test", "domain", filter, "--", "--nocapture"])
            .output()
            .unwrap_or_else(|err| panic!("run live harness smoke {filter}: {err}"));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "live harness smoke {filter} failed\nstdout:\n{}\nstderr:\n{}",
            stdout,
            stderr
        );
        let combined = format!("{stdout}\n{stderr}");
        assert!(
            !SKIP_MARKERS.iter().any(|marker| combined.contains(marker)),
            "live harness smoke {filter} skipped inside nested cargo instead of exercising the harness\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        let assertions = extract_assertion_count(&combined).unwrap_or_default();
        assert!(
            assertions > 0,
            "live harness smoke {filter} completed without a positive assertion marker\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    fn extract_assertion_count(output: &str) -> Option<usize> {
        output.lines().rev().find_map(|line| {
            line.trim().strip_prefix(ASSERTION_MARKER).and_then(|count| count.trim().parse::<usize>().ok())
        })
    }
}

#[cfg(not(feature = "live-harness"))]
mod live {
    #[test]
    fn live_harness_feature_disabled() {
        eprintln!("MEMORUM_EVAL_SKIP:live-harness feature disabled");
    }
}
