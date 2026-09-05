use crate::{
    routing::{self, RoutingRule, SpaceTemplate},
    settings::{self, RailMode, Settings},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub type TabId = u64;
pub type SpaceId = String;
pub const DEFAULT_SPACE: &str = "home";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Space {
    pub id: SpaceId,
    pub name: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub accent: Option<String>,
    #[serde(default)]
    pub rules: Vec<RoutingRule>,
    #[serde(default)]
    pub template: Option<String>,
}
impl Space {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            icon: "◉".into(),
            accent: None,
            rules: Vec::new(),
            template: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchSpec {
    pub domain: Option<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Host metadata is copied only when it changes; membership remains application-owned.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    pub id: TabId,
    pub title: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub cwd: String,
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
    #[serde(default)]
    pub unread: bool,
    #[serde(default)]
    pub bell: bool,
    #[serde(default)]
    pub pinned: bool,
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
    pub fn display_title(&self) -> &str {
        self.title_override
            .as_deref()
            .or(self.title_hook.as_deref())
            .unwrap_or(&self.title)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Intent {
    SelectSpace(SpaceId),
    CreateSpace {
        name: String,
    },
    RenameSpace {
        id: SpaceId,
        name: String,
    },
    EditSpace {
        id: SpaceId,
        icon: String,
        accent: Option<String>,
        rules: Vec<RoutingRule>,
    },
    DeleteSpace {
        id: SpaceId,
        destination: Option<SpaceId>,
    },
    MoveSpace {
        id: SpaceId,
        index: usize,
    },
    ActivateTab(TabId),
    ActivateIndex(isize),
    ActivateRelative {
        delta: isize,
        wrap: bool,
    },
    ActivateLast,
    NewTab,
    CloseTab(TabId),
    CloseOthers(TabId),
    RenameTab {
        id: TabId,
        title: String,
    },
    PinTab {
        id: TabId,
        pinned: bool,
    },
    MoveTab {
        id: TabId,
        index: usize,
    },
    AssignTab {
        id: TabId,
        space_id: SpaceId,
    },
    ReturnToAuto(TabId),
    Reopen,
    SetSetting {
        key: String,
        value: Value,
    },
    ResetSetting(String),
    ResetSettings,
    SetRail(RailMode),
    PrivateWindow,
    MoveTabToNewWindow(TabId),
    CustomAction(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostCommand {
    Activate(TabId),
    Spawn {
        space_id: SpaceId,
        launch: LaunchSpec,
    },
    Close(TabId),
    Rename {
        id: TabId,
        title: String,
    },
    Reorder {
        visible_order: Vec<TabId>,
    },
    NewWindow {
        private: bool,
        launch: LaunchSpec,
    },
    MoveTabToNewWindow(TabId),
    CustomAction(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transition {
    pub commands: Vec<HostCommand>,
    pub model_changed: bool,
    pub durable_changed: bool,
    pub layout_changed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Error {}
impl From<String> for Error {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Clone, Debug)]
pub struct Model {
    pub profile: String,
    pub spaces: Vec<Space>,
    pub templates: Vec<SpaceTemplate>,
    pub tabs: BTreeMap<TabId, Tab>,
    pub selected_space: SpaceId,
    pub selected_tab: Option<TabId>,
    pub settings: Settings,
    pub revision: u64,
    pub private: bool,
    pub config_owned: BTreeSet<String>,
    pub footer: String,
    order: Vec<TabId>,
    visible: Vec<TabId>,
    last_tabs: BTreeMap<SpaceId, TabId>,
    mru: VecDeque<TabId>,
    reopened: VecDeque<(TabId, SpaceId, LaunchSpec)>,
    departed: BTreeSet<TabId>,
    persisted_settings: BTreeMap<String, Value>,
    lua_settings: BTreeMap<String, Value>,
    hook_routes: BTreeMap<TabId, Option<SpaceId>>,
    hidden: BTreeSet<TabId>,
    next_space: u64,
    projection_revision: u64,
    space_namespace: String,
}

impl Default for Model {
    fn default() -> Self {
        Self::new("default", false)
    }
}
impl Model {
    pub fn new(profile: impl Into<String>, private: bool) -> Self {
        Self {
            profile: profile.into(),
            spaces: vec![Space::new(DEFAULT_SPACE, "Home")],
            templates: Vec::new(),
            tabs: BTreeMap::new(),
            selected_space: DEFAULT_SPACE.into(),
            selected_tab: None,
            settings: Settings::default(),
            revision: 0,
            private,
            config_owned: BTreeSet::new(),
            footer: String::new(),
            order: Vec::new(),
            visible: Vec::new(),
            last_tabs: BTreeMap::new(),
            mru: VecDeque::new(),
            reopened: VecDeque::new(),
            departed: BTreeSet::new(),
            persisted_settings: BTreeMap::new(),
            lua_settings: BTreeMap::new(),
            hook_routes: BTreeMap::new(),
            hidden: BTreeSet::new(),
            next_space: 1,
            projection_revision: 0,
            space_namespace: String::new(),
        }
    }
    pub fn visible_ids(&self) -> &[TabId] {
        &self.visible
    }
    /// The application supplies a per-instance namespace; deterministic core transitions
    /// never consult clocks or random sources themselves.
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
    pub fn visible_tabs(&self) -> Vec<&Tab> {
        self.visible
            .iter()
            .filter_map(|id| self.tabs.get(id))
            .collect()
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
    pub fn persisted_settings(&self) -> &BTreeMap<String, Value> {
        &self.persisted_settings
    }
    pub fn configured_settings(&self) -> &BTreeMap<String, Value> {
        &self.lua_settings
    }
    pub fn can_reopen(&self) -> bool {
        !self.reopened.is_empty() && !self.private
    }
    /// A native move/detach is not a closed tab. Acknowledgements may precede or follow
    /// the topology snapshot, so also remove any already-recorded history for this ID.
    pub fn acknowledge_tab_departure(&mut self, id: TabId) -> bool {
        if self.tabs.contains_key(&id) {
            self.departed.insert(id);
        }
        let before = self.reopened.len();
        self.reopened.retain(|(tab, _, _)| *tab != id);
        let changed = before != self.reopened.len();
        if changed {
            self.touch();
        }
        changed
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
    fn rebuild_visible(&mut self) {
        self.projection_revision = self.projection_revision.wrapping_add(1);
        self.visible.clear();
        for pinned in [true, false] {
            self.visible.extend(self.order.iter().copied().filter(|id| {
                self.tabs.get(id).is_some_and(|t| {
                    t.space_id == self.selected_space
                        && t.pinned == pinned
                        && !self.hidden.contains(id)
                })
            }));
        }
        if self
            .selected_tab
            .is_none_or(|id| !self.visible.contains(&id))
        {
            self.selected_tab = self.visible.first().copied();
            if let Some(id) = self.selected_tab {
                let tab = self.tabs.get_mut(&id).unwrap();
                tab.unread = false;
                tab.bell = false;
            }
        }
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
    fn activate(&mut self, id: TabId) {
        let was_hidden = self.hidden.remove(&id);
        if let Some(tab) = self.tabs.get_mut(&id) {
            let space_changed = self.selected_space != tab.space_id;
            self.selected_space = tab.space_id.clone();
            tab.unread = false;
            tab.bell = false;
            self.selected_tab = Some(id);
            self.last_tabs.insert(self.selected_space.clone(), id);
            self.mru.retain(|t| *t != id);
            self.mru.push_front(id);
            if space_changed || was_hidden {
                self.rebuild_visible();
            }
        }
    }
    fn select_space(&mut self, id: SpaceId) {
        self.selected_space = id;
        self.rebuild_visible();
        let last = self
            .last_tabs
            .get(&self.selected_space)
            .copied()
            .filter(|id| self.visible.contains(id));
        self.selected_tab = last.or_else(|| self.visible.first().copied());
        if let Some(id) = self.selected_tab {
            self.activate(id);
        }
    }
    fn clean_name(name: String) -> Result<String, Error> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 128 || name.chars().any(char::is_control) {
            Err(Error("Name must contain 1–128 printable characters".into()))
        } else {
            Ok(name.into())
        }
    }
    fn create_id(&mut self) -> String {
        loop {
            let id = if self.space_namespace.is_empty() {
                format!("space-{}", self.next_space)
            } else {
                format!("space-{}-{}", self.space_namespace, self.next_space)
            };
            self.next_space += 1;
            if !self.spaces.iter().any(|s| s.id == id) {
                return id;
            }
        }
    }
    fn route_tab(&mut self, id: TabId) {
        let Some(tab) = self.tabs.get(&id) else {
            return;
        };
        if tab.manual_assignment && self.spaces.iter().any(|s| s.id == tab.space_id) {
            return;
        }
        let route = self
            .hook_routes
            .get(&id)
            .and_then(Clone::clone)
            .or_else(|| routing::route(&self.spaces, &self.templates, tab));
        let selected = route.unwrap_or_else(|| {
            if !tab.space_id.is_empty() && self.spaces.iter().any(|s| s.id == tab.space_id) {
                tab.space_id.clone()
            } else {
                self.selected_space.clone()
            }
        });
        if !self.spaces.iter().any(|s| s.id == selected) {
            let mut space = Space::new(&selected, &selected);
            if let Some(template) = self
                .templates
                .iter()
                .find(|t| routing::expand(&t.id, tab).as_deref() == Some(&selected))
            {
                space.name =
                    routing::expand(&template.name, tab).unwrap_or_else(|| selected.clone());
                space.icon = template.icon.clone();
                space.accent = template.accent.clone();
                space.template = Some(template.id.clone());
            }
            self.spaces.push(space);
        }
        self.tabs.get_mut(&id).unwrap().space_id = selected;
    }

    /// Reconcile a complete native topology. Only a changed native active ID follows a space.
    /// This prevents an empty selected space from exposing the mux's still-active hidden tab.
    pub fn reconcile(
        &mut self,
        incoming: Vec<Tab>,
        active: Option<TabId>,
        follow_active: bool,
    ) -> Result<bool, Error> {
        let mut ids = BTreeSet::new();
        for tab in &incoming {
            if !ids.insert(tab.id) {
                return Err(Error("Duplicate host tab identity".into()));
            }
        }
        let before = self.revision;
        let removed = self
            .order
            .iter()
            .copied()
            .filter(|id| !ids.contains(id))
            .collect::<Vec<_>>();
        let old_selected = self.selected_tab;
        let old_position = self
            .visible
            .iter()
            .position(|id| Some(*id) == old_selected)
            .unwrap_or(0);
        let mut changed = !removed.is_empty();
        for id in removed {
            let departed = self.departed.remove(&id);
            if let Some(tab) = self.tabs.remove(&id)
                && !self.private
                && !departed
                && let Some(launch) = tab.launch
            {
                self.reopened.push_front((id, tab.space_id, launch));
            }
            self.hook_routes.remove(&id);
            self.hidden.remove(&id);
            self.mru.retain(|v| *v != id);
            self.last_tabs.retain(|_, v| *v != id);
        }
        self.reopened
            .truncate(usize::from(self.settings.reopen_limit));
        let native_order = incoming.iter().map(|t| t.id).collect::<Vec<_>>();
        if native_order != self.order {
            self.order = native_order;
            changed = true;
        }
        for mut tab in incoming {
            if Some(tab.id) == self.selected_tab {
                tab.unread = false;
                tab.bell = false;
            }
            if let Some(old) = self.tabs.get(&tab.id) {
                tab.space_id = old.space_id.clone();
                tab.manual_assignment = old.manual_assignment;
                tab.pinned = old.pinned;
                tab.title_override = old.title_override.clone();
                tab.title_hook = old.title_hook.clone();
                // Native discovery supplies only domain/cwd. Preserve explicit launch
                // arguments/environment captured at creation for accurate reopen intent.
                if old.launch.is_some() {
                    tab.launch = old.launch.clone();
                }
                if old == &tab {
                    continue;
                }
                self.hook_routes.remove(&tab.id);
            } else if tab.space_id.is_empty() {
                tab.space_id = self.selected_space.clone();
            }
            let id = tab.id;
            self.tabs.insert(id, tab);
            self.route_tab(id);
            changed = true;
        }
        if changed {
            self.rebuild_visible();
        }
        let selected_closed = old_selected.is_some_and(|id| !self.tabs.contains_key(&id));
        if follow_active
            && !selected_closed
            && let Some(id) = active.filter(|id| self.tabs.contains_key(id))
            && (self.selected_tab != Some(id) || self.tabs[&id].space_id != self.selected_space)
        {
            self.activate(id);
            changed = true;
        }
        if old_selected.is_some_and(|id| !self.tabs.contains_key(&id))
            && self.selected_tab.is_none()
        {
            self.selected_tab = self
                .visible
                .get(old_position.min(self.visible.len().saturating_sub(1)))
                .copied();
            if let Some(id) = self.selected_tab {
                self.activate(id);
            }
            changed = true;
        }
        if changed {
            self.touch();
        }
        Ok(self.revision != before)
    }

    pub fn dispatch(&mut self, intent: Intent) -> Result<Transition, Error> {
        let rebuild_projection = !matches!(
            &intent,
            Intent::ActivateTab(_)
                | Intent::RenameTab { .. }
                | Intent::SetSetting { .. }
                | Intent::ResetSetting(_)
                | Intent::ResetSettings
        );
        let mut out = Transition::default();
        match intent {
            Intent::SelectSpace(id) => {
                self.require_space(&id)?;
                if self.selected_space == id {
                    return Ok(out);
                }
                self.select_space(id);
                if let Some(id) = self.selected_tab {
                    out.commands.push(HostCommand::Activate(id));
                }
                out.durable_changed = true;
            }
            Intent::CreateSpace { name } => {
                let name = Self::clean_name(name)?;
                if self.spaces.len() >= 256 {
                    return Err(Error("At most 256 spaces are supported".into()));
                }
                let id = self.create_id();
                self.spaces.push(Space::new(&id, name));
                self.select_space(id);
                out.durable_changed = true;
            }
            Intent::RenameSpace { id, name } => {
                let name = Self::clean_name(name)?;
                self.require_space(&id)?;
                self.spaces.iter_mut().find(|s| s.id == id).unwrap().name = name;
                out.durable_changed = true;
            }
            Intent::EditSpace {
                id,
                icon,
                accent,
                rules,
            } => {
                self.require_space(&id)?;
                if icon.chars().count() > 16
                    || accent.as_deref().is_some_and(|c| !settings::valid_color(c))
                {
                    return Err(Error("Invalid icon or accent".into()));
                }
                routing::validate_rules(&rules).map_err(Error)?;
                let space = self.spaces.iter_mut().find(|s| s.id == id).unwrap();
                space.icon = icon;
                space.accent = accent;
                space.rules = rules;
                self.reroute();
                out.durable_changed = true;
            }
            Intent::DeleteSpace { id, destination } => {
                self.require_space(&id)?;
                if self.spaces.len() == 1 {
                    return Err(Error("The final space cannot be deleted".into()));
                }
                let occupied = self.tabs.values().any(|t| t.space_id == id);
                if occupied && destination.is_none() {
                    return Err(Error("Choose a destination for the space's tabs".into()));
                }
                if let Some(dest) = &destination {
                    self.require_space(dest)?;
                    if dest == &id {
                        return Err(Error("Destination must be another space".into()));
                    }
                }
                let fallback = destination
                    .unwrap_or_else(|| self.spaces.iter().find(|s| s.id != id).unwrap().id.clone());
                for tab in self.tabs.values_mut().filter(|t| t.space_id == id) {
                    tab.space_id = fallback.clone();
                    tab.manual_assignment = true;
                }
                self.spaces.retain(|s| s.id != id);
                self.last_tabs.remove(&id);
                if self.selected_space == id {
                    self.select_space(fallback);
                    if let Some(id) = self.selected_tab {
                        out.commands.push(HostCommand::Activate(id));
                    }
                }
                out.durable_changed = true;
            }
            Intent::MoveSpace { id, index } => {
                self.require_space(&id)?;
                let at = self.spaces.iter().position(|s| s.id == id).unwrap();
                let space = self.spaces.remove(at);
                self.spaces.insert(index.min(self.spaces.len()), space);
                out.durable_changed = true;
            }
            Intent::ActivateTab(id) => {
                self.require_tab(id)?;
                self.activate(id);
                out.commands.push(HostCommand::Activate(id));
            }
            Intent::ActivateIndex(index) => {
                let n = self.visible.len() as isize;
                let index = if index < 0 { n + index } else { index };
                if index < 0 || index >= n {
                    return Ok(out);
                }
                return self.dispatch(Intent::ActivateTab(self.visible[index as usize]));
            }
            Intent::ActivateRelative { delta, wrap } => {
                let n = self.visible.len() as isize;
                if n == 0 {
                    return Ok(out);
                }
                let at = self
                    .selected_tab
                    .and_then(|id| self.visible.iter().position(|v| *v == id))
                    .unwrap_or(0) as isize;
                let i = at.saturating_add(delta);
                let i = if wrap {
                    i.rem_euclid(n)
                } else {
                    i.clamp(0, n - 1)
                };
                return self.dispatch(Intent::ActivateTab(self.visible[i as usize]));
            }
            Intent::ActivateLast => {
                if let Some(id) = self
                    .mru
                    .iter()
                    .find(|id| Some(**id) != self.selected_tab && self.visible.contains(id))
                    .copied()
                {
                    return self.dispatch(Intent::ActivateTab(id));
                }
                return Ok(out);
            }
            Intent::NewTab => {
                let launch = self.default_launch();
                out.commands.push(HostCommand::Spawn {
                    space_id: self.selected_space.clone(),
                    launch,
                });
                return Ok(out);
            }
            Intent::CloseTab(id) => {
                self.require_tab(id)?;
                out.commands.push(HostCommand::Close(id));
                return Ok(out);
            }
            Intent::CloseOthers(id) => {
                self.require_tab(id)?;
                out.commands.extend(
                    self.visible
                        .iter()
                        .filter(|other| **other != id && !self.tabs[other].pinned)
                        .map(|id| HostCommand::Close(*id)),
                );
                return Ok(out);
            }
            Intent::RenameTab { id, title } => {
                self.require_tab(id)?;
                if title.chars().count() > 512 || title.chars().any(char::is_control) {
                    return Err(Error("Invalid tab title".into()));
                }
                self.tabs.get_mut(&id).unwrap().title_override =
                    (!title.is_empty()).then_some(title.clone());
                out.commands.push(HostCommand::Rename { id, title });
            }
            Intent::PinTab { id, pinned } => {
                self.require_tab(id)?;
                self.tabs.get_mut(&id).unwrap().pinned = pinned;
                out.durable_changed = true;
            }
            Intent::MoveTab { id, index } => {
                self.require_tab(id)?;
                let Some(at) = self.visible.iter().position(|v| *v == id) else {
                    return Err(Error("Cannot reorder a hidden tab".into()));
                };
                let pinned = self.tabs[&id].pinned;
                let mut visible = self.visible.clone();
                visible.remove(at);
                let boundary = visible.iter().take_while(|id| self.tabs[id].pinned).count();
                let index = if pinned {
                    index.min(boundary)
                } else {
                    index.max(boundary).min(visible.len())
                };
                visible.insert(index, id);
                let visible_set = self.visible.iter().copied().collect::<BTreeSet<_>>();
                let mut reordered = visible.iter();
                for entry in &mut self.order {
                    if visible_set.contains(entry) {
                        *entry = *reordered.next().unwrap();
                    }
                }
                out.commands.push(HostCommand::Reorder {
                    visible_order: visible,
                });
                out.durable_changed = true;
            }
            Intent::AssignTab { id, space_id } => {
                self.require_tab(id)?;
                self.require_space(&space_id)?;
                let tab = self.tabs.get_mut(&id).unwrap();
                tab.space_id = space_id;
                tab.manual_assignment = true;
                if self.selected_tab == Some(id) {
                    self.select_space(self.selected_space.clone());
                    if let Some(id) = self.selected_tab {
                        out.commands.push(HostCommand::Activate(id));
                    }
                }
                out.durable_changed = true;
            }
            Intent::ReturnToAuto(id) => {
                self.require_tab(id)?;
                let tab = self.tabs.get_mut(&id).unwrap();
                tab.manual_assignment = false;
                tab.space_id = self.spaces[0].id.clone();
                self.hook_routes.remove(&id);
                self.route_tab(id);
                out.durable_changed = true;
            }
            Intent::Reopen => {
                if self.private {
                    return Ok(out);
                }
                if let Some((_, space_id, launch)) = self.reopened.pop_front() {
                    let space_id = if self.spaces.iter().any(|s| s.id == space_id) {
                        space_id
                    } else {
                        self.selected_space.clone()
                    };
                    out.commands.push(HostCommand::Spawn { space_id, launch });
                }
                return Ok(out);
            }
            Intent::SetSetting { key, value } => {
                if self.config_owned.contains(&key) {
                    return Err(Error(format!("{key} is configured in Lua")));
                }
                self.settings.set(&key, value.clone())?;
                self.persisted_settings.insert(key, value);
                out.durable_changed = true;
                out.layout_changed = true;
            }
            Intent::ResetSetting(key) => {
                if self.config_owned.contains(&key) {
                    return Err(Error(format!("{key} is configured in Lua")));
                }
                let value = Settings::default()
                    .get(&key)
                    .ok_or_else(|| Error(format!("Unknown setting: {key}")))?;
                self.persisted_settings.remove(&key);
                self.settings.set(&key, value)?;
                out.durable_changed = true;
                out.layout_changed = true;
            }
            Intent::ResetSettings => {
                self.persisted_settings.clear();
                self.rebuild_settings()?;
                out.durable_changed = true;
                out.layout_changed = true;
            }
            Intent::SetRail(rail) => {
                return self.dispatch(Intent::SetSetting {
                    key: "rail".into(),
                    value: serde_json::to_value(rail).unwrap(),
                });
            }
            Intent::PrivateWindow => {
                let mut launch = self.default_launch();
                launch.env.extend(self.settings.private_env.clone());
                out.commands.push(HostCommand::NewWindow {
                    private: true,
                    launch,
                });
                return Ok(out);
            }
            Intent::MoveTabToNewWindow(id) => {
                self.require_tab(id)?;
                out.commands.push(HostCommand::MoveTabToNewWindow(id));
                return Ok(out);
            }
            Intent::CustomAction(action) => {
                out.commands.push(HostCommand::CustomAction(action));
                return Ok(out);
            }
        }
        if rebuild_projection {
            self.rebuild_visible();
        }
        self.touch();
        out.model_changed = true;
        Ok(out)
    }

    pub fn default_launch(&self) -> LaunchSpec {
        let selected = self.selected_tab.and_then(|id| self.tabs.get(&id));
        LaunchSpec {
            domain: self.settings.default_domain.clone().or_else(|| {
                selected
                    .filter(|t| !t.domain.is_empty())
                    .map(|t| t.domain.clone())
            }),
            cwd: selected
                .filter(|t| !t.cwd.is_empty())
                .map(|t| t.cwd.clone()),
            ..LaunchSpec::default()
        }
    }
    pub fn set_private(&mut self, private: bool) {
        if self.private != private {
            self.private = private;
            self.reopened.clear();
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
    pub fn restore_tab_state(
        &mut self,
        id: TabId,
        space: &str,
        manual: bool,
        pinned: bool,
    ) -> Result<bool, Error> {
        self.require_tab(id)?;
        if self.require_space(space).is_err() {
            return Ok(false);
        }
        let tab = self.tabs.get_mut(&id).unwrap();
        if tab.space_id == space && tab.manual_assignment == manual && tab.pinned == pinned {
            return Ok(false);
        }
        tab.space_id = space.into();
        tab.manual_assignment = manual;
        tab.pinned = pinned;
        self.rebuild_visible();
        self.touch();
        Ok(true)
    }
    pub fn assign_spawn(&mut self, id: TabId, space: &str) -> Result<(), Error> {
        self.assign_spawn_membership(id, space)?;
        self.activate(id);
        self.touch();
        Ok(())
    }
    pub fn assign_spawn_membership(&mut self, id: TabId, space: &str) -> Result<(), Error> {
        self.require_tab(id)?;
        self.require_space(space)?;
        self.tabs.get_mut(&id).unwrap().space_id = space.into();
        self.route_tab(id);
        self.rebuild_visible();
        self.touch();
        Ok(())
    }
    pub fn apply_route_hook(&mut self, id: TabId, space: Option<String>) -> Result<(), Error> {
        self.require_tab(id)?;
        if space
            .as_deref()
            .is_some_and(|s| s.is_empty() || s.len() > 128 || s.chars().any(char::is_control))
        {
            return Err(Error("Invalid hook space ID".into()));
        }
        self.hook_routes.insert(id, space);
        self.route_tab(id);
        self.rebuild_visible();
        self.touch();
        Ok(())
    }
    pub fn apply_title_hook(&mut self, id: TabId, title: String) -> Result<(), Error> {
        self.require_tab(id)?;
        if title.len() > 4096 || title.chars().any(char::is_control) {
            return Err(Error("Invalid hook title".into()));
        }
        self.tabs.get_mut(&id).unwrap().title_hook = Some(title);
        self.touch();
        Ok(())
    }
    pub fn apply_filter_hook(&mut self, id: TabId, visible: bool) -> Result<(), Error> {
        self.require_tab(id)?;
        if visible {
            self.hidden.remove(&id);
        } else {
            self.hidden.insert(id);
        }
        self.rebuild_visible();
        self.touch();
        Ok(())
    }
    pub fn apply_footer_hook(&mut self, footer: String) -> Result<(), Error> {
        if footer.len() > 4096
            || footer.lines().count() > 16
            || footer
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(Error(
                "Footer must contain at most 16 printable rows and 4096 bytes".into(),
            ));
        }
        self.footer = footer;
        self.touch();
        Ok(())
    }
    pub fn load_catalog(
        &mut self,
        spaces: Vec<Space>,
        templates: Vec<SpaceTemplate>,
    ) -> Result<(), Error> {
        if spaces.is_empty() || spaces.len() > 256 {
            return Err(Error("Catalog needs 1–256 spaces".into()));
        }
        let mut seen = BTreeSet::new();
        for s in &spaces {
            if s.id.is_empty()
                || s.id.len() > 128
                || s.id.chars().any(char::is_control)
                || !seen.insert(s.id.clone())
            {
                return Err(Error("Invalid or duplicate space ID".into()));
            }
            Self::clean_name(s.name.clone())?;
            if s.accent
                .as_deref()
                .is_some_and(|s| !settings::valid_color(s))
            {
                return Err(Error("Invalid space accent".into()));
            }
            routing::validate_rules(&s.rules).map_err(Error)?;
        }
        for t in &templates {
            routing::validate_rules(&t.rules).map_err(Error)?;
        }
        self.spaces = spaces;
        self.templates = templates;
        if !seen.contains(&self.selected_space) {
            self.selected_space = self.spaces[0].id.clone();
        }
        self.reroute();
        self.touch();
        Ok(())
    }
    pub fn load_preferences(&mut self, values: BTreeMap<String, Value>) -> Result<(), Error> {
        for (k, v) in &values {
            settings::validate_value(k, v).map_err(Error)?;
        }
        self.persisted_settings = values;
        self.rebuild_settings()?;
        self.touch();
        Ok(())
    }
    pub fn apply_config(&mut self, values: BTreeMap<String, Value>) -> Result<(), Error> {
        for (k, v) in &values {
            settings::validate_value(k, v).map_err(Error)?;
        }
        self.config_owned = values.keys().cloned().collect();
        self.lua_settings = values;
        self.rebuild_settings()?;
        self.hook_routes.clear();
        self.hidden.clear();
        self.footer.clear();
        for tab in self.tabs.values_mut() {
            tab.title_hook = None;
        }
        self.reroute();
        self.touch();
        Ok(())
    }
    fn rebuild_settings(&mut self) -> Result<(), Error> {
        let mut settings = Settings::default();
        for (k, v) in self
            .persisted_settings
            .iter()
            .chain(self.lua_settings.iter())
        {
            settings.set(k, v.clone()).map_err(Error)?;
        }
        self.settings = settings;
        Ok(())
    }
    fn reroute(&mut self) {
        for id in self.order.clone() {
            if self
                .tabs
                .get(&id)
                .is_some_and(|t| !self.spaces.iter().any(|s| s.id == t.space_id))
            {
                let t = self.tabs.get_mut(&id).unwrap();
                t.manual_assignment = false;
                t.space_id.clear();
            }
            self.route_tab(id);
        }
        self.rebuild_visible();
    }
}
