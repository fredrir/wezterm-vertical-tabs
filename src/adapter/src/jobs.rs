//! Jobs follow existing pane transports, including SSH and chained mux/TLS connections.
use super::Adapter;
use core::jobs::{JOBS_VAR, JobOperation, JobTarget, ShellJobs};
use std::time::{Duration, Instant};
use vtabs_app::{core, ui};

impl Adapter {
    pub(super) fn open_jobs(&mut self) {
        let jobs = self.collect_jobs();
        self.app.ui_mut().open_jobs(jobs);
        self.jobs_refresh = Some(Instant::now() + Duration::from_millis(500));
    }

    pub(super) fn poll_jobs(&mut self, now: Instant) {
        if !self.app.ui().jobs_open() {
            self.jobs_refresh = None;
        } else if self.jobs_refresh.is_none_or(|deadline| now >= deadline) {
            let jobs = self.collect_jobs();
            self.app.ui_mut().set_jobs(jobs);
            self.jobs_refresh = Some(now + Duration::from_millis(500));
        }
    }

    fn collect_jobs(&self) -> Vec<ui::JobEntry> {
        let Some(mux) = mux::Mux::try_get() else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        for pane in mux.iter_panes() {
            if pane.is_dead() {
                continue;
            }
            let Some((domain, _, _)) = mux.resolve_pane_id(pane.pane_id()) else {
                continue;
            };
            let Some(domain) = mux
                .get_domain(domain)
                .filter(|domain| domain.state() == mux::domain::DomainState::Attached)
            else {
                continue;
            };
            let vars = pane.copy_user_vars();
            let Some(shell) = vars.get(JOBS_VAR).and_then(|value| ShellJobs::parse(value)) else {
                continue;
            };
            if let Some(request) = shell.refresh() {
                if let Err(error) = pane.writer().write_all(request.as_bytes()) {
                    log::debug!("jobs refresh: {error}");
                }
            }
            for job in shell.jobs {
                entries.push(ui::JobEntry {
                    target: JobTarget {
                        pane: pane.pane_id() as u64,
                        shell: shell.shell,
                        number: job.number,
                        pid: job.pid,
                    },
                    command: job.command,
                    place: format!(
                        "{} · {} · pane {} · %{}",
                        shell.host,
                        domain.domain_name(),
                        pane.pane_id(),
                        job.number
                    ),
                    suspended: job.suspended,
                    ready: shell.ready,
                });
            }
        }
        entries.sort_by_key(|job| (job.target.pane, job.target.number));
        entries
    }

    pub(super) fn job_action(&mut self, target: JobTarget, operation: JobOperation) {
        if let Err(error) = self.send_job_action(target, operation) {
            self.app.ui_mut().set_error(error);
        }
    }

    fn send_job_action(
        &mut self,
        target: JobTarget,
        operation: JobOperation,
    ) -> Result<(), String> {
        let mux = mux::Mux::get();
        let pane = mux
            .get_pane(target.pane as usize)
            .filter(|pane| !pane.is_dead())
            .ok_or("Job's pane closed")?;
        let (domain, window, tab_id) = mux
            .resolve_pane_id(pane.pane_id())
            .ok_or("Job's pane closed")?;
        if !mux
            .get_domain(domain)
            .is_some_and(|domain| domain.state() == mux::domain::DomainState::Attached)
        {
            return Err("Job's domain disconnected".into());
        }
        let vars = pane.copy_user_vars();
        let shell = vars
            .get(JOBS_VAR)
            .and_then(|value| ShellJobs::parse(value))
            .ok_or("Job's shell unavailable")?;
        let request = shell.request(target, operation)?;
        if operation == JobOperation::Foreground {
            let tab = mux.get_tab(tab_id).ok_or("Job's tab closed")?;
            tab.set_zoomed(false);
            tab.set_active_pane(&pane);
            self.app.ui_mut().release_focus();
            if self.app.model().tabs.contains_key(&(tab_id as u64)) {
                self.dispatch(core::Intent::ActivateTab(tab_id as u64));
            } else {
                self.show_tab(window, tab_id as u64);
            }
        }
        pane.writer()
            .write_all(request.as_bytes())
            .map_err(|error| format!("Job action: {error}"))?;
        Ok(())
    }
}
