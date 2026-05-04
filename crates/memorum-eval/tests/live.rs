#[cfg(feature = "live-harness")]
mod live {
    use std::process::Command;

    #[test]
    fn claude_smoke() {
        if std::env::var_os("MEMORUM_EVAL_CLAUDE_KEY").is_none() {
            eprintln!("MEMORUM_EVAL_SKIP:SKIP_NO_AUTH");
            return;
        }
        run_domain_filter("t15_privacy_filter_refusal_and_retry");
    }

    #[test]
    fn codex_smoke() {
        if std::env::var_os("MEMORUM_EVAL_CODEX_KEY").is_none() {
            eprintln!("MEMORUM_EVAL_SKIP:SKIP_NO_AUTH");
            return;
        }
        run_domain_filter("t13_cross_harness_substrate_sharing");
    }

    fn run_domain_filter(filter: &str) {
        let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .args(["test", "-p", "memorum-eval", "--test", "domain", filter, "--", "--nocapture"])
            .output()
            .unwrap_or_else(|err| panic!("run live harness smoke {filter}: {err}"));
        assert!(
            output.status.success(),
            "live harness smoke {filter} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(not(feature = "live-harness"))]
mod live {
    #[test]
    fn live_harness_feature_disabled() {
        eprintln!("MEMORUM_EVAL_SKIP:live-harness feature disabled");
    }
}
