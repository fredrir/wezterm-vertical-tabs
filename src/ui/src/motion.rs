use std::time::Duration;
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
