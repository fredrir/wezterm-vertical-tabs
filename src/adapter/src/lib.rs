//! The only product code coupled to WezTerm's internal APIs.
mod adoption;
mod cells;
mod centering;
mod directory;
mod facts;
mod foreign;
mod hooks;
mod keys;
mod location;
mod lua;
mod message;
mod metadata;
mod mux_ops;
mod palette;
mod paste;
mod repos;
mod settings_file;
mod shutdown;
mod storage;
mod terminal_program;
mod transfers;
mod update;

use crate::termwindow::ui_host::{
    Bounds, Command, Geometry, HideOnEscape, Input, Navigation, Projection, Provider,
    QuickTerminalConfig, Reservation, RoundedSurface, Snapshot, Surface, TerminalKey,
    WindowEndpoint, spawn_completed,
};
use mux::{BACKING_WORKSPACE_PREFIX, DETACHED_WORKSPACE, QUICK_WORKSPACE_PREFIX};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use vtabs_app::{self as app, WindowApp, core, ui};
use window::{Window, WindowOps};

pub use lua::register;
pub use message::{Message, SpawnContext};

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
    palette_commands: Vec<crate::commands::ExpandedCommand>,
    launcher_entries: Vec<crate::termwindow::ui_host::launcher::Entry>,
    cursor: Option<(usize, usize)>,
    geometry: Geometry,
    config_generation: usize,
    managed_path: Option<std::path::PathBuf>,
    pointer_captured: bool,
    outstanding: Option<u64>,
    hooks: hooks::Hooks,
    input: paste::InputState,
    transfers: transfers::Transfers,
    repos: repos::Repos,
    local_host: String,
    published: Option<u64>,
    metadata: metadata::Refresh,
    adoptable_search: u64,
}

impl Adapter {
    fn endpoint(&self, mux_window_id: usize) -> Option<WindowEndpoint> {
        self.window
            .clone()
            .map(|window| WindowEndpoint::new(window, mux_window_id))
    }

