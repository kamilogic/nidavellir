#[cfg(windows)]
mod auto_resume;
mod detector_lab;
mod development_validation;
mod game_trace;
mod gpu_apply;
mod gpu_benchmark;
mod gpu_f2_sweep;
mod gpu_forge_all;
mod gpu_mem_sweep;
mod gpu_power_sweep;
mod gpu_real;
mod gpu_sweep_real;
mod gpu_undervolt;
mod gpu_verify;
mod health;
mod ipc_server;
mod manual_point;
mod program_session;
mod qualified_search;
mod safe_loop_runtime;
mod sensor_gather;
mod shutdown;
mod service_impl;
mod tdr_sentinel;

use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use tracing_subscriber::EnvFilter;
use windows_service::define_windows_service;

use gpu_benchmark::BenchmarkHandle;
use gpu_forge_all::ForgeAllHandle;
use gpu_mem_sweep::MemSweepHandle;
use gpu_power_sweep::PowerSweepHandle;
use gpu_real::GpuValidationHandle;
use gpu_sweep_real::RealSweepHandle;
use nidavellir_core::safe_loop::SafeLoopStore;
use nidavellir_driver_pawnio::DriverManager;

pub const SERVICE_NAME: &str = "NidavellirCore";
pub const PIPE_NAME: &str = r"\\.\pipe\NidavellirCore";

pub struct AppState {
    pub driver: DriverManager,
    pub sensor_engine: nidavellir_core::sensors::SensorEngine,
    pub motherboard: nidavellir_core::detector::MotherboardInfo,
    pub safe_store: SafeLoopStore,
    pub gpu_validation: GpuValidationHandle,
    pub real_sweep: RealSweepHandle,
    pub mem_sweep: MemSweepHandle,
    pub forge_all: ForgeAllHandle,
    pub benchmark: BenchmarkHandle,
    pub power_sweep: PowerSweepHandle,
    pub game_trace: game_trace::GameTraceHandle,
    pub manual_point: manual_point::ManualPointHandle,
    pub detector_lab: detector_lab::DetectorLabHandle,
}

define_windows_service!(ffi_service_main, service_main);

fn service_main(_arguments: Vec<OsString>) {
    if let Err(e) = service_impl::run_service() {
        tracing::error!("Service failed: {e}");
    }
}

/// The installed service has no console, so its log goes here too (one older 5 MB file is kept).
fn core_log_file() -> Option<std::fs::File> {
    let dir = nidavellir_core::safe_loop::default_data_dir();
    let path = dir.join("core.log");
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > 5 * 1024 * 1024) {
        let _ = std::fs::rename(&path, dir.join("core.log.1"));
    }
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::OpenOptions::new().create(true).append(true).open(path).ok()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use tracing_subscriber::prelude::*;
    let file = core_log_file().map(|file| {
        tracing_subscriber::fmt::layer().with_ansi(false).with_writer(std::sync::Mutex::new(file))
    });
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive("nidavellir=info".parse()?))
        .with(tracing_subscriber::fmt::layer())
        .with(file)
        .init();

    let args: Vec<OsString> = std::env::args_os().collect();
    if args.len() > 1 {
        match args[1].to_string_lossy().as_ref() {
            "run" | "console" => {
                match args.get(2).map(|s| s.to_string_lossy()).as_deref() {
                    None => (),
                    Some("--development-validation") if args.len() == 3 => {
                        development_validation::enable();
                        tracing::warn!("Development validation mode: one explicitly authorized Standard run; no automatic retries, Resume or profile Apply");
                    }
                    _ => return Err("Usage: console [--development-validation]".into()),
                }
                return run_standalone();
            }
            "verify-applied" => return run_verify_only(),
            "acceptance-preflight" => return run_acceptance_preflight(),
            "build-frontier" => return run_build_frontier_cmd(&args),
            "undervolt-probe" => return run_undervolt_probe_cmd(&args),
            "diagnose-f2-point" | "diagnose-f2-loads" => return gpu_undervolt::diagnostic::run(&args).map_err(Into::into),
            "game-trace" => return game_trace::run(&args),
            _ => {}
        }
    }

    windows_service::service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

