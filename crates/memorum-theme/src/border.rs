use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BorderStyle {
    Plain,
    Rounded,
    Double,
    Thick,
    Dashed,
    DoubleDashed,
}

impl Default for BorderStyle {
    fn default() -> Self {
        Self::Rounded
    }
}

/// Box-drawing glyphs for a border style.
///
/// Fields are `&'static str` rather than `char` so they drop straight into
/// ratatui's `border::Set` (whose fields are `&'static str`) without a
/// per-glyph lookup table. Every glyph is a compile-time literal, so this is a
/// free representation choice.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BorderGlyphs {
    pub top: &'static str,
    pub bottom: &'static str,
    pub left: &'static str,
    pub right: &'static str,
    pub top_left: &'static str,
    pub top_right: &'static str,
    pub bottom_left: &'static str,
    pub bottom_right: &'static str,
    pub vertical_left: &'static str,
    pub vertical_right: &'static str,
    pub horizontal_top: &'static str,
    pub horizontal_bottom: &'static str,
    pub cross: &'static str,
}

impl BorderStyle {
    pub fn glyphs(self) -> BorderGlyphs {
        match self {
            Self::Plain => BorderGlyphs::ascii(),
            Self::Rounded => BorderGlyphs {
                top: "─",
                bottom: "─",
                left: "│",
                right: "│",
                top_left: "╭",
                top_right: "╮",
                bottom_left: "╰",
                bottom_right: "╯",
                vertical_left: "├",
                vertical_right: "┤",
                horizontal_top: "┬",
                horizontal_bottom: "┴",
                cross: "┼",
            },
            Self::Double => BorderGlyphs {
                top: "═",
                bottom: "═",
                left: "║",
                right: "║",
                top_left: "╔",
                top_right: "╗",
                bottom_left: "╚",
                bottom_right: "╝",
                vertical_left: "╠",
                vertical_right: "╣",
                horizontal_top: "╦",
                horizontal_bottom: "╩",
                cross: "╬",
            },
            Self::Thick => BorderGlyphs {
                top: "━",
                bottom: "━",
                left: "┃",
                right: "┃",
                top_left: "┏",
                top_right: "┓",
                bottom_left: "┗",
                bottom_right: "┛",
                vertical_left: "┣",
                vertical_right: "┫",
                horizontal_top: "┳",
                horizontal_bottom: "┻",
                cross: "╋",
            },
            Self::Dashed => BorderGlyphs {
                top: "╌",
                bottom: "╌",
                left: "╎",
                right: "╎",
                top_left: "┌",
                top_right: "┐",
                bottom_left: "└",
                bottom_right: "┘",
                vertical_left: "├",
                vertical_right: "┤",
                horizontal_top: "┬",
                horizontal_bottom: "┴",
                cross: "┼",
            },
            Self::DoubleDashed => BorderGlyphs {
                top: "╍",
                bottom: "╍",
                left: "╏",
                right: "╏",
                top_left: "┌",
                top_right: "┐",
                bottom_left: "└",
                bottom_right: "┘",
                vertical_left: "├",
                vertical_right: "┤",
                horizontal_top: "┬",
                horizontal_bottom: "┴",
                cross: "┼",
            },
        }
    }
}

impl BorderGlyphs {
    pub const fn ascii() -> Self {
        Self {
            top: "-",
            bottom: "-",
            left: "|",
            right: "|",
            top_left: "+",
            top_right: "+",
            bottom_left: "+",
            bottom_right: "+",
            vertical_left: "+",
            vertical_right: "+",
            horizontal_top: "+",
            horizontal_bottom: "+",
            cross: "+",
        }
    }
}
