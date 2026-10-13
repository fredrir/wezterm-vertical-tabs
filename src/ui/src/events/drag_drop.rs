use crate::SidebarUi;
use crate::element::{ElementId, EntityRef};
use crate::events::Press;
use crate::intent::{HostAction, UiIntent};
use crate::runtime::motion;
use crate::views::sidebar::Slot;
use ratatui::layout::Position;
use std::time::Duration;
use vtabs_core::{Intent, Model, SpaceId, TabId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DropTarget {
    /// Beside a tab row or the settings tab row.
    Beside {
        row: ElementId,
        after: bool,
    },
    Into(TabId),
    Folder(String),
    NewTab,
    Space(SpaceId),
}

impl DropTarget {
    pub(crate) fn exists(&self, model: &Model) -> bool {
        match self {
            Self::Beside { row, .. } => row.exists(model),
            Self::Into(tab) => EntityRef::Tab(*tab).exists(model),
            Self::Folder(id) => EntityRef::Folder(id).exists(model),
            Self::Space(id) => EntityRef::Space(id).exists(model),
            Self::NewTab => true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DropMotion {
    pub from: f32,
    pub to: f32,
    pub start: Option<Duration>,
    pub progress: f32,
}

impl DropMotion {
    pub(crate) fn position(&self) -> f32 {
        motion::lerp(self.from, self.to, self.progress)
    }
}

impl SidebarUi {
    pub(crate) fn drop_target(&self, model: &Model, x: u16, y: u16) -> Option<DropTarget> {
        let source = self.pointer.drag.as_ref()?;
        let pane = matches!(source, ElementId::Pane(..));
        let from = match source {
            ElementId::Pane(tab, _) => ElementId::Tab(*tab),
            other => other.row(),
        };
        let Some(hit) = self.hit_test(x, y) else {
            let list = self.sidebar.list.contains(Position::new(x, y));
            return (pane && list).then_some(DropTarget::NewTab);
        };
        let within = (f32::from(y - hit.rect.y) + self.pointer.fraction.1)
            / f32::from(hit.rect.height.max(1));
        let beside = |row| DropTarget::Beside {
            row,
            after: within > 0.5,
        };
        match (&from, hit.id.row()) {
            (ElementId::Tab(tab), row @ ElementId::Tab(target)) if *tab != target => {
                Some(if !(0.3..=0.7).contains(&within) {
                    beside(row)
                } else {
                    DropTarget::Into(target)
                })
            }
            (ElementId::Tab(_), row @ ElementId::SettingsTab) if !pane => Some(beside(row)),
            (ElementId::SettingsTab, row @ ElementId::Tab(target))
                if model.tabs.get(&target).is_some_and(|tab| !tab.pinned) =>
            {
                Some(beside(row))
            }
            (ElementId::Tab(_) | ElementId::SettingsTab, ElementId::NewTab) => {
                Some(DropTarget::NewTab)
            }
            (ElementId::Tab(_), ElementId::Folder(id)) if !pane => Some(DropTarget::Folder(id)),
            (ElementId::Tab(_), ElementId::Space(id)) if !pane => Some(DropTarget::Space(id)),
            (ElementId::Folder(id), ElementId::Folder(target)) if *id != target => {
                Some(DropTarget::Folder(target))
            }
            (ElementId::Space(id), ElementId::Space(target)) if *id != target => {
                Some(DropTarget::Space(target))
            }
            _ => None,
        }
        .filter(|target| target.exists(model))
    }

    pub(crate) fn aim_drop(&mut self, model: &Model, x: u16, y: u16) {
        let target = self.drop_target(model, x, y);
        if target == self.pointer.drop {
            return;
        }
        let edge = |target: &DropTarget, ui: &Self| match target {
            DropTarget::Beside { row, after } => ui.paint.hit(row).map(|hit| {
                f32::from(if *after {
                    hit.rect.bottom()
                } else {
                    hit.rect.y
                })
            }),
            _ => None,
        };
        let to = target.as_ref().and_then(|target| edge(target, self));
        let from = self
            .pointer
            .drop_motion
            .filter(|_| matches!(self.pointer.drop, Some(DropTarget::Beside { .. })))
            .map(|motion| motion.position());
        self.pointer.drop_motion = target.as_ref().map(|_| DropMotion {
            from: from.or(to).unwrap_or(0.0),
            to: to.unwrap_or(0.0),
            start: None,
            progress: 0.0,
        });
        self.pointer.drop = target;
        self.frame.dirty = true;
    }

    pub(crate) fn apply_drop(
        &mut self,
        model: &Model,
        source: ElementId,
        target: DropTarget,
        intents: &mut Vec<UiIntent>,
    ) {
        if !source.exists(model) || !target.exists(model) {
            return;
        }
        match (source, target) {
            (ElementId::Pane(tab, pane), DropTarget::Into(target)) => {
                intents.push(UiIntent::Host(HostAction::JoinPane { pane, tab: target }));
                self.settle(ElementId::Tab(if tab == target { tab } else { target }));
            }
            (
                ElementId::Pane(tab, pane),
                DropTarget::Beside {
                    row: ElementId::Tab(beside),
                    after,
                },
            ) => {
                let index = model
                    .visible_ids()
                    .iter()
                    .position(|id| *id == beside)
                    .map(|at| at + usize::from(after));
                intents.push(UiIntent::Host(HostAction::DetachPane { tab, pane, index }));
            }
            (ElementId::Pane(tab, pane), DropTarget::NewTab) => {
                intents.push(UiIntent::Host(HostAction::DetachPane {
                    tab,
                    pane,
                    index: None,
                }));
            }
            (source, target) => match (source.row(), target) {
                (ElementId::Tab(id), DropTarget::Into(tab)) => {
                    intents.push(UiIntent::Host(HostAction::JoinTab { source: id, tab }));
                    self.settle(ElementId::Tab(tab));
                }
                (ElementId::Tab(id), DropTarget::Beside { row, after }) => {
                    let Some(from) = model.tabs.get(&id) else {
                        return;
                    };
                    // The settings tab only sits among unpinned tabs outside folders.
                    let (folder_id, pinned) = match &row {
                        ElementId::Tab(beside) => match model.tabs.get(beside) {
                            Some(beside) => (beside.folder_id.clone(), beside.pinned),
                            None => return,
                        },
                        _ => (None, false),
                    };
                    if from.folder_id != folder_id {
                        intents.push(UiIntent::Domain(Intent::AssignFolder {
                            tab_id: id,
                            folder_id,
                        }));
                    }
                    if from.pinned != pinned {
                        intents.push(UiIntent::Domain(Intent::PinTab { id, pinned }));
                    }
                    let slots = self.rearrange(model, Slot::Tab(id), &row, after);
                    if let Some(index) = slots
                        .iter()
                        .filter(|slot| matches!(slot, Slot::Tab(_)))
                        .position(|slot| *slot == Slot::Tab(id))
                    {
                        intents.push(UiIntent::Domain(Intent::MoveTab { id, index }));
                    }
                    self.sidebar.settle_settings(&slots);
                    self.settle(ElementId::Tab(id));
                }
                (ElementId::SettingsTab, DropTarget::Beside { row, after }) => {
                    let slots = self.rearrange(model, Slot::Settings, &row, after);
                    self.sidebar.settle_settings(&slots);
                    self.settle(ElementId::SettingsTab);
                }
                (ElementId::SettingsTab, DropTarget::NewTab) => {
                    self.move_settings(model, 0);
                    self.settle(ElementId::SettingsTab);
                }
                (ElementId::Tab(tab_id), DropTarget::Folder(folder_id)) => {
                    intents.push(UiIntent::Domain(Intent::AssignFolder {
                        tab_id,
                        folder_id: Some(folder_id),
                    }));
                    self.settle(ElementId::Tab(tab_id));
                }
                (ElementId::Tab(id), DropTarget::NewTab) => {
                    intents.push(UiIntent::Domain(Intent::AssignFolder {
                        tab_id: id,
                        folder_id: None,
                    }));
                    intents.push(UiIntent::Domain(Intent::PinTab { id, pinned: false }));
                    self.settle(ElementId::Tab(id));
                }
                (ElementId::Tab(id), DropTarget::Space(space_id)) => {
                    intents.push(UiIntent::Domain(Intent::AssignTab { id, space_id }));
                }
                (ElementId::Folder(id), DropTarget::Folder(target)) => {
                    if let Some(index) = model.selected_folders().position(|f| f.id == target) {
                        intents.push(UiIntent::Domain(Intent::MoveFolder { id, index }));
                    }
                }
                (ElementId::Space(id), DropTarget::Space(target)) => {
                    if let Some(index) = model.spaces.iter().position(|space| space.id == target) {
                        intents.push(UiIntent::Domain(Intent::MoveSpace { id, index }));
                    }
                }
                _ => {}
            },
        }
    }

    /// Sidebar order once `source` sits beside `row`.
    fn rearrange(
        &mut self,
        model: &Model,
        source: Slot,
        row: &ElementId,
        after: bool,
    ) -> Vec<Slot> {
        let target = match row {
            ElementId::Tab(id) => Slot::Tab(*id),
            _ => Slot::Settings,
        };
        let mut slots = self.sidebar.slots(model, self.settings.listed);
        slots.retain(|slot| *slot != source);
        if let Some(at) = slots.iter().position(|slot| *slot == target) {
            slots.insert(at + usize::from(after), source);
        }
        slots
    }

    fn settle(&mut self, id: ElementId) {
        self.pointer.press = Some(Press {
            id,
            down: None,
            up: Some(None),
            level: 0.0,
            animating: true,
        });
    }
}
