use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::border::BorderGlyphs;
use crate::border::BorderStyle;
use crate::density::Density;
use crate::glyphs::Glyphs;
use crate::keymap::Keymap;
use crate::motion::MotionConfig;
use crate::presets;
use crate::resolver::Resolver;
use crate::theme_load_error::LoaderError;
use crate::tokens::{ColorTokens, ResolvedColorTokens};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub name: String,
    pub colors: ColorTokens,
    #[serde(default)]
    pub glyphs: Glyphs,
    #[serde(default)]
    pub borders: BorderStyle,
    #[serde(default)]
    pub density: Density,
    #[serde(default)]
    pub motion: MotionConfig,
    #[serde(default = "Keymap::vim_arrows")]
    pub keymap: Keymap,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTheme {
    pub name: String,
    pub colors: ResolvedColorTokens,
    pub glyphs: Glyphs,
    pub border_glyphs: BorderGlyphs,
    pub density: Density,
    pub motion: MotionConfig,
    pub keymap: Keymap,
}

impl Theme {
    pub fn default_warm_dark() -> Self {
        load_theme(Some("default-warm-dark"), None).expect("embedded default preset is valid")
    }

    pub fn for_test() -> Self {
        let mut theme = Self::default_warm_dark();
        theme.name = "test-high-contrast".to_string();
        theme.colors.accent = crate::OklchColor::parse("#ff0000").expect("test color literal parses");
        theme.colors.accent_soft = crate::OklchColor::parse("#00ff00").expect("test color literal parses");
        theme.colors.status_ok = crate::OklchColor::parse("#00ff00").expect("test color literal parses");
        theme.colors.status_bad = crate::OklchColor::parse("#ff0000").expect("test color literal parses");
        theme.glyphs.review = "R".to_string();
        theme.glyphs.conflict = "C".to_string();
        theme.glyphs.recall = "H".to_string();
        theme.glyphs.dream = "D".to_string();
        theme.glyphs.due = "!".to_string();
        theme.glyphs.memory = "M".to_string();
        theme
    }

    pub fn from_loader(name: Option<&str>, config_path: Option<&std::path::Path>) -> Result<Self, LoaderError> {
        load_theme(name, config_path)
    }
    pub fn resolve(&self, resolver: &Resolver) -> ResolvedTheme {
        ResolvedTheme {
            name: self.name.clone(),
            colors: self.colors.resolve(resolver),
            glyphs: self.glyphs.clone(),
            border_glyphs: self.borders.glyphs(),
            density: self.density,
            motion: self.motion,
            keymap: self.keymap.clone(),
        }
    }
}

pub(crate) fn load_theme(name: Option<&str>, config_path: Option<&Path>) -> Result<Theme, LoaderError> {
    if let Some(name) = name {
        let body = presets::get(name).ok_or_else(|| LoaderError::UnknownPreset(name.to_string()))?;
        return parse_theme(body);
    }
    if let Some(path) = config_path {
        return parse_theme(&std::fs::read_to_string(path)?);
    }
    let body =
        presets::get("default-warm-dark").ok_or_else(|| LoaderError::UnknownPreset("default-warm-dark".to_string()))?;
    parse_theme(body)
}

pub(crate) fn parse_theme(text: &str) -> Result<Theme, LoaderError> {
    let value = text.parse::<toml::Value>().map_err(|err| LoaderError::ParseFailed(err.to_string()))?;
    let colors = value
        .get("colors")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| LoaderError::MissingToken("colors".to_string()))?;
    for token in ColorTokens::REQUIRED {
        if !colors.contains_key(token) {
            return Err(LoaderError::MissingToken(token.to_string()));
        }
    }
    toml::from_str::<Theme>(text).map_err(|err| LoaderError::ParseFailed(err.to_string()))
}
