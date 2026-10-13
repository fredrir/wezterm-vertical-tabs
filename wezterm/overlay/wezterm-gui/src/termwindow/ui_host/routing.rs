//! Routes window input and provider commands at the GUI boundary.
use super::{Command, Input, Message, Navigation, SpawnContext, TerminalKey};
use crate::termwindow::{TermWindow, TermWindowNotif};
use config::keyassignment::SpawnCommand;
use mux::{Mux, tab::TabId};
use std::time::Instant;
use window::{KeyEvent, WindowOps};

impl TermWindow {
    pub fn vtabs_activate_visible(&mut self, id: TabId) {
        self.vtabs_sync();
        if let Some(index) = self
            .ui_host
            .as_ref()
            .and_then(|ui| ui.projection.tabs.iter().position(|tab| *tab == id))
        {
            self.vtabs_navigation(Navigation::Index(index as isize));
        }
    }
    /// Runs before modals: sidebar clipboard edits, and only window actions without a pane.
    pub fn vtabs_preempt_assignment(
        &mut self,
        assignment: &config::keyassignment::KeyAssignment,
    ) -> bool {
        use config::keyassignment::KeyAssignment::*;
        self.vtabs_sync();
        if self.terminal_overlay_visible() {
            if matches!(assignment, CloseCurrentTab { .. } | CloseCurrentPane { .. }) {
                self.hide_terminal_overlay();
                return true;
            }
            if let AdjustPaneSize(direction, amount) = assignment {
                self.adjust_terminal_overlay(*direction, *amount);
                return true;
            }
            // Structural actions refer to mux tabs, which don't own this pane.
            if matches!(
                assignment,
                SplitHorizontal(_)
                    | SplitVertical(_)
                    | SplitPane(_)
                    | ActivatePaneDirection(_)
                    | ActivatePaneByIndex(_)
                    | TogglePaneZoomState
                    | RotatePanes(_)
                    | PaneSelect(_)
                    | ActivateTab(_)
                    | ActivateTabRelative(_)
                    | ActivateTabRelativeNoWrap(_)
                    | ActivateLastTab
                    | MoveTab(_)
                    | MoveTabRelative(_)
                    | SpawnTab(_)
                    | SpawnCommandInNewTab(_)
            ) {
                return true;
            }
            return false;
        }
        let page = self
            .ui_host
            .as_ref()
            .is_some_and(|ui| ui.provider.content_page());
        self.vtabs_clipboard_assignment(assignment)
            || ((self.vtabs_empty() || page)
                && !(page
                    && matches!(
                        assignment,
                        CloseCurrentTab { .. }
                            | CloseCurrentPane { .. }
                            | MoveTab(_)
                            | MoveTabRelative(_)
                    ))
                && !matches!(
                    assignment,
                    ActivateTab(_)
                        | ActivateTabRelative(_)
                        | ActivateTabRelativeNoWrap(_)
                        | ActivateLastTab
                        | SpawnTab(_)
                        | SpawnWindow
                        | SpawnCommandInNewTab(_)
                        | SpawnCommandInNewWindow(_)
                        | ShowTabNavigator
                        | ActivateCommandPalette
                        | ShowLauncher
                        | ShowLauncherArgs(_)
                        | SwitchToWorkspace { .. }
                        | SwitchWorkspaceRelative(_)
                        | EmitEvent(_)
                        | Multiple(_)
                        | ActivateKeyTable { .. }
                        | PopKeyTable
                        | ClearKeyTableStack
                        | Nop
                        | DisableDefaultAssignment
                        | QuitApplication
                        | Hide
                        | HideApplication
                        | Show
                        | ToggleFullScreen
                        | IncreaseFontSize
                        | DecreaseFontSize
                        | ResetFontSize
                        | ReloadConfiguration
                        | ActivateWindow(_)
                        | ActivateWindowRelative(_)
                        | ActivateWindowRelativeNoWrap(_)
                ))
    }
    /// Tab actions follow the sidebar's visible order, not the window's tab indices.
    pub fn vtabs_assignment(&mut self, assignment: &config::keyassignment::KeyAssignment) -> bool {
        use config::keyassignment::KeyAssignment::*;
        let navigation = match assignment {
            ActivateTab(index) => Navigation::Index(*index),
            ActivateTabRelative(delta) => Navigation::Relative(*delta, true),
            ActivateTabRelativeNoWrap(delta) => Navigation::Relative(*delta, false),
            ActivateLastTab => Navigation::Last,
            MoveTab(index) => Navigation::Move(*index),
            MoveTabRelative(delta) => Navigation::MoveRelative(*delta),
            ShowTabNavigator => Navigation::Navigator,
            CloseCurrentTab { .. } | CloseCurrentPane { .. }
                if self
                    .ui_host
                    .as_ref()
                    .is_some_and(|ui| ui.provider.content_page()) =>
            {
                Navigation::ClosePage
            }
            ActivateCommandPalette => {
                self.vtabs_open_command_palette();
                return true;
            }
            _ => return false,
        };
        self.vtabs_navigation(navigation);
        true
    }
    pub fn vtabs_navigation(&mut self, navigation: Navigation) {
        self.hide_terminal_overlay();
        self.vtabs_sync();
        let before = self.vtabs_reservation();
        if let Some(ui) = self.ui_host.as_mut() {
            ui.provider.navigation(navigation);
            ui.needs_commit = true;
        }
        self.vtabs_relayout(before);
    }
    pub fn vtabs_open_command_palette(&mut self) {
        self.hide_terminal_overlay();
        self.vtabs_sync();
        let before = self.vtabs_reservation();
        let commands = crate::termwindow::palette::CommandPalette::new(self).into_commands();
        self.cancel_modal();
        if let Some(ui) = self.ui_host.as_mut() {
            ui.provider.open_command_palette(commands);
            ui.needs_commit = true;
        }
        self.vtabs_relayout(before);
    }
    pub fn vtabs_open_launcher(
        &mut self,
        mut args: config::keyassignment::LauncherActionArgs,
        initial_choice_idx: usize,
    ) {
        self.hide_terminal_overlay();
        self.vtabs_sync();
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(ui) = self.ui_host.as_ref() else {
            return;
        };
        let visible_tabs = ui.projection.tabs.clone();
        let mux_window_id = self.mux_window_id;
        let domain = self
            .get_active_pane_or_overlay()
            .map(|pane| pane.domain_id())
            .unwrap_or_else(|| Mux::get().default_domain().domain_id());
        let config = self.config.clone();
        args.alphabet
            .get_or_insert_with(|| config.launcher_alphabet.clone());
        promise::spawn::spawn(async move {
            let entries =
                crate::overlay::launcher::LauncherArgs::new(args.flags, visible_tabs, domain)
                    .await
                    .build_entries(&config, initial_choice_idx);
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                // Domain labels may run Lua; the window can switch workspaces while awaiting them.
                if tw.mux_window_id != mux_window_id {
                    return;
                }
                let before = tw.vtabs_reservation();
                tw.cancel_modal();
                if let Some(ui) = tw.ui_host.as_mut() {
                    ui.provider.open_launcher(args, entries);
                    ui.needs_commit = true;
                }
                tw.vtabs_relayout(before);
            })));
        })
        .detach();
    }
    pub fn vtabs_message_for(&mut self, window_id: usize, message: Message) {
        if window_id == self.mux_window_id {
            self.vtabs_message(message);
        } else if let Some(ui) = self.vtabs_suspended.get_mut(&window_id) {
            ui.provider.message(message);
            ui.needs_commit = true;
            self.vtabs_schedule_provider(window_id);
        }
    }
    pub fn vtabs_message(&mut self, message: Message) {
        if message.hides_terminal() {
            self.hide_terminal_overlay();
        }
        let before = self.vtabs_reservation();
        if let Some(ui) = self.ui_host.as_mut() {
            ui.provider.message(message);
            ui.needs_commit = true;
        }
        self.vtabs_relayout(before);
    }
    pub fn vtabs_input(&mut self, input: Input<'_>) -> bool {
        self.vtabs_sync();
        if self.terminal_overlay_visible() {
            match &input {
                Input::Mouse(event, _) => {
                    if !self
                        .terminal_overlay_bounds()
                        .contains(event.coords.x as f32, event.coords.y as f32)
                    {
                        if matches!(
                            self.current_mouse_capture,
                            Some(crate::termwindow::MouseCapture::TerminalPane(_))
                        ) && matches!(
                            event.kind,
                            window::MouseEventKind::Move | window::MouseEventKind::Release(_)
                        ) {
                            return false;
                        }
                        if matches!(event.kind, window::MouseEventKind::Press(_)) {
                            self.hide_terminal_overlay();
                        }
                        return true;
                    }
                    return false;
                }
                Input::Key(_) | Input::RawKey(_) => {
                    let escape = self
                        .terminal_overlay_config()
                        .map(|config| config.hide_on_escape)
                        .unwrap_or_default();
                    let key = self
                        .ui_host
                        .as_mut()
                        .map(|ui| ui.provider.terminal_input(input, escape));
                    let consumed = match key {
                        Some(TerminalKey::Handled) => {
                            let commands = self.ui_host.as_mut().unwrap().provider.commands();
                            self.vtabs_commands(commands);
                            true
                        }
                        Some(TerminalKey::Escape) => self.terminal_overlay_escape(),
                        _ => false,
                    };
                    return consumed || self.terminal_overlay_pane().is_none();
                }
                Input::Composition(_) => return false,
                _ => {}
            }
        }
        let before = self.vtabs_reservation();
        if let Input::Mouse(event, _) = &input {
            if self.vtabs_titlebar_mouse(event) {
                self.vtabs_relayout(before);
                return true;
            }
        }
        let geometry = self.vtabs_geometry();
        let consumed = self.ui_host.as_mut().map_or(false, |ui| {
            // Input uses the same complete surface and hit regions as the next paint.
            ui.geometry = geometry;
            ui.provider.render(geometry, Instant::now());
            let consumed = ui.provider.input(input);
            ui.needs_commit |= consumed;
            consumed
        });
        if consumed {
            self.vtabs_relayout(before);
        } else {
            self.vtabs_sync();
        }
        consumed
    }
    /// macOS menu key equivalents run clipboard assignments without any key event.
    pub(super) fn vtabs_clipboard_assignment(
        &mut self,
        assignment: &config::keyassignment::KeyAssignment,
    ) -> bool {
        use config::keyassignment::KeyAssignment::{CopyTo, PasteFrom};
        let key = match assignment {
            CopyTo(_) => 'c',
            PasteFrom(_) => 'v',
            _ => return false,
        };
        if !self
            .ui_host
            .as_ref()
            .map_or(false, |ui| ui.provider.text_input_active())
        {
            return false;
        }
        let modifiers = if cfg!(target_os = "macos") {
            window::Modifiers::SUPER
        } else {
            window::Modifiers::CTRL
        };
        let event = KeyEvent {
            key: window::KeyCode::Char(key),
            modifiers,
            leds: Default::default(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        };
        self.vtabs_input(Input::Key(&event))
    }
    /// The provider adjusts the command and reserves a sidebar place for its tab.
    pub fn vtabs_prepare_spawn(
        &mut self,
        spawn: &SpawnCommand,
        spawn_where: crate::spawn::SpawnWhere,
    ) -> (SpawnCommand, Option<SpawnContext>) {
        use crate::spawn::SpawnWhere;
        let new_window = spawn_where == SpawnWhere::NewWindow;
        let mut spawn = spawn.clone();
        if let Some(ui) = self.ui_host.as_ref() {
            ui.provider.prepare_command(&mut spawn);
        }
        let context = if matches!(spawn_where, SpawnWhere::NewTab | SpawnWhere::NewWindow) {
            self.ui_host
                .as_mut()
                .map(|ui| ui.provider.reserve_spawn(new_window, &spawn))
        } else {
            None
        };
        (spawn, context)
    }
    pub(super) fn vtabs_commands(&mut self, commands: Vec<Command>) {
        for command in commands {
            let mux = Mux::get();
            match command {
                Command::ToggleTerminalOverlay => self.toggle_terminal_overlay(),
                Command::HideTerminalOverlay => self.hide_terminal_overlay(),
                Command::ToggleTerminalProgram(program) => self.toggle_terminal_program(program),
                Command::Activate(id) => {
                    if let Some(mut window) = mux.get_window_mut(self.mux_window_id) {
                        let idx = window.iter_tabs().position(|tab| tab.tab_id() == id);
                        if let Some(idx) = idx {
                            window.remember_and_set_active_tab_idx(idx);
                        }
                    }
                    if let Some(pane) = self.get_active_pane_or_overlay() {
                        pane.focus_changed(true);
                    }
                    if let Some(ui) = self.ui_host.as_mut() {
                        ui.actual_active = Some(id);
                    }
                    self.update_scrollbar();
                }
                Command::Close(id, confirm) => {
                    let idx = mux
                        .get_window(self.mux_window_id)
                        .and_then(|w| w.iter_tabs().position(|t| t.tab_id() == id));
                    if let Some(idx) = idx {
                        self.close_specific_tab(idx, confirm);
                    }
                    self.vtabs_mark_dirty();
                }
                Command::Spawn(command, new_window) => self.spawn_command(
                    &command,
                    if new_window {
                        crate::spawn::SpawnWhere::NewWindow
                    } else {
                        crate::spawn::SpawnWhere::NewTab
                    },
                ),
                Command::Resync => self.vtabs_mark_dirty(),
                Command::Clipboard(text) => {
                    if let Some(window) = &self.window {
                        window.set_clipboard(window::Clipboard::Clipboard, text);
                    }
                }
                Command::Paste(token) => {
                    let window_id = self.mux_window_id;
                    if let Some(window) = self.window.clone() {
                        promise::spawn::spawn(async move {
                            let text = window
                                .get_clipboard(window::Clipboard::Clipboard)
                                .await
                                .ok();
                            let message = Message::Paste { token, text };
                            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                                tw.vtabs_message_for(window_id, message)
                            })));
                        })
                        .detach();
                    }
                }
                Command::OpenCommandPalette => self.vtabs_open_command_palette(),
                Command::RunLauncherEntry(entry) => {
                    if let Some(id) = entry.tab_id {
                        self.vtabs_activate_visible(id);
                    } else if let Some(pane) = self.get_active_pane_or_overlay() {
                        if let Err(err) = self.perform_key_assignment(&pane, &entry.action) {
                            log::error!("Error while performing launcher action: {err:#}");
                        }
                    }
                }
                Command::RunPaletteCommand(command) => {
                    if let Err(err) = crate::termwindow::palette::save_recent(&command) {
                        log::error!("Error while saving recents: {err:#}");
                    }
                    if let Some(pane) = self.get_active_pane_or_overlay() {
                        if let Err(err) = self.perform_key_assignment(&pane, &command.action) {
                            log::error!("Error while performing {command:?}: {err:#}");
                        }
                    }
                }
                Command::Semantic(name) => self.emit_window_event(&name, None),
            }
        }
    }
}
