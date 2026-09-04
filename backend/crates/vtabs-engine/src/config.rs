//! Normalized engine configuration. Strings and optional wire sections stop at this boundary.

use std::collections::BTreeMap;

use vtabs_protocol::payload::{ConfigMsg, ContextSpec, PaddingSpec, PopoverWidth};

pub use vtabs_protocol::payload::PaddingSpec as Padding;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    #[default]
    Left,
    Right,
}

impl Position {
    pub fn is_right(self) -> bool {
        self == Self::Right
    }

    pub fn as_str(self) -> &'static str {
        if self.is_right() { "right" } else { "left" }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WheelMode {
    #[default]
    Scroll,
    Switch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContextMode {
    #[default]
    Popover,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HoverMode {
    #[default]
    Follow,
    Press,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabHeight {
    Row,
    #[default]
    Card,
    Tall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Separator {
    Rule,
    #[default]
    Gap,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PinnedStyle {
    Dense,
    #[default]
    Compact,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloseButton {
    #[default]
    Hover,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollIndicator {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MetaMode {
    #[default]
    Off,
    Auto,
    Cwd,
    Process,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PopoverOverflow {
    #[default]
    Clip,
    Grow,
}

#[derive(Debug, Clone)]
pub struct RenderConfig {
    pub padding: Padding,
    pub frame: bool,
    pub position: Position,
    pub new_tab_button: bool,
    pub new_tab_label: String,
    pub row_gap: i64,
    pub separator: Separator,
    pub tab_height: TabHeight,
    pub meta: bool,
    pub meta_sep: Option<String>,
    pub show_index: bool,
    pub icons: bool,
    pub close_button: CloseButton,
    pub hover: HoverMode,
    pub pinned_style: PinnedStyle,
    pub scroll_indicator: ScrollIndicator,
}

#[derive(Debug, Clone)]
pub struct PopoverConfig {
    pub width: Option<i64>,
    pub follow_pointer: bool,
    pub overflow: PopoverOverflow,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub rail_width: u32,
    pub icon_map: BTreeMap<String, String>,
    pub glyph_custom_block: bool,
    pub glyph_east_asian_wide: bool,
    pub double_click_ms: u64,
    pub tear_off: bool,
    pub wheel: WheelMode,
    pub context: ContextMode,
    pub hover_timeout_ms: u64,
    pub hover_highlight: bool,
    pub ellipsis: String,
    pub popover: PopoverConfig,
    pub render: RenderConfig,
    pub meta_mode: MetaMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub &'static str);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ConfigError {}

fn named<T>(value: Option<&str>, choices: &[(&str, T)]) -> Result<T, ConfigError>
where
    T: Copy + Default,
{
    let Some(value) = value else {
        return Ok(T::default());
    };
    choices
        .iter()
        .find_map(|(name, parsed)| (*name == value).then_some(*parsed))
        .ok_or(ConfigError("unknown configuration mode"))
}

impl TryFrom<ConfigMsg> for EngineConfig {
    type Error = ConfigError;

    fn try_from(msg: ConfigMsg) -> Result<Self, Self::Error> {
        let render = msg
            .render
            .unwrap_or_else(|| vtabs_protocol::payload::RenderSection {
                meta: true,
                padding: PaddingSpec::default(),
                frame: false,
                tab_height: None,
                row_gap: 0,
                separator: None,
                pinned_style: None,
                close_button: None,
                show_index: false,
                scroll_indicator: None,
                new_tab_button: false,
                new_tab_label: None,
                hover: None,
            });
        let popover = msg.popover.unwrap_or_default();
        if msg.rail_width != 0 && msg.rail_width < 3 {
            return Err(ConfigError("rail_width must be at least 3"));
        }
        if render.row_gap < 0
            || [
                render.padding.left,
                render.padding.right,
                render.padding.top,
                render.padding.bottom,
            ]
            .iter()
            .any(|value| *value < 0)
        {
            return Err(ConfigError("padding and row_gap must be non-negative"));
        }
        let width = match popover.width.as_ref() {
            Some(PopoverWidth::Fixed(width)) if *width >= 1 => Some(*width),
            Some(PopoverWidth::Fixed(_)) => {
                return Err(ConfigError("popover width must be positive"));
            }
            Some(PopoverWidth::Auto(name)) if name == "auto" => None,
            Some(PopoverWidth::Auto(_)) => return Err(ConfigError("unknown popover width")),
            None => None,
        };
        let context = match msg.context.as_ref() {
            Some(ContextSpec::Name(name)) if name == "popover" => ContextMode::Popover,
            Some(ContextSpec::Enabled(false)) => ContextMode::Disabled,
            Some(ContextSpec::Name(_)) | Some(ContextSpec::Enabled(true)) => {
                return Err(ConfigError("unknown context mode"));
            }
            None => ContextMode::default(),
        };
        let position = named(
            msg.position.as_deref(),
            &[("left", Position::Left), ("right", Position::Right)],
        )?;
        let meta_mode = named(
            msg.meta.as_deref(),
            &[
                ("auto", MetaMode::Auto),
                ("cwd", MetaMode::Cwd),
                ("process", MetaMode::Process),
            ],
        )?;
        Ok(Self {
            rail_width: msg.rail_width,
            icon_map: msg.icon_map,
            glyph_custom_block: msg.glyphs.custom_block,
            glyph_east_asian_wide: msg.glyphs.east_asian_wide,
            double_click_ms: msg.double_click_ms,
            tear_off: msg.tear_off,
            wheel: named(
                msg.wheel.as_deref(),
                &[("scroll", WheelMode::Scroll), ("switch", WheelMode::Switch)],
            )?,
            context,
            hover_timeout_ms: msg.hover_timeout_ms,
            hover_highlight: msg.hover_highlight,
            ellipsis: msg
                .ellipsis
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "…".into()),
            popover: PopoverConfig {
                width,
                follow_pointer: popover.follow_pointer,
                overflow: named(
                    popover.overflow.as_deref(),
                    &[
                        ("clip", PopoverOverflow::Clip),
                        ("grow", PopoverOverflow::Grow),
                    ],
                )?,
            },
            render: RenderConfig {
                padding: render.padding,
                frame: render.frame,
                position,
                new_tab_button: render.new_tab_button,
                new_tab_label: render
                    .new_tab_label
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "New tab".into()),
                row_gap: render.row_gap,
                separator: named(
                    render.separator.as_deref(),
                    &[
                        ("gap", Separator::Gap),
                        ("rule", Separator::Rule),
                        ("none", Separator::None),
                    ],
                )?,
                tab_height: named(
                    render.tab_height.as_deref(),
                    &[
                        ("card", TabHeight::Card),
                        ("row", TabHeight::Row),
                        ("tall", TabHeight::Tall),
                    ],
                )?,
                meta: render.meta,
                meta_sep: msg.meta_sep,
                show_index: render.show_index,
                icons: msg.icons,
                close_button: named(
                    render.close_button.as_deref(),
                    &[
                        ("hover", CloseButton::Hover),
                        ("always", CloseButton::Always),
                        ("never", CloseButton::Never),
                    ],
                )?,
                hover: named(
                    render.hover.as_deref(),
                    &[("follow", HoverMode::Follow), ("press", HoverMode::Press)],
                )?,
                pinned_style: named(
                    render.pinned_style.as_deref(),
                    &[
                        ("compact", PinnedStyle::Compact),
                        ("dense", PinnedStyle::Dense),
                        ("full", PinnedStyle::Full),
                    ],
                )?,
                scroll_indicator: named(
                    render.scroll_indicator.as_deref(),
                    &[
                        ("auto", ScrollIndicator::Auto),
                        ("always", ScrollIndicator::Always),
                        ("never", ScrollIndicator::Never),
                    ],
                )?,
            },
            meta_mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(json: &str) -> Result<EngineConfig, ConfigError> {
        EngineConfig::try_from(serde_json::from_str::<ConfigMsg>(json).unwrap())
    }

    #[test]
    fn false_disables_the_lua_context_action() {
        let cfg = config(r#"{"context":false}"#).unwrap();
        assert_eq!(cfg.context, ContextMode::Disabled);
    }

    #[test]
    fn wire_modes_and_values_are_validated_once() {
        assert!(config(r#"{"wheel":"teleport"}"#).is_err());
        assert!(config(r#"{"popover":{"width":0}}"#).is_err());
        assert!(
            config(r#"{"render":{"padding":{"left":-1,"right":0,"top":0,"bottom":0}}}"#).is_err()
        );
    }
}
