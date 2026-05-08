use memorum_theme::presets::PRESETS;
use memorum_theme::{ColorTokens, Theme};
use toml::Value;

#[test]
fn every_preset_declares_required_tokens_and_glyphs() {
    assert_eq!(PRESETS.len(), 6);
    for (name, body) in PRESETS {
        let value = body.parse::<Value>().unwrap_or_else(|error| panic!("{name} parses as TOML: {error}"));
        let colors = value.get("colors").and_then(Value::as_table).unwrap_or_else(|| panic!("{name} has colors"));
        for token in ColorTokens::REQUIRED {
            assert!(colors.contains_key(token), "{name} missing token {token}");
        }
        let theme: Theme = toml::from_str(body).unwrap_or_else(|error| panic!("{name} deserializes: {error}"));
        assert!(!theme.glyphs.review.is_empty());
        assert!(!theme.glyphs.palette_prompt.is_empty());
    }
}
