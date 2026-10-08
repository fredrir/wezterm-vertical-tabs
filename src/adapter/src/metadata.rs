//! Optional process name refresh; an unchanged sample never touches the window.
use crate::termwindow::TermWindowNotif;
use mux::pane::{CachePolicy, Pane};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use window::{Window, WindowOps};

type Panes = Vec<(usize, Arc<dyn Pane>)>;

#[derive(Default)]
pub struct Refresh {
    /// Process names from the latest snapshot, by tab.
    processes: Rc<RefCell<HashMap<usize, String>>>,
    schedule: Option<(Duration, usize)>,
    task: Option<promise::spawn::Task<()>>,
    /// Revokes a change already posted to the window when its schedule ends.
    epoch: Arc<AtomicU64>,
}

impl Refresh {
    pub fn observe(&self, processes: impl Iterator<Item = (usize, String)>) {
        *self.processes.borrow_mut() = processes.collect();
    }

    pub fn cancel(&mut self) {
        self.task = None;
        self.schedule = None;
        self.epoch.fetch_add(1, Ordering::Relaxed);
    }

    /// Samples every `interval` until a process changes, then dirties the window once.
    pub fn schedule(
        &mut self,
        window: &Window,
        window_id: usize,
        schedule: Option<(Duration, usize)>,
    ) {
        if self.schedule != schedule {
            self.cancel();
        } else if self.task.as_ref().is_some_and(|task| !task.is_finished()) {
            return;
        }
        let Some((interval, _)) = schedule else {
            return;
        };
        let Some(mut panes) = active_panes(window_id) else {
            self.schedule = None;
            return;
        };
        self.schedule = schedule;
        let processes = Rc::clone(&self.processes);
        let epoch = Arc::clone(&self.epoch);
        let token = epoch.load(Ordering::Relaxed);
        let window = window.clone();
        self.task = Some(promise::spawn::spawn(async move {
            loop {
                smol::Timer::after(interval).await;
                // Process discovery can inspect the OS process table; keep it off the GUI thread.
                let samples = smol::unblock(move || {
                    panes
                        .into_iter()
                        .map(|(tab, pane)| {
                            (
                                tab,
                                pane.pane_id(),
                                pane.get_foreground_process_name(CachePolicy::FetchImmediate)
                                    .unwrap_or_default(),
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .await;
                if changed(&processes.borrow(), &samples) {
                    window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                        if tw.mux_window_id == window_id
                            && tw.ui_host.is_some()
                            && epoch.load(Ordering::Relaxed) == token
                        {
                            tw.vtabs_mark_dirty();
                            tw.vtabs_sync();
                            if let Some(window) = &tw.window {
                                window.invalidate();
                            }
                        }
                    })));
                    return;
                }
                let Some(next) = active_panes(window_id) else {
                    return;
                };
                panes = next;
            }
        }));
    }
}

/// Mux guards are released before sampling begins.
fn active_panes(window_id: usize) -> Option<Panes> {
    let mux = mux::Mux::get();
    let window = mux.get_window(window_id)?;
    let panes = window
        .iter_tabs()
        .filter_map(|tab| tab.get_active_pane().map(|pane| (tab.tab_id(), pane)))
        .collect::<Vec<_>>();
    (!panes.is_empty()).then_some(panes)
}

fn changed(processes: &HashMap<usize, String>, samples: &[(usize, usize, String)]) -> bool {
    let mux = mux::Mux::get();
    samples.iter().any(|(tab, pane, process)| {
        processes.get(tab).is_some_and(|known| known != process)
            && mux
                .get_tab(*tab)
                .and_then(|tab| tab.get_active_pane())
                .is_some_and(|active| active.pane_id() == *pane)
    })
}
