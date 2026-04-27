use memory_substrate::git::git_preflight;
use memory_substrate::GitError;

#[test]
fn missing_merge_driver_binary_refuses_before_merge() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(temp.path().join(".git")).expect("git dir");
    let missing = temp.path().join("missing-memory-merge-driver");

    let err = git_preflight(temp.path(), &missing).expect_err("missing driver refused");

    assert!(matches!(err, GitError::MergeDriverMissing(path) if path.contains("missing-memory-merge-driver")));
}
