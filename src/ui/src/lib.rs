mod actions;
mod components;
mod element;
mod events;
mod icons;
mod input;
mod intent;
mod keybinds;
mod overlays;
mod runtime;
mod theme;
mod views;

pub use element::ElementId;
pub use input::{EditResult, Key, Modifiers, MouseButton, TextEditor, UiInput};
pub use intent::{HostAction, UiIntent};
pub use keybinds::is_shortcut;
pub use ratatui::{buffer::Buffer, layout::Rect};
pub use runtime::canvas::{HitRegion, RoundedSurface};
pub use runtime::frame::{FrameUpdate, SurfaceTransform};
pub use theme::Theme;
pub use views::launcher::{ForeignTab, JobEntry};

use actions::Action;
use events::Pointer;
use overlays::{FormKind, Menu, MenuItem, Overlay, Overlays};
use ratatui::layout::Position;
use runtime::{
    canvas::Paint,
    frame::Frame,
    motion::{self, Caret, Effects, Tooltip, Tween},
};
use std::time::Duration;
use views::{launcher::Launchers, settings_page::SettingsPage, sidebar::Sidebar};
use vtabs_core::{Intent, PaneId, TabId};

#[cfg(test)]
#[path = "../tests/snapshots.rs"]
mod snapshot_tests;

#[cfg(test)]
#[path = "../tests/ui.rs"]
mod ui_tests;

#[derive(Default)]
pub struct SidebarUi {
    theme: Theme,
    frame: Frame,
    paint: Paint,
    host: Host,
    focused: Option<ElementId>,
    pointer: Pointer,
    caret: Caret,
    tooltip: Tooltip,
    effects: Effects,
    sidebar: Sidebar,
    settings: SettingsPage,
    overlays: Overlays,
    launchers: Launchers,
}

struct Host {
    visible: bool,
    focused: bool,
    sidebar_columns: Option<u16>,
    header_inset: u16,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            visible: true,
            focused: true,
            sidebar_columns: None,
            header_inset: 0,
        }
    }
}

impl Host {
    fn live(&self) -> bool {
        self.visible && self.focused
    }
}

fn running(process: &str) -> String {
    if process.is_empty() {
        "A process is still running.".into()
    } else {
        format!("{process} is still running.")
    }
}

