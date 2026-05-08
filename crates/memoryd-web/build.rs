use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=frontend/src");
    println!("cargo:rerun-if-changed=frontend/tests");
    println!("cargo:rerun-if-changed=frontend/index.html");
    println!("cargo:rerun-if-changed=frontend/package.json");
    println!("cargo:rerun-if-changed=frontend/pnpm-lock.yaml");
    println!("cargo:rerun-if-changed=frontend/vite.config.ts");

    let frontend = Path::new("frontend");
    run(frontend, &["install", "--frozen-lockfile"]);
    run(frontend, &["run", "build"]);
}

fn run(current_dir: &Path, args: &[&str]) {
    let status = Command::new("pnpm")
        .args(args)
        .current_dir(current_dir)
        .status()
        .unwrap_or_else(|error| panic!("failed to run pnpm {}: {error}", args.join(" ")));
    if !status.success() {
        panic!("pnpm {} failed with {status}", args.join(" "));
    }
}
