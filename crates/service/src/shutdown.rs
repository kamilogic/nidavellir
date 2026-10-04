//! Shared console/SCM shutdown. A timed-out cleanup is followed by process termination,
//! never by resuming service or starting replacement GPU work alongside an abandoned thread.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use nidavellir_core::safe_loop::SafeLoopStore;

static REQUESTED: AtomicBool = AtomicBool::new(false);

/// Close admission immediately, including requests already queued on AppState.
pub(crate) fn begin() -> bool {
    !REQUESTED.swap(true, Ordering::SeqCst)
}
pub(crate) fn is_requested() -> bool {
    REQUESTED.load(Ordering::SeqCst)
}

/// Avoid loader-lock teardown with background driver threads. On failure no clean marker
/// is committed; termination ends the timed-out cleanup too, and startup owns recovery.
#[cfg(windows)]
pub(crate) fn exit_process(code: u32) -> ! {
    unsafe {
        windows::Win32::System::Threading::TerminateProcess(
            windows::Win32::System::Threading::GetCurrentProcess(),
            code,
        )
    }
    .ok();
    std::process::abort();
}

pub(crate) fn complete(
    state: Arc<Mutex<crate::AppState>>,
    timeout: Duration,
) -> Result<(), String> {
    begin();
    finish_after_cleanup(timeout, move || {
        let started = std::time::Instant::now();
        let mut state = state
            .lock()
            .map_err(|_| "Service state lock is poisoned".to_string())?;
        let store = state.safe_store.clone();
        // A leftover marker must never turn a later failed stop into a clean recovery.
        store
            .clear_clean_shutdown()
            .map_err(|e| format!("Cannot clear stale shutdown marker: {e}"))?;
        let active = crate::ipc_server::gpu_operation_running(&state);
        crate::ipc_server::request_mutating_worker_stop(&mut state, true);
        crate::ipc_server::wait_for_mutating_workers_to_quiesce(&state)?;
        crate::tdr_sentinel::quiesce_for_shutdown()?;
        tracing::info!("shutdown: workers and Sentinel quiet after {} ms", started.elapsed().as_millis());
        let applied = crate::gpu_apply::load_applied_checked()?.is_some();
        restore_stock(
            &store,
            active || applied,
            crate::gpu_power_sweep::reset_to_stock_checked,
        )?;
        state.manual_point.mark_reset();
        tracing::info!("shutdown: stock confirmed after {} ms", started.elapsed().as_millis());
        // Admission is closed and app-state readers, tuning workers and Sentinel calls
        // are quiescent. Balance NVAPI before process teardown; failure/timeout must not
        // commit a clean marker or reopen access to the runtime.
        #[cfg(windows)]
        {
            tracing::info!("shutdown: releasing NVAPI runtime after GPU users quiesced");
            nidavellir_gpu_nvapi::shutdown_runtime()?;
            tracing::info!("shutdown: NVAPI runtime released after {} ms", started.elapsed().as_millis());
        }
        Ok(store)
    })
}

/// An untouched installation (including a VM without NVIDIA hardware) has nothing to restore.
/// Driver absence/error is never used as evidence that a previously tuned GPU is already at stock.
fn restore_stock(
    store: &SafeLoopStore,
    active_or_applied: bool,
    reset: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    use nidavellir_core::safe_loop::SafeLoopState;
    let flag = store
        .read_boot_flag_checked()
        .map_err(|e| format!("Boot flag is unreadable: {e}"))?;
    let record = store
        .load_record_checked()
        .map_err(|e| format!("Safe Loop is unreadable: {e}"))?;
    if active_or_applied
        || flag.is_some()
        || record.state != SafeLoopState::Idle
        || record.safe_mode
        || record.pending_forge_incident.is_some()
        || record.last_validated.is_some()
    {
        // Hardware-only reset preserves the qualified descriptor for startup reapply.
        // Pending incidents and negative learning are neither acknowledged nor rewritten here.
        reset()?;
    }
    if let Some(flag) = flag {
        if !store
            .clear_boot_flag_if_matches(&flag)
            .map_err(|e| e.to_string())?
        {
            return Err("Boot flag ownership changed during shutdown; recovery preserved".into());
        }
    }
    Ok(())
}

