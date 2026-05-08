#![allow(clippy::disallowed_types, clippy::disallowed_methods)]

use crossterm::event::{KeyCode as CrosstermKeyCode, KeyEvent, KeyModifiers as CrosstermKeyModifiers};
use memorum_theme::resolver::AnsiColor;
use memorum_theme::{
    BorderGlyphs, ColorCapability, Glyphs, KeyChord, KeyCode, KeyModifiers, ResolvedColor, ResolvedTheme, Theme,
};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;

#[derive(Clone, Debug)]
pub struct ThemeStyles {
    pub glyphs: Glyphs,
    pub border: border::Set,
    pub base: Style,
    pub muted: Style,
    pub dim: Style,
    pub accent: Style,
    pub accent_soft: Style,
    pub ok: Style,
    pub warn: Style,
    pub bad: Style,
    pub info: Style,
    pub selected: Style,
    pub block: Style,
}

impl ThemeStyles {
    pub fn from_theme(theme: &Theme, capability: ColorCapability) -> Self {
        let resolved = theme.resolve(&memorum_theme::Resolver::with_capability(capability));
        Self::from_resolved(&resolved)
    }

    fn from_resolved(resolved: &ResolvedTheme) -> Self {
        Self {
            glyphs: resolved.glyphs.clone(),
            border: border_set(resolved.border_glyphs),
            base: Style::new().fg(to_ratatui(resolved.colors.fg)).bg(to_ratatui(resolved.colors.bg)),
            muted: Style::new().fg(to_ratatui(resolved.colors.fg_muted)),
            dim: Style::new().fg(to_ratatui(resolved.colors.fg_dim)),
            accent: Style::new().fg(to_ratatui(resolved.colors.accent)).add_modifier(Modifier::BOLD),
            accent_soft: Style::new().fg(to_ratatui(resolved.colors.accent_soft)),
            ok: Style::new().fg(to_ratatui(resolved.colors.status_ok)),
            warn: Style::new().fg(to_ratatui(resolved.colors.status_warn)),
            bad: Style::new().fg(to_ratatui(resolved.colors.status_bad)),
            info: Style::new().fg(to_ratatui(resolved.colors.status_info)),
            selected: Style::new()
                .fg(to_ratatui(resolved.colors.fg))
                .bg(to_ratatui(resolved.colors.surface_2))
                .add_modifier(Modifier::BOLD),
            block: Style::new().fg(to_ratatui(resolved.colors.border)),
        }
    }
}

pub fn to_ratatui(color: ResolvedColor) -> Color {
    match color {
        ResolvedColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
        ResolvedColor::Indexed(index) => Color::Indexed(index),
        ResolvedColor::Named(color) => ansi_to_ratatui(color),
        ResolvedColor::MonochromeWhite => Color::White,
        ResolvedColor::MonochromeBlack => Color::Black,
    }
}

fn ansi_to_ratatui(color: AnsiColor) -> Color {
    match color {
        AnsiColor::Black => Color::Black,
        AnsiColor::Red => Color::Red,
        AnsiColor::Green => Color::Green,
        AnsiColor::Yellow => Color::Yellow,
        AnsiColor::Blue => Color::Blue,
        AnsiColor::Magenta => Color::Magenta,
        AnsiColor::Cyan => Color::Cyan,
        AnsiColor::White => Color::White,
        AnsiColor::BrightBlack => Color::DarkGray,
        AnsiColor::BrightRed => Color::LightRed,
        AnsiColor::BrightGreen => Color::LightGreen,
        AnsiColor::BrightYellow => Color::LightYellow,
        AnsiColor::BrightBlue => Color::LightBlue,
        AnsiColor::BrightMagenta => Color::LightMagenta,
        AnsiColor::BrightCyan => Color::LightCyan,
        AnsiColor::BrightWhite => Color::Gray,
    }
}

pub fn border_set(glyphs: BorderGlyphs) -> border::Set {
    border::Set {
        top_left: glyph_to_static(glyphs.top_left),
        top_right: glyph_to_static(glyphs.top_right),
        bottom_left: glyph_to_static(glyphs.bottom_left),
        bottom_right: glyph_to_static(glyphs.bottom_right),
        vertical_left: glyph_to_static(glyphs.vertical_left),
        vertical_right: glyph_to_static(glyphs.vertical_right),
        horizontal_top: glyph_to_static(glyphs.horizontal_top),
        horizontal_bottom: glyph_to_static(glyphs.horizontal_bottom),
    }
}

fn glyph_to_static(glyph: char) -> &'static str {
    match glyph {
        '-' => "-",
        '|' => "|",
        '+' => "+",
        '─' => "─",
        '│' => "│",
        '┌' => "┌",
        '┐' => "┐",
        '└' => "└",
        '┘' => "┘",
        '╭' => "╭",
        '╮' => "╮",
        '╰' => "╰",
        '╯' => "╯",
        '├' => "├",
        '┤' => "┤",
        '┬' => "┬",
        '┴' => "┴",
        '═' => "═",
        '║' => "║",
        '╔' => "╔",
        '╗' => "╗",
        '╚' => "╚",
        '╝' => "╝",
        '╠' => "╠",
        '╣' => "╣",
        '╦' => "╦",
        '╩' => "╩",
        '━' => "━",
        '┃' => "┃",
        '┏' => "┏",
        '┓' => "┓",
        '┗' => "┗",
        '┛' => "┛",
        '┣' => "┣",
        '┫' => "┫",
        '┳' => "┳",
        '┻' => "┻",
        '╌' => "╌",
        '╎' => "╎",
        '╍' => "╍",
        '╏' => "╏",
        _ => "?",
    }
}

pub fn key_chord_from_crossterm(event: KeyEvent) -> Option<KeyChord> {
    let key = match event.code {
        CrosstermKeyCode::Char(ch) => KeyCode::Char(ch),
        CrosstermKeyCode::Enter => KeyCode::Enter,
        CrosstermKeyCode::Esc => KeyCode::Esc,
        CrosstermKeyCode::Up => KeyCode::Up,
        CrosstermKeyCode::Down => KeyCode::Down,
        CrosstermKeyCode::Left => KeyCode::Left,
        CrosstermKeyCode::Right => KeyCode::Right,
        CrosstermKeyCode::Tab => KeyCode::Tab,
        CrosstermKeyCode::BackTab => KeyCode::BackTab,
        _ => return None,
    };
    Some(KeyChord {
        key,
        mods: KeyModifiers {
            ctrl: event.modifiers.contains(CrosstermKeyModifiers::CONTROL),
            alt: event.modifiers.contains(CrosstermKeyModifiers::ALT),
            shift: event.modifiers.contains(CrosstermKeyModifiers::SHIFT),
        },
    })
}
