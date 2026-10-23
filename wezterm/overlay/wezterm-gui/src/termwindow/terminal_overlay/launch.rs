//! Resolving and spawning the quick terminal's shell and programs.
use super::*;

pub(super) enum Target {
    Domain(Arc<dyn Domain>),
    /// Mirrored clients spawn on the server, beside the pane, in a hidden workspace.
    Beside(Arc<dyn Domain>, PaneId),
}

pub(super) struct Launch {
    target: Target,
    command: Option<CommandBuilder>,
    cwd: Option<String>,
}

/// Keyed by the server window and stable space ID, so a restart finds the right shell.
pub(super) fn quick_workspace(
    domain: &Arc<dyn Domain>,
    window: MuxWindowId,
    space: &str,
    slot: Slot,
) -> String {
    let window = domain
        .downcast_ref::<ClientDomain>()
        .and_then(|client| client.local_to_remote_window_id(window))
        .map_or_else(|| format!("local{window}"), |id| id.to_string());
    let kind = match slot {
        Slot::Shell(_) => "shell",
        Slot::Program(_) => "program",
    };
    format!("{}{window}:{space}:{kind}", mux::QUICK_WORKSPACE_PREFIX)
}

pub(super) fn quick_panes(workspace: &str) -> Vec<Arc<dyn Pane>> {
    let mux = Mux::get();
    mux.iter_windows_in_workspace(workspace)
        .into_iter()
        .filter_map(|id| mux.get_window(id))
        .flat_map(|window| {
            window
                .iter_tabs()
                .filter_map(|tab| tab.get_active_pane())
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(super) fn directory(url: &url::Url) -> Option<String> {
    let mut local = url::Url::parse("file:///").ok()?;
    local.set_path(url.path());
    local
        .to_file_path()
        .ok()?
        .into_os_string()
        .into_string()
        .ok()
}

impl TermWindow {
    pub(in crate::termwindow) fn toggle_terminal_program(&mut self, program: TerminalProgram) {
        let shown = self.ui_host.as_ref().is_some_and(|ui| {
            let state = ui.terminal.session.borrow();
            state.showing(&program)
        });
        if shown {
            self.hide_terminal_overlay();
            return;
        }
        let reusable = self
            .ui_host
            .as_ref()
            .is_some_and(|ui| ui.terminal.session.borrow().reusable(&program, alive));
        let launch = if reusable {
            None
        } else {
            match self.terminal_launch(&program.spawn) {
                Ok(launch) => Some(launch),
                Err(error) => {
                    let error = format!("Quick terminal: {error:#}");
                    log::warn!("{error}");
                    self.vtabs_message_for(self.mux_window_id, Message::Error(error));
                    return;
                }
            }
        };
        self.show_terminal_overlay(Some((program, launch)));
    }

    /// Resolved as a split of the active tab's pane; `Beside` falls back to any domain pane.
    fn terminal_launch(&self, spawn: &SpawnCommand) -> anyhow::Result<Launch> {
        let mux = Mux::get();
        let current = mux
            .get_active_tab_for_window(self.mux_window_id)
            .and_then(|tab| tab.get_active_pane());
        let mut spawn = spawn.clone();
        if let Some(ui) = &self.ui_host {
            ui.provider.prepare_command(&mut spawn);
        }
        let domain = mux
            .resolve_spawn_tab_domain(current.as_ref().map(|pane| pane.pane_id()), &spawn.domain)?;
        let current = current.filter(|pane| pane.domain_id() == domain.domain_id());
        let explicit = spawn.cwd.map(|cwd| cwd.to_string_lossy().into_owned());
        let (target, cwd) = if let Some(client) = domain
            .downcast_ref::<ClientDomain>()
            .filter(|client| !client.local_pane_layout())
        {
            let pane = current
                .or_else(|| {
                    mux.iter_panes()
                        .into_iter()
                        .find(|pane| pane.domain_id() == domain.domain_id())
                })
                .with_context(|| format!("no pane open in {}", domain.domain_name()))?;
            let pane = pane.pane_id();
            client.validate_spawn_beside_pane(pane)?;
            (Target::Beside(domain, pane), explicit)
        } else {
            let cwd = explicit.or_else(|| {
                current
                    .and_then(|pane| pane.get_current_working_dir(CachePolicy::FetchImmediate))
                    .and_then(|url| directory(&url))
            });
            (Target::Domain(domain), cwd)
        };
        let (args, env) = (spawn.args, spawn.set_environment_variables);
        let command = (args.is_some() || !env.is_empty()).then(|| {
            let mut command = args
                .map(|args| CommandBuilder::from_argv(args.into_iter().map(Into::into).collect()))
                .unwrap_or_else(CommandBuilder::new_default_prog);
            for (key, value) in env {
                command.env(key, value);
            }
            command
        });
        Ok(Launch {
            target,
            command,
            cwd,
        })
    }

    /// A program without a launch reopens its kept command.
    pub(super) fn show_terminal_overlay(
        &mut self,
        program: Option<(TerminalProgram, Option<Launch>)>,
    ) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(ui) = self.ui_host.as_ref() else {
            return;
        };
        let preserve_shell = ui.provider.quick_terminal_config().preserve_shell;
        let space = ui.provider.selected_space().to_owned();
        let state_ref = ui.terminal.session.clone();
        if let Some(pane) = self.get_active_pane_or_overlay() {
            pane.focus_changed(false);
        }
        self.current_mouse_capture = None;
        self.current_mouse_buttons.clear();
        self.dragging = None;
        let (program, launch) = match program {
            Some((spec, launch)) => (Some((spec, launch.is_some())), launch),
            None => (None, None),
        };
        let opened = state_ref.borrow_mut().show(program, preserve_shell, alive);
        for pane in &opened.discarded {
            remove(pane);
        }
        window.invalidate();
        match opened.next {
            Open::Shell(pane) => {
                pane.focus_changed(true);
                return;
            }
            Open::Program { pane, colors } => {
                if let Some(pane) = pane {
                    pane.set_config(term_config(&self.config, colors.as_ref()));
                    pane.focus_changed(true);
                }
                return;
            }
            Open::LaunchProgram(slot) => {
                let launch = launch.expect("program launch was resolved before opening");
                self.spawn_terminal_pane(&state_ref, slot, launch);
                return;
            }
            Open::Pending => return,
            Open::ResolveShell => {}
        }
        let local = SpawnCommand {
            domain: SpawnTabDomain::DomainName("local".into()),
            ..Default::default()
        };
        let launch = self
            .terminal_launch(&SpawnCommand::default())
            .or_else(|_| self.terminal_launch(&local));
        let launch = match launch {
            Ok(launch) => launch,
            Err(error) => {
                log::warn!("Quick terminal: {error:#}");
                return;
            }
        };
        if let Target::Beside(domain, _) = &launch.target {
            let workspace = quick_workspace(domain, self.mux_window_id, &space, Slot::Shell(0));
            let panes = quick_panes(&workspace);
            if !preserve_shell {
                panes.iter().for_each(remove);
            } else if let Some(pane) = panes.into_iter().next() {
                pane.set_config(Arc::new(config::TermConfig::with_config(
                    self.config.clone(),
                )));
                pane.focus_changed(true);
                state_ref.borrow_mut().restore_shell(pane);
                return;
            }
        }
        let slot = state_ref.borrow_mut().launch_shell();
        self.spawn_terminal_pane(&state_ref, slot, launch);
    }

    /// A pane arriving after its slot stopped wanting it is removed.
    fn spawn_terminal_pane(&self, state: &Rc<RefCell<Session>>, slot: Slot, launch: Launch) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(ui) = &self.ui_host else {
            return;
        };
        let space = ui.provider.selected_space().to_owned();
        let state = Rc::downgrade(state);
        let size = self.terminal_overlay_layout().size;
        let config = self.config.clone();
        let window_id = self.mux_window_id;
        let endpoint = crate::termwindow::ui_host::WindowEndpoint::new(window.clone(), window_id);
        promise::spawn::spawn(async move {
            let Launch {
                target,
                command,
                cwd,
            } = launch;
            let result = async {
                match target {
                    Target::Domain(domain) => {
                        if domain.state() == DomainState::Detached {
                            domain.attach(Some(window_id)).await?;
                        }
                        domain.spawn_pane(size, command, cwd).await
                    }
                    Target::Beside(domain, pane_id) => {
                        let client = domain
                            .downcast_ref::<ClientDomain>()
                            .ok_or_else(|| anyhow::anyhow!("not a client domain"))?;
                        let workspace = quick_workspace(&domain, window_id, &space, slot);
                        // Programs a quit GUI couldn't end would otherwise outlive it.
                        if state.upgrade().is_some_and(|state| {
                            state.borrow_mut().sweep_workspace(slot, &workspace)
                        }) {
                            quick_panes(&workspace).iter().for_each(remove);
                        }
                        client
                            .spawn_beside_pane(pane_id, size, command, cwd, workspace)
                            .await
                    }
                }
            }
            .await;
            let Some(state) = state.upgrade() else {
                if let Ok(pane) = result {
                    remove(&pane);
                }
                return;
            };
            let mut state = state.borrow_mut();
            let pane = result.as_ref().ok().cloned();
            if !state.complete(slot, pane) {
                if let Ok(pane) = result {
                    remove(&pane);
                }
                return;
            }
            match result {
                Ok(pane) => {
                    pane.set_config(term_config(&config, state.colors(slot).as_ref()));
                    pane.focus_changed(
                        state
                            .shown()
                            .is_some_and(|shown| shown.pane_id() == pane.pane_id()),
                    );
                }
                Err(error) => {
                    endpoint.send(Message::Error(format!("Quick terminal: {error:#}")));
                }
            }
            window.invalidate();
        })
        .detach();
    }
}