/// Read-only diagnostic: classify the live VF curve against the applied profile and
/// print the result. Deliberately does NOT run startup recovery, the heartbeat,
/// `reapply_on_boot`, or the pipe server — so it performs **no apply, no reapply, and
/// no VF-curve write**. Safe to run while the GPU is at any state.
fn run_verify_only() -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("verify-applied: read-only curve verification (no reapply, no VF write)");
    let status = gpu_verify::verify_applied_curve();
    // Structured result to stdout for headless QA (in addition to the apply_verify log).
    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}

/// Inventory only: never enters AppState/startup recovery, starts a watcher, ACKs or writes GPU state.
fn run_acceptance_preflight() -> Result<(), Box<dyn std::error::Error>> {
    let store = SafeLoopStore::new(nidavellir_core::safe_loop::default_data_dir());
    let mut blockers = Vec::new();
    match store.load_record_checked() {
        Ok(record) => {
            if record.pending_forge_incident.is_some() { blockers.push("Incident acknowledgement is pending".to_string()); }
            if record.safe_mode || record.state == nidavellir_core::safe_loop::SafeLoopState::Unstable {
                blockers.push("Safe Loop recovery is required".to_string());
            }
        }
        Err(error) => blockers.push(format!("Safe Loop record is unreadable: {error}")),
    }
    match store.read_boot_flag_checked() {
        Ok(Some(_)) => blockers.push("Boot flag is armed; checked stock recovery is required".to_string()),
        Ok(None) => (),
        Err(error) => blockers.push(format!("Boot flag is unreadable: {error}")),
    }
    if let Some(reason) = gpu_power_sweep::forge_start_block_reason(&store) { blockers.push(reason); }
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "read_only": true,
        "build_revision": env!("NIDAVELLIR_BUILD_REVISION"),
        "gate": if blockers.is_empty() { "requires_live_service_verification" } else { "blocked" },
        "blockers": blockers,
        "note": "No service, recovery or GPU workload was started. This is not hardware acceptance."
    }))?);
    Ok(())
}

#[cfg(windows)]
fn start_supervised_cli_sentinel(store: &SafeLoopStore) -> Result<(), String> {
    let snapshot = tdr_sentinel::initialize_reboot_guard()?;
    let baseline = snapshot.watcher_baseline();
    if let Err(error) = tdr_sentinel::startup_reconcile(store, &snapshot) {
        tdr_sentinel::mark_gpu_reboot_required("sentinel-cli-reconcile-error");
        return Err(error);
    }
    tdr_sentinel::spawn(store.clone(), baseline).map_err(|error| {
        tdr_sentinel::mark_gpu_reboot_required("sentinel-cli-watcher-startup-error");
        format!("Sentinel watcher startup failed closed: {error}")
    })?;
    if let Some(event) = tdr_sentinel::reboot_required_event() {
        return Err(format!(
            "GPU driver recovery is latched at {event}; reboot Windows before a supervised hardware command"
        ));
    }
    Ok(())
}

/// `--confirm` present? Pure (unit-testable without hardware).
fn has_confirm_flag(args: &[OsString]) -> bool {
    args.iter().any(|a| a.to_string_lossy() == "--confirm")
}

/// `--help` / `-h` present? Pure (unit-testable without hardware).
fn has_help_flag(args: &[OsString]) -> bool {
    args.iter().any(|a| {
        let s = a.to_string_lossy();
        s == "--help" || s == "-h"
    })
}

