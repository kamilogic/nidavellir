//! The Core runs while the program is open, and without it only while a Forge run needs it (user
//! decisions 2026-10-03). The program sends `ProgramHeartbeat` every 5 s; the first one after the
//! Core starts reapplies the persisted profile (that reapply used to run at boot). With no request
//! for [`SESSION_TIMEOUT`] and no Forge run that needs the Core, the installed service stops like an
//! SCM stop, which restores stock and keeps the profile for the next start.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nidavellir_core::ipc::EXIT_REFUSED_FORGE;

use crate::AppState;

/// Twelve missed heartbeats; also the time a boot-started Core waits for the program.
const SESSION_TIMEOUT: Duration = Duration::from_secs(60);
const TICK: Duration = Duration::from_secs(5);
/// A tick this late means Windows slept: the program gets a fresh timeout to reconnect.
const SUSPEND_GAP: Duration = Duration::from_secs(30);

static REAPPLY_ALLOWED: AtomicBool = AtomicBool::new(false);
static SESSION_STARTED: AtomicBool = AtomicBool::new(false);
static LAST_CONTACT: Mutex<Option<Instant>> = Mutex::new(None);
/// The pipe serves one client at a time, so heartbeats wait behind a long request.
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Startup records whether the Sentinel guard allows a reapply; the first heartbeat performs it.
pub(crate) fn startup(reapply_allowed: bool) {
    REAPPLY_ALLOWED.store(reapply_allowed, Ordering::SeqCst);
    touch();
}

fn touch() {
    if let Ok(mut last) = LAST_CONTACT.lock() {
        *last = Some(Instant::now());
    }
}

fn idle() -> Duration {
    LAST_CONTACT.lock().ok().and_then(|last| *last).map_or(Duration::ZERO, |last| last.elapsed())
}

/// Held while one request is served; it counts as contact when it ends.
pub(crate) struct InFlight;

pub(crate) fn request_started() -> InFlight {
    IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    InFlight
}

impl Drop for InFlight {
    fn drop(&mut self) {
        touch();
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The first heartbeat with the GPU free starts the session and its one reapply. While a GPU
/// operation or a Forge run owns the GPU, a later heartbeat does it.
fn starts_session(started: bool, gpu_busy: bool) -> bool {
    !started && !gpu_busy
}

/// Called with the state lock held, so no other GPU request interleaves with the reapply.
pub(crate) fn heartbeat(state: &AppState) {
    let busy = crate::ipc_server::gpu_operation_running(state) || forge_needs_core(state);
    if !starts_session(SESSION_STARTED.load(Ordering::SeqCst), busy) {
        return;
    }
    SESSION_STARTED.store(true, Ordering::SeqCst);
    if REAPPLY_ALLOWED.load(Ordering::SeqCst) {
        crate::gpu_apply::reapply_on_boot(&state.safe_store);
    } else {
        tracing::warn!("program connected; profile reapply stays disabled by the startup guard");
    }
}

/// A Forge run is running, or will continue on its own after a crash.
fn forge_needs_core(state: &AppState) -> bool {
    crate::gpu_power_sweep::FORGE_ACTIVE.load(Ordering::SeqCst) || auto_resume_pending(state)
}

#[cfg(windows)]
fn auto_resume_pending(state: &AppState) -> bool {
    crate::auto_resume::needs_core(state)
}

#[cfg(not(windows))]
fn auto_resume_pending(_state: &AppState) -> bool {
    false
}

/// The user exited the program.
pub(crate) fn exit(state: &AppState) -> Result<(), String> {
    if forge_needs_core(state) {
        return Err(format!("{EXIT_REFUSED_FORGE}: stop the Forge run before exiting"));
    }
    if crate::service_impl::under_scm() {
        crate::service_impl::request_clean_stop();
        return Ok(());
    }
    // Console mode: the developer owns the process; exit still returns the GPU to stock.
    if crate::ipc_server::gpu_operation_running(state) {
        return Err("a GPU operation is running; stop it before exiting".into());
    }
    crate::gpu_power_sweep::reset_to_stock_checked()
}

fn should_stop(idle: Duration, in_flight: usize, forge_needs_core: bool) -> bool {
    in_flight == 0 && idle >= SESSION_TIMEOUT && !forge_needs_core
}

/// Installed service only: console mode has nobody to start the Core again.
pub(crate) fn spawn_watchdog(state: Arc<Mutex<AppState>>) {
    std::thread::spawn(move || {
        let mut last_tick = Instant::now();
        while !crate::shutdown::is_requested() {
            std::thread::sleep(TICK);
            if last_tick.elapsed() >= SUSPEND_GAP {
                touch();
            }
            last_tick = Instant::now();
            if !should_stop(idle(), IN_FLIGHT.load(Ordering::SeqCst), false) {
                continue;
            }
            let Ok(guard) = state.lock() else { continue };
            // A request that arrives now waits for this lock and is already counted in flight.
            if should_stop(idle(), IN_FLIGHT.load(Ordering::SeqCst), forge_needs_core(&guard)) {
                drop(guard);
                tracing::info!("program closed and no Forge run needs the Core; stopping at stock");
                crate::service_impl::request_clean_stop();
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_only_when_quiet_and_no_forge_run_needs_the_core() {
        assert!(should_stop(SESSION_TIMEOUT, 0, false));
        assert!(!should_stop(SESSION_TIMEOUT - Duration::from_secs(1), 0, false));
        assert!(!should_stop(SESSION_TIMEOUT * 10, 1, false), "a request is being served");
        assert!(!should_stop(SESSION_TIMEOUT * 10, 0, true), "a Forge run keeps the Core");
    }

    #[test]
    fn a_busy_gpu_defers_the_reapply_to_a_later_heartbeat() {
        assert!(starts_session(false, false));
        assert!(!starts_session(false, true), "the session must stay open for the reapply");
        assert!(!starts_session(true, false), "one reapply per Core process");
    }

    #[test]
    fn a_finished_request_counts_as_contact() {
        if let (Ok(mut last), Some(old)) =
            (LAST_CONTACT.lock(), Instant::now().checked_sub(SESSION_TIMEOUT * 2))
        {
            *last = Some(old);
        }
        let request = request_started();
        assert_eq!(IN_FLIGHT.load(Ordering::SeqCst), 1);
        drop(request);
        assert_eq!(IN_FLIGHT.load(Ordering::SeqCst), 0);
        assert!(idle() < SESSION_TIMEOUT);
    }
}
