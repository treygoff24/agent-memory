use memory_substrate::{AdoptOptions, InitOptions, OpenError, Roots, Substrate};

#[tokio::test]
async fn open_rejects_unmarked_directory_without_mutating_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("not-substrate");
    let runtime = temp.path().join("runtime");
    std::fs::create_dir_all(&repo).expect("repo");

    let err = match Substrate::open(Roots::new(&repo, &runtime)).await {
        Ok(_) => panic!("must reject non-substrate"),
        Err(err) => err,
    };

    assert!(matches!(err, OpenError::NotAMemorumSubstrate { path } if path == repo));
    assert!(!repo.join(".memorum").exists(), "open must not bootstrap a marker");
    assert!(!repo.join("events").exists(), "open must not bootstrap memory dirs");
    assert!(!runtime.exists(), "open must not create runtime dirs before repo validation");
}

#[tokio::test]
async fn init_creates_marker_that_allows_later_open() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));

    Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_openvalidation".to_string()) },
    )
    .await
    .expect("init");

    assert!(roots.repo.join(".memorum/substrate").is_file());
    Substrate::open(roots).await.expect("open initialized substrate");
}

#[tokio::test]
async fn adopt_clone_requires_explicit_merge_driver_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    memory_substrate::tree::bootstrap_repo_tree(&repo).expect("bootstrap repo");
    git(&repo, &["init"]).expect("git init");

    let err = match Substrate::adopt_clone(Roots::new(&repo, &runtime), AdoptOptions::default()).await {
        Ok(_) => panic!("ambient merge driver lookup must be refused"),
        Err(err) => err,
    };

    assert!(matches!(err, OpenError::InvalidRoots(message) if message.contains("merge_driver_path")));
}

fn git(repo: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let output =
        std::process::Command::new("git").args(args).current_dir(repo).output().map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}
