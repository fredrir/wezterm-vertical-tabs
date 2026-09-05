//! The only product code coupled to WezTerm's internal APIs.
mod cells;
mod lua;
mod shutdown;
mod storage;
mod update;

use crate::termwindow::native_ui::{
    Bounds, Command, Geometry, Input, Navigation, Projection, Provider, Reservation,
    RoundedSurface, Snapshot, Surface,
};
use std::{collections::HashMap, time::Instant};
use vtabs_app::{self as app, core, ui, WindowApp};
use window::{KeyCode, MouseEventKind, MousePress, Window};

pub use lua::register;

pub fn create(window_id: usize) -> Box<dyn Provider> {
    Box::new(Adapter::new(window_id))
}

struct Adapter {
    app: WindowApp,
    window_id: usize,
    epoch: Instant,
    window: Option<Window>,
    surface: Surface,
    primitives: Vec<RoundedSurface>,
    commands: Vec<Command>,
    cursor: Option<(usize, usize)>,
    geometry: Geometry,
    native_tabs: Vec<usize>,
    remote_tabs: HashMap<u64, (usize, usize, Vec<usize>)>,
    config_generation: usize,
    pointer_captured: bool,
    next_spawn_space: Option<String>,
    next_spawn_folder: Option<String>,
    next_private: bool,
    hook_metadata: HashMap<u64, core::Tab>,
    outstanding: Option<u64>,
    hook_pending: bool,
    hooks_enabled: bool,
    window_hook_dirty: bool,
    input_epoch: u64,
    hook_queued: HashMap<u64, core::Tab>,
}

impl Adapter {
    fn new(window_id: usize) -> Self {
        let config = lua::configuration();
        let mut app = WindowApp::new(&config.profile, false);
        app.set_window_identity(window_id as u64);
        if let Err(err) = app.config(config.value()) {
            log::error!("native tabs config: {err}");
        }
        Self {
            app,
            window_id,
            epoch: Instant::now(),
            window: None,
            surface: Surface {
                rows: Vec::new(),
                columns: 0,
                revision: 0,
                offset: (0., 0.),
                opacity: 1.,
            },
            primitives: Vec::new(),
            commands: Vec::new(),
            cursor: None,
            geometry: Geometry::default(),
            native_tabs: Vec::new(),
            remote_tabs: HashMap::new(),
            config_generation: usize::MAX,
            pointer_captured: false,
            next_spawn_space: None,
            next_spawn_folder: None,
            next_private: false,
            hook_metadata: HashMap::new(),
            outstanding: None,
            hook_pending: false,
            hooks_enabled: false,
            window_hook_dirty: true,
            input_epoch: 0,
            hook_queued: HashMap::new(),
        }
    }
    fn apply(&mut self, result: Result<app::Update, core::Error>) {
        match result {
            Ok(update) => {
                for command in update.commands {
                    self.command(command);
                }
            }
            Err(err) => {
                self.app.ui_mut().set_error(err.to_string());
                log::warn!("native tabs: {err}");
            }
        }
    }
    fn dispatch(&mut self, intent: core::Intent) {
        self.input_epoch = self.input_epoch.wrapping_add(1);
        let before = self.app.model().revision;
        let result = self.app.dispatch(intent);
        self.apply(result);
        self.window_hook_dirty |= before != self.app.model().revision;
        self.schedule_hooks();
    }
    fn command(&mut self, command: app::Command) {
        use core::HostCommand as C;
        match command {
            app::Command::Refresh => {
                std::thread::spawn(config::reload);
                self.app.ui_mut().invalidate();
            }
            app::Command::SetClipboard(text) => self.commands.push(Command::Clipboard(text)),
            app::Command::RequestClipboard => self.commands.push(Command::Paste(self.input_epoch)),
            app::Command::Host(command) => match command {
                C::Activate(id) => self.commands.push(Command::Activate(id as usize)),
                C::Close(id) => self.commands.push(Command::Close(id as usize, false)),
                C::Rename { id, title } => self.commands.push(Command::Rename(id as usize, title)),
                C::Spawn {
                    space_id,
                    launch,
                    folder_id,
                } => {
                    self.next_spawn_folder = folder_id;
                    self.next_spawn_space = Some(space_id);
                    self.commands.push(Command::Spawn(spawn(launch), false));
                }
                C::NewWindow { private, launch } => {
                    self.next_private = private;
                    self.commands.push(Command::Spawn(spawn(launch), true));
                }
                C::Reorder { visible_order } => {
                    let visible: std::collections::HashSet<_> =
                        visible_order.iter().map(|id| *id as usize).collect();
                    let mut next = visible_order.into_iter();
                    let order = self
                        .native_tabs
                        .iter()
                        .map(|id| {
                            if visible.contains(id) {
                                next.next().unwrap() as usize
                            } else {
                                *id
                            }
                        })
                        .collect();
                    self.commands.push(Command::Reorder(order));
                }
                C::MoveTabToNewWindow(id) => {
                    self.commands.push(Command::MoveToNewWindow(id as usize))
                }
                C::CustomAction(name) => self.commands.push(Command::Semantic(name)),
            },
        }
    }
    fn ui_input(&mut self, input: ui::UiInput) {
        // Passive pointer motion and composition cleanup retain an outstanding paste.
        let changes_input = match &input {
            ui::UiInput::PointerMove { .. } => self.pointer_captured,
            ui::UiInput::ImePreedit { text, .. } => !text.is_empty(),
            _ => true,
        };
        if changes_input {
            self.input_epoch = self.input_epoch.wrapping_add(1);
        }
        let before = self.app.model().revision;
        let result = self.app.input(input);
        self.apply(result);
        self.window_hook_dirty |= before != self.app.model().revision;
        self.schedule_hooks();
    }
    fn schedule_hooks(&mut self) {
        if !self.hooks_enabled {
            self.hook_queued.clear();
            self.window_hook_dirty = false;
            return;
        }
        if self.hook_pending || (self.hook_queued.is_empty() && !self.window_hook_dirty) {
            return;
        }
        if let Some(window) = self.window.clone() {
            let model = self.app.model();
            self.hook_queued.retain(|id, _| model.tabs.contains_key(id));
            let mut ids = self.hook_queued.keys().copied().collect::<Vec<_>>();
            ids.sort_unstable();
            let mut batch = Vec::with_capacity(ids.len().min(lua::MAX_HOOK_TABS));
            for id in ids.into_iter().take(lua::MAX_HOOK_TABS) {
                if let Some(tab) = self.hook_queued.remove(&id) {
                    batch.push((self.app.hook_token(Some(tab.id)), tab));
                }
            }
            self.hook_pending = true;
            self.window_hook_dirty = false;
            lua::hooks(
                window,
                batch,
                lua::WindowHookContext::from_app(&self.app),
                self.window_id,
            );
        }
    }
    fn storage(&mut self) {
        if let Some(window) = self.window.clone() {
            if let Some(request) = self.app.take_storage_request(self.epoch.elapsed()) {
                self.outstanding = Some(request.request_id);
                storage::request(window, request, self.window_id);
            }
        }
        for error in self.app.take_errors() {
            log::error!("native tabs: {error}");
            self.app.ui_mut().set_error(error);
        }
    }
    fn metrics(&self, geometry: Geometry) -> app::Metrics {
        let bounds =
            geometry.ui_bounds(self.app.ui().content_page() || self.app.ui().overlay_surface());
        app::Metrics {
            cols: (bounds.width / geometry.cell_width.max(1.))
                .floor()
                .clamp(0., u16::MAX as f32) as u16,
            rows: (bounds.height / geometry.cell_height.max(1.))
                .floor()
                .clamp(0., u16::MAX as f32) as u16,
            pixel_width: bounds.width.max(0.) as u32,
            pixel_height: bounds.height.max(0.) as u32,
            cell_width: geometry.cell_width.max(1.),
            cell_height: geometry.cell_height.max(1.),
            dpi: geometry.dpi.max(1.),
        }
    }
}

