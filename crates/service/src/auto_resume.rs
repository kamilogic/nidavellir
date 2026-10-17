//! Automatic continuation after a staircase-edge TDR (user decisions 2026-10-01; on by default since
//! 2026-10-12).
//!
//! A supervisor thread waits [`AUTO_RESUME_DELAY_MS`] once a run becomes eligible (shown in the UI;
//! a cancel, turning the option off or starting anything stops it). Then:
//! - if this Windows boot still holds the TDR latch, the installed service resets only the GPU
//!   driver (experimental, once per incident): it stops cleanly at stock, restarts the NVIDIA
//!   display device, records the exact TDR the reset covers and exits non-zero so SCM recovery
//!   starts a fresh process without that latch. When that reset is not possible, the installed
//!   service warns on screen for [`AUTO_RESTART_DELAY_MS`] and restarts Windows (user decision
//!   2026-10-13, once per incident); the service starts with Windows and continues the run.
//!   Anything else keeps the reboot requirement.
//! - otherwise it acknowledges only that run's own CandidateCrash and calls the same `resume` a
//!   user click calls, so every guard applies, including the stock controls every run starts with.
//!
//! One attempt per incident, kept across Core restarts: a refusal is logged and the run waits for
//! the user.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::DateTime;
use nidavellir_core::ipc::LogTone;
use nidavellir_core::safe_loop::ForgeIncident;

use crate::gpu_power_sweep::auto_resume_eligible;
use crate::AppState;

/// Long enough for Windows and the driver to settle after a boot or a driver reset, short enough
/// that an overnight run loses little time.
pub(crate) const AUTO_RESUME_DELAY_MS: u64 = 120_000;
/// The on-screen warning before Nidavellir restarts Windows: time to save work or cancel.
const AUTO_RESTART_DELAY_MS: u64 = 120_000;
/// Residual nvlddmkm-153 events of one episode land within seconds of the incident; a TDR further
/// away is another failure and keeps the reboot requirement.
const SAME_EPISODE_MS: i64 = 60_000;
const DEVICE_READY_TIMEOUT: Duration = Duration::from_secs(30);

pub fn spawn(state: std::sync::Arc<Mutex<AppState>>) {
    std::thread::spawn(move || run(&state));
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis() as u64)
}

fn pending_incident(state: &AppState) -> Result<Option<ForgeIncident>, String> {
    state.safe_store.load_record_checked().map(|record| record.pending_forge_incident).map_err(|e| e.to_string())
}

/// Keys this process already acted on. Shared with the program session: the Core outlives the
/// program only while a resume can still happen.
static ATTEMPTED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Bumped by every user cancel; a countdown or restart warning begun under an older value stops.
static CANCELS: AtomicU64 = AtomicU64::new(0);
/// A Windows restart warning is running: the Core stays up for it, with the program or without.
static RESTART_PENDING: AtomicBool = AtomicBool::new(false);
/// Acknowledge+resume attempts are also saved, because the Core stops when the program closes. A
/// driver-only reset is not: the restarted process must continue that same incident, and
/// `gpu_driver_reset.json` already limits the reset to one.
const SAVED_ATTEMPTS: usize = 64;

fn attempts_path() -> std::path::PathBuf {
    nidavellir_core::safe_loop::default_data_dir().join("auto_resume_attempts.json")
}

