use std::path::Path;

use thiserror::Error;

use crate::presets;
use crate::tokens::ColorTokens;
use crate::Theme;

#[derive(Debug, Error)]
pub enum LoaderError {
    #[error("missing theme token: {0}")]
    MissingToken(String),
    #[error("failed to parse theme: {0}")]
    ParseFailed(String),
    #[error("unknown theme preset: {0}")]
    UnknownPreset(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct Loader;

impl Loader {
    pub fn resolve(name: Option<&str>, config_path: Option<&Path>) -> Result<Theme, LoaderError> {
        if let Some(name) = name {
            let body = presets::get(name).ok_or_else(|| LoaderError::UnknownPreset(name.to_string()))?;
            return parse_theme(body);
        }
        if let Some(path) = config_path {
            return parse_theme(&std::fs::read_to_string(path)?);
        }
        let body = presets::get("default-warm-dark")
            .ok_or_else(|| LoaderError::UnknownPreset("default-warm-dark".to_string()))?;
        parse_theme(body)
    }
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
