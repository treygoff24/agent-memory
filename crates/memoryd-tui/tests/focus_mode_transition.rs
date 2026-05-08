use std::time::Instant;

use memorum_theme::Theme;
use memoryd_tui::app::{App, DaemonSnapshot};

#[test]
fn no_motion_skips_focus_transition() {
    let mut theme = Theme::default_warm_dark();
    theme.motion.enabled = false;
    let mut app = App::with_theme(DaemonSnapshot::sample(), theme);
    app.enter_reality_check_focus("session-1", 0, 7);

    assert_eq!(app.focus_transition_percent(), 100);
}

#[test]
fn motion_transition_advances_with_ticks() {
    let mut theme = Theme::default_warm_dark();
    theme.motion.enabled = true;
    theme.motion.slide_in_ms = 64;
    theme.motion.tick_ms = 16;
    let mut app = App::with_theme(DaemonSnapshot::sample(), theme);
    app.enter_reality_check_focus("session-1", 0, 7);

    assert_eq!(app.focus_transition_percent(), 0);
    app.on_tick(Instant::now());
    assert_eq!(app.focus_transition_percent(), 25);
    for _ in 0..3 {
        app.on_tick(Instant::now());
    }
    assert_eq!(app.focus_transition_percent(), 100);
}
