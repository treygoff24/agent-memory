use std::fs;
use std::path::PathBuf;

#[test]
fn stream_h_eval_workflow_matches_ci_contract() {
    let workflow = read_workflow();

    assert_rc_tag_examples_match_workflow_patterns(&workflow);
    assert_not_contains(&workflow, "v[0-9]+.[0-9]+.[0-9]+-rc.[0-9]+", "regex-like release-candidate tag pattern");
    assert_contains(&workflow, r#"cron: "0 3 * * *""#, "daily cron schedule");
    assert_contains(&workflow, "workflow_dispatch:", "manual dispatch trigger");
    assert_contains(&workflow, "harness_mode:", "manual harness_mode input");

    assert_contains(&workflow, r#"jq -r '.failed' "$RESULT_FILE""#, "failed-count jq expression");
    assert_contains(&workflow, r#"[ "$FAILED" != "0" ]"#, "failed-count comparison");
    assert_contains(&workflow, ".number", "failure diagnostic test number field");
    assert_contains(&workflow, ".failure_detail", "failure diagnostic detail field");
    assert_not_contains(&workflow, ".test_id", "obsolete diagnostic test_id field");
    assert_not_contains(&workflow, ".failure_reason", "obsolete diagnostic failure_reason field");

    assert_contains(&workflow, r#"jq -r '.partial // false' "$RESULT_FILE""#, "partial-run jq expression");
    assert_contains(&workflow, r#"HARNESS_MODE=$(jq -r '.harness_mode' "$RESULT_FILE")"#, "harness-mode jq expression");
    assert_contains(
        &workflow,
        r#"[ "$PARTIAL" = "true" ] && [ "$HARNESS_MODE" != "mock" ]"#,
        "non-mock partial run rejection",
    );
    assert_contains(
        &workflow,
        r#"select(.status == "passed" and .mode == "real_harness" and (.number == 13 or .number == 15))"#,
        "mock semantic pass guard",
    );
    assert_contains(
        &workflow,
        r#"jq -r '.missing_credentials // [] | join(", ")' "$RESULT_FILE""#,
        "missing-credentials jq expression",
    );

    assert_contains(
        &workflow,
        "MEMORUM_EVAL_CLAUDE_KEY: ${{ secrets.MEMORUM_EVAL_CLAUDE_KEY }}",
        "Claude auth secret injection",
    );
    assert_contains(
        &workflow,
        "MEMORUM_EVAL_CODEX_KEY: ${{ secrets.MEMORUM_EVAL_CODEX_KEY }}",
        "Codex auth secret injection",
    );
    assert_contains(&workflow, "uses: actions/upload-artifact@v4", "artifact upload step");
    assert_contains(&workflow, "if: always()", "unconditional artifact upload");
}

fn read_workflow() -> String {
    fs::read_to_string(workflow_path()).expect("Stream H eval workflow should exist")
}

fn workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/stream-h-eval.yml")
}

fn assert_rc_tag_examples_match_workflow_patterns(workflow: &str) {
    let tag_patterns = extract_push_tag_patterns(workflow);

    for tag in ["v1.2.3-rc.4", "v12.0.345-rc.67"] {
        assert!(tag_patterns.iter().any(|pattern| tag_glob_matches(pattern, tag)), "RC tag should trigger: {tag}");
    }

    for tag in ["v1.2.3", "v1.2.3-beta.4", "release/v1.2.3-rc.4", "1.2.3-rc.4"] {
        assert!(
            !tag_patterns.iter().any(|pattern| tag_glob_matches(pattern, tag)),
            "non-RC tag should not trigger: {tag}"
        );
    }
}

fn extract_push_tag_patterns(workflow: &str) -> Vec<String> {
    let tags_line = workflow
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("tags: ["))
        .expect("workflow should configure push tag patterns");
    let (_, rest) = tags_line.split_once('[').expect("tag patterns should use inline array syntax");
    let (patterns, _) = rest.split_once(']').expect("tag patterns should close inline array syntax");
    patterns.split(',').map(|pattern| pattern.trim().trim_matches('"').to_owned()).collect()
}

fn tag_glob_matches(pattern: &str, tag: &str) -> bool {
    let pattern = pattern.as_bytes();
    let tag = tag.as_bytes();
    let mut matches = vec![vec![false; tag.len() + 1]; pattern.len() + 1];
    matches[0][0] = true;

    for pattern_index in 1..=pattern.len() {
        if pattern[pattern_index - 1] == b'*' {
            matches[pattern_index][0] = matches[pattern_index - 1][0];
        }
    }

    for pattern_index in 1..=pattern.len() {
        for tag_index in 1..=tag.len() {
            matches[pattern_index][tag_index] = if pattern[pattern_index - 1] == b'*' {
                matches[pattern_index - 1][tag_index] || matches[pattern_index][tag_index - 1]
            } else {
                pattern[pattern_index - 1] == tag[tag_index - 1] && matches[pattern_index - 1][tag_index - 1]
            };
        }
    }

    matches[pattern.len()][tag.len()]
}

fn assert_contains(workflow: &str, expected: &str, label: &str) {
    assert!(workflow.contains(expected), "workflow should contain {label}: {expected}");
}

fn assert_not_contains(workflow: &str, unexpected: &str, label: &str) {
    assert!(!workflow.contains(unexpected), "workflow should not contain {label}: {unexpected}");
}