/// Parse the first-run limiter flags (`--max-targets N`, `--max-probes N`,
/// `--max-probes-per-target N`, `--safe-start-cap MV`). Syntax-only: missing/non-numeric values
/// FAIL CLOSED (`Err`). Semantic checks (0 values, cap vs crash floor) happen in
/// `gpu_power_sweep::run_build_frontier`. Pure (unit-testable).
fn parse_frontier_limits(args: &[OsString]) -> Result<gpu_power_sweep::FrontierLimits, String> {
    let strs: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let mut limits = gpu_power_sweep::FrontierLimits::default();
    let mut i = 0;
    while i < strs.len() {
        match strs[i].as_str() {
            "--max-targets" => {
                let v = strs.get(i + 1).ok_or_else(|| "--max-targets needs a value".to_string())?;
                limits.max_targets =
                    Some(v.parse().map_err(|_| format!("--max-targets: invalid number '{v}'"))?);
                i += 2;
            }
            "--max-probes" => {
                let v = strs.get(i + 1).ok_or_else(|| "--max-probes needs a value".to_string())?;
                limits.max_probes =
                    Some(v.parse().map_err(|_| format!("--max-probes: invalid number '{v}'"))?);
                i += 2;
            }
            "--max-probes-per-target" => {
                let v = strs
                    .get(i + 1)
                    .ok_or_else(|| "--max-probes-per-target needs a value".to_string())?;
                limits.max_probes_per_target = Some(
                    v.parse().map_err(|_| format!("--max-probes-per-target: invalid number '{v}'"))?,
                );
                i += 2;
            }
            "--safe-start-cap" => {
                let v = strs.get(i + 1).ok_or_else(|| "--safe-start-cap needs a value".to_string())?;
                limits.safe_start_cap_mv =
                    Some(v.parse().map_err(|_| format!("--safe-start-cap: invalid number '{v}'"))?);
                i += 2;
            }
            // Opt-in warm-start voltage-bracket carry-forward (no value; default OFF).
            "--warm-start-brackets" => {
                limits.warm_start_brackets = true;
                i += 1;
            }
            // Opt-in F1b bind-seeking v1 (no value; default OFF): stop a target at the first
            // verified+stable binding point instead of walking a fixed number of bins.
            "--bind-seeking" => {
                limits.bind_seeking = true;
                i += 1;
            }
            // Opt-in F1c power-bound knee-seeking (no value; default OFF): after a Phase-A power-bound
            // collapse, run a focused Phase-B deep descent to find the VF knee.
            "--power-bound-knee-seeking" => {
                limits.power_bound_knee_seeking = true;
                i += 1;
            }
            // Phase-B deep-descent budget (value; default None → built-in default when knee-seeking
            // is on). Only bounds the focused descent depth; the global --max-probes stays the cap.
            "--phase-b-probes" => {
                let v = strs.get(i + 1).ok_or_else(|| "--phase-b-probes needs a value".to_string())?;
                limits.phase_b_probes =
                    Some(v.parse().map_err(|_| format!("--phase-b-probes: invalid number '{v}'"))?);
                i += 2;
            }
            _ => i += 1,
        }
    }
    Ok(limits)
}

/// Supervised console entry for the F1b multi-clock frontier (`build-frontier`). WITHOUT
/// `--confirm` it is a read-only DRY-RUN that only prints the plan (no hardware). WITH
/// `--confirm` it runs startup recovery (parachute) FIRST, then the real supervised hardware
/// frontier (transient VF ceilings + game-power dwells), always restoring stock. It never
/// applies or persists a profile.
fn run_build_frontier_cmd(args: &[OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let confirm = has_confirm_flag(args);
    let limits = match parse_frontier_limits(args) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("build-frontier: invalid flags: {e}");
            println!("build-frontier: invalid flags: {e}");
            return Ok(()); // clean exit, no hardware
        }
    };
    let store = SafeLoopStore::system();
    if confirm {
        tracing::warn!(
            "build-frontier: --confirm set — running startup recovery, then the SUPERVISED hardware frontier"
        );
        safe_loop_runtime::run_startup_recovery(&store);
        start_supervised_cli_sentinel(&store).map_err(
            |error| -> Box<dyn std::error::Error> {
                format!("build-frontier: Sentinel startup guard refused hardware: {error}").into()
            },
        )?;
    } else {
        tracing::info!("build-frontier: dry-run (pass --confirm to execute the supervised hardware run)");
    }
    gpu_power_sweep::run_build_frontier(&store, confirm, limits);
    Ok(())
}

