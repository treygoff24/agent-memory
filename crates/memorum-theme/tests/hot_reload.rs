use std::time::Duration;

use memorum_theme::{HotReload, Loader};

#[tokio::test]
async fn hot_reload_advances_on_valid_change_and_holds_on_invalid_change() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("theme.toml");
    let initial = include_str!("../src/presets/default_warm_dark.toml");
    std::fs::write(&path, initial).expect("write initial");
    let theme = Loader::resolve(None, Some(&path)).expect("initial theme loads");
    let (hot_reload, mut receiver) = HotReload::start(path.clone(), theme);

    let mut changed = initial.replace("default-warm-dark", "default-warm-dark-copy");
    changed = changed.replace("oklch(0.80 0.130 72)", "oklch(0.81 0.130 72)");
    std::fs::write(&path, changed).expect("write changed");
    wait_for_change(&mut receiver).await;
    assert_eq!(receiver.borrow().name, "default-warm-dark-copy");

    std::fs::write(&path, "not = [valid").expect("write invalid");
    assert!(tokio::time::timeout(Duration::from_secs(2), receiver.changed()).await.is_err());
    assert!(hot_reload.last_error().is_some());
}

async fn wait_for_change(receiver: &mut tokio::sync::watch::Receiver<memorum_theme::Theme>) {
    tokio::time::timeout(Duration::from_secs(2), receiver.changed())
        .await
        .expect("hot reload should publish change")
        .expect("hot-reload sender should stay open");
}
