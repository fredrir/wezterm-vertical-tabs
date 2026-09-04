use serde::Deserialize;

/// The JSON payload inside one framed, session-bound stdin control record.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case", deny_unknown_fields)]
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
        keys: Option<String>,
    },
    /// Start one atomic state publication. Sections that follow are staged until Commit.
    Begin,
    /// Publish the complete staged state in one paint.
    Commit,
    /// The host theme hook's answer for the publication currently awaiting it.
    ThemeHookResult {
        overrides: Box<crate::payload::ThemeOverrides>,
    },
    Config(Box<crate::payload::ConfigMsg>),
    Theme(Box<crate::payload::ThemeMsg>),
    Model(Box<crate::payload::ModelMsg>),
    /// Raw full-window facts for the stateless spaces planner. Like the other semantic sections it
    /// is staged by Begin/Commit; sidebar and settings processes receive the same window topology.
    Spaces(Box<crate::payload::SpacesMsg>),
    /// The complete answer to the current batched `space_route_hook_request`.
    SpaceRouteHookResult {
        #[serde(default)]
        routes: Vec<crate::payload::SpaceRouteHookAnswer>,
    },
    /// Canonical settings input owned, validated, and rendered by Rust.
    Settings(Box<crate::payload::SettingsMsg>),
    Menu(Box<crate::payload::MenuMsg>),
    Fx(crate::payload::FxMsg),
    Notice(crate::payload::NoticeMsg),
    /// Kill one pane on this server through its server id.
    Kill {
        pane: u64,
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
        position: String,
    },
    /// Resize this pane's own split on the server through the server's own `wezterm cli`: on a
    /// mux domain the server's tree is the truth the GUI mirrors, so one change there is one change
    /// everywhere. The backend reads its own column count from the server's pane list and works
    /// out the delta, keeping `min_content` columns for each content band beside it.
    Adjust {
        target: u32,
        min_content: u32,
    },
}
