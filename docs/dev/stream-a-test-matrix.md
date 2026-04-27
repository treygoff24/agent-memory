# Stream A test matrix

| Gate               | Command                                                                                                  | Status                                   |
| ------------------ | -------------------------------------------------------------------------------------------------------- | ---------------------------------------- |
| Rust fmt           | `cargo fmt --all -- --check`                                                                             | CI                                       |
| Rust lint          | `cargo clippy --workspace --all-targets --all-features -- -D warnings`                                   | CI                                       |
| Unit/integration   | `cargo test --workspace`                                                                                 | CI                                       |
| Docs               | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`                                             | CI                                       |
| JS/docs format     | `pnpm run format:check`                                                                                  | CI                                       |
| JS lint            | `pnpm run lint`                                                                                          | CI                                       |
| Specgate           | `specgate validate && specgate check --output-mode deterministic`                                        | CI when available                        |
| Boundary           | `./scripts/rust-boundary-check.sh`                                                                       | CI                                       |
| Convergence smoke  | `./scripts/two-clone-convergence.sh --smoke`                                                             | CI                                       |
| Perf release linux | `./scripts/bench-gate.sh --tier release --profile linux-x86_64 --output bench/results.linux-x86_64.json` | CI/release                               |
| Perf release macOS | `./scripts/bench-gate.sh --tier release --profile darwin-arm64 --output bench/results.darwin-arm64.json` | Manual until macOS runner is provisioned |
