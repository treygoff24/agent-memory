#[path = "eval/domain/t13_cross_harness_substrate_sharing.rs"]
mod t13_cross_harness_substrate_sharing;
#[path = "eval/domain/t14_merge_driver_semantic_correctness.rs"]
mod t14_merge_driver_semantic_correctness;
#[path = "eval/domain/t15_privacy_filter_refusal_retry.rs"]
mod t15_privacy_filter_refusal_retry;
#[path = "eval/domain/t16_drift_scoring_sanity.rs"]
mod t16_drift_scoring_sanity;
#[path = "eval/domain/t17_lease_contention_resolution.rs"]
mod t17_lease_contention_resolution;
#[path = "eval/domain/t18_encrypted_tier_key_rotation.rs"]
mod t18_encrypted_tier_key_rotation;

mod support {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::Value;

    pub fn daemon_request(socket_path: &Path, request: Value) -> Value {
        let mut stream = UnixStream::connect(socket_path)
            .unwrap_or_else(|err| panic!("connect to memoryd socket {}: {err}", socket_path.display()));
        let frame = serde_json::json!({"id": unique_request_id(), "request": request});
        writeln!(stream, "{frame}").expect("write daemon request");

        let mut response = String::new();
        BufReader::new(stream).read_line(&mut response).expect("read daemon response");
        serde_json::from_str(&response).unwrap_or_else(|err| panic!("daemon response is JSON: {err}\n{response}"))
    }

    pub fn git<const N: usize>(repo: &Path, args: [&str; N]) {
        let output = Command::new("git").args(args).current_dir(repo).output().expect("run git command");
        assert!(
            output.status.success(),
            "git command failed in {}: git {:?}\nstatus={}\nstdout={}\nstderr={}",
            repo.display(),
            args,
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub fn command_output<const N: usize>(program: &Path, args: [&str; N]) -> std::process::Output {
        Command::new(program).args(args).output().unwrap_or_else(|err| panic!("run {}: {err}", program.display()))
    }

    pub fn debug_binary(name: &str, package: &str) -> std::path::PathBuf {
        let target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
        let binary = target_dir.join("debug").join(name);
        if binary.exists() {
            return binary;
        }

        let status = Command::new("cargo")
            .args(["build", "-p", package])
            .stdin(Stdio::null())
            .status()
            .unwrap_or_else(|err| panic!("build {package}: {err}"));
        assert!(status.success(), "cargo build -p {package} failed");
        binary
    }

    pub fn read_device_id(runtime_dir: &Path) -> String {
        let path = runtime_dir.join("local-device.yaml");
        let yaml = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        yaml.lines()
            .map(str::trim)
            .find_map(|line| line.strip_prefix("id: "))
            .unwrap_or_else(|| panic!("local-device.yaml missing device id:\n{yaml}"))
            .to_owned()
    }

    pub fn find_file_with_extension(root: &Path, extension: &str) -> Vec<std::path::PathBuf> {
        let mut matches = Vec::new();
        collect_files_with_extension(root, extension, &mut matches);
        matches
    }

    fn collect_files_with_extension(root: &Path, extension: &str, matches: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files_with_extension(&path, extension, matches);
            } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
                matches.push(path);
            }
        }
    }

    fn unique_request_id() -> String {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock after unix epoch").as_nanos();
        format!("memorum-eval-domain-{nanos}")
    }
}
