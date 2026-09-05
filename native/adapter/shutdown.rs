//! Bounded asynchronous persistence drain after the native window releases its provider.
use std::time::{Duration, Instant};
use vtabs_app::WindowApp;

const SHUTDOWN_BUDGET: Duration = Duration::from_secs(3);

/// The caller keeps WezTerm's activity guard alive while this future runs. Pending GUI
/// callbacks cannot reach a destroyed provider, so their logical requests are retried with
/// the store's normal optimistic revisions. No native window or mux lock is retained here.
pub async fn drain(mut app: WindowApp, outstanding: Option<u64>) {
    let started = Instant::now();
    let base = app.now();
    if let Some(request_id) = outstanding {
        app.restart_storage_after_cancel(request_id);
    }
    app.teardown();
    for error in app.take_errors() {
        log::warn!("native tabs shutdown: {error}");
    }
    if !app.storage_pending() {
        return;
    }

    let operation = async {
        while app.storage_pending() {
            let now = base.saturating_add(started.elapsed());
            if let Some(request) = app.take_storage_request(now) {
                let request_id = request.request_id;
                match super::storage::invoke(request).await {
                    Ok(response) => {
                        if let Err(error) = app.complete_storage(response) {
                            log::warn!("native tabs shutdown storage: {error}");
                        }
                    }
                    Err(error) => {
                        app.storage_failed(request_id, error.to_string());
                    }
                }
                for error in app.take_errors() {
                    log::warn!("native tabs shutdown storage: {error}");
                }
            } else if let Some(deadline) = app.storage_deadline() {
                let remaining = deadline.saturating_sub(now);
                if remaining.is_zero() {
                    // A due request that cannot be taken indicates exhausted limits or a
                    // detached flight. Yielding repeatedly would spin the GUI executor.
                    return Err("pending storage could not advance at its deadline");
                }
                smol::Timer::after(remaining).await;
            } else {
                return Err("pending storage has no retry deadline");
            }
        }
        Ok(())
    };
    let timeout = async {
        smol::Timer::after(SHUTDOWN_BUDGET.saturating_sub(started.elapsed())).await;
        Err("three-second persistence drain budget elapsed")
    };
    if let Err(error) = smol::future::race(operation, timeout).await {
        // Dropping invoke cancels its subprocess through kill_on_drop/reap_on_drop.
        log::error!("native tabs: window closed with unsaved changes: {error}");
    }
}
