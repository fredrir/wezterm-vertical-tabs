//! Order topology snapshots after local resize acknowledgements.
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

#[derive(Default)]
struct State {
    epoch: u64,
    pending: usize,
    requested: u64,
    completed: u64,
    applying: usize,
    waiter: Option<Waker>,
}

#[derive(Default)]
pub(crate) struct TopologySync {
    state: Arc<Mutex<State>>,
    pub serial: smol::lock::Mutex<()>,
}

impl TopologySync {
    pub fn begin_resize(&self) -> Option<ResizeGuard> {
        let mut state = self.state.lock().unwrap();
        // Applying remote geometry must not echo a new resize back to its source.
        if state.applying != 0 {
            return None;
        }
        state.epoch = state.epoch.wrapping_add(1);
        state.pending += 1;
        Some(ResizeGuard(Arc::clone(&self.state)))
    }
    pub fn request(&self) -> u64 {
        let mut state = self.state.lock().unwrap();
        state.requested += 1;
        state.requested
    }
    pub fn completed(&self, request: u64) -> bool {
        self.state.lock().unwrap().completed >= request
    }
    pub async fn ready(&self) -> (u64, u64) {
        futures::future::poll_fn(|cx| {
            let mut state = self.state.lock().unwrap();
            if state.pending == 0 {
                Poll::Ready((state.epoch, state.requested))
            } else {
                state.waiter = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
    pub fn accepts(&self, epoch: u64) -> bool {
        let state = self.state.lock().unwrap();
        state.pending == 0 && state.epoch == epoch
    }
    pub fn finish(&self, request: u64) -> bool {
        let mut state = self.state.lock().unwrap();
        state.completed = request;
        state.completed == state.requested
    }
    pub fn applying(&self) -> SnapshotGuard {
        self.state.lock().unwrap().applying += 1;
        SnapshotGuard(Arc::clone(&self.state))
    }
}

pub(crate) struct ResizeGuard(Arc<Mutex<State>>);
impl Drop for ResizeGuard {
    fn drop(&mut self) {
        let wake = {
            let mut state = self.0.lock().unwrap();
            state.pending -= 1;
            if state.pending == 0 {
                state.waiter.take()
            } else {
                None
            }
        };
        if let Some(waker) = wake {
            waker.wake();
        }
    }
}
pub(crate) struct SnapshotGuard(Arc<Mutex<State>>);
impl Drop for SnapshotGuard {
    fn drop(&mut self) {
        self.0.lock().unwrap().applying -= 1;
    }
}
