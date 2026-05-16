use memorum_theme::presets::PRESETS;
use memorum_theme::{ColorTokens, Theme};
use toml::Value;

const REQUIRED_GLYPHS: &[&str] = &[
    "brand",
    "review",
    "recall",
    "conflict",
    "dream",
    "due",
    "memory",
    "cursor",
    "selection_gutter",
    "progress_filled",
    "progress_empty",
    "pill_separator",
    "palette_prompt",
];

#[test]
fn every_preset_declares_required_tokens_and_glyphs() {
    assert_eq!(PRESETS.len(), 6);
    for (name, body) in PRESETS {
        let value = body.parse::<Value>().unwrap_or_else(|error| panic!("{name} parses as TOML: {error}"));
        let colors = value.get("colors").and_then(Value::as_table).unwrap_or_else(|| panic!("{name} has colors"));
        for token in ColorTokens::REQUIRED {
            assert!(colors.contains_key(token), "{name} missing token {token}");
        }
        let glyphs = value.get("glyphs").and_then(Value::as_table).unwrap_or_else(|| panic!("{name} has glyphs"));
        for glyph in REQUIRED_GLYPHS {
            let value =
                glyphs.get(*glyph).and_then(Value::as_str).unwrap_or_else(|| panic!("{name} missing glyph {glyph}"));
            assert!(!value.is_empty(), "{name} glyph {glyph} should not be empty");
        }
        let theme: Theme = toml::from_str(body).unwrap_or_else(|error| panic!("{name} deserializes: {error}"));
        assert!(!theme.glyphs.review.is_empty());
        assert!(!theme.glyphs.palette_prompt.is_empty());
    }
}
