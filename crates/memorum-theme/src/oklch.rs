use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OklchColor {
    pub l: f32,
    pub c: f32,
    pub h: f32,
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum ParseColorError {
    #[error("invalid OKLCH color: {0}")]
    InvalidOklch(String),
    #[error("invalid hex color: {0}")]
    InvalidHex(String),
}

impl OklchColor {
    pub fn parse(value: &str) -> Result<Self, ParseColorError> {
        let trimmed = value.trim();
        if trimmed.starts_with('#') {
            return Self::parse_hex(trimmed);
        }
        Self::parse_oklch(trimmed)
    }

    pub fn parse_oklch(value: &str) -> Result<Self, ParseColorError> {
        let inner = value
            .trim()
            .strip_prefix("oklch(")
            .and_then(|body| body.strip_suffix(')'))
            .ok_or_else(|| ParseColorError::InvalidOklch(value.to_string()))?;
        let without_alpha = inner.split('/').next().unwrap_or(inner);
        let parts = without_alpha.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 3 {
            return Err(ParseColorError::InvalidOklch(value.to_string()));
        }
        let l = parts[0].parse::<f32>().map_err(|_| ParseColorError::InvalidOklch(value.to_string()))?;
        let c = parts[1].parse::<f32>().map_err(|_| ParseColorError::InvalidOklch(value.to_string()))?;
        let h = parts[2].parse::<f32>().map_err(|_| ParseColorError::InvalidOklch(value.to_string()))?;
        if !(0.0..=1.0).contains(&l) || c < 0.0 || !h.is_finite() {
            return Err(ParseColorError::InvalidOklch(value.to_string()));
        }
        Ok(Self { l, c, h })
    }

    pub fn parse_hex(value: &str) -> Result<Self, ParseColorError> {
        let hex = value.strip_prefix('#').ok_or_else(|| ParseColorError::InvalidHex(value.to_string()))?;
        if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ParseColorError::InvalidHex(value.to_string()));
        }
        let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| ParseColorError::InvalidHex(value.to_string()))?;
        let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| ParseColorError::InvalidHex(value.to_string()))?;
        let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| ParseColorError::InvalidHex(value.to_string()))?;
        Ok(Self::from_srgb(r, g, b))
    }

    pub fn to_srgb(self) -> (u8, u8, u8) {
        let a = self.c * self.h.to_radians().cos();
        let b = self.c * self.h.to_radians().sin();
        let l_ = self.l + 0.396_337_78 * a + 0.215_803_76 * b;
        let m_ = self.l - 0.105_561_346 * a - 0.063_854_17 * b;
        let s_ = self.l - 0.089_484_18 * a - 1.291_485_5 * b;
        let l = l_ * l_ * l_;
        let m = m_ * m_ * m_;
        let s = s_ * s_ * s_;
        let r = 4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s;
        let g = -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s;
        let b = -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s;
        (to_byte(r), to_byte(g), to_byte(b))
    }

    fn from_srgb(r: u8, g: u8, b: u8) -> Self {
        let r = from_byte(r);
        let g = from_byte(g);
        let b = from_byte(b);
        let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
        let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
        let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
        let l_ = l.cbrt();
        let m_ = m.cbrt();
        let s_ = s.cbrt();
        let ok_l = 0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_;
        let a = 1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_;
        let b = 0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_;
        let c = (a * a + b * b).sqrt();
        let mut h = b.atan2(a).to_degrees();
        if h < 0.0 {
            h += 360.0;
        }
        Self { l: ok_l, c, h }
    }
}

fn to_byte(linear: f32) -> u8 {
    let clamped = linear.clamp(0.0, 1.0);
    let gamma = if clamped <= 0.003_130_8 { 12.92 * clamped } else { 1.055 * clamped.powf(1.0 / 2.4) - 0.055 };
    (gamma * 255.0).round().clamp(0.0, 255.0) as u8
}

fn from_byte(value: u8) -> f32 {
    let srgb = f32::from(value) / 255.0;
    if srgb <= 0.040_45 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    }
}

impl Serialize for OklchColor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("oklch({:.4} {:.4} {:.2})", self.l, self.c, self.h))
    }
}

impl<'de> Deserialize<'de> for OklchColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}
