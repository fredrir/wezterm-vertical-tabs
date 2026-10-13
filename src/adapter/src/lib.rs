//! The only product code coupled to WezTerm's internal APIs.
mod adoption;
mod cells;
mod centering;
mod commands;
mod directory;
mod facts;
mod hooks;
mod input;
mod location;
mod lua;
mod message;
mod metadata;
mod repos;
mod settings_file;
mod shutdown;
mod storage;
mod update;

use crate::termwindow::ui_host::{
    Bounds, Command, Geometry, HideOnEscape, Input, Navigation, Projection, Provider,
    QuickTerminalConfig, Reservation, RoundedSurface, Snapshot, Surface, TerminalBorder,
    TerminalKey, TerminalPadding, TerminalProgram, spawn_completed,
};
use config::keyassignment::{SpawnCommand, SpawnTabDomain};
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

const PLACEMENT_TIMEOUT: Duration = Duration::from_secs(3);

struct Adapter {
    app: WindowApp,
    window_id: usize,
    epoch: Instant,
    window: Option<Window>,
    surface: Surface,
    primitives: Vec<RoundedSurface>,
    commands: Vec<Command>,
    palette_commands: Vec<crate::commands::ExpandedCommand>,
    launcher_entries: Vec<crate::overlay::launcher::Entry>,
    cursor: Option<(usize, usize)>,
    geometry: Geometry,
    host_tabs: Vec<usize>,
    remote_tabs: HashMap<u64, (usize, usize, Vec<usize>)>,
    config_generation: usize,
    managed_path: Option<std::path::PathBuf>,
    pointer_captured: bool,
    next_spawn_space: Option<String>,
    next_spawn_folder: Option<String>,
    outstanding: Option<u64>,
    hooks: hooks::Hooks,
    input: input::InputState,
    repos: repos::Repos,
    local_host: String,
    mux_window: Option<usize>,
    placement: Option<(usize, Instant)>,
    published: Option<u64>,
    pending_show: Option<core::TabId>,
    metadata: metadata::Refresh,
    adoptable_search: u64,
}

