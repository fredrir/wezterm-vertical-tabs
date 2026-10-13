//! Provider deadlines keep durable work running in inactive windows.
use crate::termwindow::{TermWindow, TermWindowNotif};
use std::time::Instant;
use window::WindowOps;

#[derive(Default)]
pub(super) struct ProviderDeadline {
    task: Option<promise::spawn::Task<()>>,
    scheduled: Option<Instant>,
    token: u64,
}
impl ProviderDeadline {
    /// Return a new timer only when the provider's deadline changed.
    fn replace(&mut self, deadline: Option<Instant>) -> Option<(Instant, u64)> {
        if self.scheduled == deadline {
            return None;
        }
        self.cancel();
        self.scheduled = deadline;
        deadline.map(|deadline| (deadline, self.token))
    }

    pub(super) fn cancel(&mut self) {
        self.task.take();
        self.scheduled = None;
        // Timer notifications can already be queued when their task is dropped.
        self.token = self.token.wrapping_add(1);
    }

    fn complete(&mut self, token: u64) -> bool {
        if self.token != token || self.scheduled.is_none() {
            return false;
        }
        self.task.take();
        self.scheduled = None;
        true
    }
}

impl TermWindow {
    pub(super) fn vtabs_schedule_provider(&mut self, window_id: usize) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(ui) = self.vtabs_host_mut(window_id) else {
            return;
        };
        let Some((deadline, token)) = ui.deadline.replace(ui.provider.deadline()) else {
            return;
        };
        let geometry = ui.geometry;
        ui.deadline.task = Some(promise::spawn::spawn(async move {
            smol::Timer::at(deadline).await;
            let repaint = window.clone();
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                let active = tw.mux_window_id == window_id;
                let geometry = if active {
                    tw.vtabs_geometry()
                } else {
                    geometry
                };
                let Some(ui) = tw.vtabs_host_mut(window_id) else {
                    return;
                };
                if !ui.deadline.complete(token) {
                    return;
                }
                ui.geometry = geometry;
                // Rendering also advances nonvisual work, such as a debounced write.
                ui.provider.render(geometry, Instant::now());
                if active {
                    tw.vtabs_sync();
                    repaint.invalidate();
                }
                tw.vtabs_schedule_provider(window_id);
            })));
        }));
    }
}

#[cfg(test)]
mod vtabs_tests {
    use super::ProviderDeadline;
    use std::time::{Duration, Instant};

    #[test]
    fn superseded_and_cancelled_notifications_do_not_consume_the_current_deadline() {
        let now = Instant::now();
        let mut deadline = ProviderDeadline::default();
        let (_, first) = deadline.replace(Some(now)).unwrap();
        assert!(deadline.replace(Some(now)).is_none());
        let later = now + Duration::from_secs(1);
        let (_, second) = deadline.replace(Some(later)).unwrap();
        assert!(!deadline.complete(first));
        assert!(deadline.complete(second));
        assert!(!deadline.complete(second));

        let (_, cancelled) = deadline.replace(Some(later)).unwrap();
        deadline.cancel();
        assert!(!deadline.complete(cancelled));
        let (_, rescheduled) = deadline.replace(Some(later)).unwrap();
        assert!(deadline.complete(rescheduled));
    }

    #[test]
    fn removing_a_provider_deadline_invalidates_its_notification() {
        let mut deadline = ProviderDeadline::default();
        let (_, token) = deadline.replace(Some(Instant::now())).unwrap();
        assert!(deadline.replace(None).is_none());
        assert!(!deadline.complete(token));
    }
}