/// Supervised console entry for the F2 `undervolt-probe`. `--help`/`-h` prints usage and exits
/// (no hardware, no plan, no Safe Loop access). WITHOUT `--confirm` it is a read-only DRY-RUN that
/// prints the plan (no hardware). WITH `--confirm` it runs startup recovery FIRST, then a supervised
/// F2 run: anchored `--steps 1` is ONE single step; anchored `--steps 2..=3` is a bounded same-target
/// multi-step descent that stops at the first non-stable candidate (`--simple` stays single-step);
/// `--manual-prior` (opt-in dev/known-GPU shortcut, requires `--start-mv`, single-step) anchors at the
/// explicit voltage with a separate larger bounded offset cap. Confirmed mode can write bounded
/// positive VF offsets and TDR/reboot. It never persists, applies, or promotes a profile.
fn run_undervolt_probe_cmd(args: &[OsString]) -> Result<(), Box<dyn std::error::Error>> {
    // Help short-circuits BEFORE anything else — no hardware read, no plan, no Safe Loop access.
    if has_help_flag(args) {
        println!("{}", gpu_undervolt::undervolt_usage());
        return Ok(());
    }
    let confirm = has_confirm_flag(args);
    let parsed = match gpu_undervolt::parse_undervolt_args(args) {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("undervolt-probe: invalid flags: {e}");
            println!("undervolt-probe: invalid flags: {e}");
            return Ok(()); // clean exit, no hardware
        }
    };
    let store = SafeLoopStore::system();
    if confirm {
        tracing::warn!(
            "undervolt-probe: --confirm set — running startup recovery, then ONE supervised F2 single \
             step (may write a bounded positive VF offset; can TDR/reboot)"
        );
        safe_loop_runtime::run_startup_recovery(&store);
        start_supervised_cli_sentinel(&store).map_err(
            |error| -> Box<dyn std::error::Error> {
                format!("undervolt-probe: Sentinel startup guard refused hardware: {error}").into()
            },
        )?;
    } else {
        tracing::info!(
            "undervolt-probe: dry-run (pass `--steps 1 --confirm` to execute one supervised single step)"
        );
    }
    gpu_undervolt::run_undervolt_probe(&store, confirm, parsed);
    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::has_confirm_flag;
    use std::ffi::OsString;

    fn os(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn help_flag_detected_for_long_and_short() {
        assert!(!super::has_help_flag(&os(&["undervolt-probe"])));
        assert!(super::has_help_flag(&os(&["undervolt-probe", "--help"])));
        assert!(super::has_help_flag(&os(&["undervolt-probe", "-h"])));
        assert!(!super::has_help_flag(&os(&["undervolt-probe", "--target-mhz", "1800"])));
    }

    #[test]
    fn confirm_flag_detected_only_when_present() {
        assert!(!has_confirm_flag(&os(&["build-frontier"])));
        assert!(has_confirm_flag(&os(&["build-frontier", "--confirm"])));
        assert!(has_confirm_flag(&os(&["build-frontier", "--confirm", "x"])));
        assert!(!has_confirm_flag(&os(&["build-frontier", "confirm"]))); // must be the flag form
    }

    #[test]
    fn parse_limits_reads_all_flags() {
        let l = super::parse_frontier_limits(&os(&[
            "build-frontier", "--max-targets", "1", "--max-probes", "6", "--safe-start-cap", "1075",
        ]))
        .unwrap();
        assert_eq!(l.max_targets, Some(1));
        assert_eq!(l.max_probes, Some(6));
        assert_eq!(l.safe_start_cap_mv, Some(1075));
    }

    #[test]
    fn parse_limits_defaults_when_absent() {
        let l = super::parse_frontier_limits(&os(&["build-frontier"])).unwrap();
        assert_eq!(l, crate::gpu_power_sweep::FrontierLimits::default());
        assert!(!l.warm_start_brackets); // opt-in: default OFF
        assert!(!l.bind_seeking); // opt-in: default OFF
    }

    #[test]
    fn parse_bind_seeking_flag_opt_in() {
        // Absent → off; present → on. No value; other flags unaffected; warm-start stays off.
        assert!(!super::parse_frontier_limits(&os(&["build-frontier"])).unwrap().bind_seeking);
        let l = super::parse_frontier_limits(&os(&[
            "build-frontier", "--bind-seeking", "--max-targets", "7", "--max-probes-per-target", "3",
        ]))
        .unwrap();
        assert!(l.bind_seeking);
        assert!(!l.warm_start_brackets); // bind-seeking does NOT enable warm-start
        assert_eq!(l.max_targets, Some(7));
        assert_eq!(l.max_probes_per_target, Some(3));
    }

    #[test]
    fn parse_power_bound_knee_seeking_flags_opt_in() {
        // Both default OFF/None; the flag is valueless, --phase-b-probes takes a number.
        let def = super::parse_frontier_limits(&os(&["build-frontier"])).unwrap();
        assert!(!def.power_bound_knee_seeking);
        assert_eq!(def.phase_b_probes, None);
        let l = super::parse_frontier_limits(&os(&[
            "build-frontier", "--power-bound-knee-seeking", "--phase-b-probes", "12",
            "--max-probes-per-target", "3",
        ]))
        .unwrap();
        assert!(l.power_bound_knee_seeking);
        assert_eq!(l.phase_b_probes, Some(12));
        assert_eq!(l.max_probes_per_target, Some(3)); // Phase A cap unaffected
        assert!(!l.bind_seeking); // independent of bind-seeking
        // Missing / non-numeric value fails closed.
        assert!(super::parse_frontier_limits(&os(&["build-frontier", "--phase-b-probes"])).is_err());
        assert!(super::parse_frontier_limits(&os(&["build-frontier", "--phase-b-probes", "x"])).is_err());
    }

    #[test]
    fn parse_warm_start_brackets_flag_opt_in() {
        // Absent → off; present → on. Existing flags unaffected.
        assert!(!super::parse_frontier_limits(&os(&["build-frontier"])).unwrap().warm_start_brackets);
        let l = super::parse_frontier_limits(&os(&[
            "build-frontier", "--warm-start-brackets", "--max-targets", "3",
        ]))
        .unwrap();
        assert!(l.warm_start_brackets);
        assert_eq!(l.max_targets, Some(3));
    }

    #[test]
    fn parse_limits_rejects_nonnumeric_and_missing_value() {
        assert!(super::parse_frontier_limits(&os(&["build-frontier", "--max-targets", "abc"])).is_err());
        assert!(super::parse_frontier_limits(&os(&["build-frontier", "--max-probes"])).is_err());
        assert!(super::parse_frontier_limits(&os(&["build-frontier", "--safe-start-cap", "x"])).is_err());
    }

    #[test]
    fn parse_max_probes_per_target_flag() {
        // Present → parsed; absent → None; missing/non-numeric value → error.
        let l = super::parse_frontier_limits(&os(&[
            "build-frontier", "--max-targets", "7", "--max-probes", "14", "--max-probes-per-target", "2",
        ]))
        .unwrap();
        assert_eq!(l.max_probes_per_target, Some(2));
        assert_eq!(l.max_targets, Some(7));
        assert_eq!(l.max_probes, Some(14));
        assert_eq!(
            super::parse_frontier_limits(&os(&["build-frontier"])).unwrap().max_probes_per_target,
            None
        );
        assert!(super::parse_frontier_limits(&os(&["build-frontier", "--max-probes-per-target"])).is_err());
        assert!(super::parse_frontier_limits(&os(&["build-frontier", "--max-probes-per-target", "x"])).is_err());
    }
}

