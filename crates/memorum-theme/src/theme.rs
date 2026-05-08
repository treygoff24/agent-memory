use serde::{Deserialize, Serialize};

use crate::{
    BorderGlyphs, BorderStyle, ColorTokens, Density, Glyphs, Keymap, Loader, LoaderError, MotionConfig,
    ResolvedColorTokens, Resolver,
};

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
        Loader::resolve(Some("default-warm-dark"), None).expect("embedded default preset is valid")
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
        Loader::resolve(name, config_path)
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
