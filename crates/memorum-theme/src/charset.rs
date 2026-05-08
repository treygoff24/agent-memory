use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Charset {
    Full,
    Extended,
    Minimal,
}

impl Charset {
    pub fn detect() -> Self {
        let locale = std::env::var("LC_ALL")
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(|| std::env::var("LANG").ok())
            .unwrap_or_default()
            .to_ascii_uppercase();
        if !locale.contains("UTF-8") && !locale.contains("UTF8") {
            return Self::Minimal;
        }
        let term = std::env::var("TERM").unwrap_or_default().to_ascii_lowercase();
        if ["alacritty", "ghostty", "kitty", "wezterm", "foot", "iterm"].iter().any(|needle| term.contains(needle)) {
            Self::Full
        } else {
            Self::Extended
        }
    }
}
