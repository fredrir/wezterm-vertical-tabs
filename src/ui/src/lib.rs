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
mod settings;
mod theme;
mod views;

pub use element::{ElementId, ScrollArea};
pub use input::{EditResult, Key, Modifiers, MouseButton, TextEditor, UiInput};
pub use intent::{HostAction, UiIntent};
pub use keybinds::{matches_shortcut, shortcut_chord};
pub use overlays::launcher::{AdoptablePane, CommandEntry, ForeignTab, LaunchMenuConfig};
pub use ratatui::{buffer::Buffer, layout::Rect};
pub use runtime::canvas::{HitRegion, RoundedSurface};
pub use runtime::frame::{FrameUpdate, SurfaceTransform};
pub use runtime::motion::transition;
pub use theme::Theme;
pub use views::sidebar::header_fits;

use actions::Action;
use events::Pointer;
use overlays::launcher::Launchers;
use overlays::{FormKind, Menu, MenuItem, Overlay, Overlays};
use ratatui::layout::Position;
use runtime::{
    canvas::Paint,
    frame::Frame,
    motion::{self, Caret, Effects, Tooltip, Tween},
};
use std::{collections::BTreeMap, time::Duration};
use views::{
    settings_page::SettingsPage,
    sidebar::{SettingsPlace, Sidebar},
};
use vtabs_core::{Intent, Model, PaneId, SpaceId, TabId};

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
    settings_space: Option<SpaceId>,
    settings_by_space: BTreeMap<SpaceId, (SettingsPage, Option<SettingsPlace>)>,
    overlays: Overlays,
    launchers: Launchers,
}

struct Host {
    visible: bool,
    focused: bool,
    sidebar_columns: Option<u16>,
    header_inset: u16,
    page_background: Option<ratatui::style::Color>,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            visible: true,
            focused: true,
            sidebar_columns: None,
            header_inset: 0,
            page_background: None,
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
        self.overlays.is_open() || self.settings.open
    }
    pub fn theme(&self) -> &Theme {
        &self.theme
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
        match self.overlays.fail(message.into()) {
            Ok(()) => self.frame.dirty = true,
            Err(message) => self.show_error(message),
        }
    }

    pub fn invalidate(&mut self) {
        self.frame.dirty = true;
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
        self.focused = self.overlays.dismiss();
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
        if self.focused == Some(ElementId::SettingsSearch) {
            self.focused = None;
        }
        self.frame.dirty = true;
    }
    fn sync_settings_space(&mut self, model: &Model) {
        if self.settings_space.as_ref() != Some(&model.selected_space)
            && let Some(previous) = self.settings_space.replace(model.selected_space.clone())
        {
            if self.settings.open {
                self.interrupt();
                self.pointer.cancel_drag();
                self.pointer.press = None;
                self.pointer.origin = None;
            }
            if self.focused.as_ref().is_some_and(|id| {
                id.on_settings_page()
                    || matches!(id, ElementId::SettingsTab | ElementId::CloseSettingsTab)
            }) {
                self.focused = None;
                self.caret.stop();
            }
            self.cancel_settings_input();
            self.settings.query.set_preedit("", None);
            let (mut settings, place) = self
                .settings_by_space
                .remove(&model.selected_space)
                .unwrap_or_default();
            settings.chord_label = self.settings.chord_label;
            let previous_settings = std::mem::replace(&mut self.settings, settings);
            let previous_place = std::mem::replace(&mut self.sidebar.settings_place, place);
            self.settings_by_space
                .insert(previous, (previous_settings, previous_place));
            self.sidebar.rows_revision = None;
            self.sidebar.reveal_settings = self.settings.open;
            self.frame.dirty = true;
        }
        self.settings_by_space
            .retain(|id, _| model.spaces.iter().any(|space| &space.id == id));
    }
    /// Leaves the page for the tab beneath; it stays listed like any background tab.
    pub fn hide_settings(&mut self) {
        if self.settings.open {
            self.settings.open = false;
            if self
                .focused
                .as_ref()
                .is_some_and(ElementId::on_settings_page)
            {
                self.focused = None;
                self.caret.stop();
            }
            self.cancel_settings_input();
            self.frame.dirty = true;
        }
    }
    pub fn close_settings(&mut self) {
        self.settings.open = false;
        self.settings.listed = false;
        self.sidebar.settings_place = None;
        self.cancel_settings_input();
        self.dismiss();
        self.focused = None;
    }
    /// Another surface takes the keyboard, such as the quick terminal over this window.
    pub fn interrupt(&mut self) {
        self.dismiss();
        self.cancel_settings_input();
        if self.focused == Some(ElementId::SettingsSearch) {
            self.focused = None;
        }
        self.caret.stop();
    }
    fn cancel_settings_input(&mut self) {
        self.settings.editing.cancel();
    }
    pub fn has_overlay(&self) -> bool {
        self.overlays.is_open()
    }
    pub fn content_page(&self) -> bool {
        self.settings.open
    }
    pub fn overlay_surface(&self) -> bool {
        self.overlays.is_open() || self.tooltip.pending()
    }
    pub fn rounded_surfaces(&self) -> &[RoundedSurface] {
        &self.paint.surfaces
    }
    pub fn set_page_background(&mut self, color: Option<ratatui::style::Color>) {
        if self.host.page_background != color {
            self.host.page_background = color;
            self.invalidate();
        }
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
