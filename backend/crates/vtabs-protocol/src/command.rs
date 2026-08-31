use serde::Deserialize;

/// One row of an `anim`, addressed the way the frame already addresses it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AnimRow {
    pub y: u16,
    #[serde(default)]
    pub delay: u64,
}

/// `data` is one final frame; the backend generates every intermediate one.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AnimCmd {
    pub id: u64,
    pub ms: u64,
    #[serde(default)]
    pub fps: Option<u32>,
    #[serde(default)]
    pub ease: Option<String>,
    #[serde(default)]
    pub dir: Option<String>,
    pub anchor: String,
    pub rows: Vec<AnimRow>,
    pub data: String,
}

/// One JSON object per stdin line; unknown tags fail to parse and are dropped.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Command {
    Frame {
        data: String,
    },
    Clear,
    Quit,
    Ping {
        #[serde(default)]
        n: Option<u64>,
    },
    Auth {
        token: String,
    },
    Config(crate::v2::ConfigMsg),
    Theme(crate::v2::ThemeMsg),
    Model(crate::v2::ModelMsg),
    Menu(crate::v2::MenuMsg),
    Fx(crate::v2::FxMsg),
    Notice(crate::v2::NoticeMsg),
    Anim(AnimCmd),
}