impl Provider for Adapter {
    fn initialize(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>> {
        Box::pin(async move {
            // Restore public preferences before the host computes its first reservation.
            // The helper runs asynchronously and has its own bounded timeout; failures
            // remain in the coordinator's retry queue for the ordinary bound lifecycle.
            let Some(request) = self.app.take_storage_request(self.epoch.elapsed()) else {
                return;
            };
            let request_id = request.request_id;
            match storage::invoke(request).await {
                Ok(response) => {
                    let result = self.app.complete_storage(response);
                    self.apply(result);
                }
                Err(error) => self.app.storage_failed(request_id, error.to_string()),
            }
        })
    }
    fn reservation(&self) -> Reservation {
        let settings = &self.app.model().settings;
        Reservation {
            width: settings.logical_width() as f32,
            right: settings.side == core::Side::Right,
        }
    }
    fn metadata_interval(&self) -> Option<std::time::Duration> {
        let model = self.app.model();
        let process_rules = model
            .spaces
            .iter()
            .flat_map(|space| &space.rules)
            .chain(model.templates.iter().flat_map(|template| &template.rules))
            .any(|rule| {
                rule.fields
                    .iter()
                    .any(|(field, _)| *field == core::MatchField::Process)
            });
        // Lua callbacks may inspect process facts. Notification-only configurations have
        // no refresh timer and no corresponding idle wakeups.
        let process_templates = model.templates.iter().any(|template| {
            [&template.id, &template.name].iter().any(|text| {
                text.split('$').skip(1).any(|suffix| {
                    let token = suffix
                        .split(|character: char| !character.is_ascii_alphabetic())
                        .next()
                        .unwrap_or_default();
                    matches!(token, "proc" | "process")
                })
            })
        });
        (self.hooks_enabled || process_rules || process_templates)
            .then_some(std::time::Duration::from_secs(2))
    }
    fn bind(&mut self, window: Window) {
        self.window = Some(window);
        update::schedule();
        self.storage();
    }
    fn snapshot(&mut self, snapshot: Snapshot) {
        self.app.set_window_identity(snapshot.window_id as u64);
        let before = self.app.model().revision;
        if self.config_generation != snapshot.config_epoch {
            self.config_generation = snapshot.config_epoch;
            self.hooks_enabled = lua::hooks_enabled();
            self.hook_metadata.clear();
            self.window_hook_dirty = true;
            self.input_epoch = self.input_epoch.wrapping_add(1);
            let result = self.app.config(lua::configuration().value());
            self.apply(result);
        }
        self.native_tabs = snapshot.tabs.iter().map(|tab| tab.id).collect();
        let tabs = snapshot
            .tabs
            .into_iter()
            .map(|tab| {
                let launch = mux::Mux::get()
                    .get_domain_by_name(&tab.domain)
                    .filter(|domain| domain.spawnable())
                    .map(|_| core::LaunchSpec {
                        domain: Some(tab.domain.clone()),
                        cwd: (!tab.cwd.is_empty()).then_some(tab.cwd.clone()),
                        ..Default::default()
                    });
                core::Tab {
                    id: tab.id as u64,
                    title: tab.title,
                    cwd: tab.cwd.clone(),
                    domain: tab.domain.clone(),
                    process: tab.process,
                    remote: tab.remote,
                    unread: tab.unread,
                    bell: tab.bell,
                    icon: tab.user_vars.get("icon").cloned().unwrap_or_default(),
                    launch,
                    host: tab.user_vars.get("hostname").cloned().unwrap_or(tab.host),
                    user: tab.user_vars.get("username").cloned().unwrap_or(tab.user),
                    ..Default::default()
                }
            })
            .collect::<Vec<_>>();
        let changed = tabs
            .iter()
            .filter(|tab| self.hook_metadata.get(&tab.id) != Some(*tab))
            .cloned()
            .collect::<Vec<_>>();
        let native_tabs = self
            .native_tabs
            .iter()
            .map(|id| *id as u64)
            .collect::<std::collections::HashSet<_>>();
        self.hook_metadata.retain(|id, _| native_tabs.contains(id));
        let departed = {
            let mux = mux::Mux::get();
            self.app
                .model()
                .tabs
                .iter()
                .filter_map(|(id, tab)| {
                    if native_tabs.contains(id) {
                        return None;
                    }
                    let detached = mux.get_domain_by_name(&tab.domain).map_or(true, |domain| {
                        domain.state() == mux::domain::DomainState::Detached
                    });
                    let remapped = self
                        .remote_tabs
                        .get(id)
                        .and_then(|(domain, remote, _)| {
                            wezterm_client::domain::ClientDomain::get_client_inner_for_domain(
                                *domain,
                            )
                            .ok()
                            .and_then(|inner| inner.remote_to_local_tab_id(*remote))
                        })
                        .is_some_and(|current| current as u64 != *id);
                    // A tab in another window, a pane reparented into another tab,
                    // a detached domain, or a remote-ID remap is a departure.
                    let moved = mux
                        .window_containing_tab(*id as usize)
                        .is_some_and(|window| window != self.window_id)
                        || self.remote_tabs.get(id).is_some_and(|(_, _, panes)| {
                            panes.iter().any(|pane| {
                                mux.resolve_pane_id(*pane)
                                    .is_some_and(|(_, _, tab)| tab as u64 != *id)
                            })
                        });
                    (detached || remapped || moved).then_some(*id)
                })
                .collect::<Vec<_>>()
        };
        for id in departed {
            self.app.acknowledge_tab_departure(id);
        }
        self.remote_tabs = {
            let mux = mux::Mux::get();
            tabs.iter()
                .filter_map(|tab| {
                    let domain = mux.get_domain_by_name(&tab.domain)?;
                    let client = domain.downcast_ref::<wezterm_client::domain::ClientDomain>()?;
                    Some((
                        tab.id,
                        (
                            domain.domain_id(),
                            client.local_to_remote_tab_id(tab.id as usize)?,
                            mux.get_tab(tab.id as usize)?
                                .iter_panes_ignoring_zoom()
                                .iter()
                                .map(|pane| pane.pane.pane_id())
                                .collect(),
                        ),
                    ))
                })
                .collect()
        };
        for tab in &changed {
            self.hook_metadata.insert(tab.id, tab.clone());
        }
        let result = self.app.update(app::NativeSnapshot {
            revision: snapshot.revision,
            tabs,
            active_tab: snapshot.active.map(|id| id as u64),
            metrics: self.app.metrics(),
            focused: snapshot.focused,
            configuration_epoch: snapshot.config_epoch as u64,
        });
        self.apply(result);
        self.window_hook_dirty |= before != self.app.model().revision;
        for tab in changed {
            self.hook_queued.insert(tab.id, tab);
        }
        self.schedule_hooks();
        self.storage();
    }
    fn navigation(&mut self, navigation: Navigation) {
        let intent = match navigation {
            Navigation::Index(i) => core::Intent::ActivateIndex(i),
            Navigation::Relative(delta, wrap) => core::Intent::ActivateRelative { delta, wrap },
            Navigation::Last => core::Intent::ActivateLast,
            Navigation::Move(index) => match self.app.model().selected_tab {
                Some(id) => core::Intent::MoveTab { id, index },
                None => return,
            },
            Navigation::MoveRelative(delta) => {
                let model = self.app.model();
                let Some(id) = model.selected_tab else { return };
                let index = model
                    .visible_ids()
                    .iter()
                    .position(|tab| *tab == id)
                    .unwrap_or(0) as isize;
                core::Intent::MoveTab {
                    id,
                    index: index
                        .saturating_add(delta)
                        .clamp(0, model.visible_ids().len().saturating_sub(1) as isize)
                        as usize,
                }
            }
            Navigation::Navigator => {
                self.app.open_tab_navigator();
                return;
            }
        };
        self.dispatch(intent);
    }
    fn input(&mut self, input: Input<'_>) -> bool {
        match input {
            Input::RawKey(key) => {
                let mods = modifiers(key.modifiers);
                let code = self
                    .app
                    .ui()
                    .text_input_active()
                    .then(|| editor_clipboard_key(&key.key, mods))
                    .flatten()
                    .or_else(|| {
                        self.app
                            .model()
                            .settings
                            .keyboard_shortcuts
                            .then(|| shortcut_key(&key.key, mods))
                            .flatten()
                            .filter(|code| ui::is_shortcut(code, mods))
                    });
                let Some(code) = code else {
                    return false;
                };
                if key.key_is_down {
                    self.ui_input(ui::UiInput::Key {
                        key: code,
                        modifiers: mods,
                    });
                }
                true
            }
            Input::Key(key) => {
                let mods = modifiers(key.raw.as_ref().map_or(key.modifiers, |raw| raw.modifiers));
                if let Some(code) = self
                    .app
                    .ui()
                    .text_input_active()
                    .then(|| editor_clipboard_key(&key.key, mods))
                    .flatten()
                {
                    if key.key_is_down {
                        self.ui_input(ui::UiInput::Key {
                            key: code,
                            modifiers: mods,
                        });
                    }
                    return true;
                }
                if self.app.model().settings.keyboard_shortcuts {
                    if let Some(code) =
                        shortcut_key(&key.key, mods).filter(|code| ui::is_shortcut(code, mods))
                    {
                        if key.key_is_down {
                            self.ui_input(ui::UiInput::Key {
                                key: code,
                                modifiers: mods,
                            });
                        }
                        return true;
                    }
                }
                if !key.key_is_down {
                    return self.keyboard_focus();
                }
                if let KeyCode::Composed(text) = &key.key {
                    if !self.keyboard_focus() {
                        return false;
                    }
                    self.ui_input(ui::UiInput::ImeCommit(text.clone()));
                    return true;
                }
                let code = match &key.key {
                    KeyCode::Char('\r' | '\n') => ui::Key::Enter,
                    KeyCode::Char('\u{1b}') => ui::Key::Escape,
                    KeyCode::Char('\u{8}') => ui::Key::Backspace,
                    KeyCode::Char('\u{7f}') => ui::Key::Delete,
                    KeyCode::Char('\t') => ui::Key::Tab,
                    KeyCode::Char(c) => ui::Key::Character(*c),
                    KeyCode::LeftArrow => ui::Key::Left,
                    KeyCode::RightArrow => ui::Key::Right,
                    KeyCode::UpArrow => ui::Key::Up,
                    KeyCode::DownArrow => ui::Key::Down,
                    KeyCode::Home => ui::Key::Home,
                    KeyCode::End => ui::Key::End,
                    KeyCode::PageUp => ui::Key::PageUp,
                    KeyCode::PageDown => ui::Key::PageDown,
                    KeyCode::Function(2) => ui::Key::F2,
                    KeyCode::Function(10) => ui::Key::F10,
                    _ => return self.keyboard_focus(),
                };
                let shortcut = self.app.model().settings.keyboard_shortcuts
                    && ui::is_shortcut(&code, modifiers(key.modifiers));
                if !self.keyboard_focus() && !shortcut {
                    return false;
                }
                if !key.key_is_down {
                    return true;
                }
                // Navigation bindings are still native when the rail itself has focus.
                if !shortcut
                    && !self.app.is_modal()
                    && key
                        .modifiers
                        .intersects(window::Modifiers::SUPER | window::Modifiers::CTRL)
                {
                    return false;
                }
                self.ui_input(ui::UiInput::Key {
                    key: code,
                    modifiers: mods,
                });
                true
            }
            Input::Mouse(event, geometry) => {
                let bounds = geometry.ui_bounds(self.content_page() || self.overlay_surface());
                let inside = bounds.contains(event.coords.x as f32, event.coords.y as f32);
                let modal = self.app.is_modal();
                let in_sidebar = geometry
                    .sidebar
                    .contains(event.coords.x as f32, event.coords.y as f32);
                if !in_sidebar && !modal && !self.pointer_captured {
                    if matches!(event.kind, MouseEventKind::Press(_)) {
                        self.app.ui_mut().release_focus();
                        self.input_epoch = self.input_epoch.wrapping_add(1);
                    } else if matches!(event.kind, MouseEventKind::Move) {
                        self.ui_input(ui::UiInput::PointerMove {
                            x: u16::MAX,
                            y: u16::MAX,
                        });
                    }
                    return false;
                }
                let x = ((event.coords.x as f32 - bounds.x - self.surface.offset.0)
                    / geometry.cell_width.max(1.))
                .floor();
                let y = ((event.coords.y as f32 - bounds.y - self.surface.offset.1)
                    / geometry.cell_height.max(1.))
                .floor();
                let (x, y) = if inside
                    && x >= 0.
                    && y >= 0.
                    && x < self.surface.columns as f32
                    && y < self.surface.rows.len() as f32
                {
                    (x as u16, y as u16)
                } else {
                    (u16::MAX, u16::MAX)
                };
                if geometry.header_height > 0.
                    && (event.coords.y as f32) < bounds.y + geometry.header_height
                    && !self.pointer_captured
                    && !self.app.ui().has_overlay()
                    && self.app.ui().hit_test(x, y).is_none()
                {
                    return false;
                }
                let mods = modifiers(event.modifiers);
                let input = match event.kind {
                    MouseEventKind::Press(button) => {
                        self.pointer_captured = true;
                        ui::UiInput::PointerDown {
                            x,
                            y,
                            button: mouse_button(button),
                            modifiers: mods,
                        }
                    }
                    MouseEventKind::Release(button) => {
                        self.pointer_captured = false;
                        ui::UiInput::PointerUp {
                            x,
                            y,
                            button: mouse_button(button),
                        }
                    }
                    MouseEventKind::Move => ui::UiInput::PointerMove { x, y },
                    MouseEventKind::VertWheel(rows) => ui::UiInput::Scroll {
                        x,
                        y,
                        rows: -(rows as i32),
                    },
                    _ => return inside || modal,
                };
                self.ui_input(input);
                true
            }
            Input::Composition(status) => {
                if !self.keyboard_focus() {
                    return false;
                }
                self.ui_input(ui::UiInput::ImePreedit {
                    text: match status {
                        window::DeadKeyStatus::Composing(text) => text.clone(),
                        _ => String::new(),
                    },
                    cursor: None,
                });
                true
            }
            Input::Focus(focused) => {
                if !focused {
                    self.pointer_captured = false;
                }
                self.input_epoch = self.input_epoch.wrapping_add(1);
                self.app.set_focus(focused);
                if focused {
                    self.app.refresh_storage();
                    self.storage();
                }
                false
            }
        }
    }
    fn message(&mut self, message: serde_json::Value) {
        let before = self.app.model().revision;
        let hook_completion = message.get("hooks").is_some();
        if let Some(profile) = message.get("refresh_profile").and_then(|v| v.as_str()) {
            if profile == self.app.model().profile {
                self.app.refresh_storage();
            }
        } else if let Some(error) = message.get("error").and_then(|v| v.as_str()) {
            self.app.ui_mut().set_error(error);
        } else if let Some(context) = message.get("initialize") {
            if let Some(transfer) = context.get("transfer") {
                match serde_json::from_value::<app::WindowTransfer>(transfer.clone()) {
                    Ok(mut transfer) => {
                        if let Some(id) = message["tab_id"].as_u64() {
                            transfer.tab.id = id;
                        }
                        if let Err(error) = self.app.import_transfer_before_snapshot(transfer) {
                            self.app.ui_mut().set_error(error.to_string());
                        }
                    }
                    Err(error) => self.app.ui_mut().set_error(error.to_string()),
                }
            } else {
                self.app
                    .set_private(context["private"].as_bool().unwrap_or(false));
            }
        } else if let Some(spawned) = message.get("spawned") {
            if let Some(id) = spawned["tab_id"].as_u64() {
                let context = &spawned["context"];
                if context["new_window"] != true {
                    if let Ok(token) =
                        serde_json::from_value::<app::SpawnToken>(context["token"].clone())
                    {
                        let result = self.app.spawn_completed_with_token(id, token);
                        self.apply(result);
                    }
                }
                if let Ok(launch) =
                    serde_json::from_value::<core::LaunchSpec>(context["launch"].clone())
                {
                    self.app.set_launch(id, launch).ok();
                }
            }
        } else if let Some(id) = message.get("tab_departed").and_then(|id| id.as_u64()) {
            self.app.acknowledge_tab_departure(id);
        } else if let Some(context) = message.get("spawn_failed") {
            if let Ok(token) = serde_json::from_value::<app::SpawnToken>(context["token"].clone()) {
                self.app.cancel_spawn(&token);
            }
        } else if let Some(paste) = message.get("paste").and_then(|v| v.as_str()) {
            if message["token"].as_u64() == Some(self.input_epoch)
                && self.app.ui().text_input_active()
            {
                self.ui_input(ui::UiInput::Paste(paste.into()));
            }
        } else if let Some(result) = message.get("store") {
            self.outstanding = None;
            if let Some(response) = result.get("ok") {
                match serde_json::from_value(response.clone()) {
                    Ok(response) => {
                        let result = self.app.complete_storage(response);
                        self.apply(result);
                    }
                    Err(err) => log::error!("native tabs storage response: {err}"),
                }
            } else if let (Some(error), Some(request)) = (
                result.get("error").and_then(|v| v.as_str()),
                result.get("request_id").and_then(|v| v.as_u64()),
            ) {
                self.app.storage_failed(request, error);
            }
        } else if let Some(action) = message.get("action") {
            self.input_epoch = self.input_epoch.wrapping_add(1);
            match action.as_str() {
                Some("settings") => self.app.open_settings(),
                Some("create_space") => self.app.open_create_space(),
                Some("navigator") => self.app.open_tab_navigator(),
                Some("retry_storage") => self.app.retry_storage(),
                _ => match serde_json::from_value::<core::Intent>(action.clone()) {
                    Ok(intent) => self.dispatch(intent),
                    Err(err) => log::warn!("native tabs action: {err}"),
                },
            }
        } else if message.get("hooks").is_some() {
            self.hook_pending = false;
            let selection = (
                self.app.model().selected_space.clone(),
                self.app.model().selected_tab,
            );
            if !lua::complete(&mut self.app, message) {
                self.window_hook_dirty = true;
                for (id, tab) in &self.hook_metadata {
                    self.hook_queued.insert(*id, tab.clone());
                }
            }
            self.window_hook_dirty |= selection
                != (
                    self.app.model().selected_space.clone(),
                    self.app.model().selected_tab,
                );
            self.schedule_hooks();
        }
        if !hook_completion {
            self.window_hook_dirty |= before != self.app.model().revision;
        }
        self.schedule_hooks();
        self.storage();
    }
    fn projection(&self) -> Projection {
        let projection = self.app.projection();
        Projection {
            tabs: projection.visible.iter().map(|id| *id as usize).collect(),
            active: projection.active.map(|id| id as usize),
        }
    }
    fn commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.commands)
    }
    fn render(&mut self, geometry: Geometry, now: Instant) {
        if self.geometry != geometry {
            self.app.ui_mut().invalidate();
        }
        self.geometry = geometry;
        self.app.ui_mut().set_layout(
            (geometry.sidebar.width / geometry.cell_width.max(1.))
                .floor()
                .clamp(0., u16::MAX as f32) as u16,
            (geometry.header_inset / geometry.cell_width.max(1.))
                .ceil()
                .clamp(0., u16::MAX as f32) as u16,
        );
        let metrics = self.metrics(geometry);
        if let Err(err) = self.app.resize(metrics) {
            log::error!("native tabs geometry: {err}");
        }
        let elapsed = now.saturating_duration_since(self.epoch);
        let mut frame = self.app.render(elapsed);
        // A successful form can dismiss itself while rendering the updated model.
        // Publish cells and hit regions only after their viewport matches that state.
        let settled_metrics = self.metrics(geometry);
        if settled_metrics != metrics {
            if let Err(err) = self.app.resize(settled_metrics) {
                log::error!("native tabs geometry: {err}");
            }
            frame = self.app.render(elapsed);
        }
        if let Some(frame) = frame {
            cells::update(&mut self.surface, self.app.buffer(), &frame, geometry);
            let grid_offset = geometry.grid_offset(
                self.content_page() || self.overlay_surface(),
                self.surface.columns,
            );
            self.surface.offset.0 += grid_offset.0;
            self.surface.offset.1 += grid_offset.1;
            self.cursor = frame.ime_rect.map(|r| (r.x as usize, r.y as usize));
            self.primitives.clear();
            self.primitives
                .extend(
                    self.app
                        .ui()
                        .rounded_surfaces()
                        .iter()
                        .map(|shape| RoundedSurface {
                            bounds: Bounds {
                                x: shape.rect.x as f32,
                                y: shape.rect.y as f32,
                                width: shape.rect.width as f32,
                                height: shape.rect.height as f32,
                            },
                            radius: shape.radius,
                            inset: shape.inset,
                            fill: linear_color(shape.fill),
                            border: linear_color(shape.border),
                            border_width: if shape.fill == shape.border { 0. } else { 1. },
                        }),
                );
            if let Some(cursor) = frame.cursor {
                self.primitives.push(RoundedSurface {
                    bounds: Bounds {
                        x: cursor.x as f32,
                        y: cursor.y as f32,
                        width: 0.12,
                        height: 1.,
                    },
                    radius: 0.,
                    inset: 0.,
                    fill: linear_color(self.app.ui().theme.accent),
                    border: linear_color(self.app.ui().theme.accent),
                    border_width: 0.,
                });
            }
        }
        self.storage();
    }
    fn background(&self) -> Option<window::color::LinearRgba> {
        ui::Theme::parse_color(&self.app.model().settings.background).map(linear_color)
    }
    fn content_page(&self) -> bool {
        self.app.ui().content_page()
    }
    fn overlay_surface(&self) -> bool {
        self.app.ui().overlay_surface()
    }
    fn primitives(&self) -> &[RoundedSurface] {
        &self.primitives
    }
    fn surface(&self) -> &Surface {
        &self.surface
    }
    fn deadline(&self) -> Option<Instant> {
        self.app
            .next_deadline()
            .map(|duration| self.epoch + duration)
    }
    fn caret(&self) -> Option<(usize, usize)> {
        self.cursor
    }
    fn shutdown(self: Box<Self>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()>>> {
        Box::pin(shutdown::drain(self.app, self.outstanding))
    }
    fn move_context(&self, tab: usize) -> serde_json::Value {
        match self.app.export_transfer(tab as u64) {
            Ok(transfer) => serde_json::json!({"new_window":true,"transfer":transfer}),
            Err(error) => {
                log::warn!("native tabs move: {error}");
                serde_json::Value::Null
            }
        }
    }
    fn prepare_command(&self, new_window: bool, command: &mut config::keyassignment::SpawnCommand) {
        if (self.app.model().private && !new_window) || self.next_private {
            command
                .set_environment_variables
                .extend(self.app.model().settings.private_env.clone());
        }
        if self.app.model().selected_tab.is_none()
            && matches!(
                command.domain,
                config::keyassignment::SpawnTabDomain::CurrentPaneDomain
            )
        {
            if let Some(domain) = &self.app.model().settings.default_domain {
                command.domain = config::keyassignment::SpawnTabDomain::DomainName(domain.clone());
            }
        }
    }
    fn reserve_spawn(
        &mut self,
        new_window: bool,
        command: &config::keyassignment::SpawnCommand,
    ) -> serde_json::Value {
        let token = if new_window {
            None
        } else {
            let space = self.next_spawn_space.take();
            let token = match self.next_spawn_folder.take() {
                Some(folder) => self.app.reserve_spawn_in_folder(&folder).ok(),
                None => match space {
                    Some(space) => self.app.reserve_spawn_in(&space).ok(),
                    None => Some(self.app.reserve_spawn()),
                },
            };
            token
        };
        let domain = match &command.domain {
            config::keyassignment::SpawnTabDomain::DomainName(name) => Some(name.clone()),
            config::keyassignment::SpawnTabDomain::DefaultDomain => {
                Some(mux::Mux::get().default_domain().domain_name().to_string())
            }
            config::keyassignment::SpawnTabDomain::DomainId(id) => mux::Mux::get()
                .get_domain(*id)
                .map(|d| d.domain_name().to_string()),
            config::keyassignment::SpawnTabDomain::CurrentPaneDomain => self
                .app
                .model()
                .selected_tab
                .and_then(|id| self.app.model().tabs.get(&id))
                .map(|tab| tab.domain.clone()),
        };
        let launch = core::LaunchSpec {
            domain: domain.clone(),
            cwd: command
                .cwd
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .or_else(|| {
                    self.app
                        .model()
                        .selected_tab
                        .and_then(|id| self.app.model().tabs.get(&id))
                        .filter(|tab| domain.as_deref() == Some(tab.domain.as_str()))
                        .map(|tab| tab.cwd.clone())
                }),
            args: command.args.clone().unwrap_or_default(),
            env: command
                .set_environment_variables
                .clone()
                .into_iter()
                .collect(),
        };
        let private = std::mem::take(&mut self.next_private);
        serde_json::json!({"token":token,"new_window":new_window,"private":private,"launch":launch})
    }
    fn inspect(&self) -> serde_json::Value {
        let bounds = self
            .geometry
            .ui_bounds(self.content_page() || self.overlay_surface());
        serde_json::json!({"folders":self.app.model().folders,"settings_page":self.content_page(),"overlay_surface":self.overlay_surface(),"grid":{"x":bounds.x+self.surface.offset.0,"y":bounds.y+self.surface.offset.1,"columns":self.surface.columns,"rows":self.surface.rows.len(),"cell_width":self.geometry.cell_width,"cell_height":self.geometry.cell_height},"hits":self.app.ui().hit_regions().iter().map(|h|serde_json::json!({"id":format!("{:?}",h.id),"x":h.rect.x,"y":h.rect.y,"width":h.rect.width,"height":h.rect.height})).collect::<Vec<_>>(),"spaces":self.app.model().spaces,"selected_space":self.app.model().selected_space,"settings":self.app.model().settings,"surface_revision":self.surface.revision,"private":self.app.model().private,"can_reopen":self.app.model().can_reopen(), "tabs":self.app.model().tabs.values().map(|tab|serde_json::json!({"id":tab.id,"pinned":tab.pinned,"space_id":tab.space_id,"folder_id":tab.folder_id})).collect::<Vec<_>>()})
    }
    fn keyboard_focus(&self) -> bool {
        self.app.is_modal() || (self.reservation().width > 0. && self.app.ui().focused().is_some())
    }
}

