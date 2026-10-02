//! Automatic continuation after a staircase-edge TDR (user decisions 2026-10-01), opt-in per run.
//!
//! A supervisor thread waits [`AUTO_RESUME_DELAY_MS`] once a run becomes eligible (shown in the UI;
//! turning the option off or starting anything cancels it). Then:
//! - if this Windows boot still holds the TDR latch, the installed service resets only the GPU
//!   driver (experimental, once per incident): it stops cleanly at stock, restarts the NVIDIA
//!   display device, records the exact TDR the reset covers and exits non-zero so SCM recovery
//!   starts a fresh process without that latch. Anything else keeps the reboot requirement.
//! - otherwise it acknowledges only that run's own CandidateCrash and calls the same `resume` a
//!   user click calls, so every guard applies, including the stock controls every run starts with.
//!
//! One attempt per incident: a refusal is logged and the run waits for the user.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::DateTime;
use nidavellir_core::safe_loop::ForgeIncident;

use crate::gpu_power_sweep::auto_resume_eligible;
use crate::AppState;

/// Long enough for Windows and the driver to settle after a boot or a driver reset, short enough
/// that an overnight run loses little time.
pub(crate) const AUTO_RESUME_DELAY_MS: u64 = 120_000;
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

/// The incident an attempt belongs to (the run's own key once it is acknowledged), then the run.
fn eligible_keys(state: &Mutex<AppState>) -> Option<[String; 2]> {
    let guard = state.lock().ok()?;
    let progress = guard.power_sweep.progress();
    // Progress alone rules out almost every poll; the Safe Loop record is read only after that.
    if !auto_resume_eligible(&progress, None) {
        return None;
    }
    let pending = pending_incident(&guard).ok()?;
    let run = progress.run_id.clone().unwrap_or_default();
    auto_resume_eligible(&progress, pending.as_ref())
        .then(|| [pending.map_or_else(|| run.clone(), |incident| incident.id), run])
}

fn run(state: &Mutex<AppState>) {
    // Acting on an incident also spends the run's own key: a Resume refused right after the
    // acknowledgement gets no second countdown. A later incident of the run gets a new attempt.
    let mut attempted = std::collections::HashSet::new();
    while !crate::shutdown::is_requested() {
        std::thread::sleep(Duration::from_secs(5));
        let Some(keys) = eligible_keys(state) else { continue };
        if attempted.contains(&keys[0]) {
            continue;
        }
        if countdown(state) {
            attempted.extend(keys);
            act(state);
        }
    }
}

/// True when the delay ran out without the option being turned off or a run starting.
fn countdown(state: &Mutex<AppState>) -> bool {
    let deadline = now_ms() + AUTO_RESUME_DELAY_MS;
    {
        let Ok(guard) = state.lock() else { return false };
        guard.power_sweep.note_auto_resume(Some(deadline), format!(
            "Retomada automática em {} s: TDR na borda do degrau; o incidente desta run será reconhecido e a mesma run continua. Desligue a opção para cancelar.",
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
        if !progress.auto_resume || progress.running {
            if progress.auto_resume_at_ms.is_some() {
                guard.power_sweep.note_auto_resume(None, "Retomada automática cancelada.".into());
            }
            return false;
        }
    }
    true
}

fn act(state: &Mutex<AppState>) {
    let Ok(guard) = state.lock() else { return };
    let refuse = |reason: String| {
        guard.power_sweep.note_auto_resume(None, format!(
            "Retomada automática recusada: {reason}. A run aguarda ação manual."
        ));
    };
    let progress = guard.power_sweep.progress();
    let pending = match pending_incident(&guard) {
        Ok(pending) => pending,
        Err(error) => return refuse(format!("Safe Loop ilegível ({error})")),
    };
    if !auto_resume_eligible(&progress, pending.as_ref()) {
        return refuse("o estado da run mudou durante a espera".into());
    }
    if let Some(latch) = crate::tdr_sentinel::reboot_required_event() {
        let line = match pending.as_ref().ok_or_else(|| "o incidente já foi reconhecido".to_string())
            .and_then(|incident| begin_driver_reset(incident, &latch))
        {
            Ok(()) => "Reset só do driver da GPU: o serviço para com a GPU em stock, reinicia o adaptador NVIDIA e volta sozinho; a run continua sem reiniciar o Windows.".into(),
            Err(reason) => format!(
                "Este boot ainda guarda o TDR {latch} e o reset só do driver não está disponível ({reason}). Reinicie o Windows: com o serviço instalado a run continua sozinha depois do boot."
            ),
        };
        return guard.power_sweep.note_auto_resume(None, line);
    }
    if let Some(incident) = pending.as_ref() {
        if let Err(error) =
            crate::safe_loop_runtime::acknowledge_forge_incident(&guard.safe_store, Some(&incident.id))
        {
            return refuse(error);
        }
    }
    guard.power_sweep.refresh_resume_state(&guard.safe_store);
    guard.power_sweep.note_auto_resume(None, format!(
        "Retomando automaticamente{}.",
        if pending.is_some() { "; o CandidateCrash desta run foi reconhecido pela opção de retomada automática" } else { "" }
    ));
    if let Err(error) = guard.power_sweep.resume(guard.safe_store.clone()) {
        refuse(error);
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
