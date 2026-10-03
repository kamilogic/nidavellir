use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tracing::info;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};

use crate::ipc_server;
use crate::AppState;
use crate::SERVICE_NAME;
use nidavellir_driver_pawnio::DriverManager;

/// Set only under SCM: console mode has nobody to start the process again.
static STOP_TX: OnceLock<std::sync::mpsc::Sender<Result<(), String>>> = OnceLock::new();
static DRIVER_RESET_REQUESTED: AtomicBool = AtomicBool::new(false);

/// The driver-only reset exits non-zero on purpose, so SCM recovery must restart the service on
/// every failure, non-crash failures included (`sc failureflag 1`).
pub(crate) fn driver_reset_available() -> Result<(), String> {
    use windows_service::service::{ServiceAccess, ServiceActionType};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    if STOP_TX.get().is_none() {
        return Err("exige o serviço instalado, não o modo console".into());
    }
    let scm = |error: windows_service::Error| format!("SCM: {error}");
    let service = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(scm)?
        .open_service(SERVICE_NAME, ServiceAccess::QUERY_CONFIG)
        .map_err(scm)?;
    let actions = service.get_failure_actions().map_err(scm)?.actions.unwrap_or_default();
    let restarts = !actions.is_empty()
        && actions.iter().all(|action| action.action_type == ServiceActionType::Restart);
    if restarts && service.get_failure_actions_on_non_crash_failures().map_err(scm)? {
        Ok(())
    } else {
        Err("a recuperação do serviço no Windows não está configurada para reiniciá-lo".into())
    }
}

pub(crate) fn under_scm() -> bool {
    STOP_TX.get().is_some()
}

/// Stop like an SCM stop and exit zero, so SCM recovery leaves it stopped: the program exited, or
/// nothing needs the Core any more. A closed channel means the service is already stopping.
pub(crate) fn request_clean_stop() {
    if let Some(stop) = STOP_TX.get() {
        crate::shutdown::begin();
        let _ = stop.send(Ok(()));
    }
}

/// Stop like an SCM stop (workers released, stock confirmed, NVAPI released), then restart the
/// GPU device and exit non-zero; see `auto_resume`.
pub(crate) fn request_driver_reset() -> Result<(), String> {
    let stop = STOP_TX.get().ok_or("exige o serviço instalado")?;
    DRIVER_RESET_REQUESTED.store(true, Ordering::SeqCst);
    stop.send(Err("driver-only GPU reset requested".into()))
        .map_err(|_| "o serviço já está parando".to_string())
}

