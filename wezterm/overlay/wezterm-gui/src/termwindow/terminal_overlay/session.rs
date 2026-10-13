//! Session lifetime, kept programs and token-checked launch completion.
use super::*;

fn axis(size: &mut (Option<f32>, Option<f32>), horizontal: bool) -> &mut Option<f32> {
    if horizontal { &mut size.0 } else { &mut size.1 }
}

#[derive(Default)]
pub(super) struct Session {
    visible: bool,
    pending: Option<u64>,
    launches: u64,
    config: Option<QuickTerminalConfig>,
    /// Session size steps; a settings change clears its axis.
    adjusted: (Option<f32>, Option<f32>),
    motion: Option<Motion>,
    pane: Option<Arc<dyn Pane>>,
    /// The shell's mirrored tab, where `attach_mux` swaps in its replacement.
    tab: Option<TabId>,
    /// Shown instead of the shell; exists only while visible.
    program: Option<Program>,
    kept: Vec<Program>,
    programs: u64,
    /// Program workspaces cleared of what a quit GUI left behind.
    swept: HashSet<String>,
    /// The window config the quick panes last took.
    applied: Option<config::ConfigHandle>,
}

struct Program {
    id: u64,
    spec: TerminalProgram,
    /// The spec's size; steps change only the axes it sets.
    size: (Option<f32>, Option<f32>),
    pane: Option<Arc<dyn Pane>>,
}

