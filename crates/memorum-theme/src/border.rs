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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BorderGlyphs {
    pub top: char,
    pub bottom: char,
    pub left: char,
    pub right: char,
    pub top_left: char,
    pub top_right: char,
    pub bottom_left: char,
    pub bottom_right: char,
    pub vertical_left: char,
    pub vertical_right: char,
    pub horizontal_top: char,
    pub horizontal_bottom: char,
    pub cross: char,
}

impl BorderStyle {
    pub fn glyphs(self) -> BorderGlyphs {
        match self {
            Self::Plain => BorderGlyphs::ascii(),
            Self::Rounded => BorderGlyphs {
                top: '─',
                bottom: '─',
                left: '│',
                right: '│',
                top_left: '╭',
                top_right: '╮',
                bottom_left: '╰',
                bottom_right: '╯',
                vertical_left: '├',
                vertical_right: '┤',
                horizontal_top: '┬',
                horizontal_bottom: '┴',
                cross: '┼',
            },
            Self::Double => BorderGlyphs {
                top: '═',
                bottom: '═',
                left: '║',
                right: '║',
                top_left: '╔',
                top_right: '╗',
                bottom_left: '╚',
                bottom_right: '╝',
                vertical_left: '╠',
                vertical_right: '╣',
                horizontal_top: '╦',
                horizontal_bottom: '╩',
                cross: '╬',
            },
            Self::Thick => BorderGlyphs {
                top: '━',
                bottom: '━',
                left: '┃',
                right: '┃',
                top_left: '┏',
                top_right: '┓',
                bottom_left: '┗',
                bottom_right: '┛',
                vertical_left: '┣',
                vertical_right: '┫',
                horizontal_top: '┳',
                horizontal_bottom: '┻',
                cross: '╋',
            },
            Self::Dashed => BorderGlyphs {
                top: '╌',
                bottom: '╌',
                left: '╎',
                right: '╎',
                top_left: '┌',
                top_right: '┐',
                bottom_left: '└',
                bottom_right: '┘',
                vertical_left: '├',
                vertical_right: '┤',
                horizontal_top: '┬',
                horizontal_bottom: '┴',
                cross: '┼',
            },
            Self::DoubleDashed => BorderGlyphs {
                top: '╍',
                bottom: '╍',
                left: '╏',
                right: '╏',
                top_left: '┌',
                top_right: '┐',
                bottom_left: '└',
                bottom_right: '┘',
                vertical_left: '├',
                vertical_right: '┤',
                horizontal_top: '┬',
                horizontal_bottom: '┴',
                cross: '┼',
            },
        }
    }
}

impl BorderGlyphs {
    pub const fn ascii() -> Self {
        Self {
            top: '-',
            bottom: '-',
            left: '|',
            right: '|',
            top_left: '+',
            top_right: '+',
            bottom_left: '+',
            bottom_right: '+',
            vertical_left: '+',
            vertical_right: '+',
            horizontal_top: '+',
            horizontal_bottom: '+',
            cross: '+',
        }
    }
}