fn run_standalone() -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting Nidavellir Core Service in console mode");

    // Parachute first: recover from any prior crash before doing anything else.
    let safe_store = SafeLoopStore::system();
    #[cfg(windows)]
    gpu_power_sweep::reconcile_interrupted_forge(&safe_store);
    safe_loop_runtime::run_startup_recovery(&safe_store);
    safe_loop_runtime::spawn_heartbeat(safe_store.clone());
    // The startup guard decides whether the program's first heartbeat may re-apply the persisted
    // GPU profile (volatile offsets); reapply itself still refuses after a crash or in Safe Mode.
    // v17 boot reconciliation: a hard wedge freezes the live sentinel with the machine — detect a
    // TDR that happened while we were down BEFORE re-applying the very profile that caused it.
    #[cfg(windows)]
    let sentinel_ready = match tdr_sentinel::initialize_reboot_guard() {
        Ok(snapshot) => {
            let baseline = snapshot.watcher_baseline();
            match tdr_sentinel::startup_reconcile(&safe_store, &snapshot) {
                Ok(_) => {
                    // Reapply is authorized only after the watcher thread acknowledges that its
                    // checked seed/floor have been durably persisted.
                    match tdr_sentinel::spawn(safe_store.clone(), baseline) {
                        Ok(()) => true,
                        Err(error) => {
                            tdr_sentinel::mark_gpu_reboot_required(
                                "sentinel-watcher-startup-error",
                            );
                            tracing::error!(
                                "sentinel watcher startup failed closed ({error}); console service remains stock"
                            );
                            false
                        }
                    }
                }
                Err(error) => {
                    tdr_sentinel::mark_gpu_reboot_required("sentinel-startup-reconcile-error");
                    tracing::error!(
                        "sentinel startup reconciliation failed closed ({error}); console service remains stock and the uncommitted cursor will be retried"
                    );
                    false
                }
            }
        }
        Err(error) => {
            tracing::error!(
                "sentinel Event Log initialization failed closed ({error}); console service remains stock"
            );
            false
        }
    };
    // The profile is reapplied when the program connects, not at startup (2026-10-03).
    #[cfg(windows)]
    let reapply_allowed = sentinel_ready && !development_validation::enabled();
    #[cfg(not(windows))]
    let reapply_allowed = true;
    if !reapply_allowed {
        tracing::warn!("persisted GPU profile reapply disabled by the Sentinel/development startup guard");
    }
    program_session::startup(reapply_allowed);

    let state = Arc::new(Mutex::new(AppState {
        driver: DriverManager::new(),
        sensor_engine: nidavellir_core::sensors::SensorEngine::new(),
        // Only the board is needed here; the full probe spawns PowerShell (~2 s) on every start.
        motherboard: nidavellir_core::detector::detect_motherboard(),
        safe_store,
        gpu_validation: GpuValidationHandle::default(),
        real_sweep: RealSweepHandle::default(),
        mem_sweep: MemSweepHandle::default(),
        forge_all: ForgeAllHandle::default(),
        benchmark: BenchmarkHandle::default(),
        // Seed from the persisted forge result so a restart restores forged
        // profiles/points instead of showing an unforged GPU.
        power_sweep: gpu_power_sweep::restore_handle(),
        game_trace: game_trace::GameTraceHandle::default(),
        manual_point: manual_point::ManualPointHandle::default(),
        detector_lab: detector_lab::DetectorLabHandle::default(),
    }));
    #[cfg(windows)]
    console_shutdown::install(Arc::clone(&state));
    #[cfg(windows)]
    auto_resume::spawn(Arc::clone(&state));
    let result = ipc_server::run_pipe_server(Arc::clone(&state), None);
    // A fatal listener error must also release workers and restore any applied tuning.
    let cleanup = shutdown::complete(state, std::time::Duration::from_secs(30));
    if let Err(error) = &cleanup { tracing::error!("console cleanup incomplete: {error}"); }
    if let Err(error) = &result { tracing::error!("console IPC failed: {error}"); }
    shutdown::exit_process(if result.is_err() || cleanup.is_err() { 1 } else { 0 });
}