impl Program {
    fn new(id: u64, spec: TerminalProgram) -> Self {
        Self {
            id,
            size: (spec.width, spec.height),
            spec,
            pane: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Slot {
    Shell(u64),
    Program(u64),
}

/// Effects of opening, with all preservation and slot decisions already made.
pub(super) struct Opening {
    pub(super) discarded: Vec<Arc<dyn Pane>>,
    pub(super) next: Open,
}

pub(super) enum Open {
    Shell(Arc<dyn Pane>),
    Program {
        pane: Option<Arc<dyn Pane>>,
        colors: Option<config::Palette>,
    },
    ResolveShell,
    LaunchProgram(Slot),
    Pending,
}

pub(super) struct Hidden {
    pub(super) unfocus: Vec<Arc<dyn Pane>>,
    pub(super) discarded: Vec<Arc<dyn Pane>>,
}

pub(super) struct PaneConfiguration {
    pub(super) pane: Arc<dyn Pane>,
    pub(super) colors: Option<config::Palette>,
}

pub(super) struct Refresh {
    pub(super) changed: bool,
    pub(super) discarded: Vec<Arc<dyn Pane>>,
    pub(super) configurations: Vec<PaneConfiguration>,
}

/// Eases drawing from the last painted size and origin while the PTY uses its target grid.
#[derive(Clone, Copy)]
struct Motion {
    size: (f32, f32),
    origin: (f32, f32),
    start: Instant,
    duration: Duration,
    eased: f32,
}

impl Motion {
    fn advance(&mut self, now: Instant) -> bool {
        let t =
            now.saturating_duration_since(self.start).as_secs_f32() / self.duration.as_secs_f32();
        self.eased = 1. - (1. - t.min(1.)).powi(3);
        t < 1.
    }

    fn lerp(&self, from: (f32, f32), to: (f32, f32)) -> (f32, f32) {
        (
            from.0 + (to.0 - from.0) * self.eased,
            from.1 + (to.1 - from.1) * self.eased,
        )
    }
}

impl Session {
    pub(super) fn visible(&self) -> bool {
        self.visible
    }

    pub(super) fn showing_program(&self) -> bool {
        self.visible && self.program.is_some()
    }

    pub(super) fn showing(&self, spec: &TerminalProgram) -> bool {
        self.visible
            && self
                .program
                .as_ref()
                .is_some_and(|program| program.spec == *spec)
    }

    pub(super) fn effective_config(&self, config: QuickTerminalConfig) -> QuickTerminalConfig {
        match self.program.as_ref().filter(|_| self.visible) {
            Some(program) => config.with(&program.spec),
            None => config,
        }
    }

    pub(super) fn panes(&self) -> impl Iterator<Item = &Arc<dyn Pane>> {
        self.pane.iter().chain(self.program_panes())
    }

    pub(super) fn pane_ids(&self) -> (Option<PaneId>, Option<PaneId>, Vec<PaneId>) {
        (
            self.pane.as_ref().map(|pane| pane.pane_id()),
            self.program
                .as_ref()
                .and_then(|program| program.pane.as_ref())
                .map(|pane| pane.pane_id()),
            self.kept
                .iter()
                .filter_map(|program| program.pane.as_ref())
                .map(|pane| pane.pane_id())
                .collect(),
        )
    }

    pub(super) fn hide(
        &mut self,
        preserve_shell: bool,
        alive: impl Fn(&Arc<dyn Pane>) -> bool,
    ) -> Hidden {
        let unfocus = self.panes().cloned().collect();
        self.visible = false;
        self.motion = None;
        let mut discarded: Vec<_> = self.put_away(alive).into_iter().collect();
        if !preserve_shell {
            discarded.extend(self.discard_shell());
        }
        Hidden { unfocus, discarded }
    }

    /// `can_launch` is false when preflight selected an already running/pending preserved program.
    pub(super) fn show(
        &mut self,
        program: Option<(TerminalProgram, bool)>,
        preserve_shell: bool,
        alive: impl Fn(&Arc<dyn Pane>) -> bool,
    ) -> Opening {
        let mut discarded = Vec::new();
        if !preserve_shell {
            discarded.extend(self.discard_shell());
        }
        discarded.extend(self.put_away(&alive));
        discarded.extend(self.prune(&alive));
        self.visible = true;
        self.motion = None;
        let next = if let Some((spec, can_launch)) = program {
            if let Some(kept) = self.take_kept(&spec) {
                let next = Open::Program {
                    pane: kept.pane.clone(),
                    colors: kept.spec.colors.clone(),
                };
                self.program = Some(kept);
                next
            } else if can_launch {
                self.programs += 1;
                self.program = Some(Program::new(self.programs, spec));
                Open::LaunchProgram(Slot::Program(self.programs))
            } else {
                self.visible = false;
                Open::Pending
            }
        } else {
            if self.pane.as_ref().is_some_and(|pane| !alive(pane)) {
                discarded.extend(self.pane.take());
            }
            match &self.pane {
                Some(pane) => Open::Shell(pane.clone()),
                None if self.pending.is_some() => Open::Pending,
                None => Open::ResolveShell,
            }
        };
        Opening { discarded, next }
    }

    pub(super) fn restore_shell(&mut self, pane: Arc<dyn Pane>) {
        self.tab = mirrored_tab(&pane);
        self.pane = Some(pane);
    }

    pub(super) fn sweep_workspace(&mut self, slot: Slot, workspace: &str) -> bool {
        matches!(slot, Slot::Program(_)) && self.swept.insert(workspace.to_owned())
    }

    pub(super) fn refresh(
        &mut self,
        config: QuickTerminalConfig,
        applied: &config::ConfigHandle,
        now: Instant,
        alive: impl Fn(&Arc<dyn Pane>) -> bool,
    ) -> Refresh {
        let changed = self.configure(config);
        self.advance(now);
        let mut discarded = self.prune(alive);
        if !config.preserve_shell && (!self.visible || self.program.is_some()) {
            discarded.extend(self.discard_shell());
        }
        let reloaded = self.applied.as_ref().map_or(true, |previous| {
            !std::ptr::eq::<config::Config>(&**previous, &**applied)
        });
        let mut configurations = Vec::new();
        if reloaded {
            self.applied = Some(applied.clone());
            configurations.extend(self.pane.iter().map(|pane| PaneConfiguration {
                pane: pane.clone(),
                colors: None,
            }));
            configurations.extend(self.program.iter().chain(&self.kept).filter_map(|program| {
                program.pane.as_ref().map(|pane| PaneConfiguration {
                    pane: pane.clone(),
                    colors: program.spec.colors.clone(),
                })
            }));
        }
        Refresh {
            changed,
            discarded,
            configurations,
        }
    }

    pub(super) fn resize(
        &mut self,
        horizontal: bool,
        size: f32,
        drawn: ((f32, f32), (f32, f32)),
        duration: Duration,
        now: Instant,
    ) {
        self.adjust(horizontal, size);
        self.motion = (!duration.is_zero()).then_some(Motion {
            size: drawn.0,
            origin: drawn.1,
            start: now,
            duration,
            eased: 0.,
        });
    }

    fn advance(&mut self, now: Instant) {
        if self
            .motion
            .as_mut()
            .is_some_and(|motion| !motion.advance(now))
        {
            self.motion = None;
        }
    }

    pub(super) fn animate_to(
        &self,
        size: (f32, f32),
        origin: (f32, f32),
    ) -> Option<((f32, f32), (f32, f32))> {
        self.motion.map(|motion| {
            (
                motion.lerp(motion.size, size),
                motion.lerp(motion.origin, origin),
            )
        })
    }

    /// The shell's tab shows a different pane once `attach_mux` replaced it.
    pub(super) fn follow(&mut self) -> Option<Arc<dyn Pane>> {
        let current = self.pane.as_ref()?;
        let replacement = Mux::get().get_tab(self.tab?)?.get_active_pane()?;
        (replacement.pane_id() != current.pane_id()).then(|| {
            self.pane = Some(replacement.clone());
            replacement
        })
    }

    pub(super) fn shown(&self) -> Option<Arc<dyn Pane>> {
        if !self.visible {
            return None;
        }
        match &self.program {
            Some(program) => program.pane.clone(),
            None => self.pane.clone(),
        }
    }

    pub(super) fn size(&self, configured: (f32, f32)) -> (f32, f32) {
        let shell = (
            self.adjusted.0.unwrap_or(configured.0),
            self.adjusted.1.unwrap_or(configured.1),
        );
        self.program.as_ref().map_or(shell, |program| {
            (
                program.size.0.unwrap_or(shell.0),
                program.size.1.unwrap_or(shell.1),
            )
        })
    }

    /// A program's own axis changes only that program; others change the window's size.
    fn adjust(&mut self, horizontal: bool, size: f32) {
        match self
            .program
            .as_mut()
            .map(|program| axis(&mut program.size, horizontal))
        {
            Some(explicit) if explicit.is_some() => *explicit = Some(size),
            _ => *axis(&mut self.adjusted, horizontal) = Some(size),
        }
    }

    /// Whether the config changed; a changed size setting replaces that axis's steps.
    fn configure(&mut self, config: QuickTerminalConfig) -> bool {
        if self.config == Some(config) {
            return false;
        }
        if let Some(previous) = self.config {
            if previous.size.0 != config.size.0 {
                self.adjusted.0 = None;
            }
            if previous.size.1 != config.size.1 {
                self.adjusted.1 = None;
            }
        }
        self.config = Some(config);
        true
    }

    fn wants(&self, slot: Slot) -> bool {
        match slot {
            Slot::Shell(id) => self.pending == Some(id),
            Slot::Program(id) => self
                .program
                .iter()
                .chain(&self.kept)
                .any(|program| program.id == id),
        }
    }

    fn program_panes(&self) -> impl Iterator<Item = &Arc<dyn Pane>> {
        self.program
            .iter()
            .chain(&self.kept)
            .filter_map(|program| program.pane.as_ref())
    }

    fn put_away(&mut self, alive: impl Fn(&Arc<dyn Pane>) -> bool) -> Option<Arc<dyn Pane>> {
        let program = self.program.take()?;
        if program.spec.preserve && program.pane.as_ref().map_or(true, alive) {
            self.kept.push(program);
            return None;
        }
        program.pane
    }

    pub(super) fn reusable(
        &self,
        spec: &TerminalProgram,
        alive: impl Fn(&Arc<dyn Pane>) -> bool,
    ) -> bool {
        spec.preserve
            && self.program.iter().chain(&self.kept).any(|program| {
                program.spec.preserve
                    && program.spec.spawn == spec.spawn
                    && program.pane.as_ref().map_or(true, &alive)
            })
    }

    /// Takes on `spec`'s settings, so another binding running the same command reopens it as its own.
    fn take_kept(&mut self, spec: &TerminalProgram) -> Option<Program> {
        let index = self
            .kept
            .iter()
            .position(|kept| spec.preserve && kept.spec.spawn == spec.spawn)?;
        let kept = self.kept.remove(index);
        if kept.spec == *spec {
            return Some(kept);
        }
        Some(Program {
            pane: kept.pane,
            ..Program::new(kept.id, spec.clone())
        })
    }

    fn prune(&mut self, alive: impl Fn(&Arc<dyn Pane>) -> bool) -> Vec<Arc<dyn Pane>> {
        let mut ended = Vec::new();
        self.kept.retain(|program| match &program.pane {
            Some(pane) if !alive(pane) => {
                ended.push(pane.clone());
                false
            }
            _ => true,
        });
        ended
    }

    pub(super) fn colors(&self, slot: Slot) -> Option<config::Palette> {
        let Slot::Program(id) = slot else {
            return None;
        };
        self.program
            .iter()
            .chain(&self.kept)
            .find(|program| program.id == id)?
            .spec
            .colors
            .clone()
    }

    fn discard_shell(&mut self) -> Option<Arc<dyn Pane>> {
        self.pending = None;
        self.tab = None;
        self.pane.take()
    }

    pub(super) fn launch_shell(&mut self) -> Slot {
        self.launches += 1;
        self.pending = Some(self.launches);
        Slot::Shell(self.launches)
    }

    /// Reject stale completions here, so callers cannot accidentally revive a discarded slot.
    pub(super) fn complete(&mut self, slot: Slot, pane: Option<Arc<dyn Pane>>) -> bool {
        if !self.wants(slot) {
            return false;
        }
        match (slot, pane) {
            (Slot::Shell(_), pane) => {
                self.pending = None;
                self.visible &= pane.is_some() || self.program.is_some();
                self.tab = pane.as_ref().and_then(mirrored_tab);
                self.pane = pane;
            }
            (Slot::Program(id), Some(pane)) => {
                if let Some(program) = self
                    .program
                    .iter_mut()
                    .chain(&mut self.kept)
                    .find(|program| program.id == id)
                {
                    program.pane = Some(pane);
                }
            }
            (Slot::Program(id), None) => {
                self.kept.retain(|program| program.id != id);
                if self
                    .program
                    .as_ref()
                    .is_some_and(|program| program.id == id)
                {
                    self.program = None;
                    self.visible = false;
                }
            }
        }
        true
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let programs: Vec<_> = self.program_panes().cloned().collect();
        for pane in self.pane.take().into_iter().chain(programs) {
            remove(&pane);
        }
    }
}
