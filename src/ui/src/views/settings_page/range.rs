use serde_json::{Value, json};
use vtabs_core::{Model, SettingKind, settings};

#[derive(Clone, Copy)]
pub(crate) struct Range {
    min: f64,
    max: f64,
    step: f64,
    percent: bool,
}

impl Range {
    pub(super) fn of(kind: &SettingKind) -> Option<Self> {
        let percent = |min| Self {
            min,
            max: 1.,
            step: 0.01,
            percent: true,
        };
        match *kind {
            SettingKind::Number { min, max } => Some(Self {
                min: min.into(),
                max: max.into(),
                step: if max - min > 100 { 10. } else { 1. },
                percent: false,
            }),
            SettingKind::Fraction => Some(percent(0.01)),
            SettingKind::Opacity => Some(percent(0.)),
            SettingKind::Scale => Some(percent(0.5)),
            _ => None,
        }
    }
    pub(crate) fn for_key(key: &str) -> Option<Self> {
        settings::descriptor(key).and_then(|field| Self::of(&field.kind))
    }
    fn bound(self, value: f64) -> f64 {
        (value.max(self.min).min(self.max) * 100.).round() / 100.
    }
    /// Step multiples, except that both ends stay reachable.
    fn snap(self, value: f64) -> f64 {
        if value >= self.max - self.step / 2. {
            self.max
        } else if value <= self.min + self.step / 2. {
            self.min
        } else {
            self.bound((value / self.step).round() * self.step)
        }
    }
    pub(super) fn step(self, value: f64, steps: i32) -> f64 {
        self.snap(value + self.step * f64::from(steps))
    }
    pub(crate) fn at(self, fraction: f32) -> f64 {
        self.snap(self.min + f64::from(fraction.clamp(0., 1.)) * (self.max - self.min))
    }
    pub(super) fn fraction(self, value: f64) -> f64 {
        ((value - self.min) / (self.max - self.min)).clamp(0., 1.)
    }
    pub(super) fn label(self, value: f64) -> String {
        if self.percent {
            format!("{}%", (value * 100.).round())
        } else {
            format!("{}", value.round())
        }
    }
    pub(super) fn entry_text(self, value: f64) -> String {
        if self.percent {
            format!("{}", (value * 100.).round())
        } else {
            format!("{}", value.round())
        }
    }
    /// Typed digits in display units; the upper bound applies while typing.
    pub(super) fn sanitize(self, text: &str) -> String {
        let digits: String = text.chars().filter(char::is_ascii_digit).take(5).collect();
        let limit = if self.percent { 100. } else { self.max };
        match digits.parse::<f64>() {
            Ok(typed) if typed > limit => format!("{limit}"),
            _ => digits,
        }
    }
    pub(super) fn parse(self, text: &str) -> Option<f64> {
        let typed = text.parse::<f64>().ok()?;
        Some(self.bound(if self.percent { typed / 100. } else { typed }))
    }
    pub(super) fn json(self, value: f64) -> Value {
        if self.percent {
            json!(value)
        } else {
            json!(value.round() as u64)
        }
    }
}

pub(super) fn number(model: &Model, key: &str) -> f64 {
    model
        .settings
        .get(key)
        .and_then(|value| value.as_f64())
        .unwrap_or_default()
}