/// Console-mode Ctrl+C / window-close handling. Without this, the main thread sits blocked in
/// `ConnectNamedPipe` and worker threads keep the GPU saturated, so process teardown stalls on
/// driver DLL detach until all queued GPU work drains — Ctrl+C and "End task" appear dead for a
/// long time. The handler signals every motor's cooperative stop (the same path the IPC Stop
/// request uses: the dwell cancels within a band/frame, resets to stock and clears the boot
/// flag), waits a bounded grace for the workers to land, then exits. Timeout exits nonzero with
/// recovery evidence preserved. Windows may preempt the grace when closing the console window.
#[cfg(windows)]
mod console_shutdown {
    use std::sync::{Arc, Mutex, OnceLock};

    use windows::Win32::Foundation::BOOL;
    use windows::Win32::System::Console::{
        SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT,
        CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
    };

    use crate::AppState;

    static STATE: OnceLock<Arc<Mutex<AppState>>> = OnceLock::new();
    fn shutdown_grace(event: u32) -> Option<std::time::Duration> {
        match event {
            CTRL_C_EVENT | CTRL_BREAK_EVENT => Some(std::time::Duration::from_secs(30)),
            // Windows normally allows only five seconds for closing a console. Leave room
            // for process termination; an unfinished cleanup must preserve recovery evidence.
            CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT => {
                Some(std::time::Duration::from_secs(4))
            }
            _ => None,
        }
    }

