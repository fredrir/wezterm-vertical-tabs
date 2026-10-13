//! Window-owned surfaces. Product state lives in the registered provider.
use crate::termwindow::{TabInformation, TermWindow, TermWindowNotif};
use mux::{tab::TabId, Mux, MuxNotification};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

mod chrome;
mod contract;
mod endpoint;
mod geometry;
pub mod launcher;
mod render;
mod routing;
mod scheduler;

pub use contract::*;
pub(crate) use endpoint::WindowEndpoint;
pub use geometry::{Bounds, Geometry, Reservation};

thread_local! {
    static ARRIVALS: RefCell<HashMap<usize, Vec<Message>>> = Default::default();
}
pub fn spawn_completed(window: usize, tab: usize, context: SpawnContext) {
    ARRIVALS.with(|arrivals| {
        arrivals
            .borrow_mut()
            .entry(window)
            .or_default()
            .push(Message::Spawned {
                tab: tab as u64,
                context,
            })
    });
}
pub fn spawn_failed(window: Option<usize>, context: Option<SpawnContext>) {
    let (Some(window), Some(context)) = (window, context) else {
        return;
    };
    if let Some(gui) = crate::frontend::front_end().gui_window_for_mux_window(window) {
        WindowEndpoint::new(gui.window, window).send(Message::SpawnFailed(context));
    }
}

pub struct UiHost {
    pub(super) terminal: super::terminal_overlay::TerminalOverlay,
    pub provider: Box<dyn Provider>,
    pub projection: Projection,
    pub dirty: bool,
    needs_commit: bool,
    revision: u64,
    actual_active: Option<TabId>,
    known: HashSet<TabId>,
    bells: HashSet<TabId>,
    bound: bool,
    initialized: bool,
    render: render::RenderCache,
    chrome: chrome::Chrome,
    geometry: Geometry,
    deadline: scheduler::ProviderDeadline,
}
impl UiHost {
    pub fn new(window_id: usize) -> Self {
        let mut host = Self {
            terminal: Default::default(),
            provider: crate::vtabs::create(window_id),
            projection: Projection::default(),
            dirty: true,
            needs_commit: true,
            revision: 0,
            actual_active: None,
            known: HashSet::new(),
            bells: HashSet::new(),
            bound: false,
            initialized: false,
            render: Default::default(),
            chrome: Default::default(),
            geometry: Geometry::default(),
            deadline: Default::default(),
        };
        ARRIVALS.with(|arrivals| {
            if let Some(arrivals) = arrivals.borrow().get(&window_id) {
                host.initialize(arrivals);
            }
        });
        host
    }
    pub async fn initialized(window_id: usize) -> Self {
        let mut host = Self::new(window_id);
        host.provider.initialize().await;
        host
    }
    fn initialize(&mut self, arrivals: &[Message]) {
        for arrival in arrivals {
            if let Message::Spawned { tab, context } = arrival {
                if context.new_window {
                    self.provider.message(Message::Initialize {
                        tab: *tab,
                        context: context.clone(),
                    });
                    self.initialized = true;
                }
            }
        }
    }
    pub fn shutdown(mut self) {
        self.deadline.cancel();
        self.provider.suspend();
        let activity = mux::activity::Activity::new();
        promise::spawn::spawn(async move {
            self.provider.shutdown().await;
            drop(activity);
        })
        .detach();
    }
    fn commit(&mut self) {
        let next = self.provider.projection();
        let mut seen = HashSet::new();
        if next
            .tabs
            .iter()
            .all(|id| self.known.contains(id) && seen.insert(*id))
            && next.active.map_or(true, |id| seen.contains(&id))
        {
            self.projection = next;
        } else {
            log::error!("UI rejected invalid tab projection");
        }
    }
}

