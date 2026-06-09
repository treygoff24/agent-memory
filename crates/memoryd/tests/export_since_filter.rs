//! Wright item `export-since-filter-02`: integration test for
//! `memoryd export --since <ISO>` semantics.
//!
//! Verifies the §5 / §8.2 closure: the filter is `updated_at >= since`
//! (inclusive at the boundary) and the parser strictly rejects bare
//! dates with exit code 2 plus a stderr hint at the canonical RFC3339
//! form.
//!
//! Also covers B1 regression: spec §5 accepts both `Z` and `+00:00` offset
//! forms; the original implementation rejected `+00:00` with exit 2.

use std::process::Command;

#[path = "export_fixture/mod.rs"]
mod export_fixture;

use export_fixture::{init_substrate, make_plaintext_memory, write_plaintext};

const DEVICE_ID: &str = "dev_exportsince02";

#[tokio::test]
async fn since_filter_is_inclusive_and_rejects_bare_dates() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp, DEVICE_ID).await;

    // Four memories at exact 1-day intervals. T0 chosen well after any
    // chrono epoch defaults so a missing timestamp would never sort
    // ahead of any of these.
    let t0 = "2026-05-01T00:00:00Z"; // not included
    let t1 = "2026-05-02T00:00:00Z"; // not included
    let boundary = "2026-05-03T00:00:00Z"; // INCLUDED (== --since)
    let t3 = "2026-05-04T00:00:00Z"; // INCLUDED

    let id_t0 = "mem_20260501_aaaa00000000aaaa_000001";
    let id_t1 = "mem_20260502_aaaa00000000aaaa_000002";
    let id_boundary = "mem_20260503_aaaa00000000aaaa_000003";
    let id_t3 = "mem_20260504_aaaa00000000aaaa_000004";

    write_plaintext(&substrate, make_plaintext_memory(id_t0, "body t0", t0)).await;
    write_plaintext(&substrate, make_plaintext_memory(id_t1, "body t1", t1)).await;
    write_plaintext(&substrate, make_plaintext_memory(id_boundary, "body boundary", boundary)).await;
    write_plaintext(&substrate, make_plaintext_memory(id_t3, "body t3", t3)).await;

    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");

    // ------------------------------------------------------------------
    // Sub-case 1: inclusive boundary
    // ------------------------------------------------------------------
    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args([
            "export",
            "--repo",
            repo.to_str().expect("repo utf8"),
            "--runtime",
            runtime.to_str().expect("runtime utf8"),
            "--since",
            boundary,
        ])
        .output()
        .expect("spawn memoryd export with --since");
    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout utf-8");
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr utf-8");
    assert!(output.status.success(), "expected exit 0 with valid --since; got {}\nstderr: {stderr}", output.status);

    let value: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("stdout is not valid JSON: {e}\nstdout: {stdout}"));

    assert_eq!(value["memory_count"], serde_json::json!(2), "memory_count must be 2 (boundary inclusive)");
    let memories = value["memories"].as_array().expect("memories array");
    assert_eq!(memories.len(), 2, "memories.length must be 2");

    // Sort order is (updated_at, id) ascending — boundary comes first, t3 second.
    let included_ids: Vec<&str> = memories.iter().map(|m| m["id"].as_str().expect("id string")).collect();
    assert_eq!(
        included_ids,
        vec![id_boundary, id_t3],
        "included ids must be exactly the boundary (T+2d) and T+3d memories"
    );

    // filters.since should echo back the verbatim ISO string the operator passed.
    assert_eq!(value["filters"]["since"].as_str(), Some(boundary), "filters.since must be the verbatim --since value");

    // ------------------------------------------------------------------
    // Sub-case 2: bare-date input -> exit 2 with canonical-form hint
    // ------------------------------------------------------------------
    let bare = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args([
            "export",
            "--repo",
            repo.to_str().expect("repo utf8"),
            "--runtime",
            runtime.to_str().expect("runtime utf8"),
            "--since",
            "2026-05-01",
        ])
        .output()
        .expect("spawn memoryd export with bare-date --since");
    let bare_stderr = String::from_utf8(bare.stderr.clone()).expect("stderr utf-8");
    let bare_exit = bare.status.code().expect("export should exit with a code");
    assert_eq!(
        bare_exit, 2,
        "bare-date --since must exit 2 (argparse failure); got {bare_exit}\nstderr: {bare_stderr}"
    );
    assert!(
        bare.stdout.is_empty(),
        "bare-date --since failure must leave stdout empty; got:\n{}",
        String::from_utf8_lossy(&bare.stdout)
    );
    // The error message must point at the canonical RFC3339 form. The
    // spec allows wording flexibility — the test asserts the canonical
    // example token appears so an operator pasting a bare date sees the
    // exact form to use next.
    assert!(
        bare_stderr.contains("2026-05-01T00:00:00Z"),
        "bare-date error must mention the canonical RFC3339 form; got:\n{bare_stderr}"
    );
}

