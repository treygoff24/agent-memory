use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Density {
    Comfortable,
    Compact,
}

impl Default for Density {
    fn default() -> Self {
        Self::Comfortable
    }
}

impl Density {
    pub const fn pad_top(self) -> u16 {
        match self {
            Self::Comfortable => 1,
            Self::Compact => 0,
        }
    }
    pub const fn pad_bottom(self) -> u16 {
        match self {
            Self::Comfortable => 1,
            Self::Compact => 0,
        }
    }
    pub const fn row_height(self) -> u16 {
        match self {
            Self::Comfortable => 3,
            Self::Compact => 1,
        }
    }
    pub const fn gutter_width(self) -> u16 {
        match self {
            Self::Comfortable => 3,
            Self::Compact => 2,
        }
    }
}