impl SidebarUi {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn buffer(&self) -> &Buffer {
        &self.frame.buffer
    }
    pub fn hit_regions(&self) -> &[HitRegion] {
        &self.paint.hits
    }
    pub fn focused(&self) -> Option<&ElementId> {
        self.focused.as_ref()
    }
    pub fn is_modal(&self) -> bool {
        self.overlays.current.is_some() || self.settings.open
    }
    pub fn theme(&self) -> &Theme {
        &self.theme
    }
    pub fn has_focus(&self) -> bool {
        self.focused.is_some() || self.is_modal()
    }
    pub fn release_focus(&mut self) {
        self.hide_settings();
        if self.sidebar.rename.take().is_some() {
            self.caret.stop();
            self.frame.dirty = true;
        }
        self.focused = None;
        self.pointer.drag = None;
    }
    pub fn confirm_close_tab(&mut self, id: TabId, process: &str) {
        self.dismiss();
        self.confirm(
            "Close tab?",
            running(process),
            "Close",
            Action::Domain(Intent::CloseTab(id)),
        );
    }
    pub fn confirm_close_pane(&mut self, tab: TabId, pane: PaneId, process: &str) {
        self.dismiss();
        self.confirm(
            "Close split?",
            running(process),
            "Close",
            Action::KillPane(tab, pane),
        );
    }
    pub fn set_foreign_tabs(&mut self, tabs: Vec<ForeignTab>) {
        self.launchers.foreign_tabs = tabs;
    }
    pub(crate) fn show_error(&mut self, message: impl Into<String>) {
        let items = vec![MenuItem::new("dismiss", "Dismiss", Action::Close)];
        self.open_overlay(Overlay::Menu(Menu::new(message, items)));
    }
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.overlays.pending_form = None;
        if let Some(Overlay::Form(form)) = &mut self.overlays.current {
            form.error = Some(message.into());
            self.frame.dirty = true;
        } else {
            self.show_error(message);
        }
    }
    pub fn invalidate(&mut self) {
        self.frame.dirty = true;
    }
    pub fn set_config_owned(&mut self, keys: impl IntoIterator<Item = String>) {
        let keys = keys.into_iter().collect();
        if self.settings.config_owned != keys {
            self.settings.config_owned = keys;
            self.invalidate();
        }
    }
    pub fn hit_test(&self, x: u16, y: u16) -> Option<&HitRegion> {
        self.paint
            .hits
            .iter()
            .rev()
            .find(|hit| hit.rect.contains(Position::new(x, y)))
    }
    pub fn next_deadline(&self) -> Option<Duration> {
        if !self.host.live() || self.frame.buffer.area.is_empty() {
            return None;
        }
        [
            self.effects
                .cell
                .as_ref()
                .map(|_| self.frame.last + motion::FRAME),
            self.effects
                .surface
                .map(|_| self.frame.last + motion::FRAME),
            self.pointer
                .press
                .as_ref()
                .filter(|press| press.animating)
                .map(|_| self.frame.last + motion::FRAME),
            self.pointer
                .drop_motion
                .filter(|motion| motion.progress < 1.0)
                .map(|_| self.frame.last + motion::FRAME),
            self.caret.deadline,
            self.tooltip.deadline,
        ]
        .into_iter()
        .flatten()
        .min()
    }
    pub fn has_animation(&self) -> bool {
        self.effects.cell.is_some()
            || self.effects.surface.is_some()
            || self
                .pointer
                .press
                .as_ref()
                .is_some_and(|press| press.animating)
            || self
                .pointer
                .drop_motion
                .is_some_and(|motion| motion.progress < 1.0)
    }
    pub fn set_pointer_fraction(&mut self, x: f32, y: f32) {
        self.pointer.fraction = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
    }
    pub fn set_clock(&mut self, now: Duration) {
        self.frame.now = self.frame.now.max(now);
    }
    pub fn cancel_effects(&mut self) {
        if self.effects.cancel() {
            self.frame.dirty = true;
        }
    }
    pub fn transition_surface(&mut self, from: f32, to: f32, now: Duration, duration: Duration) {
        self.effects.surface = (duration > Duration::ZERO).then_some(Tween {
            from,
            to,
            start: now,
            duration,
        });
        self.frame.now = now;
        self.frame.dirty = true;
    }
    pub fn dismiss(&mut self) {
        self.overlays.current = None;
        self.overlays.pending_form = None;
        self.overlays.stack.clear();
        self.overlays.anchor = None;
        self.focused = self.overlays.restore_focus.take();
        self.caret.stop();
        self.tooltip.hide();
        self.cancel_effects();
        self.frame.dirty = true;
    }
    pub fn open_settings(&mut self) {
        self.dismiss();
        self.settings.open = true;
        self.sidebar.reveal_settings = !self.settings.listed;
        self.settings.listed = true;
        self.settings.search_focused = false;
        self.frame.dirty = true;
    }
    pub(crate) fn hide_settings(&mut self) {
        if self.settings.open {
            self.settings.open = false;
            self.settings.search_focused = false;
            self.frame.dirty = true;
        }
    }
    pub fn close_settings(&mut self) {
        self.settings.open = false;
        self.settings.listed = false;
        self.sidebar.settings_place = None;
        self.settings.search_focused = false;
        self.dismiss();
        self.focused = None;
    }
    pub fn has_overlay(&self) -> bool {
        self.overlays.current.is_some()
    }
    pub fn content_page(&self) -> bool {
        self.settings.open
    }
    pub fn overlay_surface(&self) -> bool {
        self.overlays.current.is_some() || self.tooltip.pending()
    }
    pub fn rounded_surfaces(&self) -> &[RoundedSurface] {
        &self.paint.surfaces
    }
    pub fn set_layout(&mut self, sidebar_columns: u16, header_inset: u16) {
        if self.host.sidebar_columns != Some(sidebar_columns)
            || self.host.header_inset != header_inset
        {
            self.host.sidebar_columns = Some(sidebar_columns);
            self.host.header_inset = header_inset;
            self.invalidate();
        }
    }
    pub(crate) fn open_create_folder(&mut self) {
        self.open_form("New folder", FormKind::CreateFolder, "");
    }
    pub fn open_create_space(&mut self) {
        self.open_form("Create space", FormKind::CreateSpace, "");
    }
    fn reset_caret(&mut self) {
        self.caret.restart(self.frame.now, self.host.live());
    }
}
