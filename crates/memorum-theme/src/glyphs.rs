use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Glyphs {
    #[serde(default = "default_brand")]
    pub brand: String,
    #[serde(default = "default_review")]
    pub review: String,
    #[serde(default = "default_recall")]
    pub recall: String,
    #[serde(default = "default_conflict")]
    pub conflict: String,
    #[serde(default = "default_dream")]
    pub dream: String,
    #[serde(default = "default_due")]
    pub due: String,
    #[serde(default = "default_memory")]
    pub memory: String,
    #[serde(default = "default_cursor")]
    pub cursor: String,
    #[serde(default = "default_selection_gutter")]
    pub selection_gutter: String,
    #[serde(default = "default_progress_filled")]
    pub progress_filled: String,
    #[serde(default = "default_progress_empty")]
    pub progress_empty: String,
    #[serde(default = "default_pill_separator")]
    pub pill_separator: String,
    #[serde(default = "default_palette_prompt")]
    pub palette_prompt: String,
}

impl Glyphs {
    pub fn ascii_fallback() -> Self {
        Self {
            brand: "*".to_string(),
            review: "*".to_string(),
            recall: ">".to_string(),
            conflict: "!".to_string(),
            dream: "<>".to_string(),
            due: "#".to_string(),
            memory: "o".to_string(),
            cursor: ">".to_string(),
            selection_gutter: "|".to_string(),
            progress_filled: "#".to_string(),
            progress_empty: "-".to_string(),
            pill_separator: "|".to_string(),
            palette_prompt: ">".to_string(),
        }
    }
}

impl Default for Glyphs {
    fn default() -> Self {
        Self {
            brand: "◆".to_string(),
            review: "●".to_string(),
            recall: "▸".to_string(),
            conflict: "⚠".to_string(),
            dream: "◇".to_string(),
            due: "▣".to_string(),
            memory: "○".to_string(),
            cursor: "▸".to_string(),
            selection_gutter: "▌".to_string(),
            progress_filled: "█".to_string(),
            progress_empty: "░".to_string(),
            pill_separator: "·".to_string(),
            palette_prompt: "⌘".to_string(),
        }
    }
}

fn default_brand() -> String {
    Glyphs::default().brand
}
fn default_review() -> String {
    Glyphs::default().review
}
fn default_recall() -> String {
    Glyphs::default().recall
}
fn default_conflict() -> String {
    Glyphs::default().conflict
}
fn default_dream() -> String {
    Glyphs::default().dream
}
fn default_due() -> String {
    Glyphs::default().due
}
fn default_memory() -> String {
    Glyphs::default().memory
}
fn default_cursor() -> String {
    Glyphs::default().cursor
}
fn default_selection_gutter() -> String {
    Glyphs::default().selection_gutter
}
fn default_progress_filled() -> String {
    Glyphs::default().progress_filled
}
fn default_progress_empty() -> String {
    Glyphs::default().progress_empty
}
fn default_pill_separator() -> String {
    Glyphs::default().pill_separator
}
fn default_palette_prompt() -> String {
    Glyphs::default().palette_prompt
}
