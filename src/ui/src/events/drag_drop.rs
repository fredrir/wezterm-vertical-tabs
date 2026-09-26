use crate::SidebarUi;
use crate::element::ElementId;
use crate::events::Press;
use crate::intent::{HostAction, UiIntent};
use crate::runtime::motion;
use ratatui::layout::Position;
use std::time::Duration;
use vtabs_core::{Intent, Model, SpaceId, TabId};

/// Where a drag would land, resolved on every pointer move so the preview never lies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DropTarget {
    /// Reorder next to a tab; a dragged pane becomes a tab of its own there.
    Beside {
        tab: TabId,
        after: bool,
    },
    /// Become a split of this tab.
    Into(TabId),
    Folder(String),
    /// Leave folders and pins behind; a dragged pane becomes the last tab.
    NewTab,
    Space(SpaceId),
}

/// The drop preview glides between targets instead of jumping.
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
    /// Rows are two cells tall at most, so the pointer's place inside its cell decides
    /// between landing beside a tab and landing inside it.
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
        match (&from, hit.id.row()) {
            (ElementId::Tab(tab), ElementId::Tab(target)) if *tab != target => {
                let within = (f32::from(y - hit.rect.y) + self.pointer.fraction.1)
                    / f32::from(hit.rect.height.max(1));
                Some(if !(0.3..=0.7).contains(&within) {
                    DropTarget::Beside {
                        tab: target,
                        after: within > 0.5,
                    }
                } else {
                    DropTarget::Into(target)
                })
            }
            (ElementId::Tab(_), ElementId::NewTab) => Some(DropTarget::NewTab),
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
        .filter(|target| match target {
            DropTarget::Beside { tab, .. } | DropTarget::Into(tab) => model.tabs.contains_key(tab),
            _ => true,
        })
    }

    pub(crate) fn aim_drop(&mut self, model: &Model, x: u16, y: u16) {
        let target = self.drop_target(model, x, y);
        if target == self.pointer.drop {
            return;
        }
        // The insertion bar glides to its new boundary; other previews pop in place.
        let edge = |target: &DropTarget, ui: &Self| match target {
            DropTarget::Beside { tab, after } => ui.paint.hit(&ElementId::Tab(*tab)).map(|hit| {
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
        let place = |tab: TabId, after: bool, from: Option<TabId>| {
            let visible = model.visible_ids();
            let at = visible.iter().position(|id| *id == tab)? + usize::from(after);
            // Core removes the dragged tab before inserting it again.
            let above = from
                .and_then(|from| visible.iter().position(|id| *id == from))
                .is_some_and(|from| from < at);
            Some(at - usize::from(above))
        };
        match (source, target) {
            (ElementId::Pane(tab, pane), DropTarget::Into(target)) => {
                intents.push(UiIntent::Host(HostAction::JoinPane { pane, tab: target }));
                self.settle(ElementId::Tab(if tab == target { tab } else { target }));
            }
            (ElementId::Pane(tab, pane), DropTarget::Beside { tab: beside, after }) => {
                intents.push(UiIntent::Host(HostAction::DetachPane {
                    tab,
                    pane,
                    index: place(beside, after, None),
                }));
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
                (ElementId::Tab(id), DropTarget::Beside { tab, after }) => {
                    let (Some(from), Some(beside)) = (model.tabs.get(&id), model.tabs.get(&tab))
                    else {
                        return;
                    };
                    if from.folder_id != beside.folder_id {
                        intents.push(UiIntent::Domain(Intent::AssignFolder {
                            tab_id: id,
                            folder_id: beside.folder_id.clone(),
                        }));
                    }
                    if from.pinned != beside.pinned {
                        intents.push(UiIntent::Domain(Intent::PinTab {
                            id,
                            pinned: beside.pinned,
                        }));
                    }
                    if let Some(index) = place(tab, after, Some(id)) {
                        intents.push(UiIntent::Domain(Intent::MoveTab { id, index }));
                    }
                    self.settle(ElementId::Tab(id));
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

    /// A landed row gives the same quick squeeze as a press, confirming where it went.
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