fn spawn(launch: core::LaunchSpec) -> config::keyassignment::SpawnCommand {
    config::keyassignment::SpawnCommand {
        args: (!launch.args.is_empty()).then_some(launch.args),
        cwd: launch.cwd.map(Into::into),
        set_environment_variables: launch.env.into_iter().collect(),
        domain: launch
            .domain
            .map(config::keyassignment::SpawnTabDomain::DomainName)
            .unwrap_or(config::keyassignment::SpawnTabDomain::CurrentPaneDomain),
        ..Default::default()
    }
}
fn modifiers(m: window::Modifiers) -> ui::Modifiers {
    ui::Modifiers {
        shift: m.contains(window::Modifiers::SHIFT),
        control: m.contains(window::Modifiers::CTRL),
        alt: m.contains(window::Modifiers::ALT),
        super_key: m.contains(window::Modifiers::SUPER),
    }
}
fn mouse_button(button: MousePress) -> ui::MouseButton {
    match button {
        MousePress::Right => ui::MouseButton::Right,
        MousePress::Middle => ui::MouseButton::Middle,
        _ => ui::MouseButton::Left,
    }
}

#[cfg(test)]
mod native_modal_tests {
    use super::*;

    #[test]
    fn native_form_completion_publishes_settled_grid_and_pointer_origin() {
        config::designate_this_as_the_main_thread();
        for right in [false, true] {
            let mut adapter = Adapter::new(1);
            adapter
                .app
                .config(serde_json::json!({"settings":{
                    "animations":false,"side":if right {"right"} else {"left"}
                }}))
                .unwrap();
            let geometry = Geometry {
                sidebar: Bounds {
                    x: if right { 1140. } else { 0. },
                    y: 0.,
                    width: 256.,
                    height: 920.,
                },
                content: Bounds {
                    x: if right { 0. } else { 256. },
                    y: 0.,
                    width: 1140.,
                    height: 920.,
                },
                cell_width: 10.,
                cell_height: 22.,
                dpi: 96.,
                ..Geometry::default()
            };
            adapter.render(geometry, Instant::now());
            adapter.app.open_create_space();
            adapter.render(geometry, Instant::now());
            assert_eq!(adapter.surface.columns, 139);
            adapter.ui_input(ui::UiInput::Text("Complete".into()));
            adapter.ui_input(ui::UiInput::Key {
                key: ui::Key::Enter,
                modifiers: ui::Modifiers::default(),
            });
            assert!(adapter.overlay_surface());
            adapter.render(geometry, Instant::now());
            assert!(!adapter.overlay_surface());
            assert_eq!(adapter.surface.columns, 25);
            assert_eq!(adapter.app.buffer().area.width, 25);
            assert_eq!(adapter.surface.offset, (0., 0.));
            assert_eq!(adapter.inspect()["grid"]["x"], geometry.sidebar.x);
            assert!(adapter
                .app
                .ui()
                .hit_regions()
                .iter()
                .all(|hit| hit.rect.right() <= 25));
        }
    }

