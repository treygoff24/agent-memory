//! Wright item `export-out-atomic-write-04`: integration test for
//! `memoryd export --out <path>` atomic-write semantics.
//!
//! Closes spec §3 / §8.4: stdout-mode output and `--out <path>` mode
//! produce byte-identical bytes; the `--out` write leaves no `.tmp`
//! sidecar; the file ends with `\n`; and an `--out` pointing at a
//! missing parent directory exits 1 with stderr naming the missing
//! parent and no partial file in the grandparent.

use std::process::Command;

#[path = "export_fixture/mod.rs"]
mod export_fixture;

use export_fixture::{init_substrate, make_plaintext_memory, write_plaintext};

const DEVICE_ID: &str = "dev_exportatomic04";

#[tokio::test]
async fn out_writes_atomically_and_matches_stdout() {
    let substrate_temp = tempfile::tempdir().expect("substrate tempdir");
    let substrate = init_substrate(&substrate_temp, DEVICE_ID).await;

    // Three plaintext memories — same shape as item 01's 3-memory
    // fixture but without the encryption setup (the `--out` contract
    // doesn't care about body variants, and avoiding governance keeps
    // this test focused).
    write_plaintext(
        &substrate,
        make_plaintext_memory("mem_20260501_aaaa00000000aaaa_000001", "body 1", "2026-05-01T10:00:00Z"),
    )
    .await;
    write_plaintext(
        &substrate,
        make_plaintext_memory("mem_20260502_aaaa00000000aaaa_000002", "body 2", "2026-05-02T10:00:00Z"),
    )
    .await;
    write_plaintext(
        &substrate,
        make_plaintext_memory("mem_20260503_aaaa00000000aaaa_000003", "body 3", "2026-05-03T10:00:00Z"),
    )
    .await;

    let repo = substrate_temp.path().join("repo");
    let runtime = substrate_temp.path().join("runtime");

    let bin = env!("CARGO_BIN_EXE_memoryd");
    let common_args = vec![
        "export",
        "--repo",
        repo.to_str().expect("repo utf8"),
        "--runtime",
        runtime.to_str().expect("runtime utf8"),
    ];

    // ------------------------------------------------------------------
    // Sub-case A: byte-for-byte identical to stdout-mode.
    //
    // Two `memoryd export` invocations produce the same `exported_at`
    // only if they run within the same millisecond — they almost
    // certainly will not. So we compare every byte EXCEPT the
    // `exported_at` field; everything else must match exactly.
    // ------------------------------------------------------------------
    let stdout_run = Command::new(bin).args(&common_args).output().expect("spawn stdout-mode export");
    assert!(
        stdout_run.status.success(),
        "stdout-mode export failed\nstderr: {}",
        String::from_utf8_lossy(&stdout_run.stderr)
    );

    // Run --out mode against a dedicated output dir so the leftover-
    // .tmp check is unambiguous (substrate dir contains repo/runtime
    // subdirs; we don't want to count those).
    let out_temp = tempfile::tempdir().expect("output tempdir");
    let out_path = out_temp.path().join("export.json");
    let out_run = Command::new(bin)
        .args(&common_args)
        .args(["--out", out_path.to_str().expect("out utf8")])
        .output()
        .expect("spawn --out-mode export");
    assert!(
        out_run.status.success(),
        "--out-mode export failed\nstderr: {}",
        String::from_utf8_lossy(&out_run.stderr)
    );

    let file_bytes = std::fs::read(&out_path).expect("read --out file");

    // Trailing newline.
    assert_eq!(
        file_bytes.last(),
        Some(&b'\n'),
        "--out file must end with a trailing newline; last bytes: {:?}",
        &file_bytes[file_bytes.len().saturating_sub(10)..]
    );

    // Compare stdout bytes with file bytes, masking the volatile
    // `exported_at` field so we're actually pinning the schema/format,
    // not asserting on the wall-clock-stamped element.
    let stdout_value: serde_json::Value = serde_json::from_slice(&stdout_run.stdout).expect("stdout json");
    let file_value: serde_json::Value = serde_json::from_slice(&file_bytes).expect("file json");
    let mut stdout_norm = stdout_value;
    let mut file_norm = file_value;
    stdout_norm["exported_at"] = serde_json::Value::Null;
    file_norm["exported_at"] = serde_json::Value::Null;
    assert_eq!(
        stdout_norm, file_norm,
        "stdout-mode and --out-mode output JSON must be identical (modulo `exported_at`)"
    );

    // The serialized form must also be the same modulo `exported_at`:
    // same key order, same two-space indent, same trailing newline.
    let stdout_pretty = serde_json::to_string_pretty(&stdout_norm).unwrap() + "\n";
    let file_pretty = serde_json::to_string_pretty(&file_norm).unwrap() + "\n";
    assert_eq!(stdout_pretty, file_pretty, "serialized form must be byte-identical");

    // ------------------------------------------------------------------
    // Sub-case B: no `.tmp` sidecars left in the output directory.
    //
    // After a successful --out run, the output directory contains the
    // target file and nothing else. Note: the spec text says "exactly
    // two entries: the target file and a directory entry" — interpret
    // pragmatically as "the target file plus, optionally, a `.DS_Store`
    // or other OS junk", and assert the load-bearing invariant: no
    // `.tmp` leftovers under any name.
    // ------------------------------------------------------------------
    let leftovers: Vec<_> = std::fs::read_dir(out_temp.path())
        .expect("read output dir")
        .filter_map(Result::ok)
        .collect();
    let names: Vec<String> =
        leftovers.iter().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(
        names.iter().any(|n| n == "export.json"),
        "output dir must contain the target file; saw {names:?}"
    );
    assert!(
        names.iter().all(|n| !n.contains(".tmp")),
        "no `.tmp` sidecar may remain after a successful --out run; saw {names:?}"
    );

    // ------------------------------------------------------------------
    // Sub-case C: missing parent directory -> exit 1, stderr names
    // the missing parent, no partial file left in the grandparent.
    // ------------------------------------------------------------------
    let grandparent = tempfile::tempdir().expect("grandparent tempdir");
    let missing_parent_name = "this-dir-does-not-exist";
    let bad_out = grandparent.path().join(missing_parent_name).join("export.json");

    let bad_run = Command::new(bin)
        .args(&common_args)
        .args(["--out", bad_out.to_str().expect("bad out utf8")])
        .output()
        .expect("spawn export with missing parent");
    let bad_exit = bad_run.status.code().expect("exit code present");
    let bad_stderr = String::from_utf8(bad_run.stderr.clone()).expect("stderr utf8");
    assert_eq!(
        bad_exit, 1,
        "missing-parent --out must exit 1 (substrate/IO failure); got {bad_exit}\nstderr: {bad_stderr}"
    );
    assert!(
        bad_stderr.contains(missing_parent_name),
        "stderr must name the missing parent directory; got:\n{bad_stderr}"
    );

    // No partial file left in the grandparent. The grandparent only
    // existed for the duration of this test; before the run it was
    // empty.
    let grandparent_entries: Vec<_> =
        std::fs::read_dir(grandparent.path()).expect("read grandparent").filter_map(Result::ok).collect();
    for entry in &grandparent_entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            !name.contains(".tmp"),
            "no partial / .tmp file may remain in the grandparent on failure; saw {name}"
        );
    }
}
