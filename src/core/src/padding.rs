//! WezTerm's `window_padding`, for the quick terminal.
use crate::settings::{SettingDescriptor, SettingKind};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Pixels, or a string with a `px`, `%`, `pt` or `cell` unit, read like WezTerm's `window_padding`.
#[derive(Clone, Debug, PartialEq, JsonSchema)]
#[serde(untagged)]
pub enum Dimension {
    Number(f64),
    Text(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Pixels,
    /// Of the quick terminal's width or height.
    Percent,
    Points,
    Cells,
}

const INVALID: &str = "padding takes pixels or a px, %, pt or cell string";

impl Dimension {
    /// The amount and unit, `50` for `50%`; `None` where WezTerm rejects it or it is negative.
    pub fn parse(&self) -> Option<(f32, Unit)> {
        let (amount, unit) = match self {
            Self::Number(amount) => (*amount as f32, Unit::Pixels),
            Self::Text(text) => match text.parse::<f32>() {
                Ok(amount) => (amount, Unit::Pixels),
                Err(_) => [
                    ("px", Unit::Pixels),
                    ("%", Unit::Percent),
                    ("pt", Unit::Points),
                    ("cell", Unit::Cells),
                ]
                .into_iter()
                .find_map(|(suffix, unit)| {
                    Some((text.strip_suffix(suffix)?.trim().parse().ok()?, unit))
                })?,
            },
        };
        (amount.is_finite() && amount >= 0.).then_some((amount, unit))
    }

    pub fn from_text(text: &str) -> Result<Self, String> {
        let dimension = match text.parse::<f64>() {
            Ok(amount) => Self::Number(amount),
            Err(_) => Self::Text(text.into()),
        };
        dimension
            .parse()
            .map(|_| dimension)
            .ok_or_else(|| INVALID.into())
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(amount) => write!(f, "{amount}"),
            Self::Text(text) => f.write_str(text),
        }
    }
}

impl Serialize for Dimension {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Number(amount) if amount.fract() == 0. && amount.abs() < 1e15 => {
                serializer.serialize_i64(*amount as i64)
            }
            Self::Number(amount) => serializer.serialize_f64(*amount),
            Self::Text(text) => serializer.serialize_str(text),
        }
    }
}

impl<'de> Deserialize<'de> for Dimension {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Number(f64),
            Text(String),
        }
        let dimension = match Raw::deserialize(deserializer)? {
            Raw::Number(amount) => Self::Number(amount),
            Raw::Text(text) => Self::Text(text),
        };
        match dimension.parse() {
            Some(_) => Ok(dimension),
            None => Err(serde::de::Error::custom(INVALID)),
        }
    }
}

/// Sides left out follow WezTerm's `window_padding`; a quick program's follow the quick terminal's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WindowPadding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<Dimension>,
}

impl WindowPadding {
    pub fn side(&self, side: &str) -> Option<&Dimension> {
        match side {
            "left" => self.left.as_ref(),
            "top" => self.top.as_ref(),
            "right" => self.right.as_ref(),
            "bottom" => self.bottom.as_ref(),
            _ => None,
        }
    }

    pub fn side_mut(&mut self, side: &str) -> Option<&mut Option<Dimension>> {
        match side {
            "left" => Some(&mut self.left),
            "top" => Some(&mut self.top),
            "right" => Some(&mut self.right),
            "bottom" => Some(&mut self.bottom),
            _ => None,
        }
    }
}

const ROW: &str = "Pixels, or px, %, pt or cell; empty follows WezTerm window_padding";

/// Padding rows, each editing one side of `quick_terminal_window_padding`.
pub const DESCRIPTORS: &[SettingDescriptor] = &[
    SettingDescriptor {
        key: "quick_terminal_window_padding.left",
        label: "Padding left",
        group: "quick_terminal",
        kind: SettingKind::Dimension,
        description: ROW,
    },
    SettingDescriptor {
        key: "quick_terminal_window_padding.top",
        label: "Padding top",
        group: "quick_terminal",
        kind: SettingKind::Dimension,
        description: ROW,
    },
    SettingDescriptor {
        key: "quick_terminal_window_padding.right",
        label: "Padding right",
        group: "quick_terminal",
        kind: SettingKind::Dimension,
        description: ROW,
    },
    SettingDescriptor {
        key: "quick_terminal_window_padding.bottom",
        label: "Padding bottom",
        group: "quick_terminal",
        kind: SettingKind::Dimension,
        description: ROW,
    },
];

/// The side a padding row edits.
pub fn side(key: &str) -> Option<&str> {
    key.strip_prefix("quick_terminal_window_padding.")
}