    fn new(window_id: usize) -> Self {
        let config = lua::configuration();
        let mut app = WindowApp::new(&config.profile);
        app.ui_mut().set_chord_label(palette::chord_label);
        app.set_home(Some(mux::location::home()));
        if let Err(err) = app.config(config.value()) {
            log::error!("tabs config: {err}");
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
                centered: Vec::new(),
            },
            primitives: Vec::new(),
            commands: Vec::new(),
            palette_commands: Vec::new(),
            launcher_entries: Vec::new(),
            cursor: None,
            geometry: Geometry::default(),
            config_generation: usize::MAX,
            managed_path: config.managed_path,
            pointer_captured: false,
            outstanding: None,
            hooks: hooks::Hooks::default(),
            input: paste::InputState::default(),
            transfers: transfers::Transfers::default(),
            repos: repos::Repos::default(),
            local_host: sysinfo::System::host_name().unwrap_or_default(),
            published: None,
            metadata: metadata::Refresh::default(),
            adoptable_search: 0,
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
                log::warn!("tabs: {err}");
            }
        }
    }
    fn finish_update(&mut self, before: u64) {
        self.hooks.changed(before != self.app.model().revision);
        self.schedule_hooks();
        self.publish();
    }
    fn dispatch(&mut self, intent: core::Intent) {
        self.transact(|app| app.dispatch(intent));
    }
    fn transact(&mut self, run: impl FnOnce(&mut WindowApp) -> Result<app::Update, core::Error>) {
        self.cancel_paste();
        let before = self.app.model().revision;
        let result = run(&mut self.app);
        self.apply(result);
        self.finish_update(before);
    }
    fn command(&mut self, command: app::Command) {
        match command {
            app::Command::OpenCommands => self.commands.push(Command::OpenCommandPalette),
            app::Command::RunCommand(id) => {
                if let Some(entry) = self.launcher_entries.get(id).cloned() {
                    self.commands.push(Command::RunLauncherEntry(entry));
                } else if let Some(command) = self.palette_commands.get(id).cloned() {
                    self.commands.push(Command::RunPaletteCommand(command));
                }
                self.palette_commands.clear();
                self.launcher_entries.clear();
            }
            app::Command::QuickTerminal => self.show_terminal(Command::ToggleTerminalOverlay),
            app::Command::Refresh => {
                std::thread::spawn(config::reload);
                self.app.ui_mut().invalidate();
            }
            app::Command::SetClipboard(text) => self.commands.push(Command::Clipboard(text)),
            app::Command::ConfirmClose(id) => self.check_close(id, None),
            app::Command::ConfirmClosePane(tab, pane) => self.check_close(tab, Some(pane)),
            app::Command::ShowTab { window, tab } => self.show_tab(window as usize, tab),
            app::Command::SearchTabs => self.open_tab_navigator(),
            app::Command::AdoptPane { domain, pane } => self.rearrange(adoption::adopt(
                self.endpoint(self.window_id),
                self.window_id,
                domain,
                pane,
            )),
            app::Command::RequestClipboard => {
                let token = self.input.request_paste(Instant::now());
                self.commands.push(Command::Paste(token));
            }
            app::Command::Host(command) => self.host_command(command),
        }
    }
    fn show_terminal(&mut self, command: Command) {
        self.cancel_paste();
        self.app.ui_mut().interrupt();
        self.commands.push(command);
    }
    fn schedule_hooks(&mut self) {
        if let Some(endpoint) = self.endpoint(self.window_id) {
            if let Some(batch) = self.hooks.take_batch(&self.app) {
                lua::hooks(endpoint, batch.tabs, batch.window);
            }
        }
    }
    fn storage(&mut self) {
        if let Some(endpoint) = self.endpoint(self.window_id) {
            if let Some(request) = self.app.take_storage_request(self.epoch.elapsed()) {
                self.outstanding = Some(request.request_id);
                storage::request(endpoint.clone(), request);
            }
            if let Some(source) = self.app.take_managed_write() {
                match self.managed_path.clone() {
                    Some(path) => settings_file::save(endpoint, path, source),
                    None => self
                        .app
                        .complete_managed_write(Err("no settings_file configured".into())),
                }
            }
        }
        for error in self.app.take_errors() {
            log::error!("tabs: {error}");
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
    fn schedule_metadata(&mut self) {
        let schedule = metadata::interval(self.app.model(), self.hooks.enabled())
            .filter(|_| !self.transfers.hosts().is_empty())
            .map(|interval| {
                (
                    interval.clamp(Duration::from_millis(500), Duration::from_secs(2)),
                    self.config_generation,
                )
            });
        if let Some(window) = &self.window {
            self.metadata.schedule(window, self.window_id, schedule);
        }
    }
}

impl Provider for Adapter {
    fn initialize(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>> {
        Box::pin(async move {
            // Preferences load before the first reservation; failures stay queued for retry.
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
    fn header_fits(&self, sidebar_width: f32, cell_width: f32, inset: f32) -> bool {
        let cell_width = cell_width.max(1.);
        ui::header_fits(
            (sidebar_width / cell_width)
                .floor()
                .clamp(0., u16::MAX as f32) as u16,
            (inset / cell_width).ceil().clamp(0., u16::MAX as f32) as u16,
        )
    }
    fn bind(&mut self, window: Window) {
        self.window = Some(window);
        update::schedule();
        self.storage();
    }
    fn snapshot(&mut self, snapshot: Snapshot) {
        let facts = facts::Facts::of(snapshot);
        self.metadata
            .observe(facts.tabs.iter().map(|tab| (tab.id, tab.process.clone())));
        self.apply_facts(facts);
    }
    fn suspend(&mut self) {
        self.metadata.cancel();
    }
    fn navigation(&mut self, navigation: Navigation) {
        if self.content_page() {
            match navigation {
                // The tab beneath the page is the one it was opened from.
                Navigation::Last => return self.app.ui_mut().hide_settings(),
                Navigation::Move(index) => return self.app.move_settings(index),
                Navigation::MoveRelative(delta) => return self.app.move_settings_relative(delta),
                _ => {}
            }
        }
        let intent = match navigation {
            Navigation::Index(index) => return self.transact(|app| app.activate_index(index)),
            Navigation::Relative(delta, wrap) => {
                return self.transact(|app| app.activate_relative(delta, wrap));
            }
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
                self.open_tab_navigator();
                return;
            }
            Navigation::ClosePage => {
                self.app.ui_mut().close_settings();
                return;
            }
        };
        self.dispatch(intent);
    }
    fn open_command_palette(&mut self, commands: Vec<crate::commands::ExpandedCommand>) {
        self.show_palette(commands);
    }
    fn open_launcher(
        &mut self,
        args: config::keyassignment::LauncherActionArgs,
        launcher: crate::termwindow::ui_host::launcher::LauncherEntries,
    ) {
        self.show_launcher(args, launcher);
    }
    fn terminal_input(&mut self, input: Input<'_>, escape: HideOnEscape) -> TerminalKey {
        self.terminal_key(input, escape)
    }
    fn input(&mut self, input: Input<'_>) -> bool {
        self.handle_input(input)
    }
    fn message(&mut self, message: Message) {
        self.handle_message(message);
    }
    fn projection(&self) -> Projection {
        let projection = self.app.projection();
        Projection {
            tabs: projection.visible.iter().map(|id| *id as usize).collect(),
            active: projection.active.map(|id| id as usize),
        }
    }
    fn commands(&mut self) -> Vec<Command> {
        self.schedule_metadata();
        std::mem::take(&mut self.commands)
    }
    fn render(&mut self, geometry: Geometry, now: Instant) {
        self.expire_paste(now);
        if self.geometry != geometry {
            self.app.ui_mut().invalidate();
        }
        self.geometry = geometry;
        self.app.ui_mut().set_page_background(
            geometry
                .page_background
                .map(|(r, g, b)| ratatui::style::Color::Rgb(r, g, b)),
        );
        self.app.ui_mut().set_layout(
            (geometry.sidebar.width / geometry.cell_width.max(1.))
                .floor()
                .clamp(0., u16::MAX as f32) as u16,
            (geometry.header_inset / geometry.cell_width.max(1.))
                .ceil()
                .clamp(0., u16::MAX as f32) as u16,
        );
        let elapsed = now.saturating_duration_since(self.epoch);
        self.app.prepare_render(elapsed);
        let metrics = self.metrics(geometry);
        if let Err(err) = self.app.resize(metrics) {
            log::error!("tabs geometry: {err}");
        }
        let frame = self.app.render(elapsed);
        if let Some(frame) = frame {
            cells::update(&mut self.surface, self.app.buffer(), &frame, geometry);
            let grid_offset = cells::grid_offset(
                geometry,
                self.content_page() || self.overlay_surface(),
                self.surface.columns,
            );
            self.surface.offset.0 += grid_offset.0;
            self.surface.offset.1 += grid_offset.1;
            self.cursor = frame.ime_rect.map(|r| (r.x as usize, r.y as usize));
            self.primitives.clear();
            let rows = self.surface.rows.len();
            let centered = &mut self.surface.centered;
            centered.resize_with(rows, Vec::new);
            centered.iter_mut().for_each(Vec::clear);
            for shape in self.app.ui().rounded_surfaces() {
                let bounds = Bounds {
                    x: shape.rect.x as f32,
                    y: shape.rect.y as f32
                        + shape.shift_y
                        + shape.rect.height as f32 * (1. - shape.scale_y) / 2.,
                    width: shape.rect.width as f32,
                    height: shape.rect.height as f32 * shape.scale_y,
                };
                centering::add(centered, bounds, shape.stacked, geometry.cell_width);
                self.primitives.push(RoundedSurface {
                    bounds,
                    radius: shape.radius,
                    inset: shape.inset,
                    square: shape.square,
                    fill: linear_color(shape.fill),
                });
            }
            if let Some(cursor) = frame.cursor {
                self.primitives.push(RoundedSurface {
                    bounds: Bounds {
                        x: cursor.x as f32,
                        y: cursor.y as f32 + frame.cursor_shift,
                        width: 0.12,
                        height: 1.,
                    },
                    radius: 0.,
                    inset: 0.,
                    square: false,
                    fill: linear_color(self.app.ui().theme().accent),
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
            .into_iter()
            .chain(self.input.deadline())
            .min()
    }
    fn caret(&self) -> Option<(usize, usize)> {
        self.cursor
    }
    fn shutdown(self: Box<Self>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()>>> {
        // A window holding only a detached domain's tabs closes before any snapshot drops them.
        for id in self.app.model().tabs.keys() {
            self.remember_detached(*id);
        }
        directory::withdraw(self.window_id);
        Box::pin(shutdown::drain(self.app, self.outstanding))
    }
    fn glyph_scale(&self) -> f64 {
        self.app.model().settings.glyph_size
    }
    fn quick_terminal_config(&self) -> QuickTerminalConfig {
        terminal_program::quick_terminal(&self.app.model().settings)
    }
    fn prepare_command(&self, command: &mut config::keyassignment::SpawnCommand) {
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
    ) -> SpawnContext {
        let token = self.transfers.reserve_spawn(&mut self.app, new_window);
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
        SpawnContext {
            token,
            new_window,
            launch: Some(launch),
            transfer: None,
        }
    }
    fn inspect(&self) -> serde_json::Value {
        let bounds = self
            .geometry
            .ui_bounds(self.content_page() || self.overlay_surface());
        serde_json::json!({"folders":self.app.model().folders,"settings_page":self.content_page(),"overlay_surface":self.overlay_surface(),"grid":{"x":bounds.x+self.surface.offset.0,"y":bounds.y+self.surface.offset.1,"columns":self.surface.columns,"rows":self.surface.rows.len(),"cell_width":self.geometry.cell_width,"cell_height":self.geometry.cell_height},"hits":self.app.ui().hit_regions().iter().map(|h|serde_json::json!({"id":format!("{:?}",h.id),"x":h.rect.x,"y":h.rect.y,"width":h.rect.width,"height":h.rect.height})).collect::<Vec<_>>(),"spaces":self.app.model().spaces,"selected_space":self.app.model().selected_space,"settings":self.app.model().settings,"surface_revision":self.surface.revision,"tabs":self.app.model().tabs.values().map(|tab|serde_json::json!({"id":tab.id,"pinned":tab.pinned,"space_id":tab.space_id,"folder_id":tab.folder_id})).collect::<Vec<_>>()})
    }
    fn keyboard_focus(&self) -> bool {
        self.app.is_modal() || (self.reservation().width > 0. && self.app.ui().focused().is_some())
    }
    fn text_input_active(&self) -> bool {
        self.app.ui().text_input_active()
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

#[cfg(test)]
#[path = "../tests/support.rs"]
mod support;

#[cfg(test)]
#[path = "../tests/modal.rs"]
mod modal_tests;
