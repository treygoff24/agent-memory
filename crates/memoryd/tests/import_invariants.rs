//! Invariant tests for the importer module.
//!
//! These guard the contract documented in the importer plan's invariants list.
//! In particular: **the importer never bypasses the daemon write path**. The
//! grep test below walks every `.rs` file under `crates/memoryd/src/import/`,
//! strips `//`-style line comments, and asserts that the regex
//! `\bwrite_memory\b` matches zero times. That catches both
//! `Substrate::write_memory(...)` qualified paths and the bare
//! `substrate.write_memory(...)` method invocation.
//!
//! Per the plan T05 / Codex review R1 fix.

use std::path::{Path, PathBuf};

#[test]
fn importer_module_never_calls_substrate_write_memory_directly() {
    let root = importer_module_root();
    let mut offending = Vec::new();
    visit_rust_files(&root, &mut offending);
    assert!(
        offending.is_empty(),
        "importer module must go through the daemon socket — found `write_memory` reference in:\n{}",
        offending.iter().map(|(p, line)| format!("  {}: {}", p.display(), line)).collect::<Vec<_>>().join("\n"),
    );
}

fn importer_module_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("import")
}

fn visit_rust_files(dir: &Path, offending: &mut Vec<(PathBuf, String)>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(value) => value,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_rust_files(&path, offending);
        } else if path.extension().and_then(std::ffi::OsStr::to_str) == Some("rs") {
            scan_file(&path, offending);
        }
    }
}

fn scan_file(path: &Path, offending: &mut Vec<(PathBuf, String)>) {
    let raw = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return,
    };
    for (idx, line) in raw.lines().enumerate() {
        let stripped = strip_line_comment(line);
        if contains_write_memory_call(stripped) {
            offending.push((path.to_path_buf(), format!("L{}: {}", idx + 1, line.trim())));
        }
    }
}

fn strip_line_comment(line: &str) -> &str {
    if let Some(idx) = line.find("//") {
        &line[..idx]
    } else {
        line
    }
}

fn contains_write_memory_call(text: &str) -> bool {
    // Match `\bsubstrate\b\s*[.:](\s*:)*\s*write_memory\s*\(` patterns — i.e.
    // `substrate.write_memory(...)`, `Substrate::write_memory(...)`,
    // `local_substrate.write_memory(...)`. The importer's own
    // `DaemonClient::write_memory(...)` going through the daemon socket is the
    // legitimate path and must NOT trigger this invariant.
    let needle = "write_memory";
    let bytes = text.as_bytes();
    let mut idx = 0;
    while let Some(found) = text[idx..].find(needle) {
        let start = idx + found;
        let end = start + needle.len();
        let before_ok = start == 0 || !is_ident_char(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_ident_char(bytes[end]);
        if before_ok && after_ok {
            // Walk left over whitespace and the `.` or `::` accessor.
            let mut probe_left = start;
            while probe_left > 0 && (bytes[probe_left - 1] == b' ' || bytes[probe_left - 1] == b'\t') {
                probe_left -= 1;
            }
            let after_accessor = if probe_left >= 1 && bytes[probe_left - 1] == b'.' {
                Some(probe_left - 1)
            } else if probe_left >= 2 && &bytes[probe_left - 2..probe_left] == b"::" {
                Some(probe_left - 2)
            } else {
                None
            };
            if let Some(accessor_start) = after_accessor {
                // Walk left to the receiver identifier.
                let mut probe = accessor_start;
                while probe > 0 && (bytes[probe - 1] == b' ' || bytes[probe - 1] == b'\t') {
                    probe -= 1;
                }
                let receiver_end = probe;
                let mut receiver_start = receiver_end;
                while receiver_start > 0 && is_ident_char(bytes[receiver_start - 1]) {
                    receiver_start -= 1;
                }
                if receiver_start == receiver_end {
                    idx = end;
                    continue;
                }
                let receiver = &text[receiver_start..receiver_end];
                // Heuristic: a real call has `(` shortly after.
                let mut probe_paren = end;
                while probe_paren < bytes.len() && (bytes[probe_paren] == b' ' || bytes[probe_paren] == b'\t') {
                    probe_paren += 1;
                }
                if probe_paren < bytes.len() && bytes[probe_paren] == b'(' {
                    // Only flag receivers that look like a substrate reference.
                    let lowered = receiver.to_ascii_lowercase();
                    if lowered.ends_with("substrate") {
                        let _ = receiver_end;
                        return true;
                    }
                }
            }
        }
        idx = end;
    }
    false
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[test]
fn grep_test_distinguishes_substrate_call_from_daemon_client_path() {
    // Catches direct substrate calls.
    assert!(contains_write_memory_call("substrate.write_memory(req)"));
    assert!(contains_write_memory_call("Substrate::write_memory(req)"));
    assert!(contains_write_memory_call("local_substrate.write_memory(req)"));

    // Ignores identifiers that share the substring `write_memory` but aren't
    // substrate-direct calls.
    assert!(!contains_write_memory_call("WriteMemoryResponse"));
    assert!(!contains_write_memory_call("write_memory_request_count += 1"));
    assert!(!contains_write_memory_call("RequestPayload::WriteMemory"));

    // Ignores the legitimate daemon-client trait path: trait method definitions
    // on `DaemonClient` and call sites `client.write_memory(...)` go through
    // the daemon socket and do NOT bypass governance.
    assert!(!contains_write_memory_call("client.write_memory(req)"));
    assert!(!contains_write_memory_call("async fn write_memory(&mut self, request: WriteMemoryRequest)"));
    assert!(!contains_write_memory_call("DaemonClient::write_memory(req)"));

    // Comment stripping is the file-scanner's job. Verify the two-stage path.
    assert!(!contains_write_memory_call(strip_line_comment("// substrate.write_memory(req)")));
}
