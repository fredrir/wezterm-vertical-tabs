use crate::{SpaceId, location};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type TabId = u64;
pub type PaneId = u64;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LaunchSpec {
    pub domain: Option<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TabPane {
    pub id: PaneId,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub repo_root: Option<String>,
    /// The owning machine's home; this host's when omitted.
    #[serde(default)]
    pub home: Option<String>,
    #[serde(default)]
    pub active: bool,
    /// Splits of one tab can live on different machines.
    #[serde(default)]
    pub remote: bool,
    #[serde(default)]
    pub os: String,
    /// Cell extent inside the tab, so the sidebar can mirror the split layout.
    #[serde(default)]
    pub left: u16,
    #[serde(default)]
    pub top: u16,
    #[serde(default)]
    pub width: u16,
    #[serde(default)]
    pub height: u16,
}

impl TabPane {
    pub fn label(&self, home: Option<&str>) -> String {
        location::label(
            &self.cwd,
            self.repo_root.as_deref(),
            self.home.as_deref().or(home),
        )
        .unwrap_or_else(|| self.title.clone())
    }
}

/// Facts discovered by the host, without application membership or title overrides.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostTab {
    pub id: TabId,
    pub title: String,
    pub icon: String,
    pub cwd: String,
    pub repo_root: Option<String>,
    pub home: Option<String>,
    pub domain: String,
    pub host: String,
    pub user: String,
    pub process: String,
    pub remote: bool,
    pub os: String,
    pub panes: Vec<TabPane>,
    pub unread: bool,
    pub bell: bool,
    /// Discovery's launch hint is adopted only until the application has a launch.
    pub launch: Option<LaunchSpec>,
}

/// Host metadata and application-owned state, also used by the transfer contract.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Tab {
    pub id: TabId,
    pub title: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub cwd: String,
    /// Repository root when the directory is inside a Git work tree.
    #[serde(default)]
    pub repo_root: Option<String>,
    /// The owning machine's home; this host's when omitted.
    #[serde(default)]
    pub home: Option<String>,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub process: String,
    #[serde(default)]
    pub remote: bool,
    /// os-release style ID of the machine running the tab's active pane.
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub panes: Vec<TabPane>,
    #[serde(default)]
    pub unread: bool,
    #[serde(default)]
    pub bell: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub folder_id: Option<String>,
    #[serde(default)]
    pub space_id: SpaceId,
    #[serde(default)]
    pub manual_assignment: bool,
    #[serde(default)]
    pub launch: Option<LaunchSpec>,
    #[serde(default)]
    pub title_override: Option<String>,
    #[serde(default)]
    pub title_hook: Option<String>,
}

impl Tab {
    /// An explicit rename from the context menu or a Lua title hook.
    pub fn custom_title(&self) -> Option<&str> {
        self.title_override
            .as_deref()
            .or(self.title_hook.as_deref())
    }

    pub fn display_title(&self) -> &str {
        self.custom_title().unwrap_or(&self.title)
    }

    pub fn location(&self, home: Option<&str>) -> Option<String> {
        location::label(
            &self.cwd,
            self.repo_root.as_deref(),
            self.home.as_deref().or(home),
        )
    }

    pub(crate) fn reconcile_host_metadata(&mut self, incoming: HostTab) -> bool {
        let HostTab {
            id,
            title,
            icon,
            cwd,
            repo_root,
            home,
            domain,
            host,
            user,
            process,
            remote,
            os,
            panes,
            unread,
            bell,
            launch,
        } = incoming;
        let mut changed = false;
        macro_rules! update {
            ($($field:ident),+ $(,)?) => {
                $(if self.$field != $field {
                    self.$field = $field;
                    changed = true;
                })+
            };
        }
        update!(
            id, title, icon, cwd, repo_root, home, domain, host, user, process, remote, os, panes,
            unread, bell
        );
        // Discovery supplies only domain and cwd, so a captured launch is kept.
        if self.launch.is_none() && launch.is_some() {
            self.launch = launch;
            changed = true;
        }
        changed
    }
}

impl From<HostTab> for Tab {
    fn from(facts: HostTab) -> Self {
        let mut tab = Self::default();
        tab.reconcile_host_metadata(facts);
        tab
    }
}