impl TermWindow {
    fn vtabs_host_mut(&mut self, window_id: usize) -> Option<&mut UiHost> {
        if window_id == self.mux_window_id {
            self.ui_host.as_mut()
        } else {
            self.vtabs_suspended.get_mut(&window_id)
        }
    }
    pub fn vtabs_begin_paint(&mut self) {
        self.vtabs_sync();
        self.resize_terminal_overlay();
        self.update_vtabs_drag_exclusion();
        self.update_vtabs_title_buttons();
    }
    pub fn vtabs_empty(&self) -> bool {
        self.ui_host
            .as_ref()
            .map_or(false, |ui| !ui.dirty && ui.projection.active.is_none())
    }
    pub fn vtabs_content_hidden(&self) -> bool {
        self.ui_host
            .as_ref()
            .is_some_and(|ui| ui.provider.content_page())
            || (self.vtabs_empty()
                && !Mux::get()
                    .get_active_tab_for_window(self.mux_window_id)
                    .map_or(false, |tab| self.tab_state(tab.tab_id()).overlay.is_some()))
    }
    pub fn vtabs_bell(&mut self, pane_id: usize) {
        if let Some((_, window, tab)) = Mux::get().resolve_pane_id(pane_id) {
            if window == self.mux_window_id {
                if let Some(ui) = self.ui_host.as_mut() {
                    ui.bells.insert(tab);
                    ui.dirty = true;
                }
            }
        }
    }
    pub fn vtabs_mark_dirty(&mut self) {
        if let Some(ui) = self.ui_host.as_mut() {
            ui.dirty = true;
        }
    }
    pub fn vtabs_refresh(&mut self) {
        self.vtabs_mark_dirty();
        self.vtabs_sync();
    }
    /// Shows a tab by window index, bypassing the sidebar's visible order.
    pub fn vtabs_show_tab_idx(&mut self, tab_idx: usize) {
        if let Some(mut window) = Mux::get().get_window_mut(self.mux_window_id) {
            window.remember_and_set_active_tab_idx(tab_idx);
        }
        self.vtabs_refresh();
    }
    pub fn vtabs_sync(&mut self) {
        let before = self.vtabs_reservation();
        let Some(mut ui) = self.ui_host.take() else {
            return;
        };
        let arrivals = ARRIVALS.with(|arrivals| {
            arrivals
                .borrow_mut()
                .remove(&self.mux_window_id)
                .unwrap_or_default()
        });
        ui.dirty |= !arrivals.is_empty()
            || ui.actual_active
                != Mux::get()
                    .get_active_tab_for_window(self.mux_window_id)
                    .map(|tab| tab.tab_id());
        if !ui.dirty && !ui.needs_commit && ui.bound && arrivals.is_empty() {
            self.ui_host = Some(ui);
            return;
        }
        if !ui.initialized {
            ui.initialize(&arrivals);
        }
        if !ui.bound {
            if let Some(window) = self.window.clone() {
                ui.provider.bind(window);
                ui.bound = true;
            }
        }
        if ui.dirty {
            let (tabs, active) = match Mux::get().get_window(self.mux_window_id) {
                Some(window) => (
                    window.iter_tabs().cloned().collect::<Vec<_>>(),
                    window.get_active_tab().map(|tab| tab.tab_id()),
                ),
                None => (Vec::new(), None),
            };
            if let Some(id) = active {
                ui.bells.remove(&id);
            }
            ui.actual_active = active;
            ui.revision += 1;
            ui.known = tabs.iter().map(|tab| tab.tab_id()).collect();
            ui.provider.snapshot(Snapshot {
                revision: ui.revision,
                tabs,
                bells: ui.bells.clone(),
                active,
                focused: self.focused.is_some(),
                config: self.config.clone(),
            });
            ui.dirty = false;
        }
        for arrival in arrivals {
            ui.provider.message(arrival);
        }
        ui.commit();
        ui.needs_commit = false;
        let mut commands = ui.provider.commands();
        if let Some(id) = ui.projection.active {
            if Mux::get()
                .get_active_tab_for_window(self.mux_window_id)
                .map(|tab| tab.tab_id())
                != Some(id)
                && !commands
                    .iter()
                    .any(|command| matches!(command, Command::Activate(target) if *target == id))
            {
                commands.push(Command::Activate(id));
            }
        }
        self.ui_host = Some(ui);
        self.vtabs_commands(commands);
        self.vtabs_schedule_provider(self.mux_window_id);
        self.vtabs_apply_reservation(before);
    }
    /// Tabs in the sidebar's visible order; None only without a mux window.
    pub fn vtabs_tab_information(&self) -> Option<Vec<TabInformation>> {
        let mux = Mux::get();
        let window = mux.get_window(self.mux_window_id)?;
        let Some(ui) = self.ui_host.as_ref() else {
            return Some(vec![]);
        };
        let last = window
            .get_last_active_tab_idx()
            .and_then(|idx| window.iter_tabs().nth(idx))
            .map(|tab| tab.tab_id());
        let tabs = ui
            .projection
            .tabs
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                let tab = mux.get_tab(*id)?;
                let panes = self.get_pos_panes_for_tab(&tab);
                Some(TabInformation {
                    tab_index: index,
                    tab_id: *id,
                    is_active: ui.projection.active == Some(*id),
                    is_last_active: last == Some(*id),
                    window_id: self.mux_window_id,
                    tab_title: tab.get_title(),
                    active_pane: panes
                        .iter()
                        .find(|p| p.is_active)
                        .map(Self::pos_pane_to_pane_info),
                })
            });
        Some(tabs.collect())
    }
    /// Tabs that arrived before subscribing missed TabAddedToWindow; they adopt this size.
    pub fn vtabs_adopt_tab_sizes(&self) {
        if let Some(window) = Mux::get().get_window(self.mux_window_id) {
            for tab in window.iter_tabs() {
                if tab.get_size() != self.terminal_size {
                    tab.resize(self.terminal_size);
                }
            }
        }
    }
    /// The newly shown mux window adopts this layout; hosts of closed windows end.
    pub fn vtabs_switched_window(&mut self, window: &window::Window) {
        let mux = Mux::get();
        let closed = self
            .vtabs_suspended
            .extract_if(|id, _| mux.get_window(*id).is_none());
        for (_, ui) in closed {
            ui.shutdown();
        }
        self.vtabs_sync();
        self.apply_dimensions(&self.dimensions.clone(), None, window);
    }
    pub fn vtabs_shutdown(&mut self) {
        if let Some(ui) = self.ui_host.take() {
            ui.shutdown();
        }
        for (_, ui) in self.vtabs_suspended.drain() {
            ui.shutdown();
        }
    }
    /// Returns true when upstream must not handle the notification.
    pub fn vtabs_notif(&mut self, notif: &TermWindowNotif) -> bool {
        match notif {
            TermWindowNotif::MuxNotification(notification) => {
                if !matches!(
                    notification,
                    MuxNotification::PaneOutput(_) | MuxNotification::TabResized(_)
                ) {
                    self.vtabs_mark_dirty();
                }
                match notification {
                    MuxNotification::Alert {
                        alert: wezterm_term::Alert::Bell,
                        pane_id,
                    } => self.vtabs_bell(*pane_id),
                    // The window owns content dimensions; new tabs adopt them without resizing it.
                    MuxNotification::TabAddedToWindow { tab_id, .. } => {
                        if let Some(tab) = Mux::get().get_tab(*tab_id) {
                            tab.resize(self.terminal_size);
                        }
                        return true;
                    }
                    MuxNotification::TabResized(_) => return true,
                    MuxNotification::WindowRemoved(window_id) => {
                        if let Some(ui) = self.vtabs_suspended.remove(window_id) {
                            ui.shutdown();
                        }
                    }
                    _ => {}
                }
            }
            TermWindowNotif::SwitchToMuxWindow(window_id) => {
                self.hide_terminal_overlay();
                if let Some(mut ui) = self.ui_host.take() {
                    ui.provider.suspend();
                    ui.provider.input(Input::Focus(false));
                    ui.dirty = true;
                    self.vtabs_suspended.insert(self.mux_window_id, ui);
                }
                self.ui_host = Some(
                    self.vtabs_suspended
                        .remove(window_id)
                        .unwrap_or_else(|| UiHost::new(*window_id)),
                );
            }
            _ => {}
        }
        false
    }
}