    pub fn install(state: Arc<Mutex<AppState>>) {
        let _ = STATE.set(state);
        unsafe {
            if SetConsoleCtrlHandler(Some(ctrl_handler), true).is_err() {
                tracing::warn!("console shutdown handler could not be installed");
            }
        }
    }

    /// Exit WITHOUT running DLL_PROCESS_DETACH/atexit: the v17 sentinel keeps background threads
    /// inside CreateProcess (wevtutil) / NVML / wgpu, and a normal `exit` deadlocks on the loader
    /// lock during DLL detach — the "shutdown complete but the window never closes" hang.
    /// A nonzero exit preserves unconfirmed recovery instead of claiming the GPU reached stock.
    fn hard_exit(code: u32) -> ! {
        crate::shutdown::exit_process(code)
    }

    unsafe extern "system" fn ctrl_handler(event: u32) -> BOOL {
        let Some(grace) = shutdown_grace(event) else { return BOOL(0); };
        // A second Ctrl+C while already landing = exit immediately.
        if !crate::shutdown::begin() {
            hard_exit(1);
        }
        // Console I/O is unreliable during CTRL_CLOSE_EVENT. In particular, a blocked log
        // here or after cleanup would bypass the cleanup deadline and prevent native exit.
        let result = STATE.get()
            .ok_or_else(|| "Console state is unavailable".to_string())
            .and_then(|state| crate::shutdown::complete(Arc::clone(state), grace));
        hard_exit(if result.is_ok() { 0 } else { 1 });
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn callback_exits_even_when_console_logging_would_block() {
            const EVENT: &str = "NIDAVELLIR_CONSOLE_CLOSE_TEST_EVENT";
            if let Ok(event) = std::env::var(EVENT) {
                struct BlockedConsole;
                impl std::io::Write for BlockedConsole {
                    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                        loop { std::thread::park(); }
                    }
                    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
                }
                let subscriber = tracing_subscriber::fmt()
                    .with_max_level(tracing::Level::TRACE)
                    .with_writer(|| BlockedConsole)
                    .finish();
                // Invoke the real callback in an isolated process without AppState/hardware.
                // A log before cleanup or native exit would wedge this blocked console sink.
                tracing::subscriber::with_default(subscriber, || unsafe {
                    let _ = ctrl_handler(event.parse().unwrap());
                });
                panic!("a recognized control event must terminate the fixture process");
            }
            for event in [CTRL_C_EVENT, CTRL_CLOSE_EVENT] {
                let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "console_shutdown::tests::callback_exits_even_when_console_logging_would_block"])
                    .env(EVENT, event.to_string())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn().unwrap();
                let started = std::time::Instant::now();
                let status = loop {
                    if let Some(status) = child.try_wait().unwrap() { break status; }
                    if started.elapsed() > std::time::Duration::from_secs(5) {
                        child.kill().unwrap();
                        child.wait().unwrap();
                        panic!("console callback was blocked before native exit");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                };
                // No service state: fail closed rather than claiming confirmed stock recovery.
                assert_eq!(status.code(), Some(1));
            }
        }

        #[test]
        fn close_events_fit_windows_deadline_without_shortening_ctrl_c_cleanup() {
            for event in [CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT] {
                assert_eq!(shutdown_grace(event), Some(std::time::Duration::from_secs(4)));
            }
            for event in [CTRL_C_EVENT, CTRL_BREAK_EVENT] {
                assert_eq!(shutdown_grace(event), Some(std::time::Duration::from_secs(30)));
            }
            assert_eq!(shutdown_grace(u32::MAX), None);
        }
    }
}
