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
    /// `keys: "server"` asks the backend to deliver forwarded keys through the server's own cli.
    Auth {
        token: String,
        #[serde(default)]
        caps: Vec<String>,
        #[serde(default)]
        keys: Option<String>,
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
    /// Kill one pane on this server through its own `wezterm cli`: `pane` names it by server id,
    /// `title` by the backend title marker exactly one pane carries.
    Kill {
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        pane: Option<u64>,
    },
    /// The first inbox message of a session; it proves Lua can write where `ready` pointed.
    TransportProbe {
        session: String,
    },
    /// The last stdin frame before Lua switches: the probe is either on disk by now or never.
    TransportBarrier {
        session: String,
    },
    /// Lua goes back to stdin; whatever the inbox still holds is applied first, in order.
    TransportStop {
        session: String,
    },
    /// Move every other pane of this tab that sits inside the sidebar's `band` columns beside the
    /// content; `position` is the sidebar's edge, `left` unless `right`.
    Rescue {
        band: u32,
        #[serde(default)]
        position: Option<String>,
    },
    /// Resize this pane's own split on the server through the server's own `wezterm cli`: on a
    /// mux domain the server's tree is the truth the GUI mirrors, so one change there is one change
    /// everywhere. With `target`, the width to land at, the backend reads its own column count
    /// from the server's pane list and works the delta out there, keeping `min_content` for each
    /// band of content beside it: a mirror that lags the server may name a target, never a delta.
    /// Without it, `amount` cells in `direction` (`AdjustPaneSize`'s `Left` or `Right`) are asked
    /// for as given. The walk starts at the tab's active pane; when that pane cannot reach the
    /// sidebar's split, the sidebar takes focus for the adjust and hands it back, unless `park`
    /// keeps it (a resize burst) until an adjust without `park` follows.
    Adjust {
        direction: String,
        amount: u32,
        #[serde(default)]
        park: bool,
        #[serde(default)]
        target: Option<u32>,
        #[serde(default)]
        min_content: Option<u32>,
    },
}
