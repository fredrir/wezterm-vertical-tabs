//! Quick terminal settings and Lua quick programs as host terminal overlays.
use super::*;
use crate::termwindow::ui_host::{TerminalBorder, TerminalPadding, TerminalProgram};
use config::keyassignment::{SpawnCommand, SpawnTabDomain};

pub(super) fn quick_terminal(settings: &core::Settings) -> QuickTerminalConfig {
    let color = |value: Option<&str>| value.and_then(ui::Theme::parse_color).map(linear_color);
    QuickTerminalConfig {
        size: (
            settings.quick_terminal_width as f32,
            settings.quick_terminal_height as f32,
        ),
        padding: padding(&settings.quick_terminal_window_padding),
        opacity: settings.quick_terminal_opacity as f32,
        blur: settings.quick_terminal_blur as f32,
        border: settings
            .quick_terminal_border_color
            .as_deref()
            .and_then(|text| border(text).ok())
            .unwrap_or(TerminalBorder::Palette),
        background_color: color(settings.quick_terminal_background_color.as_deref()),
        auto_hide: settings.quick_terminal_auto_hide,
        hide_on_escape: hide_on_escape(settings.quick_terminal_hide_on_escape),
        preserve_shell: settings.quick_terminal_preserve_shell,
        transition: ui::transition(settings),
    }
}

/// `colors` and `border_color` go through WezTerm's own parsers, as in its config.
pub(super) fn quick_program(program: core::QuickProgram) -> Result<TerminalProgram, String> {
    let border = program.border_color.as_deref().map(border).transpose()?;
    let colors = program.colors.map(palette).transpose()?;
    Ok(TerminalProgram {
        spawn: SpawnCommand {
            args: (!program.args.is_empty()).then_some(program.args),
            cwd: program.cwd.map(Into::into),
            set_environment_variables: program.set_environment_variables.into_iter().collect(),
            domain: match program.domain {
                None | Some(core::SpawnDomain::CurrentPaneDomain) => {
                    SpawnTabDomain::CurrentPaneDomain
                }
                Some(core::SpawnDomain::DefaultDomain) => SpawnTabDomain::DefaultDomain,
                Some(core::SpawnDomain::DomainName(name)) => SpawnTabDomain::DomainName(name),
                Some(core::SpawnDomain::DomainId(id)) => SpawnTabDomain::DomainId(id),
            },
            ..Default::default()
        },
        width: program.width,
        height: program.height,
        padding: padding(&program.window_padding),
        opacity: program.window_background_opacity,
        blur: program.blur.map(f32::from),
        border,
        colors,
        auto_hide: program.auto_hide,
        hide_on_escape: program.hide_on_escape.map(hide_on_escape),
        preserve: program.preserve,
    })
}

fn border(text: &str) -> Result<TerminalBorder, String> {
    if text == "none" {
        return Ok(TerminalBorder::None);
    }
    text.parse::<window::color::SrgbaTuple>()
        .map(|color| TerminalBorder::Color(color.to_linear()))
        .map_err(|_| format!("invalid border_color {text}"))
}

fn padding(padding: &core::WindowPadding) -> TerminalPadding {
    use vtabs_app::core::padding::Unit;
    let side = |side: &Option<core::Dimension>| {
        let (amount, unit) = side.as_ref()?.parse()?;
        Some(match unit {
            Unit::Pixels => config::Dimension::Pixels(amount),
            Unit::Percent => config::Dimension::Percent(amount / 100.),
            Unit::Points => config::Dimension::Points(amount),
            Unit::Cells => config::Dimension::Cells(amount),
        })
    };
    TerminalPadding {
        left: side(&padding.left),
        top: side(&padding.top),
        right: side(&padding.right),
        bottom: side(&padding.bottom),
    }
}

fn hide_on_escape(mode: core::HideOnEscape) -> HideOnEscape {
    match mode {
        core::HideOnEscape::Never => HideOnEscape::Never,
        core::HideOnEscape::Always => HideOnEscape::Always,
        core::HideOnEscape::Auto => HideOnEscape::Auto,
    }
}

fn palette(colors: serde_json::Map<String, serde_json::Value>) -> Result<config::Palette, String> {
    use wezterm_dynamic::{FromDynamic, FromDynamicOptions, UnknownFieldAction};
    let options = FromDynamicOptions {
        unknown_fields: UnknownFieldAction::Deny,
        deprecated_fields: UnknownFieldAction::Warn,
    };
    config::Palette::from_dynamic(&dynamic(&serde_json::Value::Object(colors)), options)
        .map_err(|error| format!("invalid colors: {error}"))
}

/// Lua's integer keys, as in `indexed`, arrive as JSON strings.
fn dynamic(value: &serde_json::Value) -> wezterm_dynamic::Value {
    use serde_json::Value;
    use wezterm_dynamic::{ToDynamic, Value as Dynamic};
    match value {
        Value::Null => Dynamic::Null,
        Value::Bool(value) => Dynamic::Bool(*value),
        Value::Number(number) => match (number.as_u64(), number.as_i64()) {
            (Some(value), _) => value.to_dynamic(),
            (_, Some(value)) => value.to_dynamic(),
            _ => number.as_f64().unwrap_or_default().to_dynamic(),
        },
        Value::String(value) => Dynamic::String(value.clone()),
        Value::Array(items) => Dynamic::Array(items.iter().map(dynamic).collect::<Vec<_>>().into()),
        Value::Object(fields) => Dynamic::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    let key = key
                        .parse::<u64>()
                        .map_or_else(|_| Dynamic::String(key.clone()), Dynamic::U64);
                    (key, dynamic(value))
                })
                .collect::<std::collections::BTreeMap<_, _>>()
                .into(),
        ),
    }
}

#[cfg(test)]
#[path = "../tests/terminal_program.rs"]
mod tests;