/// Keep the commit on the waiting thread. A late cleanup reply cannot write a clean marker.
fn finish_after_cleanup(
    timeout: Duration,
    cleanup: impl FnOnce() -> Result<SafeLoopStore, String> + Send + 'static,
) -> Result<(), String> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("nidavellir-shutdown".into())
        .spawn(move || {
            let _ = tx.send(cleanup());
        })
        .map_err(|e| format!("Cannot start shutdown cleanup: {e}"))?;
    let store = rx.recv_timeout(timeout).map_err(|error| format!(
        "Shutdown did not complete within the allowed time ({error}); stock/worker release is unconfirmed"
    ))??;
    if store
        .read_boot_flag_checked()
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Err("Boot flag is still armed after cleanup; clean shutdown refused".into());
    }
    store
        .write_clean_shutdown()
        .map_err(|e| format!("Cannot persist clean shutdown: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store(name: &str) -> SafeLoopStore {
        SafeLoopStore::new(
            std::env::temp_dir().join(format!("nidavellir-shutdown-{}-{name}", std::process::id())),
        )
    }

    #[test]
    fn untouched_installation_does_not_require_a_gpu_but_prior_activity_does() {
        let store = store("untouched");
        restore_stock(&store, false, || {
            panic!("no hardware calls on untouched install")
        })
        .unwrap();
        assert_eq!(
            restore_stock(&store, true, || Err("driver unavailable".into())).unwrap_err(),
            "driver unavailable"
        );
    }

    #[test]
    fn stock_failure_preserves_transaction_and_recovery_record() {
        use nidavellir_core::safe_loop::{BootFlag, SafeLoopRecord, SafeLoopState, TuningPoint};
        let store = store("stock-failure");
        let flag = BootFlag::new(TuningPoint::from_axes([("gpu_freq_mhz", 1920)]), "dwell");
        store.arm_boot_flag(&flag).unwrap();
        let record = SafeLoopRecord {
            state: SafeLoopState::Unstable,
            ..Default::default()
        };
        store.save_record(&record).unwrap();
        let before = std::fs::read(store.record_path()).unwrap();
        let flag_before = std::fs::read(store.boot_flag_path()).unwrap();
        assert!(restore_stock(&store, false, || Err("stock reset failed".into())).is_err());
        assert_eq!(std::fs::read(store.boot_flag_path()).unwrap(), flag_before);
        assert_eq!(std::fs::read(store.record_path()).unwrap(), before);
        // Only a confirmed reset may clear the same transaction; the incident record stays intact.
        restore_stock(&store, false, || Ok(())).unwrap();
        assert!(store.read_boot_flag_checked().unwrap().is_none());
        assert_eq!(std::fs::read(store.record_path()).unwrap(), before);
        // Historical non-idle safety state still requires recovery, even without a current flag.
        assert!(restore_stock(&store, false, || Err("driver unavailable".into())).is_err());
        std::fs::remove_dir_all(store.base_dir()).unwrap();
    }

    #[test]
    fn clean_marker_requires_completed_cleanup_and_preserves_other_files() {
        let store = store("complete");
        std::fs::create_dir_all(store.base_dir()).unwrap();
        store.clear_clean_shutdown().unwrap();
        let evidence = store.base_dir().join("condemnation_ledger.jsonl");
        std::fs::write(&evidence, "preserved negative evidence").unwrap();
        let done = store.clone();
        finish_after_cleanup(Duration::from_secs(1), move || Ok(done)).unwrap();
        assert!(store.is_clean_shutdown_present());
        assert_eq!(
            std::fs::read_to_string(evidence).unwrap(),
            "preserved negative evidence"
        );
        std::fs::remove_dir_all(store.base_dir()).unwrap();
    }

    #[test]
    fn timeout_never_commits_even_if_cleanup_finishes_late() {
        let store = store("timeout");
        store.clear_clean_shutdown().unwrap();
        let late = store.clone();
        let (release, wait) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let result = finish_after_cleanup(Duration::from_millis(20), move || {
            wait.recv().unwrap();
            finished.send(()).unwrap();
            Ok(late)
        });
        assert!(result.unwrap_err().contains("unconfirmed"));
        assert!(!store.is_clean_shutdown_present());
        release.send(()).unwrap();
        done.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(!store.is_clean_shutdown_present());
    }

    #[cfg(windows)]
    #[test]
    fn process_exit_is_bounded_and_reports_cleanup_failure() {
        const CASE: &str = "NIDAVELLIR_SHUTDOWN_TEST_CASE";
        const DIR: &str = "NIDAVELLIR_SHUTDOWN_TEST_DIR";
        if let Ok(case) = std::env::var(CASE) {
            // This test process never builds AppState or loads the GPU. Only the native exit
            // and real shutdown supervisor run; the stalled cleanup remains owned until exit.
            let store = SafeLoopStore::new(std::env::var_os(DIR).unwrap());
            begin();
            let result = finish_after_cleanup(Duration::from_millis(30), move || {
                if case == "stalled" {
                    loop {
                        std::thread::park();
                    }
                }
                Ok(store)
            });
            exit_process(if result.is_ok() { 0 } else { 1 });
        }
        for case in ["complete", "stalled"] {
            let store = store(&format!("process-{case}"));
            store.clear_clean_shutdown().unwrap();
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "shutdown::tests::process_exit_is_bounded_and_reports_cleanup_failure",
                ])
                .env(CASE, case)
                .env(DIR, store.base_dir())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let started = std::time::Instant::now();
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if started.elapsed() > Duration::from_secs(5) {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("shutdown left its test process running after the deadline");
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            assert_eq!(status.code(), Some(if case == "complete" { 0 } else { 1 }));
            assert_eq!(store.is_clean_shutdown_present(), case == "complete");
            if store.base_dir().exists() {
                std::fs::remove_dir_all(store.base_dir()).unwrap();
            }
        }
    }

    #[test]
    fn failed_panicked_or_armed_cleanup_never_reports_clean_stop() {
        let failed = finish_after_cleanup(Duration::from_secs(1), || {
            Err("worker still owns GPU".into())
        });
        assert_eq!(failed.unwrap_err(), "worker still owns GPU");
        assert!(
            finish_after_cleanup(Duration::from_secs(1), || panic!("injected cleanup panic"))
                .is_err()
        );
        let store = store("armed");
        std::fs::create_dir_all(store.base_dir()).unwrap();
        store.clear_clean_shutdown().unwrap();
        std::fs::write(
            store.base_dir().join("boot_flag.json"),
            "{invalid but preserved}",
        )
        .unwrap();
        let armed = store.clone();
        assert!(finish_after_cleanup(Duration::from_secs(1), move || Ok(armed)).is_err());
        assert!(!store.is_clean_shutdown_present());
        assert_eq!(
            std::fs::read_to_string(store.base_dir().join("boot_flag.json")).unwrap(),
            "{invalid but preserved}"
        );
        let flag = nidavellir_core::safe_loop::BootFlag::new(
            nidavellir_core::safe_loop::TuningPoint::stock(),
            "injected",
        );
        store.clear_boot_flag().unwrap();
        store.arm_boot_flag(&flag).unwrap();
        let armed = store.clone();
        assert!(
            finish_after_cleanup(Duration::from_secs(1), move || Ok(armed))
                .unwrap_err()
                .contains("still armed")
        );
        assert!(!store.is_clean_shutdown_present());
        assert_eq!(store.read_boot_flag_checked().unwrap().unwrap(), flag);
        std::fs::remove_dir_all(store.base_dir()).unwrap();
    }
}
