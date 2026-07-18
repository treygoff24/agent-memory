//! Presentation theme primitives for Memorum terminal and web surfaces.
//!
//! The crate resolves an authored [`Theme`] in three stages: load an embedded
//! preset or user TOML, detect terminal color/charset capability, then lower
//! OKLCH tokens into [`ResolvedColor`] values the consumer can map to its UI
//! backend. It intentionally has no `ratatui` dependency.

pub mod border;
pub mod charset;
pub mod density;
pub mod glyphs;
pub mod hot_reload;
pub mod keymap;
pub mod loader;
pub mod motion;
pub mod oklch;
pub mod presets;
pub mod resolver;
pub mod theme;
mod theme_load_error;
pub mod tokens;

pub use border::{BorderGlyphs, BorderStyle};
pub use charset::Charset;
pub use density::Density;
pub use glyphs::Glyphs;
pub use hot_reload::HotReload;
pub use keymap::{Action, KeyChord, KeyCode, KeyModifiers, Keymap};
pub use loader::{Loader, LoaderError};
pub use motion::MotionConfig;
pub use oklch::{OklchColor, ParseColorError};
pub use resolver::{ColorCapability, ResolvedColor, Resolver};
pub use theme::{ResolvedTheme, Theme};
pub use tokens::{ColorTokens, ResolvedColorTokens};
