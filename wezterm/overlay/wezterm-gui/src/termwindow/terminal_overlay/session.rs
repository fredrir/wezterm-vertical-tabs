//! Session lifetime, kept programs and token-checked launch completion.
use super::*;

#[derive(Default)]
pub(super) struct State {
    pub(super) visible: bool,
    pub(super) pending: Option<u64>,
    pub(super) launches: u64,
    pub(super) config: Option<QuickTerminalConfig>,
    /// Session size steps; a settings change clears its axis.
    pub(super) adjusted: (Option<f32>, Option<f32>),
    pub(super) motion: Option<Motion>,
    pub(super) pane: Option<Arc<dyn Pane>>,
    /// The shell's mirrored tab, where `attach_mux` swaps in its replacement.
    pub(super) tab: Option<TabId>,
    /// Shown instead of the shell; exists only while visible.
    pub(super) program: Option<Program>,
    pub(super) kept: Vec<Program>,
    pub(super) programs: u64,
    /// Program workspaces cleared of what a quit GUI left behind.
    pub(super) swept: HashSet<String>,
    /// The window config the quick panes last took.
    pub(super) applied: Option<config::ConfigHandle>,
}

pub(super) struct Program {
    pub(super) id: u64,
    pub(super) spec: TerminalProgram,
    /// The spec's size; steps change only the axes it sets.
    pub(super) size: (Option<f32>, Option<f32>),
    pub(super) pane: Option<Arc<dyn Pane>>,
}

impl Program {
    pub(super) fn new(id: u64, spec: TerminalProgram) -> Self {
        Self {
            id,
            size: (spec.width, spec.height),
            spec,
            pane: None,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Slot {
    Shell(u64),
    Program(u64),
}

impl State {
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
    pub(super) fn adjust(&mut self, horizontal: bool, size: f32) {
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
    pub(super) fn configure(&mut self, config: QuickTerminalConfig) -> bool {
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

    pub(super) fn wants(&self, slot: Slot) -> bool {
        match slot {
            Slot::Shell(id) => self.pending == Some(id),
            Slot::Program(id) => self
                .program
                .iter()
                .chain(&self.kept)
                .any(|program| program.id == id),
        }
    }

    pub(super) fn program_panes(&self) -> impl Iterator<Item = &Arc<dyn Pane>> {
        self.program
            .iter()
            .chain(&self.kept)
            .filter_map(|program| program.pane.as_ref())
    }

    pub(super) fn put_away(
        &mut self,
        alive: impl Fn(&Arc<dyn Pane>) -> bool,
    ) -> Option<Arc<dyn Pane>> {
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
    pub(super) fn take_kept(&mut self, spec: &TerminalProgram) -> Option<Program> {
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

    pub(super) fn prune(&mut self, alive: impl Fn(&Arc<dyn Pane>) -> bool) -> Vec<Arc<dyn Pane>> {
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

    pub(super) fn discard_shell(&mut self) -> Option<Arc<dyn Pane>> {
        self.pending = None;
        self.tab = None;
        self.pane.take()
    }

    pub(super) fn launch_shell(&mut self) -> Slot {
        self.launches += 1;
        self.pending = Some(self.launches);
        Slot::Shell(self.launches)
    }

    pub(super) fn settle(&mut self, slot: Slot, pane: Option<Arc<dyn Pane>>) {
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
    }
}

impl Drop for State {
    fn drop(&mut self) {
        let programs: Vec<_> = self.program_panes().cloned().collect();
        for pane in self.pane.take().into_iter().chain(programs) {
            remove(&pane);
        }
    }
}
