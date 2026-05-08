use serde::{Deserialize, Serialize};

use crate::oklch::OklchColor;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ColorCapability {
    TrueColor,
    Indexed256,
    Indexed16,
    Monochrome,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ResolvedColor {
    Rgb(u8, u8, u8),
    Indexed(u8),
    Named(AnsiColor),
    MonochromeWhite,
    MonochromeBlack,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AnsiColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Resolver {
    capability: ColorCapability,
}

impl Resolver {
    pub fn detect() -> Self {
        if let Some(capability) = Self::override_from_env() {
            return Self { capability };
        }
        let colorterm = std::env::var("COLORTERM").unwrap_or_default().to_ascii_lowercase();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            return Self { capability: ColorCapability::TrueColor };
        }
        let term = std::env::var("TERM").unwrap_or_default().to_ascii_lowercase();
        if term.is_empty() || term == "dumb" {
            return Self { capability: ColorCapability::Monochrome };
        }
        if term.contains("256color") {
            return Self { capability: ColorCapability::Indexed256 };
        }
        Self { capability: ColorCapability::Indexed16 }
    }

    pub fn with_capability(capability: ColorCapability) -> Self {
        Self { capability }
    }
    pub fn capability(self) -> ColorCapability {
        self.capability
    }

    pub fn override_from_env() -> Option<ColorCapability> {
        match std::env::var("MEMORUM_FORCE_COLOR").ok()?.to_ascii_lowercase().as_str() {
            "truecolor" | "24bit" => Some(ColorCapability::TrueColor),
            "256" | "256color" => Some(ColorCapability::Indexed256),
            "16" | "ansi" => Some(ColorCapability::Indexed16),
            "mono" | "monochrome" | "none" => Some(ColorCapability::Monochrome),
            _ => None,
        }
    }

    pub fn resolve_oklch(&self, color: &OklchColor) -> ResolvedColor {
        let (r, g, b) = color.to_srgb();
        match self.capability {
            ColorCapability::TrueColor => ResolvedColor::Rgb(r, g, b),
            ColorCapability::Indexed256 => ResolvedColor::Indexed(nearest_xterm_256(r, g, b)),
            ColorCapability::Indexed16 => ResolvedColor::Named(nearest_ansi(r, g, b)),
            ColorCapability::Monochrome => {
                if luminance(r, g, b) >= 128 {
                    ResolvedColor::MonochromeWhite
                } else {
                    ResolvedColor::MonochromeBlack
                }
            }
        }
    }
}

fn nearest_xterm_256(r: u8, g: u8, b: u8) -> u8 {
    let mut best = 0u8;
    let mut best_distance = u32::MAX;
    for index in 16u8..=231 {
        let distance = distance_squared((r, g, b), xterm_color(index));
        if distance < best_distance {
            best = index;
            best_distance = distance;
        }
    }
    for index in 232u8..=255 {
        let distance = distance_squared((r, g, b), xterm_color(index));
        if distance < best_distance {
            best = index;
            best_distance = distance;
        }
    }
    best
}

fn xterm_color(index: u8) -> (u8, u8, u8) {
    if (16..=231).contains(&index) {
        let n = index - 16;
        let value = |component: u8| if component == 0 { 0 } else { 55 + component * 40 };
        return (value(n / 36), value((n / 6) % 6), value(n % 6));
    }
    let gray = 8 + (index - 232) * 10;
    (gray, gray, gray)
}

fn nearest_ansi(r: u8, g: u8, b: u8) -> AnsiColor {
    const COLORS: [(AnsiColor, u8, u8, u8); 16] = [
        (AnsiColor::Black, 0, 0, 0),
        (AnsiColor::Red, 128, 0, 0),
        (AnsiColor::Green, 0, 128, 0),
        (AnsiColor::Yellow, 128, 128, 0),
        (AnsiColor::Blue, 0, 0, 128),
        (AnsiColor::Magenta, 128, 0, 128),
        (AnsiColor::Cyan, 0, 128, 128),
        (AnsiColor::White, 192, 192, 192),
        (AnsiColor::BrightBlack, 128, 128, 128),
        (AnsiColor::BrightRed, 255, 0, 0),
        (AnsiColor::BrightGreen, 0, 255, 0),
        (AnsiColor::BrightYellow, 255, 255, 0),
        (AnsiColor::BrightBlue, 0, 0, 255),
        (AnsiColor::BrightMagenta, 255, 0, 255),
        (AnsiColor::BrightCyan, 0, 255, 255),
        (AnsiColor::BrightWhite, 255, 255, 255),
    ];
    COLORS
        .iter()
        .min_by_key(|(_, cr, cg, cb)| distance_squared((r, g, b), (*cr, *cg, *cb)))
        .map(|(color, _, _, _)| *color)
        .unwrap_or(AnsiColor::White)
}

fn distance_squared((r, g, b): (u8, u8, u8), (cr, cg, cb): (u8, u8, u8)) -> u32 {
    let dr = i32::from(r) - i32::from(cr);
    let dg = i32::from(g) - i32::from(cg);
    let db = i32::from(b) - i32::from(cb);
    (dr * dr + dg * dg + db * db) as u32
}
fn luminance(r: u8, g: u8, b: u8) -> u8 {
    ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000) as u8
}