    #[test]
    fn hidden_and_collapsed_native_modals_use_window_bounds_without_resizing_the_rail() {
        // Adapter construction reads the GUI's current Lua configuration. Initialize
        // the same thread-local host context as wezterm-gui startup before exercising it.
        config::designate_this_as_the_main_thread();
        for rail in ["hidden", "collapsed"] {
            for action in ["native_navigator", "settings", "create_space"] {
                let mut adapter = Adapter::new(1);
                adapter
                    .app
                    .config(serde_json::json!({"settings":{
                        "rail":rail,"width":256,"rail_width":48
                    }}))
                    .unwrap();
                let saved = serde_json::to_value(&adapter.app.model().settings).unwrap();
                let original = adapter.reservation().width;
                if action == "native_navigator" {
                    adapter.navigation(Navigation::Navigator);
                } else {
                    adapter.message(serde_json::json!({"action":action}));
                }
                assert_eq!(adapter.reservation().width, original);
                assert!(adapter.keyboard_focus());
                let mut geometry = Geometry::default();
                geometry.sidebar.width = adapter.reservation().width;
                geometry.sidebar.height = 480.;
                geometry.content = Bounds {
                    x: original,
                    y: 0.,
                    width: 800.,
                    height: 480.,
                };
                geometry.cell_width = 8.;
                geometry.cell_height = 20.;
                geometry.dpi = 96.;
                adapter.render(geometry, Instant::now());
                assert_eq!(adapter.surface.columns, ((original + 800.) / 8.) as usize);
                assert_eq!(adapter.content_page(), action == "settings");
                assert_eq!(adapter.overlay_surface(), action != "settings");
                let inspected = adapter.inspect();
                assert_eq!(inspected["overlay_surface"], action != "settings");
                assert_eq!(inspected["grid"]["x"], 0.);
                assert_eq!(inspected["grid"]["y"], 0.);
                assert_eq!(inspected["grid"]["columns"], adapter.surface.columns);
                if action != "settings" {
                    let caret = adapter
                        .caret()
                        .expect("centered editor has an IME rectangle");
                    assert!(caret.0 > (original / 8.) as usize);
                }
                assert!(!adapter.surface.rows.is_empty());
                assert_eq!(adapter.surface.offset, (0., 0.));
                assert_eq!(adapter.surface.opacity, 1.);
                let text = adapter
                    .app
                    .buffer()
                    .content
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>();
                let title = match action {
                    "settings" => "Settings",
                    "create_space" => "Create space",
                    _ => "⌕",
                };
                assert!(text.contains(title), "modal was not composed: {}", text);
                let escape = window::KeyEvent {
                    key: KeyCode::Char('\u{1b}'),
                    modifiers: window::Modifiers::NONE,
                    leds: Default::default(),
                    repeat_count: 1,
                    key_is_down: true,
                    raw: None,
                    #[cfg(windows)]
                    win32_uni_char: None,
                };
                assert!(adapter.input(Input::Key(&escape)));
                assert!(!adapter.app.is_modal());
                assert!(!adapter.keyboard_focus());
                assert_eq!(adapter.reservation().width, original);
                adapter.render(geometry, Instant::now());
                assert_eq!(adapter.surface.columns, (original / 8.) as usize);
                assert_eq!(
                    serde_json::to_value(&adapter.app.model().settings).unwrap(),
                    saved
                );
            }
        }
    }
}

