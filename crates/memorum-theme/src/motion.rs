use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_slide_in_ms")]
    pub slide_in_ms: u16,
    #[serde(default = "default_undo_window_ms")]
    pub undo_window_ms: u16,
    #[serde(default = "default_tick_ms")]
    pub tick_ms: u16,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            slide_in_ms: default_slide_in_ms(),
            undo_window_ms: default_undo_window_ms(),
            tick_ms: default_tick_ms(),
        }
    }
}

impl MotionConfig {
    pub fn reduced() -> Self {
        Self { enabled: false, slide_in_ms: 0, undo_window_ms: default_undo_window_ms(), tick_ms: default_tick_ms() }
    }
}

fn default_enabled() -> bool {
    true
}
fn default_slide_in_ms() -> u16 {
    350
}
fn default_undo_window_ms() -> u16 {
    3000
}
fn default_tick_ms() -> u16 {
    16
}
