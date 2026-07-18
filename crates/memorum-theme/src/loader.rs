use std::path::Path;

use crate::theme::{load_theme, Theme};
pub use crate::theme_load_error::LoaderError;

pub struct Loader;

impl Loader {
    pub fn resolve(name: Option<&str>, config_path: Option<&Path>) -> Result<Theme, LoaderError> {
        load_theme(name, config_path)
    }
}