fn linear_color(color: ratatui::style::Color) -> window::color::LinearRgba {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => {
            window::color::SrgbaTuple::from((r, g, b)).to_linear()
        }
        _ => window::color::LinearRgba(0., 0., 0., 0.),
    }
}

fn shortcut_key(key: &KeyCode, mods: ui::Modifiers) -> Option<ui::Key> {
    Some(match key {
        KeyCode::Physical(code) => return shortcut_key(&code.to_key_code(), mods),
        KeyCode::Char('\t') => ui::Key::Tab,
        KeyCode::Char(c) => {
            let c = if mods.control && ('\u{1}'..='\u{1a}').contains(c) {
                char::from(*c as u8 + b'a' - 1)
            } else {
                *c
            };
            let c = if mods.shift {
                match c {
                    '<' => ',',
                    '!' => '1',
                    '@' => '2',
                    '#' => '3',
                    '$' => '4',
                    '%' => '5',
                    '^' => '6',
                    '&' => '7',
                    '*' => '8',
                    '(' => '9',
                    _ => c,
                }
            } else {
                c
            };
            ui::Key::Character(c)
        }
        KeyCode::LeftArrow => ui::Key::Left,
        KeyCode::RightArrow => ui::Key::Right,
        _ => return None,
    })
}

fn editor_clipboard_key(key: &KeyCode, mods: ui::Modifiers) -> Option<ui::Key> {
    if !mods.command() || mods.alt {
        return None;
    }
    shortcut_key(key, mods).filter(|key| {
        matches!(
            key,
            ui::Key::Character('a' | 'A' | 'c' | 'C' | 'x' | 'X' | 'v' | 'V')
        )
    })
}

