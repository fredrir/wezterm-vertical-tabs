mod config;
mod dispatch;
mod hooks;
mod projection;
mod reconcile;

use crate::{
    DEFAULT_SPACE, Error, Folder, LaunchSpec, MAX_NAME_CHARS, Space, SpaceId, Tab, TabId,
    routing::SpaceTemplate,
    settings::{RailMode, Settings},
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug)]
pub struct Model {
    pub profile: String,
    pub spaces: Vec<Space>,
    pub folders: Vec<Folder>,
    pub templates: Vec<SpaceTemplate>,
    pub tabs: BTreeMap<TabId, Tab>,
    pub selected_space: SpaceId,
    pub selected_tab: Option<TabId>,
    pub settings: Settings,
    pub revision: u64,
    /// Host home directory; labels shorten only, paths are never rewritten.
    pub home: Option<String>,
    pub config_owned: BTreeSet<String>,
    pub footer: String,
    order: Vec<TabId>,
    visible: Vec<TabId>,
    last_tabs: BTreeMap<SpaceId, TabId>,
    mru: VecDeque<TabId>,
    managed_settings: BTreeMap<String, Value>,
    lua_settings: BTreeMap<String, Value>,
    rail_override: Option<RailMode>,
    hook_routes: BTreeMap<TabId, Option<SpaceId>>,
    hidden: BTreeSet<TabId>,
    next_space: u64,
    next_folder: u64,
    projection_revision: u64,
    space_namespace: String,
}

impl Default for Model {
    fn default() -> Self {
        Self::new("default")
    }
}

