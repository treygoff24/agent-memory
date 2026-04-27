//! Rust boundary checks for Stream A.

use std::path::{Path, PathBuf};

fn main() {
    let root = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    if let Err(err) = run(&root) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run(root: &Path) -> Result<(), String> {
    check_no_absolute_test_paths(root)?;
    check_no_unwrap_expect(root)?;
    Ok(())
}

fn check_no_absolute_test_paths(root: &Path) -> Result<(), String> {
    let tests = root.join("crates/memory-substrate/tests");
    if !tests.exists() {
        return Ok(());
    }
    for file in rust_files(&tests) {
        let text = std::fs::read_to_string(&file).map_err(|err| err.to_string())?;
        for (line, content) in text.lines().enumerate() {
            if ["/Users/", "/home/", "/var/", "/tmp/"].iter().any(|needle| content.contains(needle))
                && !content.contains("Roots")
            {
                return Err(format!("absolute path literal in {}:{}", file.display(), line + 1));
            }
        }
    }
    Ok(())
}

fn check_no_unwrap_expect(root: &Path) -> Result<(), String> {
    let src = root.join("crates/memory-substrate/src");
    if !src.exists() {
        return Ok(());
    }
    for file in rust_files(&src) {
        let text = std::fs::read_to_string(&file).map_err(|err| err.to_string())?;
        let lines: Vec<&str> = text.lines().collect();
        for (line, content) in lines.iter().enumerate() {
            let has_forbidden = content.contains(".unwrap()") || content.contains(".expect(");
            let current_line_justifies = content.contains("unwrap-justified:") || content.contains("expect-justified:");
            let next_line_justifies = lines
                .get(line + 1)
                .is_some_and(|next| next.contains("unwrap-justified:") || next.contains("expect-justified:"));
            if has_forbidden && !current_line_justifies && !next_line_justifies {
                return Err(format!("raw unwrap/expect in {}:{}", file.display(), line + 1));
            }
        }
    }
    Ok(())
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let path = entry.into_path();
            if path.extension().is_some_and(|ext| ext == "rs") {
                Some(path)
            } else {
                None
            }
        })
        .collect()
}
