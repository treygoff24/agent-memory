//! Task 4: the `reveal` client-side gate and `observe` validation, end to end.
//!
//! `reveal` without `--allow-reveal` must refuse (exit 77) before any socket
//! connection — proven by pointing it at a socket path that does not exist and
//! asserting we still get the client gate, not a daemon-unreachable error.
//! `observe` round-trips against a live daemon, and its bounds (16 KiB text,
//! `ent_*` entities) surface as exit-65 validation errors with envelopes.

use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use serde_json::Value;

#[test]
fn reveal_without_allow_flag_refuses_with_77_before_connecting() {
    let missing = std::env::temp_dir().join("memoryd-reveal-gate-absent.sock");
    let _ = std::fs::remove_file(&missing);
    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args(["reveal", "mem_20260708_a1b2c3d4e5f60718_000001", "--reason", "check", "--socket"])
        .arg(&missing)
        .output()
        .expect("run reveal without --allow-reveal");
    assert_eq!(output.status.code(), Some(77), "reveal gate is exit 77, not daemon-unreachable 75");
    assert!(output.stdout.is_empty(), "gate refusal writes nothing to stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    let json: Value = serde_json::from_str(stderr.trim()).expect("stderr is one JSON error envelope");
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "reveal_not_allowed");
    assert!(json["error"]["suggested_fix"].as_str().unwrap().contains("--allow-reveal"));
}

struct ServeGuard {
    child: Child,
}

impl Drop for ServeGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_daemon(repo: &Path, runtime: &Path, socket: &Path) -> ServeGuard {
    let child = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args(["serve", "--init", "--force-unsafe-durability", "--repo"])
        .arg(repo)
        .arg("--runtime")
        .arg(runtime)
        .arg("--socket")
        .arg(socket)
        .spawn()
        .expect("spawn memoryd serve");
    let guard = ServeGuard { child };
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let ready = Command::new(env!("CARGO_BIN_EXE_memoryd"))
            .args(["status", "--socket"])
            .arg(socket)
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);
        if ready {
            return guard;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("daemon did not become ready within 30s");
}

fn observe(socket: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .arg("observe")
        .args(args)
        .arg("--socket")
        .arg(socket)
        .output()
        .expect("run observe")
}

#[test]
fn observe_round_trips_and_enforces_bounds() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let socket = temp.path().join("memoryd.sock");
    let _daemon = start_daemon(&repo, &runtime, &socket);

    // Happy path: a plain observation round-trips to a success envelope.
    let ok = observe(&socket, &["a useful observation about the build", "--kind", "observation"]);
    assert_eq!(ok.status.code(), Some(0), "observe should succeed: {}", String::from_utf8_lossy(&ok.stderr));
    let json: Value = serde_json::from_slice(&ok.stdout).expect("observe stdout is one JSON success envelope");
    assert_eq!(json["ok"], true);
    assert!(json["data"]["fragment_id"].is_string(), "observe returns a fragment id");

    // Oversize text (>16 KiB) is a 65-class validation error on stderr.
    let big = "x".repeat(17 * 1024);
    let oversize = observe(&socket, &[big.as_str(), "--kind", "observation"]);
    assert_eq!(oversize.status.code(), Some(65), "oversize observe text is a validation error");
    let err: Value =
        serde_json::from_slice(&oversize.stderr).expect("oversize observe error is one JSON envelope on stderr");
    assert_eq!(err["ok"], false);

    // A malformed entity id (missing the `ent_` prefix) is likewise a 65.
    let bad_entity = observe(&socket, &["fine text", "--kind", "signal", "--entity", "not-an-entity-id"]);
    assert_eq!(bad_entity.status.code(), Some(65), "bad entity id is a validation error");
    assert!(bad_entity.stdout.is_empty(), "validation failure writes nothing to stdout");
}
