use crate::actions::Action;
use crate::components::scrollbar::Scrollbar;
use crate::element::ElementId;
use crate::input::{Key, Modifiers, TextEditor};
use crate::intent::{HostAction, UiIntent};
use crate::overlays::{Menu, MenuItem, Overlay};
use crate::views::{tab_machine, tab_name};
use crate::{SidebarUi, icons};
use vtabs_core::{Intent, Model, Tab, TabId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LauncherKind {
    Tabs,
    Commands,
    Keybind,
    LaunchMenu,
}

/// The host supplies its matching and label rules to preserve native launcher behavior.
#[derive(Clone, Debug)]
pub struct LaunchMenuConfig {
    pub title: String,
    pub help_text: String,
    pub fuzzy_help_text: String,
    pub alphabet: String,
    pub fuzzy: bool,
    pub labels: fn(&str, usize) -> Vec<String>,
    pub matches: fn(&str, &str) -> Option<u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct LaunchMenu {
    pub config: LaunchMenuConfig,
    pub filtering: bool,
    pub selection: String,
    pub labels: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Launcher {
    pub kind: LauncherKind,
    pub editor: TextEditor,
    pub all_items: Vec<MenuItem>,
    pub empty: &'static str,
    pub scrollbar: Option<Scrollbar>,
    pub recording: bool,
    pub launch_menu: Option<Box<LaunchMenu>>,
}

impl Launcher {
    pub fn edits_query(&self) -> bool {
        !self.recording
            && self.kind != LauncherKind::Keybind
            && self.launch_menu.as_ref().is_none_or(|menu| menu.filtering)
    }
}

#[derive(Default)]
pub(crate) struct Launchers {
    pub foreign_tabs: Vec<ForeignTab>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignTab {
    pub window: u64,
    pub id: TabId,
    pub label: String,
    pub title: String,
    pub place: String,
    pub remote: bool,
    pub os: String,
}

impl ForeignTab {
    pub fn new(window: u64, tab: &Tab, home: Option<&str>, place: impl Into<String>) -> Self {
        Self {
            window,
            id: tab.id,
            label: tab_name(tab, home).unwrap_or_else(|| tab.title.clone()),
            title: tab.title.clone(),
            place: place.into(),
            remote: tab.remote,
            os: tab.os.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandEntry {
    pub label: String,
    pub shortcuts: Vec<String>,
    pub description: String,
}

impl SidebarUi {
    pub fn open_launch_menu(
        &mut self,
        config: LaunchMenuConfig,
        commands: Vec<CommandEntry>,
        selected: usize,
    ) {
        let items = commands
            .into_iter()
            .enumerate()
            .map(|(id, command)| {
                MenuItem::new(
                    format!("command/{id}"),
                    command.label,
                    Action::Host(HostAction::RunCommand(id)),
                )
            })
            .collect();
        self.open_launcher(
            LauncherKind::LaunchMenu,
            &config.title,
            "No launcher entries",
            items,
            selected,
        );
        if let Some(Overlay::Menu(menu)) = &mut self.overlays.current {
            menu.selected = menu.selected.min(menu.items.len().saturating_sub(1));
            menu.search.as_mut().unwrap().launch_menu = Some(Box::new(LaunchMenu {
                filtering: config.fuzzy,
                config,
                selection: String::new(),
                labels: Vec::new(),
            }));
        }
    }

    pub(crate) fn launch_menu_key(
        &mut self,
        model: &Model,
        key: &Key,
        modifiers: Modifiers,
        intents: &mut Vec<UiIntent>,
    ) -> bool {
        let Some(Overlay::Menu(menu)) = &mut self.overlays.current else {
            return false;
        };
        let Some(search) = &mut menu.search else {
            return false;
        };
        let Some(launcher) = &mut search.launch_menu else {
            return false;
        };
        if modifiers.command() || modifiers.alt {
            return false;
        }
        if launcher.filtering {
            if *key == Key::Backspace && search.editor.text().is_empty() && !launcher.config.fuzzy {
                launcher.filtering = false;
                self.frame.dirty = true;
                return true;
            }
            return false;
        }
        match key {
            Key::Character(c) if launcher.config.alphabet.contains(*c) => {
                launcher.selection.push(*c);
                let action = launcher
                    .labels
                    .iter()
                    .position(|label| *label == launcher.selection)
                    .and_then(|index| menu.items.get(menu.scroll + index))
                    .map(|item| item.action.clone());
                if !launcher
                    .labels
                    .iter()
                    .any(|label| label.starts_with(&launcher.selection))
                {
                    launcher.selection.clear();
                }
                if let Some(action) = action {
                    self.run_action(model, action, intents);
                }
            }
            Key::Character('/') => {
                launcher.filtering = true;
                launcher.selection.clear();
            }
            Key::Character('j' | 'k') => {
                menu.selected = crate::overlays::next_enabled(
                    &menu.items,
                    menu.selected,
                    if *key == Key::Character('j') { 1 } else { -1 },
                );
                launcher.selection.clear();
            }
            Key::Backspace => {
                launcher.selection.pop();
            }
            Key::Character(_) => {}
            _ => return false,
        }
        self.frame.dirty = true;
        true
    }

    pub fn open_commands(&mut self, commands: Vec<CommandEntry>) {
        let mut items: Vec<MenuItem> = commands
            .into_iter()
            .enumerate()
            .map(|(id, command)| {
                let mut item = MenuItem::new(
                    format!("command/{id}"),
                    command.label,
                    Action::Host(HostAction::RunCommand(id)),
                );
                item.hint = command.shortcuts.first().cloned().unwrap_or_default();
                item.shortcuts = command.shortcuts;
                item.keywords = command.description;
                item
            })
            .collect();
        items.sort_by_key(|item| (item.hint.is_empty(), item.label.to_lowercase()));
        self.open_launcher(
            LauncherKind::Commands,
            "Search commands",
            "No commands available",
            items,
            0,
        );
    }

    pub fn recording_keybind(&self) -> bool {
        matches!(&self.overlays.current, Some(Overlay::Menu(menu)) if menu.search.as_ref().is_some_and(|s| s.kind == LauncherKind::Keybind && s.recording))
    }

    pub fn recording_shortcut(&self) -> bool {
        matches!(&self.overlays.current, Some(Overlay::Menu(menu))
            if menu.search.as_ref().is_some_and(|search| search.recording))
    }

    pub(crate) fn toggle_shortcut_recording(&mut self) {
        if let Some(Overlay::Menu(menu)) = &mut self.overlays.current
            && let Some(search) = &mut menu.search
            && matches!(search.kind, LauncherKind::Commands | LauncherKind::Keybind)
        {
            search.recording = !search.recording;
            if search.recording {
                search.editor = TextEditor::default();
            }
            filter_menu(menu);
            self.focused = Some(ElementId::Editor);
            self.frame.dirty = true;
        }
    }

    pub(crate) fn record_shortcut(&mut self, shortcut: String) {
        if let Some(Overlay::Menu(menu)) = &mut self.overlays.current
            && let Some(search) = &mut menu.search
            && search.recording
        {
            if shortcut.is_empty() {
                // Keep the captured chord when leaving recording mode.
                search.recording = false;
            } else {
                search.editor = TextEditor::new(shortcut);
            }
            filter_menu(menu);
            self.frame.dirty = true;
        }
    }

    pub(crate) fn open_launcher(
        &mut self,
        kind: LauncherKind,
        title: &str,
        empty: &'static str,
        items: Vec<MenuItem>,
        selected: usize,
    ) {
        self.dismiss();
        let search = Some(Launcher {
            kind,
            editor: TextEditor::default(),
            all_items: items.clone(),
            empty,
            scrollbar: None,
            recording: false,
            launch_menu: None,
        });
        self.open_overlay(Overlay::Menu(Menu {
            selected,
            search,
            ..Menu::new(title, items)
        }));
    }

    pub fn open_tab_navigator(&mut self, model: &Model) {
        let home = model.home.as_deref();
        let row = |tab: &Tab| {
            let name = tab_name(tab, home);
            let mut item = MenuItem::new(
                format!("tab/{}", tab.id),
                name.as_deref().unwrap_or(&tab.title),
                Action::Domain(Intent::ActivateTab(tab.id)),
            );
            let (remote, os) = tab_machine(tab);
            item.icon = icons::host(remote, os);
            item.icon_color = self.theme.host(remote, os);
            if name.is_some_and(|name| name != tab.title) {
                item.keywords = tab.title.clone();
            }
            item
        };
        let mut items: Vec<_> = model
            .visible_ids()
            .iter()
            .filter_map(|id| model.tabs.get(id))
            .enumerate()
            .map(|(index, tab)| {
                let mut item = row(tab);
                item.index = Some(index + 1);
                item.hint = item.keywords.clone();
                item
            })
            .collect();
        let current = model.selected_space.as_str();
        for space in model.spaces.iter() {
            for id in model.space_tabs(&space.id) {
                let hidden = model.is_hidden(id);
                if space.id == current && !hidden {
                    continue;
                }
                let mut item = row(&model.tabs[&id]);
                item.hint = if hidden {
                    format!("{}  hidden", space.name)
                } else {
                    space.name.clone()
                };
                items.push(item);
            }
        }
        items.extend(self.launchers.foreign_tabs.iter().map(|tab| {
            let mut item = MenuItem::new(
                format!("window/{}/tab/{}", tab.window, tab.id),
                &tab.label,
                Action::Host(HostAction::ShowTab {
                    window: tab.window,
                    tab: tab.id,
                }),
            );
            item.icon = icons::host(tab.remote, &tab.os);
            item.icon_color = self.theme.host(tab.remote, &tab.os);
            item.hint = tab.place.clone();
            if tab.label != tab.title {
                item.keywords = tab.title.clone();
            }
            item
        }));
        if model.settings.rail == vtabs_core::RailMode::Hidden {
            let mut item = MenuItem::new(
                "sidebar/expand",
                "Show sidebar",
                Action::Domain(Intent::SetRail(vtabs_core::RailMode::Expanded)),
            );
            item.icon = icons::SIDEBAR;
            items.push(item);
        }
        let selected = model
            .selected_tab
            .and_then(|id| model.visible_ids().iter().position(|tab| *tab == id))
            .unwrap_or(0);
        self.open_launcher(
            LauncherKind::Tabs,
            "Search tabs",
            "No matching tabs",
            items,
            selected,
        );
    }
}

pub(crate) fn filter_menu(menu: &mut Menu) {
    let Some(search) = &mut menu.search else {
        return;
    };
    if search.kind == LauncherKind::Keybind {
        menu.items = search.all_items.clone();
        if let Some(save) = menu.items.first_mut() {
            save.enabled = !search.editor.text().is_empty();
        }
        return;
    }
    search.scrollbar = None;
    if let Some(launcher) = &search.launch_menu {
        let query = search.editor.text();
        let mut matches: Vec<_> = search
            .all_items
            .iter()
            .filter_map(|item| {
                (launcher.config.matches)(query, &item.label).map(|score| (score, item.clone()))
            })
            .collect();
        if !query.is_empty() {
            matches.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        }
        menu.items = matches.into_iter().map(|(_, item)| item).collect();
        menu.selected = 0;
        menu.scroll = 0;
        return;
    }
    let query = search.editor.text().to_lowercase();
    let selected_id = menu.items.get(menu.selected).map(|item| item.id.clone());
    menu.items.clear();
    menu.items.extend(
        search
            .all_items
            .iter()
            .filter(|item| {
                if search.recording {
                    return query.is_empty()
                        || item
                            .shortcuts
                            .iter()
                            .any(|shortcut| shortcut.to_lowercase() == query);
                }
                item.label.to_lowercase().contains(&query)
                    || item.hint.to_lowercase().contains(&query)
                    || item
                        .shortcuts
                        .iter()
                        .any(|shortcut| shortcut.to_lowercase().contains(&query))
                    || item.keywords.to_lowercase().contains(&query)
                    || item.index.is_some_and(|index| index.to_string() == query)
            })
            .cloned()
            .map(|mut item| {
                if search.recording
                    && let Some(shortcut) = item
                        .shortcuts
                        .iter()
                        .find(|shortcut| shortcut.to_lowercase() == query)
                {
                    item.hint = shortcut.clone();
                }
                item
            }),
    );
    menu.selected = selected_id
        .and_then(|id| menu.items.iter().position(|item| item.id == id))
        .unwrap_or(0);
    menu.scroll = 0;
}
