use vtabs_core::{PaneId, SpaceId, TabId};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ElementId {
    SpaceTitle,
    Search,
    Refresh,
    CreateFolder,
    Folder(String),
    SettingsCategory(String),
    SettingsSearch,
    CloseSettings,
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
    Setting(String),
    Editor,
    Submit,
    Cancel,
}

impl ElementId {
    pub(crate) fn row(&self) -> ElementId {
        match self {
            Self::CloseTab(id) | Self::Pane(id, _) | Self::ClosePane(id, _) => Self::Tab(*id),
            Self::CloseSettingsTab => Self::SettingsTab,
            Self::CreateFolder => Self::SpaceTitle,
            other => other.clone(),
        }
    }
    pub(crate) fn on_settings_page(&self) -> bool {
        matches!(
            self,
            Self::SettingsCategory(_)
                | Self::SettingsSearch
                | Self::CloseSettings
                | Self::ResetSettings
                | Self::Setting(_)
        )
    }
}
