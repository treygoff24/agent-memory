use std::time::Duration;

use memorum_theme::{presets, HotReload, Theme};

#[tokio::test]
async fn hot_reload_advances_on_valid_theme_and_rejects_bad_toml() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("theme.toml");
    let initial_text = presets::get("default-warm-dark").expect("preset");
    std::fs::write(&path, initial_text).expect("write initial theme");
    let initial = Theme::from_loader(Some("default-warm-dark"), Some(&path)).expect("initial theme");
    let (hot_reload, mut rx) = HotReload::start(path.clone(), initial);

    let changed = initial_text
        .replace("name = \"default-warm-dark\"", "name = \"hot-reload-test\"")
        .replace("accent = \"oklch(0.80 0.130 72)\"", "accent = \"oklch(0.70 0.160 120)\"");
    std::fs::write(&path, changed).expect("write changed theme");
    wait_for_change(&mut rx).await.expect("valid change should advance");
    assert_eq!(rx.borrow().name, "hot-reload-test");

    let stable = rx.borrow().clone();
    std::fs::write(&path, "not = [valid").expect("write bad theme");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(*rx.borrow(), stable);
    assert!(hot_reload.last_error().is_some());
}

async fn wait_for_change(rx: &mut tokio::sync::watch::Receiver<Theme>) -> Result<(), tokio::time::error::Elapsed> {
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            interval.tick().await;
            if rx.has_changed().unwrap_or(false) {
                return;
            }
        }
    })
    .await
}
