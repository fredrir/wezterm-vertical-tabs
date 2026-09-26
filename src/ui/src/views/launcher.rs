use crate::actions::Action;
use crate::input::TextEditor;
use crate::intent::HostAction;
use crate::overlays::{Menu, MenuItem, Overlay};
use crate::views::{tab_machine, tab_name};
use crate::{SidebarUi, icons};
use vtabs_core::jobs::{JobOperation, JobTarget};
use vtabs_core::{Intent, Model, Tab, TabId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LauncherKind {
    Tabs,
    Jobs,
}

#[derive(Clone, Debug)]
pub(crate) struct Launcher {
    pub kind: LauncherKind,
    pub editor: TextEditor,
    pub all_items: Vec<MenuItem>,
    pub empty: &'static str,
}

#[derive(Default)]
pub(crate) struct Launchers {
    pub foreign_tabs: Vec<ForeignTab>,
    pub jobs: Vec<JobEntry>,
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
pub struct JobEntry {
    pub target: JobTarget,
    pub command: String,
    pub place: String,
    pub suspended: bool,
    pub ready: bool,
}

impl SidebarUi {
    fn open_launcher(
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
        });
        self.open_overlay(Overlay::Menu(Menu {
            selected,
            search,
            ..Menu::new(title, items)
        }));
    }

    pub fn jobs_open(&self) -> bool {
        self.overlays.current.iter().chain(self.overlays.stack.iter()).any(|overlay| {
            matches!(overlay, Overlay::Menu(menu) if menu.search.as_ref().is_some_and(|search| search.kind == LauncherKind::Jobs))
        })
    }

    pub fn open_jobs(&mut self, jobs: Vec<JobEntry>) {
        self.launchers.jobs = jobs;
        let items = self.job_items();
        self.open_launcher(
            LauncherKind::Jobs,
            "Search jobs",
            "No running or suspended jobs",
            items,
            0,
        );
    }

    pub fn set_jobs(&mut self, jobs: Vec<JobEntry>) {
        if self.launchers.jobs == jobs {
            return;
        }
        self.launchers.jobs = jobs;
        let items = self.job_items();
        for overlay in self
            .overlays
            .current
            .iter_mut()
            .chain(self.overlays.stack.iter_mut())
        {
            if let Overlay::Menu(menu) = overlay
                && let Some(search) = &mut menu.search
                && search.kind == LauncherKind::Jobs
            {
                search.all_items = items.clone();
                filter_menu(menu);
                self.frame.dirty = true;
            }
        }
    }

    fn job_items(&self) -> Vec<MenuItem> {
        self.launchers
            .jobs
            .iter()
            .map(|job| {
                let action = |operation| Action::Host(HostAction::Job(job.target, operation));
                let mut item = MenuItem::new(
                    format!(
                        "job/{}/{}/{}/{}",
                        job.target.pane, job.target.shell, job.target.number, job.target.pid
                    ),
                    &job.command,
                    action(JobOperation::Foreground),
                );
                item.icon = icons::COMMAND;
                item.hint = format!(
                    "{}{} · {}",
                    if job.suspended {
                        "suspended"
                    } else {
                        "running"
                    },
                    if job.ready { "" } else { " · shell busy" },
                    job.place
                );
                item.keywords = format!("%{} {}", job.target.number, job.target.pid);
                item.enabled = job.ready;
                item.actions = vec![
                    MenuItem::new(
                        "fg",
                        "Bring to foreground",
                        action(JobOperation::Foreground),
                    ),
                    MenuItem::new(
                        "bg",
                        "Resume in background",
                        action(JobOperation::Background),
                    ),
                    MenuItem::new(
                        "terminate",
                        "Terminate",
                        Action::Confirm {
                            label: format!("Terminate {}?", job.command),
                            action: Box::new(action(JobOperation::Terminate)),
                        },
                    ),
                ];
                item.actions[1].enabled = job.suspended;
                for action in &mut item.actions {
                    action.enabled &= job.ready;
                }
                item
            })
            .collect()
    }

    pub(crate) fn launcher_actions(&mut self, id: Option<&str>) -> bool {
        let Some(Overlay::Menu(menu)) = &self.overlays.current else {
            return false;
        };
        let item = match id {
            Some(id) => menu.items.iter().find(|item| item.id == id),
            None => menu.items.get(menu.selected),
        };
        let Some(item) = item.filter(|item| !item.actions.is_empty()) else {
            return false;
        };
        let (title, actions) = (item.label.clone(), item.actions.clone());
        self.push_menu(title, actions);
        true
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
                    format!("{} · hidden", space.name)
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
    let Some(search) = &menu.search else {
        return;
    };
    let query = search.editor.text().to_lowercase();
    let selected_id = menu.items.get(menu.selected).map(|item| item.id.clone());
    menu.items.clear();
    menu.items.extend(
        search
            .all_items
            .iter()
            .filter(|item| {
                item.label.to_lowercase().contains(&query)
                    || item.hint.to_lowercase().contains(&query)
                    || item.keywords.to_lowercase().contains(&query)
                    || item.index.is_some_and(|index| index.to_string() == query)
            })
            .cloned(),
    );
    menu.selected = selected_id
        .and_then(|id| menu.items.iter().position(|item| item.id == id))
        .unwrap_or(0);
    menu.scroll = 0;
}

#[cfg(test)]
#[path = "../../tests/views/launcher.rs"]
mod tests;
