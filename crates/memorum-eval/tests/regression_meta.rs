use std::fs;
use std::path::{Path, PathBuf};

const REGRESSION_DIR: &str = "tests/eval/regression";
const REQUIRED_FIELDS: &[RequiredField] = &[
    RequiredField { label: "test number", needles: &["regression test #", "test #"] },
    RequiredField { label: "incident date", needles: &["incident:", "incident date:"] },
    RequiredField { label: "description", needles: &["description:", "production failure"] },
    RequiredField { label: "root cause", needles: &["root cause:"] },
    RequiredField { label: "fix commit", needles: &["fix commit:", "fixed in commit"] },
];

struct RequiredField {
    label: &'static str,
    needles: &'static [&'static str],
}

#[test]
fn regression_tests_have_required_metadata_blocks() {
    let regression_dir = regression_dir();
    assert!(
        regression_dir.is_dir(),
        "regression test directory must exist and be scannable: {}",
        regression_dir.display()
    );

    let mut failures =
        regression_files(&regression_dir).into_iter().filter_map(validate_regression_file).collect::<Vec<_>>();
    failures.sort();

    assert!(failures.is_empty(), "regression metadata contract violations:\n{}", failures.join("\n"));
}

fn regression_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(REGRESSION_DIR)
}

fn regression_files(regression_dir: &Path) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(regression_dir)
        .unwrap_or_else(|error| panic!("regression test directory should be readable: {error}"))
        .map(|entry| entry.expect("regression directory entries should be readable").path())
        .filter(|path| is_numbered_regression_file(path))
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn is_numbered_regression_file(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    path.extension().and_then(|extension| extension.to_str()) == Some("rs")
        && file_name.starts_with('t')
        && file_name.get(1..3).is_some_and(|digits| digits.chars().all(|character| character.is_ascii_digit()))
        && file_name.get(3..4) == Some("_")
}

fn validate_regression_file(path: PathBuf) -> Option<String> {
    let body =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()));
    let doc_block = leading_doc_comment_block(&body);
    let relative_path = path.strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap_or(&path);

    if doc_block.is_empty() {
        return Some(format!("{}: missing leading //! doc-comment metadata block", relative_path.display()));
    }

    let normalized = doc_block.to_ascii_lowercase();
    let missing = REQUIRED_FIELDS
        .iter()
        .filter(|field| !field.needles.iter().any(|needle| normalized.contains(needle)))
        .map(|field| field.label)
        .collect::<Vec<_>>();

    if missing.is_empty() {
        None
    } else {
        Some(format!("{}: missing {}", relative_path.display(), missing.join(", ")))
    }
}

fn leading_doc_comment_block(body: &str) -> String {
    let mut block = Vec::new();
    for line in body.lines() {
        if line.starts_with("//!") {
            block.push(line);
            continue;
        }

        if line.trim().is_empty() && !block.is_empty() {
            block.push(line);
            continue;
        }

        break;
    }

    block.join("\n")
}