pub fn run_service() -> windows_service::Result<()> {
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    let pipe_failure_tx = shutdown_tx.clone();
    let _ = STOP_TX.set(shutdown_tx.clone());

    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                crate::shutdown::begin();
                let _ = shutdown_tx.send(Ok(()));
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::StartPending,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 1,
        wait_hint: Duration::from_secs(30),
        process_id: None,
    })?;

    info!("Nidavellir Core Service initializing");

    // Parachute first: the service boots before login, so it reads the
    // boot-flag and recovers from any prior crash before touching hardware.
    let safe_store = nidavellir_core::safe_loop::SafeLoopStore::system();
    crate::gpu_power_sweep::reconcile_interrupted_forge(&safe_store);
    crate::safe_loop_runtime::run_startup_recovery(&safe_store);
    crate::safe_loop_runtime::spawn_heartbeat(safe_store.clone());
    // The installed Windows service is the product runtime. It must own the same boot and live TDR
    // reconciliation as console mode before any persisted profile can be reapplied.
    let sentinel_ready = match crate::tdr_sentinel::initialize_reboot_guard() {
        Ok(snapshot) => {
            let baseline = snapshot.watcher_baseline();
            match crate::tdr_sentinel::startup_reconcile(&safe_store, &snapshot) {
                Ok(_) => {
                    // The synchronous handshake proves the watcher thread is active and its
                    // seed/floor are durable before this branch may authorize reapply.
                    match crate::tdr_sentinel::spawn(safe_store.clone(), baseline) {
                        Ok(()) => true,
                        Err(error) => {
                            crate::tdr_sentinel::mark_gpu_reboot_required(
                                "sentinel-watcher-startup-error",
                            );
                            tracing::error!(
                                "sentinel watcher startup failed closed ({error}); service remains stock"
                            );
                            false
                        }
                    }
                }
                Err(error) => {
                    crate::tdr_sentinel::mark_gpu_reboot_required(
                        "sentinel-startup-reconcile-error",
                    );
                    tracing::error!(
                        "sentinel startup reconciliation failed closed ({error}); service remains stock and the uncommitted cursor will be retried"
                    );
                    false
                }
            }
        }
        Err(error) => {
            tracing::error!(
                "sentinel Event Log initialization failed closed ({error}); service remains stock"
            );
            false
        }
    };
    if !sentinel_ready {
        tracing::warn!("persisted GPU profile reapply disabled by the Sentinel startup guard");
    }
    // The profile is reapplied when the program connects, not at boot (2026-10-03).
    crate::program_session::startup(sentinel_ready);

    let hw = nidavellir_core::detect_hardware();
    let state = Arc::new(Mutex::new(AppState {
        driver: DriverManager::new(),
        sensor_engine: nidavellir_core::sensors::SensorEngine::new(),
        motherboard: hw.motherboard,
        safe_store: safe_store.clone(),
        gpu_validation: crate::gpu_real::GpuValidationHandle::default(),
        real_sweep: crate::gpu_sweep_real::RealSweepHandle::default(),
        mem_sweep: crate::gpu_mem_sweep::MemSweepHandle::default(),
        forge_all: crate::gpu_forge_all::ForgeAllHandle::default(),
        benchmark: crate::gpu_benchmark::BenchmarkHandle::default(),
        // Seed from the persisted forge result so a restart restores forged
        // profiles/points instead of showing an unforged GPU.
        power_sweep: crate::gpu_power_sweep::restore_handle(),
        game_trace: crate::game_trace::GameTraceHandle::default(),
        manual_point: crate::manual_point::ManualPointHandle::default(),
        detector_lab: crate::detector_lab::DetectorLabHandle::default(),
    }));

    crate::auto_resume::spawn(Arc::clone(&state));
    crate::program_session::spawn_watchdog(Arc::clone(&state));
    let pipe_state = Arc::clone(&state);
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        if let Err(e) = ipc_server::run_pipe_server(pipe_state, Some(ready_tx)) {
            tracing::error!("Pipe server error: {e}");
            crate::shutdown::begin();
            let _ = pipe_failure_tx.send(Err(e));
        }
    });

    let startup = ready_rx.recv_timeout(Duration::from_secs(5))
        .map_err(|error| format!("IPC listener readiness was not confirmed: {error}"))
        .and_then(|result| result)
        .and_then(|()| {
            status_handle.set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Running,
                controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            }).map_err(|error| format!("Cannot report Running: {error}"))
        });
    let stop_reason = match startup {
        Ok(()) => {
            info!("Nidavellir Core Service ready");
            shutdown_rx.recv().unwrap_or_else(|error| Err(format!("Service control channel closed: {error}")))
        }
        Err(error) => Err(error),
    };
    crate::shutdown::begin();
    if let Err(error) = &stop_reason { tracing::error!("service failed; shutting down: {error}"); }

    let stop_status = status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::StopPending,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 1,
        wait_hint: Duration::from_secs(20),
        process_id: None,
    });
    if let Err(error) = &stop_status {
        tracing::error!("cannot report StopPending; still performing shutdown cleanup: {error}");
    }
    let result = crate::shutdown::complete(state, Duration::from_secs(20));
    if DRIVER_RESET_REQUESTED.load(Ordering::SeqCst) {
        // pnputil (≤ 60 s) plus the adapter readiness wait (≤ 30 s) outlast the first stop hint.
        let _ = status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::StopPending,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 2,
            wait_hint: Duration::from_secs(120),
            process_id: None,
        });
        crate::auto_resume::finish_driver_reset(&result);
    }
    let failed = result.is_err() || stop_status.is_err() || stop_reason.is_err();
    match result {
        Ok(()) => info!("service shutdown complete: workers released and required stock recovery confirmed"),
        Err(error) => tracing::error!("service shutdown incomplete; recovery evidence preserved: {error}"),
    }

    if let Err(error) = status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: if failed { ServiceExitCode::ServiceSpecific(1) } else { ServiceExitCode::Win32(0) },
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    }) {
        tracing::error!("cannot report final service status: {error}");
        crate::shutdown::exit_process(1);
    }

    crate::shutdown::exit_process(if failed { 1 } else { 0 });
}
