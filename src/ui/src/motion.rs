use ratatui::layout::Rect;
use std::time::Duration;
use tachyonfx::Effect;
use vtabs_core::Settings;

/// Frame cadence while anything animates.
pub(crate) const FRAME: Duration = Duration::from_millis(8);

pub(crate) fn enabled(settings: &Settings) -> bool {
    settings.animations && !settings.reduced_motion && settings.animation_ms > 0
}

/// A fraction of the configured animation length, or zero when motion is off.
pub(crate) fn span(settings: &Settings, numerator: u64, denominator: u64) -> Duration {
    if enabled(settings) {
        Duration::from_millis(u64::from(settings.animation_ms) * numerator / denominator)
    } else {
        Duration::ZERO
    }
}

pub(crate) fn progress(elapsed: Duration, span: Duration) -> f32 {
    (elapsed.as_secs_f32() / span.as_secs_f32()).clamp(0.0, 1.0)
}

pub(crate) fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// Eased progress of an animation that started at `from`; a zero span finishes at once.
pub(crate) fn eased(now: Duration, from: Duration, span: Duration) -> f32 {
    if span.is_zero() {
        1.0
    } else {
        ease_out(progress(now.saturating_sub(from), span))
    }
}

pub(crate) fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Tween {
    pub from: f32,
    pub to: f32,
    pub start: Duration,
    pub duration: Duration,
}

/// A finite cell effect over one area, and the surface transform the compositor applies.
#[derive(Default)]
pub(crate) struct Effects {
    pub cell: Option<Effect>,
    pub cell_area: Option<Rect>,
    pub surface: Option<Tween>,
}

impl Effects {
    /// Returns whether anything was running.
    pub fn cancel(&mut self) -> bool {
        let cell = self.cell.take().is_some();
        let surface = self.surface.take().is_some();
        self.cell_area = None;
        cell || surface
    }
}

pub(crate) const CARET_BLINK: Duration = Duration::from_millis(600);
pub(crate) const TOOLTIP_DELAY: Duration = Duration::from_millis(600);

pub(crate) struct Caret {
    pub visible: bool,
    pub deadline: Option<Duration>,
}

impl Default for Caret {
    fn default() -> Self {
        Self {
            visible: true,
            deadline: None,
        }
    }
}

impl Caret {
    /// Shows the caret and restarts its blink; a hidden or unfocused window does not blink.
    pub fn restart(&mut self, now: Duration, live: bool) {
        self.visible = true;
        self.deadline = live.then_some(now + CARET_BLINK);
    }
    pub fn stop(&mut self) {
        self.deadline = None;
    }
    /// Returns whether the caret toggled.
    pub fn tick(&mut self, now: Duration) -> bool {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.visible = !self.visible;
            self.deadline = Some(now + CARET_BLINK);
            return true;
        }
        false
    }
}

#[derive(Default)]
pub(crate) struct Tooltip {
    pub deadline: Option<Duration>,
    pub shown: bool,
}

impl Tooltip {
    pub fn hide(&mut self) {
        self.shown = false;
        self.deadline = None;
    }
    pub fn pending(&self) -> bool {
        self.shown || self.deadline.is_some()
    }
    /// Returns whether the delay ran out; the tip shows only if something is still hovered.
    pub fn tick(&mut self, now: Duration, hovering: bool) -> bool {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.deadline = None;
            self.shown = hovering;
            return true;
        }
        false
    }
}