#[tokio::test]
async fn since_rejects_nonzero_offsets_with_empty_stdout() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp, DEVICE_ID).await;

    write_plaintext(
        &substrate,
        make_plaintext_memory("mem_20260503_cccc00000000cccc_000001", "body", "2026-05-03T00:00:00Z"),
    )
    .await;

    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args([
            "export",
            "--repo",
            repo.to_str().expect("repo utf8"),
            "--runtime",
            runtime.to_str().expect("runtime utf8"),
            "--since",
            "2026-05-03T00:00:00-05:00",
        ])
        .output()
        .expect("spawn memoryd export with nonzero offset --since");

    assert_eq!(
        output.status.code(),
        Some(2),
        "nonzero-offset --since must exit 2; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "nonzero-offset --since failure must leave stdout empty; got:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(
        stderr.contains("UTC") && stderr.contains("non-UTC"),
        "stderr must explain UTC-only --since contract; got:\n{stderr}"
    );
}

#[tokio::test]
async fn since_accepts_offset_qualified_rfc3339() {
    // B1 regression: spec §5 requires both `Z` and `+00:00` forms to be
    // accepted. The original implementation parsed `DateTime<Utc>` directly
    // and returned the error before the FixedOffset fallback ran, so any
    // `+00:00` value was rejected with exit 2.
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp, DEVICE_ID).await;

    let ts = "2026-05-03T00:00:00Z";
    let id_included = "mem_20260503_bbbb00000000bbbb_000001";
    let id_excluded = "mem_20260501_bbbb00000000bbbb_000002";

    write_plaintext(&substrate, make_plaintext_memory(id_included, "body included", ts)).await;
    write_plaintext(&substrate, make_plaintext_memory(id_excluded, "body excluded", "2026-05-01T00:00:00Z")).await;

    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");

    // Equivalent to `2026-05-03T00:00:00Z` but using `+00:00` suffix.
    let since_offset = "2026-05-03T00:00:00+00:00";

    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args([
            "export",
            "--repo",
            repo.to_str().expect("repo utf8"),
            "--runtime",
            runtime.to_str().expect("runtime utf8"),
            "--since",
            since_offset,
        ])
        .output()
        .expect("spawn memoryd export with +00:00 --since");

    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr utf-8");
    assert!(
        output.status.success(),
        "spec §5 requires +00:00 offset form to be accepted (B1 regression); exit={}\nstderr: {stderr}",
        output.status
    );

    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout utf-8");
    let value: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("stdout not valid JSON: {e}"));

    assert_eq!(
        value["memory_count"],
        serde_json::json!(1),
        "only the boundary memory (id_included) should pass the +00:00 filter"
    );
    let memories = value["memories"].as_array().expect("memories array");
    assert_eq!(memories[0]["id"].as_str(), Some(id_included), "included id mismatch");

    // filters.since must echo back the verbatim +00:00 string.
    assert_eq!(
        value["filters"]["since"].as_str(),
        Some(since_offset),
        "filters.since must reflect the verbatim --since value passed"
    );
}
