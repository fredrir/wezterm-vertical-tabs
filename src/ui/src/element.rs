use vtabs_core::{Model, PaneId, SpaceId, TabId};

/// A model entity referenced by an interaction, independent of its visual control.
#[derive(Clone, Copy, Debug)]
pub(crate) enum EntityRef<'a> {
    Tab(TabId),
    Pane(TabId, PaneId),
    Space(&'a str),
    Folder(&'a str),
}

impl EntityRef<'_> {
    pub(crate) fn exists(self, model: &Model) -> bool {
        match self {
            Self::Tab(id) => model.tabs.contains_key(&id),
            Self::Pane(tab, pane) => model
                .tabs
                .get(&tab)
                .is_some_and(|tab| tab.panes.iter().any(|entry| entry.id == pane)),
            Self::Space(id) => model.spaces.iter().any(|space| space.id == id),
            Self::Folder(id) => model.folders.iter().any(|folder| folder.id == id),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollArea {
    Launcher,
    Settings,
    Sidebar,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ElementId {
    SpaceTitle,
    Search,
    Refresh,
    CreateFolder,
    Folder(String),
    SettingsCategory(String),
    SettingsSearch,
    SettingsRecord,
    ResetSettings,
    CreateSpace,
    NewTab,
    Settings,
    SettingsTab,
    CloseSettingsTab,
    Rail,
    Space(SpaceId),
    Tab(TabId),
    Pane(TabId, PaneId),
    ClosePane(TabId, PaneId),
    CloseTab(TabId),
    Menu(String),
    MenuRemove(String),
    Setting(String),
    Slider(String),
    SettingsEntry,
    Editor,
    ColorPlane,
    ColorHue,
    ColorSwatch(usize),
    ColorFormat,
    ColorReset,
    ColorOriginal,
    RecordShortcut,
    Scrollbar(ScrollArea),
    Submit,
    Cancel,
}

impl ElementId {
    pub(crate) fn entity(&self) -> Option<EntityRef<'_>> {
        match self {
            Self::Tab(id) | Self::CloseTab(id) => Some(EntityRef::Tab(*id)),
            Self::Pane(tab, pane) | Self::ClosePane(tab, pane) => {
                Some(EntityRef::Pane(*tab, *pane))
            }
            Self::Space(id) => Some(EntityRef::Space(id)),
            Self::Folder(id) => Some(EntityRef::Folder(id)),
            _ => None,
        }
    }

    pub(crate) fn exists(&self, model: &Model) -> bool {
        self.entity().is_none_or(|entity| entity.exists(model))
    }

    pub(crate) fn row(&self) -> ElementId {
        match self {
            Self::CloseTab(id) | Self::Pane(id, _) | Self::ClosePane(id, _) => Self::Tab(*id),
            Self::CloseSettingsTab => Self::SettingsTab,
            Self::MenuRemove(id) => Self::Menu(id.clone()),
            Self::CreateFolder => Self::SpaceTitle,
            other => other.clone(),
        }
    }
    pub(crate) fn on_settings_page(&self) -> bool {
        matches!(
            self,
            Self::SettingsCategory(_)
                | Self::SettingsSearch
                | Self::SettingsEntry
                | Self::SettingsRecord
                | Self::ResetSettings
                | Self::Scrollbar(ScrollArea::Settings)
        ) || self.setting().is_some()
    }
    /// The setting a row, or a control inside it, belongs to.
    pub(crate) fn setting(&self) -> Option<&str> {
        match self {
            Self::Setting(key) | Self::Slider(key) => Some(key),
            _ => None,
        }
    }
}