#[cfg(test)]
mod native_input_tests {
    use super::*;

    fn raw(key: KeyCode, mods: window::Modifiers, down: bool) -> window::RawKeyEvent {
        window::RawKeyEvent {
            key,
            modifiers: mods,
            leds: Default::default(),
            phys_code: None,
            raw_code: 0,
            #[cfg(windows)]
            scan_code: 0,
            repeat_count: 1,
            key_is_down: down,
            handled: window::Handled::new(),
        }
    }
    fn geometry() -> Geometry {
        Geometry {
            sidebar: Bounds {
                x: 0.,
                y: 0.,
                width: 256.,
                height: 480.,
            },
            content: Bounds {
                x: 256.,
                y: 0.,
                width: 800.,
                height: 480.,
            },
            cell_width: 8.,
            cell_height: 20.,
            dpi: 96.,
            ..Geometry::default()
        }
    }
    #[test]
    fn native_shortcuts_precede_bindings_and_do_not_fire_on_release() {
        config::designate_this_as_the_main_thread();
        let mut adapter = Adapter::new(9841);
        let mods = if cfg!(target_os = "macos") {
            window::Modifiers::SUPER
        } else {
            window::Modifiers::CTRL | window::Modifiers::SHIFT
        };
        assert!(!adapter.keyboard_focus());
        assert!(adapter.input(Input::RawKey(&raw(KeyCode::Char(','), mods, true))));
        assert!(adapter.content_page());
        assert!(adapter.input(Input::RawKey(&raw(KeyCode::Char(','), mods, false))));
        assert!(adapter.content_page());
        adapter.render(geometry(), Instant::now());
        assert_eq!(adapter.surface.columns, 132);
        assert_eq!(adapter.reservation().width, 256.);
        assert!(adapter.input(Input::RawKey(&raw(KeyCode::Char(','), mods, true))));
        assert!(!adapter.content_page());
        assert!(!adapter.input(Input::RawKey(&raw(
            KeyCode::Char('t'),
            window::Modifiers::CTRL,
            true
        ))));
    }
    #[test]
    fn native_shifted_and_control_encoded_keys_resolve_shortcuts() {
        let mods = ui::Modifiers {
            control: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            shortcut_key(&KeyCode::Char('<'), mods),
            Some(ui::Key::Character(','))
        );
        assert_eq!(
            shortcut_key(&KeyCode::Char('!'), mods),
            Some(ui::Key::Character('1'))
        );
        assert_eq!(
            shortcut_key(&KeyCode::Char('\u{7}'), mods),
            Some(ui::Key::Character('g'))
        );
        assert_eq!(shortcut_key(&KeyCode::Char('\t'), mods), Some(ui::Key::Tab));
    }