impl Adapter {
    fn new(window_id: usize) -> Self {
        let config = lua::configuration();
        let mut app = WindowApp::new(&config.profile);
        app.set_window_identity(window_id as u64);
        app.ui_mut().set_chord_label(commands::chord_label);
        app.set_home(home_dir());
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
            host_tabs: Vec::new(),
            remote_tabs: HashMap::new(),
            config_generation: usize::MAX,
            managed_path: config.managed_path,
            pointer_captured: false,
            next_spawn_space: None,
            next_spawn_folder: None,
            outstanding: None,
            hooks: hooks::Hooks::default(),
            input: input::InputState::default(),
            repos: repos::Repos::default(),
            local_host: sysinfo::System::host_name().unwrap_or_default(),
            mux_window: None,
            placement: None,
            published: None,
            pending_show: None,
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
    /// Local panes answer at once. A client pane cannot see its process, so its owning
    /// server is asked and the verdict returns as a `close_check` message.
    fn check_close(&mut self, tab: core::TabId, pane: Option<core::PaneId>) {
        let Some(host) = mux::Mux::try_get().and_then(|mux| mux.get_tab(tab as usize)) else {
            return self.close_checked(tab, pane, None);
        };
        let reason = if pane.is_some() {
            mux::pane::CloseReason::Pane
        } else {
            mux::pane::CloseReason::Tab
        };
        let (remote, local): (Vec<_>, Vec<_>) = host
            .iter_panes_ignoring_zoom()
            .into_iter()
            .map(|entry| entry.pane)
            .filter(|entry| pane.is_none_or(|pane| entry.pane_id() as u64 == pane))
            .partition(|entry| {
                entry
                    .downcast_ref::<wezterm_client::pane::ClientPane>()
                    .is_some()
            });
        let busy = local
            .iter()
            .find(|entry| !entry.can_close_without_prompting(reason))
            .map(|entry| process_name(entry.as_ref()));
        if busy.is_some() || remote.is_empty() {
            return self.close_checked(tab, pane, busy);
        }
        let (window, id) = (self.window.clone(), self.mux_window);
        promise::spawn::spawn(async move {
            let mut busy = None;
            for entry in &remote {
                let Some(client) = entry.downcast_ref::<wezterm_client::pane::ClientPane>() else {
                    continue;
                };
                // A stock server cannot answer; keep upstream's cautious prompt for it.
                match client.close_info().await {
                    Ok(info) if !info.prompt => {}
                    Ok(info) => busy = Some(info.process),
                    Err(_) => busy = Some(String::new()),
                }
                if busy.is_some() {
                    break;
                }
            }
            if let (Some(window), Some(id)) = (window, id) {
                window.notify(crate::termwindow::TermWindowNotif::Apply(Box::new(
                    move |tw| {
                        tw.vtabs_message_for(
                            id,
                            Message::CloseCheck {
                                tab,
                                pane,
                                process: busy,
                            },
                        );
                    },
                )));
            }
        })
        .detach();
    }
    fn close_checked(
        &mut self,
        tab: core::TabId,
        pane: Option<core::PaneId>,
        busy: Option<String>,
    ) {
        match (pane, busy) {
            (None, Some(process)) => self.app.ui_mut().confirm_close_tab(tab, &process),
            (None, None) => self.dispatch(core::Intent::CloseTab(tab)),
            (Some(pane), Some(process)) => {
                self.app.ui_mut().confirm_close_pane(tab, pane, &process)
            }
            (Some(pane), None) => {
                self.command(app::Command::Host(core::HostCommand::KillPane(pane)))
            }
        }
    }
    /// Each pane lands as a split beside the target tab's active pane.
    fn join(&mut self, panes: Vec<usize>, tab: usize) {
        let Some(beside) = mux::Mux::get()
            .get_tab(tab)
            .and_then(|tab| tab.get_active_pane())
            .map(|pane| pane.pane_id())
        else {
            return;
        };
        self.rearrange(async move {
            for pane in panes {
                mux::Mux::get()
                    .split_pane(
                        beside,
                        mux::tab::SplitRequest {
                            target_is_second: true,
                            ..Default::default()
                        },
                        mux::domain::SplitSource::MovePane(pane),
                        config::keyassignment::SpawnTabDomain::CurrentPaneDomain,
                    )
                    .await?;
            }
            Ok(())
        });
    }
    /// The mux moves panes asynchronously; a refusal surfaces as the sidebar's error.
    fn rearrange(&self, work: impl std::future::Future<Output = anyhow::Result<()>> + 'static) {
        let window = self.window.clone();
        let id = self.mux_window;
        promise::spawn::spawn(async move {
            let Err(error) = work.await else { return };
            log::warn!("tabs: {error:#}");
            if let (Some(window), Some(id)) = (window, id) {
                window.notify(crate::termwindow::TermWindowNotif::Apply(Box::new(
                    move |tw| {
                        tw.vtabs_message_for(id, Message::Error(format!("{error:#}")));
                    },
                )));
            }
        })
        .detach();
    }
    fn finish_update(&mut self, before: u64) {
        self.hooks.window_dirty |= before != self.app.model().revision;
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
        use core::HostCommand as C;
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
            app::Command::AdoptPane { domain, pane } => {
                if let Some(id) = self.mux_window {
                    self.rearrange(adoption::adopt(self.window.clone(), id, domain, pane));
                }
            }
            app::Command::RequestClipboard => {
                let token = self.input.request_paste(Instant::now());
                self.commands.push(Command::Paste(token));
            }
            app::Command::Host(command) => match command {
                C::Activate(id) => self.commands.push(Command::Activate(id as usize)),
                C::Close(id) => self.commands.push(Command::Close(id as usize, false)),
                C::Rename { id, title } => {
                    if let Some(tab) = mux::Mux::get().get_tab(id as usize) {
                        tab.set_title(&title);
                    }
                }
                C::Spawn {
                    space_id,
                    launch,
                    folder_id,
                } => {
                    self.next_spawn_folder = folder_id;
                    self.next_spawn_space = Some(space_id);
                    self.commands.push(Command::Spawn(spawn(launch), false));
                }
                C::Reorder { visible_order } => self.reorder(visible_order),
                C::MoveTabToNewWindow(id) => self.move_to_new_window(id as usize),
                C::KillPane(pane) => {
                    let mux = mux::Mux::get();
                    if let Some((_, _, tab)) = mux.resolve_pane_id(pane as usize) {
                        if let Some(tab) = mux.get_tab(tab) {
                            tab.kill_pane(pane as usize);
                        }
                    }
                }
                C::DetachPane { pane, index } => {
                    self.placement = index.map(|index| (index, Instant::now()));
                    let window = self.mux_window;
                    self.rearrange(async move {
                        mux::Mux::get()
                            .move_pane_to_new_tab(pane as usize, window, None)
                            .await
                            .map(|_| ())
                    });
                }
                C::JoinPane { pane, tab } => self.join(vec![pane as usize], tab as usize),
                C::JoinTab { source, tab } => {
                    let panes = mux::Mux::get()
                        .get_tab(source as usize)
                        .map(|tab| tab.iter_panes_ignoring_zoom())
                        .unwrap_or_default()
                        .iter()
                        .map(|entry| entry.pane.pane_id())
                        .collect();
                    self.join(panes, tab as usize);
                }
                C::FocusPane { tab, pane } => {
                    let mux = mux::Mux::get();
                    if let (Some(tab), Some(pane)) =
                        (mux.get_tab(tab as usize), mux.get_pane(pane as usize))
                    {
                        tab.set_active_pane(&pane);
                    }
                }
                C::CustomAction(name) => self.commands.push(Command::Semantic(name)),
            },
        }
    }
    /// Other windows list this one's tabs; republish only when they could have changed.
    fn publish(&mut self) {
        let revision = self.app.model().revision;
        if self.published == Some(revision) {
            return;
        }
        self.published = Some(revision);
        let model = self.app.model();
        let home = model.home.as_deref();
        let window = self.window_id as u64;
        let tabs = model
            .spaces
            .iter()
            .flat_map(|space| {
                model
                    .space_tabs(&space.id)
                    .into_iter()
                    .map(move |id| ui::ForeignTab::new(window, &model.tabs[&id], home, &space.name))
            })
            .collect();
        directory::publish(self.window_id, tabs);
    }
    /// Windows shown in a GUI list themselves; the mux lists windows of other workspaces.
    fn sync_foreign(&mut self) {
        let front_end = crate::frontend::try_front_end();
        let shown = |id| {
            front_end
                .as_ref()
                .is_some_and(|front_end| front_end.gui_window_for_mux_window(id).is_some())
        };
        let mut tabs = directory::foreign(self.window_id, shown);
        let Some(mux) = mux::Mux::try_get() else {
            return self.app.set_foreign_tabs(tabs);
        };
        let home = self.app.model().home.clone();
        for id in mux.iter_windows() {
            if id == self.window_id || shown(id) {
                continue;
            }
            let Some(window) = mux.get_window(id) else {
                continue;
            };
            let place = match window.get_workspace() {
                workspace
                    if workspace.starts_with(BACKING_WORKSPACE_PREFIX)
                        || workspace.starts_with(QUICK_WORKSPACE_PREFIX) =>
                {
                    continue;
                }
                DETACHED_WORKSPACE => "detached".to_owned(),
                workspace => workspace.to_owned(),
            };
            for tab in window.iter_tabs() {
                let Some(pane) = tab.get_active_pane() else {
                    continue;
                };
                let cwd = pane
                    .get_current_working_dir(mux::pane::CachePolicy::AllowStale)
                    .map(|url| url.path().to_string())
                    .unwrap_or_default();
                let location =
                    location::Location::of(&pane, &cwd, &self.local_host, &mut self.repos);
                let tab = core::Tab {
                    id: tab.tab_id() as u64,
                    title: Some(tab.get_title())
                        .filter(|title| !title.is_empty())
                        .unwrap_or_else(|| pane.get_title()),
                    repo_root: location.repo_root,
                    home: location.home,
                    remote: location.remote,
                    os: location.os,
                    cwd,
                    ..Default::default()
                };
                tabs.push(ui::ForeignTab::new(
                    id as u64,
                    &tab,
                    home.as_deref(),
                    &place,
                ));
            }
        }
        self.app.set_foreign_tabs(tabs);
    }
    fn open_tab_navigator(&mut self) {
        self.sync_foreign();
        self.app.set_adoptable_panes(Vec::new());
        self.app.open_tab_navigator();
        self.adoptable_search += 1;
        if let (Some(window), Some(id)) = (self.window.clone(), self.mux_window) {
            adoption::find(window, id, self.adoptable_search);
        }
    }
    /// Detached tabs come into this window; others are shown where they live.
    fn show_tab(&mut self, window: usize, tab: core::TabId) {
        if self.reattach(tab) {
            return;
        }
        let Some(gui) = crate::frontend::try_front_end()
            .and_then(|front_end| front_end.gui_window_for_mux_window(window))
        else {
            let mux = mux::Mux::get();
            let Some(workspace) = mux.get_window(window).map(|w| w.get_workspace().to_owned())
            else {
                return self.app.ui_mut().set_error("Window closed");
            };
            if workspace == DETACHED_WORKSPACE {
                return self.adopt(tab);
            }
            if let Some(mut window) = mux.get_window_mut(window) {
                if let Some(index) = window.get_tab_idx_for_id(tab as usize) {
                    window.remember_and_set_active_tab_idx(index);
                }
            }
            return mux.set_active_workspace(&workspace);
        };
        gui.window.focus();
        gui.window
            .notify(crate::termwindow::TermWindowNotif::Apply(Box::new(
                move |tw| {
                    tw.vtabs_message_for(window, Message::ShowTab(tab));
                },
            )));
    }
    /// The tab's active pane becomes a tab here and its other panes rejoin it.
    fn adopt(&mut self, tab: core::TabId) {
        let Some(host) = mux::Mux::get().get_tab(tab as usize) else {
            return;
        };
        let mut panes = host.iter_panes_ignoring_zoom();
        panes.sort_by_key(|entry| !entry.is_active);
        let mut panes = panes.into_iter().map(|entry| entry.pane.pane_id());
        let Some(first) = panes.next() else {
            return;
        };
        let rest: Vec<_> = panes.collect();
        let (window, id) = (self.window.clone(), self.mux_window);
        self.rearrange(async move {
            let mux = mux::Mux::get();
            let (tab, _) = mux.move_pane_to_new_tab(first, id, None).await?;
            for pane in rest {
                mux.split_pane(
                    first,
                    mux::tab::SplitRequest {
                        target_is_second: true,
                        ..Default::default()
                    },
                    mux::domain::SplitSource::MovePane(pane),
                    config::keyassignment::SpawnTabDomain::CurrentPaneDomain,
                )
                .await?;
            }
            if let (Some(window), Some(id)) = (window, id) {
                let tab = tab.tab_id();
                window.notify(crate::termwindow::TermWindowNotif::Apply(Box::new(
                    move |tw| {
                        tw.vtabs_message_for(id, Message::ShowTab(tab as u64));
                    },
                )));
            }
            Ok(())
        });
    }
    fn reattach(&mut self, tab: core::TabId) -> bool {
        let Some(domain) = directory::detached(|records| {
            let mut domain = None;
            for record in records {
                record.show = record.tab.id == tab;
                if record.show {
                    domain = Some(record.domain);
                }
            }
            domain
        }) else {
            return false;
        };
        let Some(domain) = mux::Mux::get().get_domain(domain) else {
            return true;
        };
        let window = self.mux_window;
        promise::spawn::spawn(async move {
            if let Err(error) = domain.attach(window).await {
                log::warn!("tabs reattach: {error:#}");
            }
        })
        .detach();
        true
    }
    /// Only a client domain can reattach, and only while the mux still knows it.
    fn remember_detached(&self, id: core::TabId) {
        let Some(&(domain, remote, _)) = self.remote_tabs.get(&id) else {
            return;
        };
        let Some(tab) = self.app.model().tabs.get(&id) else {
            return;
        };
        if mux::Mux::get()
            .get_domain(domain)
            .is_none_or(|domain| domain.state() != mux::domain::DomainState::Detached)
        {
            return;
        }
        let model = self.app.model();
        directory::remember(directory::DetachedTab {
            listing: ui::ForeignTab::new(
                self.window_id as u64,
                tab,
                model.home.as_deref(),
                format!("{}  detached", tab.domain),
            ),
            tab: tab.clone(),
            domain,
            remote,
            show: false,
        });
    }
    /// Reattached tabs return with new IDs; whichever window receives one restores it.
    fn restore_reattached(&mut self) {
        let mux = mux::Mux::get();
        let model = self.app.model();
        let returned = directory::detached(|records| {
            let mut returned = Vec::new();
            for record in std::mem::take(records) {
                let Some(domain) = mux.get_domain(record.domain) else {
                    continue;
                };
                if domain.state() == mux::domain::DomainState::Detached {
                    records.push(record);
                    continue;
                }
                let Some(id) = wezterm_client::domain::ClientDomain::get_client_inner_for_domain(
                    record.domain,
                )
                .ok()
                .and_then(|inner| inner.remote_to_local_tab_id(record.remote)) else {
                    continue;
                };
                if model.tabs.contains_key(&(id as u64)) {
                    returned.push((id as u64, record));
                } else if mux.get_tab(id).is_some() {
                    records.push(record);
                }
            }
            returned
        });
        let mut show = None;
        for (id, record) in returned {
            let result = self.app.restore_tab(id, &record.tab);
            self.apply(result);
            if record.show {
                show = Some(id);
            }
        }
        if let Some(id) = show {
            self.dispatch(core::Intent::ActivateTab(id));
        }
    }
    fn schedule_hooks(&mut self) {
        self.hooks
            .schedule(self.window.as_ref(), &self.app, self.window_id);
    }
    fn storage(&mut self) {
        if let Some(window) = self.window.clone() {
            if let Some(request) = self.app.take_storage_request(self.epoch.elapsed()) {
                self.outstanding = Some(request.request_id);
                storage::request(window.clone(), request, self.window_id);
            }
            if let Some(source) = self.app.take_managed_write() {
                match self.managed_path.clone() {
                    Some(path) => settings_file::save(window, path, source, self.window_id),
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
    fn apply_facts(&mut self, snapshot: facts::Facts) {
        if !snapshot.focused {
            self.cancel_paste();
        }
        self.app.set_window_identity(snapshot.window_id as u64);
        self.mux_window = Some(snapshot.window_id);
        // A pane dragged out to a position becomes a tab there once the mux reports it.
        let arrived = self
            .placement
            .take()
            .filter(|(_, since)| since.elapsed() < PLACEMENT_TIMEOUT)
            .and_then(|placement| {
                let known: std::collections::HashSet<_> = self.host_tabs.iter().collect();
                match snapshot.tabs.iter().find(|tab| !known.contains(&tab.id)) {
                    Some(tab) => Some((tab.id as u64, placement.0)),
                    None => {
                        self.placement = Some(placement);
                        None
                    }
                }
            });
        let before = self.app.model().revision;
        if self.config_generation != snapshot.config_epoch {
            self.config_generation = snapshot.config_epoch;
            self.hooks.configure(lua::hooks_enabled());
            self.cancel_paste();
            let config = lua::configuration();
            self.managed_path = config.managed_path.clone();
            let result = self.app.config(config.value());
            self.apply(result);
        }
        self.host_tabs = snapshot.tabs.iter().map(|tab| tab.id).collect();
        let repos = &mut self.repos;
        let local_host = &self.local_host;
        let mux = mux::Mux::get();
        let tabs = snapshot
            .tabs
            .into_iter()
            .map(|tab| {
                let domain = mux.get_domain_by_name(&tab.domain);
                let launch = domain
                    .as_ref()
                    .filter(|domain| domain.spawnable())
                    .map(|_| core::LaunchSpec {
                        domain: Some(tab.domain.clone()),
                        cwd: (!tab.cwd.is_empty()).then_some(tab.cwd.clone()),
                        ..Default::default()
                    });
                let host_tab = mux.get_tab(tab.id);
                let location = match host_tab.as_ref().and_then(|host| host.get_active_pane()) {
                    Some(pane) => location::Location::of(&pane, &tab.cwd, local_host, repos),
                    None => location::Location::unanswered(false, String::new(), &tab.cwd, repos),
                };
                let panes = host_tab
                    .map(|host| host.iter_panes_ignoring_zoom())
                    .filter(|panes| panes.len() > 1)
                    .into_iter()
                    .flatten()
                    .map(|entry| {
                        let cwd = entry
                            .pane
                            .get_current_working_dir(mux::pane::CachePolicy::AllowStale)
                            .map(|url| url.path().to_string())
                            .unwrap_or_default();
                        let location = location::Location::of(&entry.pane, &cwd, local_host, repos);
                        core::TabPane {
                            id: entry.pane.pane_id() as u64,
                            title: entry.pane.get_title(),
                            cwd,
                            repo_root: location.repo_root,
                            home: location.home,
                            active: entry.is_active,
                            remote: location.remote,
                            os: location.os,
                            left: entry.left.min(usize::from(u16::MAX)) as u16,
                            top: entry.top.min(usize::from(u16::MAX)) as u16,
                            width: entry.width.min(usize::from(u16::MAX)) as u16,
                            height: entry.height.min(usize::from(u16::MAX)) as u16,
                        }
                    })
                    .collect();
                core::Tab {
                    id: tab.id as u64,
                    title: tab.title,
                    cwd: tab.cwd.clone(),
                    repo_root: location.repo_root,
                    home: location.home,
                    domain: tab.domain.clone(),
                    process: tab.process,
                    remote: location.remote,
                    os: location.os,
                    panes,
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
        self.hooks.observe(&tabs);
        let host_tabs = self
            .host_tabs
            .iter()
            .map(|id| *id as u64)
            .collect::<std::collections::HashSet<_>>();
        let detached = {
            let mux = mux::Mux::get();
            self.app
                .model()
                .tabs
                .iter()
                .filter_map(|(id, tab)| {
                    if host_tabs.contains(id) {
                        return None;
                    }
                    let detached = mux.get_domain_by_name(&tab.domain).map_or(true, |domain| {
                        domain.state() == mux::domain::DomainState::Detached
                    });
                    detached.then_some(*id)
                })
                .collect::<Vec<_>>()
        };
        for id in detached {
            self.remember_detached(id);
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
        let result = self.app.update(app::HostSnapshot {
            revision: snapshot.revision,
            tabs,
            active_tab: snapshot.active.map(|id| id as u64),
            metrics: self.app.metrics(),
            focused: snapshot.focused,
            configuration_epoch: snapshot.config_epoch as u64,
        });
        self.apply(result);
        self.hooks.window_dirty |= before != self.app.model().revision;
        if let Some((id, index)) = arrived {
            self.dispatch(core::Intent::MoveTab { id, index });
        }
        self.restore_reattached();
        if let Some(id) = self
            .pending_show
            .filter(|id| self.app.model().tabs.contains_key(id))
        {
            self.pending_show = None;
            self.dispatch(core::Intent::ActivateTab(id));
        }
        self.schedule_hooks();
        self.storage();
        self.publish();
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
        let process_titles = matches!(
            model.settings.tab_title,
            core::TabTitle::Process | core::TabTitle::ProcessAndDirectory
        );
        (self.hooks.enabled || process_rules || process_templates || process_titles)
            .then_some(std::time::Duration::from_secs(2))
    }
    /// Applies the sidebar's order to every tab of the mux window, keeping its active tab.
    fn reorder(&mut self, visible_order: Vec<core::TabId>) {
        let visible: std::collections::HashSet<_> =
            visible_order.iter().map(|id| *id as usize).collect();
        let mut next = visible_order.into_iter();
        let order: Vec<usize> = self
            .host_tabs
            .iter()
            .map(|id| {
                if visible.contains(id) {
                    next.next().unwrap() as usize
                } else {
                    *id
                }
            })
            .collect();
        if let Some(mut window) = mux::Mux::get().get_window_mut(self.window_id) {
            let active = window.get_active_tab().map(|t| t.tab_id());
            let known: std::collections::HashSet<_> =
                window.iter_tabs().map(|t| t.tab_id()).collect();
            if order.len() == known.len()
                && order
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
                    == known
            {
                for (index, id) in order.iter().enumerate() {
                    let old = window.iter_tabs().position(|t| t.tab_id() == *id).unwrap();
                    if old != index {
                        let tab = window.remove_tab_idx(old);
                        window.insert_tab_at_idx(index, &tab);
                    }
                }
                let index = window.iter_tabs().position(|t| Some(t.tab_id()) == active);
                if let Some(index) = index {
                    window.set_active_tab_idx_without_saving(index);
                }
            }
        }
        self.commands.push(Command::Resync);
    }
    /// The new window restores the tab's sidebar state from the exported transfer.
    fn move_to_new_window(&mut self, id: usize) {
        let mux = mux::Mux::get();
        let Some(tab) = mux.get_tab(id) else { return };
        let context = match self.app.export_transfer(id as u64) {
            Ok(transfer) => SpawnContext {
                new_window: true,
                transfer: Some(transfer),
                ..SpawnContext::default()
            },
            Err(error) => {
                log::warn!("tabs move: {error}");
                SpawnContext::default()
            }
        };
        if let Some(pane) = tab.get_active_pane() {
            if pane
                .downcast_ref::<wezterm_client::pane::ClientPane>()
                .is_some()
            {
                if tab.count_panes() != Some(1) {
                    self.message(Message::Error("WezTerm mux supports moving single-pane tabs; this split tab stays in its window".into()));
                    return;
                }
                let pane_id = pane.pane_id();
                let source_id = self.window_id;
                let workspace = mux
                    .get_window(source_id)
                    .map(|w| w.get_workspace().to_owned());
                promise::spawn::spawn(async move {
                    match mux::Mux::get()
                        .move_pane_to_new_tab(pane_id, None, workspace)
                        .await
                    {
                        Ok((tab, window)) => {
                            spawn_completed(window, tab.tab_id(), context);
                        }
                        Err(error) => log::error!("move tab: {error}"),
                    }
                })
                .detach();
                return;
            }
        }
        // This operation retains the tab and its entire split tree.
        let builder = mux.new_empty_window(Some(mux.active_workspace()), None);
        let destination = *builder;
        if let Some(mut source) = mux.get_window_mut(self.window_id) {
            source.remove_tab_id(id);
        }
        if let Err(error) = mux.add_tab_to_window(&tab, destination) {
            let _ = mux.add_tab_to_window(&tab, self.window_id);
            self.message(Message::Error(format!("move tab: {error}")));
            return;
        }
        spawn_completed(destination, id, context);
        drop(builder);
        mux.prune_dead_windows();
        self.commands.push(Command::Resync);
    }
    fn schedule_metadata(&mut self) {
        let schedule = self
            .metadata_interval()
            .filter(|_| !self.host_tabs.is_empty())
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
        self.cancel_paste();
        self.launcher_entries.clear();
        let entries = commands::entries(&commands, &self.app.model().settings);
        self.palette_commands = commands;
        self.app.ui_mut().open_commands(entries);
    }
    fn open_launcher(
        &mut self,
        args: config::keyassignment::LauncherActionArgs,
        launcher: crate::overlay::launcher::LauncherEntries,
    ) {
        self.cancel_paste();
        self.palette_commands.clear();
        let entries = launcher
            .entries
            .iter()
            .map(|entry| ui::CommandEntry {
                label: entry.label.clone(),
                shortcuts: Vec::new(),
                description: String::new(),
            })
            .collect();
        self.launcher_entries = launcher.entries;
        self.app.ui_mut().open_launch_menu(
            ui::LaunchMenuConfig {
                title: args.title.unwrap_or_else(|| "Launcher".into()),
                help_text: args
                    .help_text
                    .unwrap_or_else(|| "Enter=launch  Esc=cancel  /=filter".into()),
                fuzzy_help_text: args
                    .fuzzy_help_text
                    .unwrap_or_else(|| "Fuzzy matching: ".into()),
                alphabet: args
                    .alphabet
                    .unwrap_or_else(|| config::configuration().launcher_alphabet.clone()),
                fuzzy: args
                    .flags
                    .contains(config::keyassignment::LauncherFlags::FUZZY),
                labels:
                    crate::overlay::quickselect::compute_labels_for_alphabet_with_preserved_case,
                matches: |query, label| {
                    crate::overlay::selector::matcher_score(
                        &crate::overlay::selector::matcher_pattern(query),
                        label,
                    )
                },
            },
            entries,
            launcher.selected,
        );
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
        let metrics = self.metrics(geometry);
        if let Err(err) = self.app.resize(metrics) {
            log::error!("tabs geometry: {err}");
        }
        let elapsed = now.saturating_duration_since(self.epoch);
        let mut frame = self.app.render(elapsed);
        // A successful form can dismiss itself while rendering the updated model.
        // Publish cells and hit regions only after their viewport matches that state.
        let settled_metrics = self.metrics(geometry);
        if settled_metrics != metrics {
            if let Err(err) = self.app.resize(settled_metrics) {
                log::error!("tabs geometry: {err}");
            }
            frame = self.app.render(elapsed);
        }
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
            .chain(self.input.pending.as_ref().map(|pending| pending.deadline))
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
        let settings = &self.app.model().settings;
        let color = |value: Option<&str>| value.and_then(ui::Theme::parse_color).map(linear_color);
        let border = settings.quick_terminal_border_color.as_deref();
        QuickTerminalConfig {
            size: (
                settings.quick_terminal_width as f32,
                settings.quick_terminal_height as f32,
            ),
            padding: terminal_padding(&settings.quick_terminal_window_padding),
            opacity: settings.quick_terminal_opacity as f32,
            blur: settings.quick_terminal_blur as f32,
            border: match border {
                Some("none") => TerminalBorder::None,
                _ => color(border).map_or(TerminalBorder::Palette, TerminalBorder::Color),
            },
            background_color: color(settings.quick_terminal_background_color.as_deref()),
            auto_hide: settings.quick_terminal_auto_hide,
            hide_on_escape: hide_on_escape(settings.quick_terminal_hide_on_escape),
            preserve_shell: settings.quick_terminal_preserve_shell,
            transition: ui::transition(settings),
        }
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

fn process_name(pane: &dyn mux::pane::Pane) -> String {
    pane.get_foreground_process_name(mux::pane::CachePolicy::AllowStale)
        .and_then(|path| {
            std::path::Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}

fn home_dir() -> Option<String> {
    config::HOME_DIR.to_str().map(str::to_string)
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
/// `colors` and `border_color` go through WezTerm's own parsers, as in its config.
fn terminal_program(program: core::QuickProgram) -> Result<TerminalProgram, String> {
    let border = match program.border_color.as_deref() {
        None => None,
        Some("none") => Some(TerminalBorder::None),
        Some(text) => Some(TerminalBorder::Color(
            text.parse::<window::color::SrgbaTuple>()
                .map_err(|_| format!("invalid border_color {text}"))?
                .to_linear(),
        )),
    };
    let colors = program.colors.map(palette).transpose()?;
    Ok(TerminalProgram {
        spawn: SpawnCommand {
            args: (!program.args.is_empty()).then_some(program.args),
            cwd: program.cwd.map(Into::into),
            set_environment_variables: program.set_environment_variables.into_iter().collect(),
            domain: match program.domain {
                None | Some(core::SpawnDomain::CurrentPaneDomain) => {
                    SpawnTabDomain::CurrentPaneDomain
                }
                Some(core::SpawnDomain::DefaultDomain) => SpawnTabDomain::DefaultDomain,
                Some(core::SpawnDomain::DomainName(name)) => SpawnTabDomain::DomainName(name),
                Some(core::SpawnDomain::DomainId(id)) => SpawnTabDomain::DomainId(id),
            },
            ..Default::default()
        },
        width: program.width,
        height: program.height,
        padding: terminal_padding(&program.window_padding),
        opacity: program.window_background_opacity,
        blur: program.blur.map(f32::from),
        border,
        colors,
        auto_hide: program.auto_hide,
        hide_on_escape: program.hide_on_escape.map(hide_on_escape),
        preserve: program.preserve,
    })
}
fn terminal_padding(padding: &core::WindowPadding) -> TerminalPadding {
    use core::padding::Unit;
    let side = |side: &Option<core::Dimension>| {
        let (amount, unit) = side.as_ref()?.parse()?;
        Some(match unit {
            Unit::Pixels => config::Dimension::Pixels(amount),
            Unit::Percent => config::Dimension::Percent(amount / 100.),
            Unit::Points => config::Dimension::Points(amount),
            Unit::Cells => config::Dimension::Cells(amount),
        })
    };
    TerminalPadding {
        left: side(&padding.left),
        top: side(&padding.top),
        right: side(&padding.right),
        bottom: side(&padding.bottom),
    }
}
fn hide_on_escape(mode: core::HideOnEscape) -> HideOnEscape {
    match mode {
        core::HideOnEscape::Never => HideOnEscape::Never,
        core::HideOnEscape::Always => HideOnEscape::Always,
        core::HideOnEscape::Auto => HideOnEscape::Auto,
    }
}
fn palette(colors: serde_json::Map<String, serde_json::Value>) -> Result<config::Palette, String> {
    use wezterm_dynamic::{FromDynamic, FromDynamicOptions, UnknownFieldAction};
    let options = FromDynamicOptions {
        unknown_fields: UnknownFieldAction::Deny,
        deprecated_fields: UnknownFieldAction::Warn,
    };
    config::Palette::from_dynamic(&dynamic(&serde_json::Value::Object(colors)), options)
        .map_err(|error| format!("invalid colors: {error}"))
}
/// Lua's integer keys, as in `indexed`, arrive as JSON strings.
fn dynamic(value: &serde_json::Value) -> wezterm_dynamic::Value {
    use serde_json::Value;
    use wezterm_dynamic::{ToDynamic, Value as Dynamic};
    match value {
        Value::Null => Dynamic::Null,
        Value::Bool(value) => Dynamic::Bool(*value),
        Value::Number(number) => match (number.as_u64(), number.as_i64()) {
            (Some(value), _) => value.to_dynamic(),
            (_, Some(value)) => value.to_dynamic(),
            _ => number.as_f64().unwrap_or_default().to_dynamic(),
        },
        Value::String(value) => Dynamic::String(value.clone()),
        Value::Array(items) => Dynamic::Array(items.iter().map(dynamic).collect::<Vec<_>>().into()),
        Value::Object(fields) => Dynamic::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    let key = key
                        .parse::<u64>()
                        .map_or_else(|_| Dynamic::String(key.clone()), Dynamic::U64);
                    (key, dynamic(value))
                })
                .collect::<std::collections::BTreeMap<_, _>>()
                .into(),
        ),
    }
}
#[cfg(test)]
#[path = "../tests/hook.rs"]
mod hook_tests;

#[cfg(test)]
#[path = "../tests/repos.rs"]
mod repos_tests;

#[cfg(test)]
#[path = "../tests/modal.rs"]
mod modal_tests;

fn linear_color(color: ratatui::style::Color) -> window::color::LinearRgba {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => {
            window::color::SrgbaTuple::from((r, g, b)).to_linear()
        }
        _ => window::color::LinearRgba(0., 0., 0., 0.),
    }
}

#[cfg(test)]
#[path = "../tests/commands.rs"]
mod command_tests;

#[cfg(test)]
#[path = "../tests/input.rs"]
mod input_tests;
