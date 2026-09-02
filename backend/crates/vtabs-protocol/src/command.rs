use serde::Deserialize;

/// The JSON payload inside one framed, session-bound stdin control record.
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
        #[serde(default)]
        caps: Vec<String>,
    },
    /// Start one atomic state publication. Sections that follow are staged until Commit.
    Begin {
        generation: u64,
    },
    /// Publish the complete staged generation in one paint.
    Commit {
        generation: u64,
    },
    /// The host hook's answer for the one generation currently awaiting it.
    ThemeHookResult {
        generation: u64,
        overrides: Box<crate::v2::ThemeOverrides>,
    },
    Config(Box<crate::v2::ConfigMsg>),
    Theme(Box<crate::v2::ThemeMsg>),
    Model(Box<crate::v2::ModelMsg>),
    /// Raw full-window facts for the stateless spaces planner. Like the other semantic sections it
    /// is staged by Begin/Commit; sidebar and settings processes receive the same window topology.
    Spaces(Box<crate::v2::SpacesMsg>),
    /// One generation's complete answer to a batched `space_route_hook_request`.
    SpaceRouteHookResult {
        generation: u64,
        #[serde(default)]
        routes: Vec<crate::v2::SpaceRouteHookAnswer>,
    },
    /// Canonical settings input for a `settings_document` capable backend. Older clients keep
    /// sending a pre-rendered settings `model` and remain supported by that command.
    Settings(Box<crate::v2::SettingsMsg>),
    Menu(Box<crate::v2::MenuMsg>),
    Fx(crate::v2::FxMsg),
    Notice(crate::v2::NoticeMsg),
    /// Kill the one pane on this server titled `title`, through the server's own `wezterm cli`.
    Kill {
        title: String,
    },
    /// Move every other pane of this tab that sits inside the sidebar's `band` columns beside the
    /// content; `position` is the sidebar's edge, `left` unless `right`.
    Rescue {
        band: u32,
        #[serde(default)]
        position: Option<String>,
    },
}