    fn logical_key(key: KeyCode, mods: window::Modifiers) -> window::KeyEvent {
        window::KeyEvent {
            key,
            modifiers: mods,
            leds: Default::default(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        }
    }

    fn paste_token(adapter: &mut Adapter) -> u64 {
        adapter
            .commands()
            .into_iter()
            .find_map(|command| match command {
                Command::Paste(token) => Some(token),
                _ => None,
            })
            .expect("native clipboard read request")
    }

    fn copied_text(adapter: &mut Adapter) -> String {
        adapter
            .commands()
            .into_iter()
            .find_map(|command| match command {
                Command::Clipboard(text) => Some(text),
                _ => None,
            })
            .expect("native clipboard write request")
    }

    #[test]
    fn native_editors_decode_clipboard_control_bytes_with_global_shortcuts_disabled() {
        config::designate_this_as_the_main_thread();
        for open_search in [false, true] {
            let mut adapter = Adapter::new(9843);
            adapter
                .app
                .config(serde_json::json!({"settings":{"keyboard_shortcuts":false}}))
                .unwrap();
            if open_search {
                adapter.app.open_tab_navigator();
            } else {
                adapter.app.open_create_space();
            }
            adapter.ui_input(ui::UiInput::Text("Copy 界".into()));
            assert!(adapter.input(Input::Key(&logical_key(
                KeyCode::Char('\u{1}'),
                window::Modifiers::CTRL
            ))));
            assert!(adapter.input(Input::Key(&logical_key(
                KeyCode::Char('\u{3}'),
                window::Modifiers::CTRL
            ))));
            assert_eq!(copied_text(&mut adapter), "Copy 界");
            assert!(adapter.input(Input::Key(&logical_key(
                KeyCode::Char('\u{16}'),
                window::Modifiers::CTRL
            ))));
            let token = paste_token(&mut adapter);
            adapter.message(serde_json::json!({"paste":"Pasted 界","token":token}));
            adapter.input(Input::Key(&logical_key(
                KeyCode::Char('\u{1}'),
                window::Modifiers::CTRL,
            )));
            adapter.input(Input::Key(&logical_key(
                KeyCode::Char('\u{3}'),
                window::Modifiers::CTRL,
            )));
            assert_eq!(copied_text(&mut adapter), "Pasted 界");
        }
    }

    #[test]
    fn native_raw_clipboard_commands_are_scoped_to_text_focus() {
        config::designate_this_as_the_main_thread();
        for mods in [
            window::Modifiers::CTRL,
            window::Modifiers::CTRL | window::Modifiers::SHIFT,
            window::Modifiers::SUPER,
        ] {
            let mut adapter = Adapter::new(9844);
            assert!(!adapter.input(Input::RawKey(&raw(KeyCode::Char('c'), mods, true))));
            adapter.app.open_create_space();
            adapter.ui_input(ui::UiInput::Text("Native copy".into()));
            assert!(adapter.input(Input::RawKey(&raw(KeyCode::Char('a'), mods, true))));
            assert!(adapter.input(Input::RawKey(&raw(KeyCode::Char('c'), mods, true))));
            assert_eq!(copied_text(&mut adapter), "Native copy");
            assert!(adapter.input(Input::RawKey(&raw(KeyCode::Char('c'), mods, false))));
            assert!(adapter.commands().is_empty());
        }
    }

    #[test]
    fn native_paste_survives_hover_and_empty_composition_notifications() {
        config::designate_this_as_the_main_thread();
        let mut adapter = Adapter::new(9845);
        adapter.app.open_create_space();
        adapter.ui_input(ui::UiInput::Text("Before".into()));
        adapter.input(Input::RawKey(&raw(
            KeyCode::Char('a'),
            window::Modifiers::CTRL,
            true,
        )));
        adapter.input(Input::RawKey(&raw(
            KeyCode::Char('v'),
            window::Modifiers::CTRL,
            true,
        )));
        let token = paste_token(&mut adapter);
        adapter.ui_input(ui::UiInput::PointerMove {
            x: u16::MAX,
            y: u16::MAX,
        });
        adapter.ui_input(ui::UiInput::ImePreedit {
            text: String::new(),
            cursor: None,
        });
        adapter.message(serde_json::json!({"paste":"After","token":token}));
        adapter.input(Input::RawKey(&raw(
            KeyCode::Char('a'),
            window::Modifiers::CTRL,
            true,
        )));
        adapter.input(Input::RawKey(&raw(
            KeyCode::Char('c'),
            window::Modifiers::CTRL,
            true,
        )));
        assert_eq!(copied_text(&mut adapter), "After");
    }

    #[test]
    fn native_paste_is_discarded_after_editing_focus_changes() {
        config::designate_this_as_the_main_thread();
        let mut adapter = Adapter::new(9846);
        adapter.app.open_create_space();
        adapter.ui_input(ui::UiInput::Text("Keep".into()));
        adapter.input(Input::RawKey(&raw(
            KeyCode::Char('v'),
            window::Modifiers::CTRL,
            true,
        )));
        let token = paste_token(&mut adapter);
        adapter.input(Input::Focus(false));
        adapter.input(Input::Focus(true));
        adapter.message(serde_json::json!({"paste":"Discard","token":token}));
        adapter.input(Input::RawKey(&raw(
            KeyCode::Char('a'),
            window::Modifiers::CTRL,
            true,
        )));
        adapter.input(Input::RawKey(&raw(
            KeyCode::Char('c'),
            window::Modifiers::CTRL,
            true,
        )));
        assert_eq!(copied_text(&mut adapter), "Keep");
    }

    #[test]
    fn native_composed_release_does_not_duplicate_text_and_caret_is_visible() {
        config::designate_this_as_the_main_thread();
        let mut adapter = Adapter::new(9842);
        adapter.app.ui_mut().open_create_space();
        let key = window::KeyEvent {
            key: KeyCode::Composed("Hello".into()),
            modifiers: window::Modifiers::NONE,
            leds: Default::default(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        };
        assert!(adapter.input(Input::Key(&key)));
        let mut release = key.clone();
        release.key_is_down = false;
        assert!(adapter.input(Input::Key(&release)));
        adapter.render(geometry(), Instant::now());
        let text = adapter
            .app
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Hello"));
        assert!(!text.contains("HelloHello"));
        assert!(adapter
            .primitives
            .iter()
            .any(|shape| shape.bounds.width == 0.12));
        adapter.pointer_captured = true;
        adapter.input(Input::Focus(false));
        assert!(!adapter.pointer_captured);
        adapter.render(geometry(), Instant::now());
        assert!(!adapter
            .primitives
            .iter()
            .any(|shape| shape.bounds.width == 0.12));
    }
}
