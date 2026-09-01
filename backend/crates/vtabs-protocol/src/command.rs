use serde::Deserialize;

/// One JSON object per stdin line; unknown tags fail to parse and are dropped.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Command {
    /// Repaint from the stored state; the pane's pixels are the backend's to own.
    Clear,
    Quit,
    Ping {
        #[serde(default)]
        n: Option<u64>,
    },
    Auth {
        token: String,
    },
    Config(Box<crate::v2::ConfigMsg>),
    Theme(Box<crate::v2::ThemeMsg>),
    Model(Box<crate::v2::ModelMsg>),
    Menu(Box<crate::v2::MenuMsg>),
    Fx(crate::v2::FxMsg),
    Notice(crate::v2::NoticeMsg),
}