/// A missing or unreadable file means no saved attempt: the resume guards still apply.
fn saved_attempts(path: &std::path::Path) -> Vec<String> {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn save_attempt(path: &std::path::Path, keys: &[String; 2]) -> Result<(), String> {
    let mut saved = saved_attempts(path);
    saved.extend(keys.iter().cloned());
    let excess = saved.len().saturating_sub(SAVED_ATTEMPTS);
    saved.drain(..excess);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, serde_json::to_vec(&saved).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn attempted(key: &str) -> bool {
    ATTEMPTED.lock().map_or(true, |keys| keys.iter().any(|attempted| attempted == key))
        || saved_attempts(&attempts_path()).iter().any(|attempted| attempted == key)
}

/// The incident an attempt belongs to (the run's own key once it is acknowledged), then the run.
fn eligible_keys(state: &AppState) -> Option<[String; 2]> {
    let progress = state.power_sweep.progress();
    // Progress alone rules out almost every poll; the Safe Loop record is read only after that.
    if !auto_resume_eligible(&progress, None) {
        return None;
    }
    let pending = pending_incident(state).ok()?;
    let run = progress.run_id.clone().unwrap_or_default();
    auto_resume_eligible(&progress, pending.as_ref())
        .then(|| [pending.map_or_else(|| run.clone(), |incident| incident.id), run])
}

/// True while this run will still continue on its own (countdown, next poll or restart warning).
pub(crate) fn needs_core(state: &AppState) -> bool {
    RESTART_PENDING.load(Ordering::SeqCst) || eligible_keys(state).is_some_and(|keys| !attempted(&keys[0]))
}

/// The user cancels this run's automatic continuation: a countdown or restart warning stops and
/// the run waits for them. The saved auto-continue choice stays (2026-10-13: turning the option
/// off was the only cancel, and it then stayed off for every later run).
pub(crate) fn cancel(state: &AppState) {
    let keys = eligible_keys(state);
    if let (Some(keys), Ok(mut attempted)) = (&keys, ATTEMPTED.lock()) {
        attempted.extend(keys.iter().cloned());
    }
    CANCELS.fetch_add(1, Ordering::SeqCst);
    if let Some(keys) = keys {
        if let Err(error) = save_attempt(&attempts_path(), &keys) {
            tracing::warn!("auto-resume cancel not saved ({error}); a restarted Core may try once more");
        }
    }
}

fn run(state: &Mutex<AppState>) {
    // Acting on an incident also spends the run's own key: a Resume refused right after the
    // acknowledgement gets no second countdown. A later incident of the run gets a new attempt.
    while !crate::shutdown::is_requested() {
        std::thread::sleep(Duration::from_secs(5));
        // Read before the keys: a cancel from here on stops whatever follows.
        let cancels = CANCELS.load(Ordering::SeqCst);
        let Some(keys) = state.lock().ok().and_then(|guard| eligible_keys(&guard)) else { continue };
        if attempted(&keys[0]) {
            continue;
        }
        if countdown(state, cancels) {
            if let Some(reason) = act(state, &keys) {
                restart_after_warning(state, &keys, &reason, cancels);
            }
        }
    }
}

/// True when the delay ran out without a cancel, the option being turned off or a run starting.
fn countdown(state: &Mutex<AppState>, cancels: u64) -> bool {
    let deadline = now_ms() + AUTO_RESUME_DELAY_MS;
    {
        let Ok(guard) = state.lock() else { return false };
        guard.power_sweep.note_auto_resume(Some(deadline), LogTone::Attention, format!(
            "Driver crash at a test edge · the same run continues in {} s. Turn off auto-continue to cancel.",
            AUTO_RESUME_DELAY_MS / 1000
        ));
    }
    while now_ms() < deadline {
        if crate::shutdown::is_requested() {
            return false;
        }
        std::thread::sleep(Duration::from_secs(1));
        let Ok(guard) = state.lock() else { return false };
        let progress = guard.power_sweep.progress();
        if !progress.auto_resume || progress.running || CANCELS.load(Ordering::SeqCst) != cancels {
            if progress.auto_resume_at_ms.is_some() {
                guard.power_sweep.note_auto_resume(None, LogTone::Step, "Automatic continuation cancelled.".into());
            }
            return false;
        }
    }
    true
}

/// Some(reason) when Windows must restart next: the boot still holds the TDR latch and the
/// driver-only reset is not possible for `reason`.
fn act(state: &Mutex<AppState>, keys: &[String; 2]) -> Option<String> {
    let Ok(guard) = state.lock() else { return None };
    // Under the state lock, so `needs_core` never sees the attempt before its outcome.
    if let Ok(mut attempted) = ATTEMPTED.lock() {
        attempted.extend(keys.iter().cloned());
    }
    let refuse = |reason: String| {
        guard.power_sweep.note_auto_resume(None, LogTone::Attention, format!(
            "Automatic continuation refused: {reason}. The run waits for you."
        ));
        None
    };
    let progress = guard.power_sweep.progress();
    let pending = match pending_incident(&guard) {
        Ok(pending) => pending,
        Err(error) => return refuse(format!("the Safe Loop state could not be read ({error})")),
    };
    if !auto_resume_eligible(&progress, pending.as_ref()) {
        return refuse("the run changed during the wait".into());
    }
    if let Some(latch) = crate::tdr_sentinel::reboot_required_event() {
        // A crash at factory settings gets a full Windows restart; the driver-only reset is for
        // test edges (user decision 2026-10-14).
        let reset = if progress.stock_crash_paused {
            Err("a driver crash during the stock checks needs a full Windows restart".to_string())
        } else {
            pending.as_ref().ok_or_else(|| "the incident was already acknowledged".to_string())
                .and_then(|incident| begin_driver_reset(incident, &latch))
        };
        let reason = match reset {
            Ok(()) => {
                guard.power_sweep.note_auto_resume(None, LogTone::Attention, "Restarting only the GPU driver: the service stops with the GPU at stock, restarts the NVIDIA adapter and comes back by itself; the run continues without a Windows restart.".into());
                return None;
            }
            Err(reason) => reason,
        };
        let restarted = restart_issued(&restart_marker_path(), &keys[0]);
        if restart_allowed(crate::service_impl::under_scm(), restarted) {
            // Set under the state lock, so the Core never looks unneeded before the warning starts.
            RESTART_PENDING.store(true, Ordering::SeqCst);
            return Some(reason);
        }
        let line = if restarted {
            format!("Windows was already restarted once for this driver crash, and this boot still needs a restart ({latch}). The run waits for you.")
        } else {
            format!("This boot still holds driver crash {latch} and a driver-only restart is not available ({reason}). Restart Windows: with the installed service the run continues by itself after the restart.")
        };
        guard.power_sweep.note_auto_resume(None, LogTone::Attention, line);
        return None;
    }
    // Saved before acting, so a Core the program restarts never tries this incident again.
    if let Err(error) = save_attempt(&attempts_path(), keys) {
        tracing::warn!("auto-resume attempt not saved ({error}); a restarted Core may try it once more");
    }
    if let Some(incident) = pending.as_ref() {
        if let Err(error) =
            crate::safe_loop_runtime::acknowledge_forge_incident(&guard.safe_store, Some(&incident.id))
        {
            return refuse(error);
        }
    }
    guard.power_sweep.refresh_resume_state(&guard.safe_store);
    guard.power_sweep.note_auto_resume(None, LogTone::Step, format!(
        "Continuing the run automatically{}.",
        if pending.is_some() { "; auto-continue acknowledged this run's driver crash" } else { "" }
    ));
    if let Err(error) = guard.power_sweep.resume(guard.safe_store.clone()) {
        return refuse(error);
    }
    None
}

/// The boot still needs a restart and the driver-only reset could not give one: warn on screen for
/// [`AUTO_RESTART_DELAY_MS`], then restart Windows unless the user cancels (user decision
/// 2026-10-13). The installed service starts with Windows and continues the run.
fn restart_after_warning(state: &Mutex<AppState>, keys: &[String; 2], reason: &str, cancels: u64) {
    let deadline = now_ms() + AUTO_RESTART_DELAY_MS;
    let warned = state
        .lock()
        .map(|guard| {
            guard.power_sweep.set_auto_restart_at(Some(deadline));
            guard.power_sweep.note_auto_resume(None, LogTone::Attention, format!(
                "This driver crash needs a Windows restart (a driver-only restart is not available: {reason}). Windows restarts in {} s and the run continues after it. Cancel to keep working; the run then waits for you.",
                AUTO_RESTART_DELAY_MS / 1000
            ));
        })
        .is_ok();
    // No warning on screen, no restart.
    let restarting = warned && {
        let outcome = restart_when_due(state, &keys[0], deadline, cancels);
        if let Ok(guard) = state.lock() {
            guard.power_sweep.set_auto_restart_at(None);
            let (tone, line) = match &outcome {
                Ok(()) => (LogTone::Step, "Restarting Windows now. The run continues after the restart.".to_string()),
                Err(line) => (LogTone::Attention, line.clone()),
            };
            guard.power_sweep.note_auto_resume(None, tone, line);
        }
        outcome.is_ok()
    };
    // A restart under way keeps the Core until Windows stops it.
    if !restarting {
        RESTART_PENDING.store(false, Ordering::SeqCst);
    }
}

/// A refused driver reset leads to a Windows restart only in the installed service, and once per
/// key: in console mode no Core starts with Windows, and a second restart could loop.
fn restart_allowed(under_scm: bool, restarted_before: bool) -> bool {
    under_scm && !restarted_before
}

/// What the warning does on each tick.
#[derive(Debug, PartialEq)]
enum WarningStep {
    Wait,
    Restart,
    Stop(&'static str),
}

/// A cancel, auto-continue turned off or a run starting always wins, even at the deadline.
fn warning_step(cancelled: bool, auto_resume: bool, running: bool, due: bool) -> WarningStep {
    if cancelled {
        WarningStep::Stop("Windows restart cancelled. The run waits for you: restart Windows when you are ready, then continue it from the Forge screen.")
    } else if !auto_resume || running {
        WarningStep::Stop("Windows restart cancelled. The run waits for you.")
    } else if due {
        WarningStep::Restart
    } else {
        WarningStep::Wait
    }
}

/// Restarts Windows when the warning runs out with the run still waiting for it; Err carries the
/// line that says why Windows was not restarted.
fn restart_when_due(state: &Mutex<AppState>, key: &str, deadline: u64, cancels: u64) -> Result<(), String> {
    loop {
        if crate::shutdown::is_requested() {
            return Err("The Core stopped during the warning; Windows was not restarted.".into());
        }
        std::thread::sleep(Duration::from_secs(1));
        let guard = state.lock().map_err(|_| "Windows was not restarted: the Core state is unavailable.".to_string())?;
        let progress = guard.power_sweep.progress();
        match warning_step(
            CANCELS.load(Ordering::SeqCst) != cancels,
            progress.auto_resume,
            progress.running,
            now_ms() >= deadline,
        ) {
            WarningStep::Wait => continue,
            WarningStep::Stop(line) => return Err(line.into()),
            WarningStep::Restart => {}
        }
        let pending = pending_incident(&guard)
            .map_err(|error| format!("Windows was not restarted: the Safe Loop state could not be read ({error})."))?;
        if !auto_resume_eligible(&progress, pending.as_ref()) {
            return Err("Windows was not restarted: the run changed during the warning.".into());
        }
        // The boot after the restart refuses Resume while the flag is armed (safety audit 2026-10-13).
        if guard.safe_store.is_boot_flag_armed() {
            return Err("Windows was not restarted: the Safe Loop boot flag is still armed, so the run could not continue after it. The run waits for you.".into());
        }
        // Still under the state lock: nothing starts between these checks and the restart.
        return issue_restart(key);
    }
}

/// The record comes first, so a boot that still needs a restart waits for the user instead of
/// restarting again.
fn issue_restart(key: &str) -> Result<(), String> {
    save_restart_marker(&restart_marker_path(), key).map_err(|error| {
        format!("Windows was not restarted: the restart record could not be saved ({error}). Restart Windows yourself; the run continues after it.")
    })?;
    restart_windows()
        .map_err(|error| format!("Windows could not be restarted ({error}). Restart Windows yourself; the run continues after it."))
}

fn restart_marker_path() -> std::path::PathBuf {
    nidavellir_core::safe_loop::default_data_dir().join("forge_restart.json")
}

/// Windows was already restarted for this key (the incident, or the run once acknowledged). An
/// unreadable record counts as restarted, so a damaged file never loops restarts.
fn restart_issued(path: &std::path::Path, key: &str) -> bool {
    match std::fs::read(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
        Ok(bytes) => serde_json::from_slice::<String>(&bytes).map_or(true, |saved| saved == key),
    }
}

fn save_restart_marker(path: &std::path::Path, key: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, serde_json::to_vec(key).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Windows' own restart, by absolute path (the service runs as LocalSystem). `/f` closes open
/// programs without saving, as the warning said.
fn restart_windows() -> Result<(), String> {
    let root = std::env::var_os("SystemRoot").ok_or("SystemRoot is not set")?;
    let status = std::process::Command::new(std::path::Path::new(&root).join(r"System32\shutdown.exe"))
        .args(["/r", "/f", "/t", "0", "/d", "p:4:1", "/c", "Nidavellir restarts Windows to continue the Forge after a GPU driver crash."])
        .status()
        .map_err(|e| format!("shutdown.exe: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("shutdown.exe exited with {:?}", status.code()))
    }
}

/// Durable record of the one driver reset allowed per incident. `covers_tdr` is written only after
/// the adapter came back; it is the only thing that lets a restart skip the reboot latch.
#[derive(serde::Serialize, serde::Deserialize)]
struct DriverReset {
    incident_id: String,
    detected_at: String,
    covers_tdr: Option<String>,
    error: Option<String>,
}

fn marker_path() -> std::path::PathBuf {
    nidavellir_core::safe_loop::default_data_dir().join("gpu_driver_reset.json")
}

/// Ok(None) only when no attempt was ever recorded; an unreadable record counts as an attempt.
fn load_marker() -> Result<Option<DriverReset>, String> {
    match std::fs::read_to_string(marker_path()) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("gpu_driver_reset.json ilegível: {error}")),
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| format!("gpu_driver_reset.json inválido: {e}")),
    }
}

fn save_marker(marker: &DriverReset) -> Result<(), String> {
    std::fs::create_dir_all(nidavellir_core::safe_loop::default_data_dir()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string(marker).map_err(|e| e.to_string())?;
    std::fs::write(marker_path(), json).map_err(|e| format!("gpu_driver_reset.json não pôde ser gravado: {e}"))
}

/// Startup half of the latch: a completed reset that covers exactly the newest TDR replaces the reboot.
pub(crate) fn driver_reset_covers(tdr: &str) -> bool {
    load_marker().ok().flatten().and_then(|marker| marker.covers_tdr).as_deref() == Some(tdr)
}

fn same_episode(detected_at: &str, tdr: &str) -> bool {
    match (DateTime::parse_from_rfc3339(detected_at), DateTime::parse_from_rfc3339(tdr)) {
        (Ok(detected), Ok(tdr)) => (tdr - detected).num_milliseconds().abs() <= SAME_EPISODE_MS,
        _ => false,
    }
}

/// Everything that can refuse is checked while the service still runs.
fn begin_driver_reset(incident: &ForgeIncident, latch: &str) -> Result<(), String> {
    if DateTime::parse_from_rfc3339(latch).is_err() {
        return Err(format!("o bloqueio não veio de um TDR do log ({latch})"));
    }
    crate::service_impl::driver_reset_available()?;
    if let Some(marker) = load_marker()?.filter(|marker| marker.incident_id == incident.id) {
        return Err(match marker.error {
            Some(error) => format!("o reset deste incidente já falhou: {error}"),
            None => "o reset deste incidente já foi tentado".into(),
        });
    }
    save_marker(&DriverReset {
        incident_id: incident.id.clone(),
        detected_at: incident.detected_at.clone(),
        covers_tdr: None,
        error: None,
    })?;
    crate::service_impl::request_driver_reset()
}

/// Service half, after the stop: restart the adapter only when the stop was clean (stock confirmed,
/// NVAPI released). Any failure is recorded and keeps the reboot requirement.
pub(crate) fn finish_driver_reset(clean_stop: &Result<(), String>) {
    let Ok(Some(mut marker)) = load_marker() else {
        tracing::warn!("driver reset skipped: attempt record missing or unreadable");
        return;
    };
    let outcome = clean_stop
        .as_ref()
        .map_err(|error| format!("parada limpa não confirmada: {error}"))
        .and_then(|()| covered_tdr_after_restart(&marker.detected_at));
    match outcome {
        Ok(tdr) => {
            tracing::info!("GPU driver reset completed; it covers TDR {tdr}");
            marker.covers_tdr = Some(tdr);
        }
        Err(error) => {
            tracing::warn!("GPU driver reset failed; reboot still required: {error}");
            marker.error = Some(error);
        }
    }
    if let Err(error) = save_marker(&marker) {
        tracing::warn!("{error}");
    }
}

fn covered_tdr_after_restart(detected_at: &str) -> Result<String, String> {
    let tdr = crate::tdr_sentinel::query_latest_tdr_event_checked()?
        .ok_or("nenhum TDR no log do Windows")?;
    if !same_episode(detected_at, &tdr) {
        return Err(format!("o TDR mais recente ({tdr}) não pertence a este incidente"));
    }
    restart_device(&single_instance_id(&powershell(
        "Get-PnpDevice -Class Display -PresentOnly | Where-Object { $_.InstanceId -like 'PCI\\VEN_10DE*' } | ForEach-Object { $_.InstanceId }",
    )?)?)?;
    Ok(tdr)
}

fn powershell(script: &str) -> Result<String, String> {
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &format!(
            "[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); {script}"
        )])
        .output()
        .map_err(|e| format!("powershell: {e}"))?;
    if !out.status.success() {
        return Err(format!("powershell falhou: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn single_instance_id(output: &str) -> Result<String, String> {
    let ids: Vec<&str> = output.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    match ids.as_slice() {
        [id] if !id.contains('\'') => Ok(id.to_string()),
        _ => Err(format!("{} adaptador(es) NVIDIA presentes; o reset exige exatamente um", ids.len())),
    }
}

/// Windows' own PnP restart of the adapter (driver unload and reload, like a driver update).
fn restart_device(instance_id: &str) -> Result<(), String> {
    let mut child = std::process::Command::new("pnputil")
        .args(["/restart-device", instance_id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("pnputil: {e}"))?;
    // A hung restart must not leave the service in StopPending; the reboot fallback still holds.
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| format!("pnputil: {e}"))? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            return Err("pnputil /restart-device não terminou em 60 s".into());
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    if !status.success() {
        // 3010 means Windows itself wants a reboot to finish.
        return Err(format!("pnputil /restart-device saiu com {:?}", status.code()));
    }
    let deadline = Instant::now() + DEVICE_READY_TIMEOUT;
    loop {
        if powershell(&format!("(Get-PnpDevice -InstanceId '{instance_id}').Status"))? == "OK" {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("o adaptador não voltou ao estado OK em 30 s".into());
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_attempt_survives_a_restart_and_the_file_stays_bounded() {
        let dir = std::env::temp_dir().join(format!("nidavellir-attempts-{}", std::process::id()));
        let path = dir.join("auto_resume_attempts.json");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(saved_attempts(&path).is_empty(), "no file: nothing attempted");
        save_attempt(&path, &["incident-a".into(), "run-1".into()]).unwrap();
        assert_eq!(saved_attempts(&path), ["incident-a", "run-1"]);
        for n in 0..SAVED_ATTEMPTS {
            save_attempt(&path, &[format!("incident-{n}"), "run-1".into()]).unwrap();
        }
        let saved = saved_attempts(&path);
        assert_eq!(saved.len(), SAVED_ATTEMPTS);
        assert!(!saved.contains(&"incident-a".to_string()), "oldest keys go first");
        std::fs::write(&path, "not json").unwrap();
        assert!(saved_attempts(&path).is_empty(), "a damaged file never blocks every future run");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_restart_warning_yields_to_any_cancel_and_restarts_only_the_installed_service_once() {
        assert_eq!(warning_step(false, true, false, false), WarningStep::Wait);
        assert_eq!(warning_step(false, true, false, true), WarningStep::Restart);
        assert!(matches!(warning_step(true, true, false, true), WarningStep::Stop(_)), "a cancel wins even at the deadline");
        assert!(matches!(warning_step(false, false, false, true), WarningStep::Stop(_)), "auto-continue turned off");
        assert!(matches!(warning_step(false, true, true, true), WarningStep::Stop(_)), "a run started");
        assert!(restart_allowed(true, false));
        assert!(!restart_allowed(false, false), "console mode never restarts Windows");
        assert!(!restart_allowed(true, true), "one automatic restart per incident");
    }

    #[test]
    fn windows_restarts_once_per_incident_and_a_damaged_record_never_restarts_it() {
        let dir = std::env::temp_dir().join(format!("nidavellir-restart-{}", std::process::id()));
        let path = dir.join("forge_restart.json");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!restart_issued(&path, "incident-a"), "no record: the first restart may happen");
        save_restart_marker(&path, "incident-a").unwrap();
        assert!(restart_issued(&path, "incident-a"), "the same incident never restarts Windows twice");
        assert!(!restart_issued(&path, "incident-b"), "a later incident may restart once");
        std::fs::write(&path, "not json").unwrap();
        assert!(restart_issued(&path, "incident-b"), "a damaged record never loops restarts");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn driver_reset_covers_only_one_tdr_episode_on_exactly_one_nvidia_adapter() {
        // wevtutil SystemTime (7 fractional digits) against chrono's incident timestamp.
        let detected = "2026-10-01T03:12:40.123456789+00:00";
        assert!(same_episode(detected, "2026-10-01T03:12:38.1234567Z"));
        assert!(same_episode(detected, "2026-10-01T03:13:35.0000000Z"));
        assert!(!same_episode(detected, "2026-10-01T03:14:00.0000000Z"));
        assert!(!same_episode(detected, "2026-10-01T02:50:00.0000000Z"));
        assert!(!same_episode(detected, "event-log-query-error:tdr"));

        let id = r"PCI\VEN_10DE&DEV_2489&SUBSYS_88231043&REV_A1\4&1D3A2B6C&0&0008";
        assert_eq!(single_instance_id(&format!("\r\n{id}\r\n")).unwrap(), id);
        assert!(single_instance_id("").is_err());
        assert!(single_instance_id(&format!("{id}\n{id}")).is_err());
        assert!(single_instance_id("PCI\\VEN_10DE'; Remove-Item x").is_err());
    }
}