impl Model {
    pub fn new(profile: impl Into<String>) -> Self {
        Self {
            profile: profile.into(),
            spaces: vec![Space::new(DEFAULT_SPACE, "Home")],
            folders: Vec::new(),
            templates: Vec::new(),
            tabs: BTreeMap::new(),
            selected_space: DEFAULT_SPACE.into(),
            selected_tab: None,
            settings: Settings::default(),
            revision: 0,
            home: None,
            config_owned: BTreeSet::new(),
            footer: String::new(),
            order: Vec::new(),
            visible: Vec::new(),
            last_tabs: BTreeMap::new(),
            mru: VecDeque::new(),
            managed_settings: BTreeMap::new(),
            lua_settings: BTreeMap::new(),
            rail_override: None,
            hook_routes: BTreeMap::new(),
            hidden: BTreeSet::new(),
            next_space: 1,
            next_folder: 1,
            projection_revision: 0,
            space_namespace: String::new(),
        }
    }
    pub fn visible_ids(&self) -> &[TabId] {
        &self.visible
    }
    /// A per-instance namespace keeps created IDs unique without clocks or randomness in core.
    pub fn set_space_namespace(&mut self, namespace: String) {
        self.space_namespace = namespace
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(64)
            .collect();
    }
    pub fn projection_revision(&self) -> u64 {
        self.projection_revision
    }
    pub fn selected_folders(&self) -> impl Iterator<Item = &Folder> {
        self.folders
            .iter()
            .filter(|folder| folder.space_id == self.selected_space)
    }
    /// Collapsing a folder changes its sidebar presentation, never its live tab projection.
    pub fn folder_tabs<'a>(&'a self, folder_id: &'a str) -> impl Iterator<Item = &'a Tab> {
        self.order.iter().filter_map(move |id| {
            self.tabs.get(id).filter(|tab| {
                tab.folder_id.as_deref() == Some(folder_id) && !self.hidden.contains(id)
            })
        })
    }
    pub fn tab_order(&self) -> &[TabId] {
        &self.order
    }
    pub fn selected_space(&self) -> &Space {
        self.spaces
            .iter()
            .find(|s| s.id == self.selected_space)
            .unwrap_or(&self.spaces[0])
    }
    pub fn managed_settings(&self) -> &BTreeMap<String, Value> {
        &self.managed_settings
    }
    pub fn configured_settings(&self) -> &BTreeMap<String, Value> {
        &self.lua_settings
    }
    pub fn space_activity(&self, id: &str) -> (usize, bool, bool) {
        self.tabs
            .values()
            .filter(|t| t.space_id == id)
            .fold((0, false, false), |(n, u, b), t| {
                (n + 1, u || t.unread, b || t.bell)
            })
    }
    pub fn validate_projection(&self, ids: &[TabId], selected: Option<TabId>) -> Result<(), Error> {
        let mut seen = BTreeSet::new();
        for id in ids {
            if !self.tabs.contains_key(id) || !seen.insert(*id) {
                return Err(Error(
                    "Projection contains an unknown or duplicate tab".into(),
                ));
            }
        }
        if selected.is_some_and(|id| !seen.contains(&id)) {
            return Err(Error("Selected tab is outside projection".into()));
        }
        Ok(())
    }
    /// A space's tabs in sidebar order, including those a filter hook hides.
    pub fn space_tabs(&self, space: &str) -> Vec<TabId> {
        let mut ids = Vec::new();
        self.project_space(space, true, &mut ids);
        ids
    }
    pub fn is_hidden(&self, id: TabId) -> bool {
        self.hidden.contains(&id)
    }
    pub fn default_launch(&self) -> LaunchSpec {
        let selected = self.selected_tab.and_then(|id| self.tabs.get(&id));
        let domain = self.settings.default_domain.clone().or_else(|| {
            selected
                .filter(|tab| !tab.domain.is_empty())
                .map(|tab| tab.domain.clone())
        });
        let cwd = selected
            .filter(|tab| {
                !tab.cwd.is_empty() && domain.as_deref().unwrap_or_default() == tab.domain
            })
            .map(|tab| tab.cwd.clone());
        LaunchSpec {
            domain,
            cwd,
            ..LaunchSpec::default()
        }
    }
    pub fn set_home(&mut self, home: Option<String>) {
        if self.home != home {
            self.home = home;
            self.touch();
        }
    }
    pub fn set_launch(&mut self, id: TabId, launch: LaunchSpec) -> Result<(), Error> {
        self.require_tab(id)?;
        if launch.args.len() > 256
            || launch.env.len() > 128
            || launch.args.iter().any(|s| s.len() > 4096)
            || launch
                .env
                .iter()
                .any(|(k, v)| k.len() > 256 || v.len() > 4096)
            || launch.domain.as_ref().is_some_and(|s| s.len() > 4096)
            || launch.cwd.as_ref().is_some_and(|s| s.len() > 4096)
        {
            return Err(Error("Launch metadata exceeds supported limits".into()));
        }
        self.tabs.get_mut(&id).unwrap().launch = Some(launch);
        Ok(())
    }
    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
    fn require_tab(&self, id: TabId) -> Result<(), Error> {
        if self.tabs.contains_key(&id) {
            Ok(())
        } else {
            Err(Error(format!("Tab {id} no longer exists")))
        }
    }
    fn require_space(&self, id: &str) -> Result<(), Error> {
        if self.spaces.iter().any(|s| s.id == id) {
            Ok(())
        } else {
            Err(Error(format!("Unknown space: {id}")))
        }
    }
    fn require_folder(&self, id: &str) -> Result<usize, Error> {
        self.folders
            .iter()
            .position(|folder| folder.id == id)
            .ok_or_else(|| Error(format!("Unknown folder: {id}")))
    }
}

fn clean_name(name: String) -> Result<String, Error> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        Err(Error(format!(
            "Name must contain 1-{MAX_NAME_CHARS} printable characters"
        )))
    } else {
        Ok(name.into())
    }
}

fn next_id(namespace: &str, kind: &str, next: &mut u64, taken: impl Fn(&str) -> bool) -> String {
    loop {
        let id = if namespace.is_empty() {
            format!("{kind}-{next}")
        } else {
            format!("{kind}-{namespace}-{next}")
        };
        *next += 1;
        if !taken(&id) {
            return id;
        }
    }
}
