use std::time::Duration;

use memorum_theme::presets;
use memoryd_tui::{app::App, config::UiConfig};

#[tokio::test]
async fn hot_reload_advances_on_valid_theme_and_rejects_bad_toml() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("theme.toml");
    let initial_text = presets::get("default-warm-dark").expect("preset");
    std::fs::write(&path, initial_text).expect("write initial theme");
    let mut app = App::new(UiConfig { theme_config: Some(path.clone()), ..UiConfig::default() });

    let changed = initial_text
        .replace("name = \"default-warm-dark\"", "name = \"hot-reload-test\"")
        .replace("accent = \"oklch(0.80 0.130 72)\"", "accent = \"oklch(0.70 0.160 120)\"");
    std::fs::write(&path, changed).expect("write changed theme");
    wait_for_theme_name(&mut app, "hot-reload-test").await.expect("valid change should advance");
    assert_eq!(app.theme_name(), "hot-reload-test");

    let stable = app.theme().clone();
    std::fs::write(&path, "not = [valid").expect("write bad theme");
    wait_for_error(&mut app).await.expect("invalid theme should record an error");
    assert_eq!(app.theme(), &stable);
    assert!(app.hot_reload_error().is_some());
}

async fn wait_for_theme_name(app: &mut App, name: &str) -> Result<(), tokio::time::error::Elapsed> {
    wait_until(app, |app| app.theme_name() == name).await
}

async fn wait_for_error(app: &mut App) -> Result<(), tokio::time::error::Elapsed> {
    wait_until(app, |app| app.hot_reload_error().is_some()).await
}

async fn wait_until(app: &mut App, mut condition: impl FnMut(&App) -> bool) -> Result<(), tokio::time::error::Elapsed> {
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            interval.tick().await;
            app.on_tick(std::time::Instant::now());
            if condition(app) {
                return;
            }
        }
    })
    .await
}
