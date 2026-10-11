use vtabs_core::{PaneId, SpaceId, TabId};

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
    RecordShortcut,
    Scrollbar(ScrollArea),
    Submit,
    Cancel,
}

impl ElementId {
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
