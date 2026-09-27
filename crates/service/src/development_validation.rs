//! Explicit, single-run development authorization. Historical exclusions are never edited.
//! Permission exists only in this console process; the append-only audit cannot restore it.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use nidavellir_core::condemnation::{CondemnationEvent, CondemnationLedger};
use nidavellir_core::ipc::IpcRequest;
use nidavellir_core::safe_loop::{SafeLoopRecord, SafeLoopState, SafeLoopStore};
use serde_json::json;

static ENABLED: AtomicBool = AtomicBool::new(false);
static AUTHORIZATION: Mutex<Option<Authorization>> = Mutex::new(None);

#[derive(Debug, PartialEq)]
enum Phase {
    Ready,
    Running,
    Finished,
}

struct Authorization {
    gpu_key: String,
    crashes: Vec<CondemnationEvent>,
    phase: Phase,
    audit_path: PathBuf,
    diagnostic_pair: Option<(u32, u32)>,
}

impl Authorization {
    fn check_pair(&self, pair: (u32, u32)) -> Result<(), String> {
        if self.diagnostic_pair.is_some_and(|allowed| allowed != pair) {
            return Err("Point diagnostic authorization does not cover this V/F pair".into());
        }
        Ok(())
    }

    fn budget(&self, crashes: &[CondemnationEvent], gpu_key: &str) -> Result<(), String> {
        if self.gpu_key != gpu_key {
            return Err("Development validation belongs to another GPU".into());
        }
        if self.phase == Phase::Finished {
            return Err("Development validation ended. Export the report for review before authorizing another run. Full Reset does not renew this authorization.".into());
        }
        // Comparing the actual evidence, not just its count, also refuses removed/rewritten events.
        if self.crashes != crashes {
            return Err("Development validation stopped: new or changed crash evidence. Keep the GPU at stock and review the report; no automatic retry is allowed.".into());
        }
        Ok(())
    }

    fn claim(&mut self) -> Result<(), String> {
        if self.phase != Phase::Ready {
            return Err("Development authorization has already been used; a second Start or Resume is not allowed".into());
        }
        // Consume before I/O. A failed audit write never leaves reusable permission.
        self.phase = Phase::Finished;
        append_audit(&self.audit_path, json!({"event":"claimed", "at":now()}))?;
        self.phase = Phase::Running;
        Ok(())
    }

    fn check_and_latch(
        &mut self,
        crashes: &[CondemnationEvent],
        gpu_key: &str,
    ) -> Result<(), String> {
        let result = self.budget(crashes, gpu_key);
        if let Err(error) = &result {
            if let Err(audit_error) = self.finish(error, None) {
                tracing::error!("{audit_error}; development permission remains consumed");
            }
        }
        result
    }

    fn finish(&mut self, reason: &str, run_id: Option<&str>) -> Result<(), String> {
        if self.phase == Phase::Finished {
            return Ok(());
        }
        self.phase = Phase::Finished;
        append_audit(
            &self.audit_path,
            json!({"event":"finished", "at":now(), "reason":reason, "run_id":run_id}),
        )
    }
}

fn now() -> String {
    nidavellir_core::f2_observation::now_rfc3339()
}

pub(crate) fn enable() {
    ENABLED.store(true, Ordering::SeqCst);
}
pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// None means ordinary production policy. Development mode always requires explicit authorization.
pub(crate) fn budget_override(
    crashes: &[CondemnationEvent],
    gpu_key: &str,
) -> Option<Result<(), String>> {
    if !enabled() {
        return None;
    }
    Some(AUTHORIZATION.lock().map_err(|_| "Development authorization lock is poisoned".to_string())
        .and_then(|mut authorization| authorization.as_mut()
            .ok_or_else(|| "Development validation requires explicit authorization. Run scripts/authorize-validation.ps1 after reviewing the incident report; Full Reset does not authorize a run.".to_string())?
            .check_and_latch(crashes, gpu_key)))
}

/// This opt-in console session supports only one fresh Standard Forge. Other tuning entry points
/// cannot borrow its budget exception, including manual diagnostics and profile Apply.
pub(crate) fn allows_tuning_request(request: &IpcRequest) -> bool {
    if AUTHORIZATION.lock().map_or(true, |a| a.as_ref().is_some_and(|a| a.diagnostic_pair.is_some())) {
        return false;
    }
    matches!(
        request,
        IpcRequest::AuthorizeDevelopmentValidation { .. }
            | IpcRequest::StartPowerSweep
            | IpcRequest::StartPowerSweepClean
            | IpcRequest::StartPowerSweepFast
    )
}

pub(crate) fn status_note() -> Option<String> {
    if !enabled() {
        return None;
    }
    let authorization = AUTHORIZATION.lock().ok()?;
    Some(match authorization.as_ref().map(|a| &a.phase) {
        None => "Development mode: review the incident report, then explicitly authorize one Standard validation from the command line.",
        Some(Phase::Ready) => "One development validation is authorized. Existing unsafe points remain excluded. Start Standard manually; the first new crash ends this authorization.",
        Some(Phase::Running) => "Single development validation in progress. Keep this build fixed; stopping the run consumes the authorization.",
        Some(Phase::Finished) => "Development validation ended. Export the report for review. No further run, Resume or profile Apply is authorized in this service session.",
    }.into())
}

pub(crate) fn audit_path() -> Option<PathBuf> {
    AUTHORIZATION
        .lock()
        .ok()?
        .as_ref()
        .map(|a| a.audit_path.clone())
}

fn require_ready(
    record: &SafeLoopRecord,
    boot_flag: bool,
    applied: bool,
    checkpoint: bool,
) -> Result<(), String> {
    if record.state != SafeLoopState::Idle
        || record.safe_mode
        || record.pending_forge_incident.is_some()
        || record.last_validated.is_some()
        || boot_flag
        || applied
    {
        return Err("Development authorization requires idle Safe Loop, confirmed stock, no applied profile and no pending recovery. Resolve recovery first.".into());
    }
    if checkpoint {
        return Err("Review/export the saved run, then Full Reset its measurements before authorizing a fresh development validation.".into());
    }
    Ok(())
}

fn append_audit(path: &Path, entry: serde_json::Value) -> Result<(), String> {
    let mut expected = fs::read(path).map_err(|e| format!("Read validation audit: {e}"))?;
    let mut line = serde_json::to_vec(&entry).map_err(|e| e.to_string())?;
    line.push(b'\n');
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|e| format!("Open validation audit: {e}"))?;
    file.write_all(&line)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("Persist validation audit: {e}"))?;
    expected.extend(line);
    if fs::read(path).map_err(|e| e.to_string())? != expected {
        return Err("Validation audit readback mismatch".into());
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn authorize(store: &SafeLoopStore, reason: &str) -> Result<(), String> {
    authorize_scope(store, reason, None)
}

/// CLI-only, one explicitly selected point; cannot grant Start/Resume/Apply over IPC.
#[cfg(windows)]
pub(crate) fn authorize_point_diagnostic(store: &SafeLoopStore, reason: &str, pair: (u32, u32)) -> Result<(), String> {
    authorize_scope(store, reason, Some(pair))
}

pub(crate) fn check_diagnostic_pair(target_mhz: u32, anchor_mv: u32) -> Result<(), String> {
    let slot = AUTHORIZATION.lock().map_err(|_| "Development authorization lock is poisoned".to_string())?;
    if let Some(authorization) = slot.as_ref() {
        authorization.check_pair((target_mhz, anchor_mv))?;
    }
    Ok(())
}

#[cfg(windows)]
fn authorize_scope(store: &SafeLoopStore, reason: &str, diagnostic_pair: Option<(u32, u32)>) -> Result<(), String> {
    if !enabled() {
        return Err("Development authorization is disabled. Start the console service with --development-validation; installed services never enable it.".into());
    }
    let reason = reason.trim();
    if !(8..=500).contains(&reason.len()) {
        return Err("Provide a review reason between 8 and 500 bytes".into());
    }
    let mut slot = AUTHORIZATION
        .lock()
        .map_err(|_| "Development authorization lock is poisoned".to_string())?;
    if slot.is_some() {
        return Err("This service session already has an authorization. Review its report before a new explicit session.".into());
    }
    let record = store
        .load_record_checked()
        .map_err(|e| format!("Read Safe Loop: {e}"))?;
    let boot_flag = store
        .read_boot_flag_checked()
        .map_err(|e| format!("Read boot flag: {e}"))?
        .is_some();
    require_ready(
        &record,
        boot_flag,
        crate::gpu_apply::load_applied_checked()?.is_some(),
        store
            .base_dir()
            .join("forge_state.json")
            .try_exists()
            .map_err(|e| e.to_string())?,
    )?;
    let events = CondemnationLedger::new(store.base_dir())
        .load_all_checked()
        .map_err(|e| format!("Read safety history: {e}"))?;
    let compatibility = crate::gpu_power_sweep::current_forge_resume_compatibility()?;
    let crashes =
        crate::gpu_power_sweep::f2_effective_candidate_crashes(&events, &compatibility.gpu_key);
    if crashes.len() <= 2 && diagnostic_pair.is_none() {
        return Err("No exhausted crash budget requires development authorization; use the ordinary console mode.".into());
    }
    // Hardware-only checked reset: no ACK, ledger rewrite, observation deletion or workload.
    crate::gpu_power_sweep::reset_to_stock_checked()?;
    let directory = store.base_dir().join("development-validations");
    fs::create_dir_all(&directory)
        .map_err(|e| format!("Create validation audit directory: {e}"))?;
    let id = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    );
    let audit_path = directory.join(format!("{id}.jsonl"));
    let entry = json!({"event":"authorized", "at":now(), "reason":reason, "compatibility":compatibility,
        "crash_allowance":1, "scope":if diagnostic_pair.is_some() { "one CLI point diagnostic; no search, Resume or profile Apply" } else { "one fresh Standard run in this console process; no Resume or Apply" },
        "diagnostic_pair":diagnostic_pair,
        "baseline_crashes":crashes, "safe_loop_snapshot":record, "condemnation_snapshot":events});
    let mut bytes = serde_json::to_vec(&entry).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&audit_path)
        .map_err(|e| format!("Create validation audit: {e}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("Persist validation authorization: {e}"))?;
    if fs::read(&audit_path).map_err(|e| e.to_string())? != bytes {
        return Err("Authorization readback mismatch".into());
    }
    tracing::warn!(
        "One development validation authorized; historical exclusions preserved. Audit: {}",
        audit_path.display()
    );
    *slot = Some(Authorization {
        gpu_key: compatibility.gpu_key,
        crashes,
        phase: Phase::Ready,
        audit_path,
        diagnostic_pair,
    });
    Ok(())
}

pub(crate) fn claim() -> Result<(), String> {
    if !enabled() {
        return Ok(());
    }
    AUTHORIZATION
        .lock()
        .map_err(|_| "Development authorization lock is poisoned".to_string())?
        .as_mut()
        .ok_or_else(|| "Development validation is not authorized".to_string())?
        .claim()
}

pub(crate) fn finish(reason: &str, run_id: Option<&str>) {
    if !enabled() {
        return;
    }
    if let Ok(mut slot) = AUTHORIZATION.lock() {
        if let Some(authorization) = slot.as_mut() {
            if let Err(error) = authorization.finish(reason, run_id) {
                tracing::error!("{error}; development permission remains consumed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nidavellir_core::condemnation::CondemnationSeverity;

    fn crash(mv: u32) -> CondemnationEvent {
        CondemnationEvent {
            timestamp: format!("t-{mv}"),
            gpu_key: Some("gpu-a".into()),
            severity: CondemnationSeverity::Rigid,
            kind: "candidate-crash".into(),
            target_mhz: 1920,
            vf_bin_mv: mv,
            run_id: Some(format!("run-{mv}")),
            qualification_contract_version: Some(29),
            note: Some("TDR".into()),
            rehabilitated: false,
        }
    }
    fn authorization() -> Authorization {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "nidavellir-validation-test-{}-{}-{}.jsonl",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, "{\"event\":\"authorized\"}\n").unwrap();
        Authorization {
            gpu_key: "gpu-a".into(),
            crashes: vec![crash(931), crash(943), crash(950)],
            phase: Phase::Ready,
            audit_path: path,
            diagnostic_pair: None,
        }
    }

    #[test]
    fn diagnostic_authorization_is_bound_to_one_pair_and_one_claim() {
        let mut a = authorization();
        a.diagnostic_pair = Some((1830, 943));
        assert!(a.check_pair((1830, 943)).is_ok());
        assert!(a.check_pair((1845, 943)).is_err());
        assert!(a.check_pair((1830, 950)).is_err());
        a.claim().unwrap();
        assert!(a.claim().is_err());
        let mut changed = a.crashes.clone();
        changed.push(crash(956));
        assert!(a.check_and_latch(&changed, "gpu-a").is_err());
        assert!(a.budget(&a.crashes, "gpu-a").is_err());
        fs::remove_file(a.audit_path).unwrap();
    }

    #[test]
    fn one_claim_and_any_terminal_outcome_consume_permission() {
        for outcome in ["completed", "paused", "panic", "reset", "failed"] {
            let mut a = authorization();
            assert!(a.budget(&a.crashes, "gpu-a").is_ok());
            a.claim().unwrap();
            assert!(a.claim().is_err());
            assert!(a.budget(&a.crashes, "gpu-a").is_ok());
            a.finish(outcome, Some("new-run")).unwrap();
            assert!(a.budget(&a.crashes, "gpu-a").is_err());
            assert!(a.claim().is_err());
            let events: Vec<serde_json::Value> = fs::read_to_string(&a.audit_path)
                .unwrap()
                .lines()
                .map(|s| serde_json::from_str(s).unwrap())
                .collect();
            assert_eq!(events.len(), 3);
            assert_eq!(events[2]["reason"], outcome);
            fs::remove_file(a.audit_path).unwrap();
        }
    }

    #[test]
    fn first_new_crash_changed_history_and_other_gpu_are_refused() {
        let a = authorization();
        let mut changed = a.crashes.clone();
        changed.push(crash(956));
        assert!(a.budget(&changed, "gpu-a").is_err());
        assert!(a.budget(&a.crashes[..2], "gpu-a").is_err());
        let mut replaced = a.crashes.clone();
        replaced[0] = crash(925);
        assert!(a.budget(&replaced, "gpu-a").is_err());
        assert!(a.budget(&a.crashes, "gpu-b").is_err());
        fs::remove_file(a.audit_path).unwrap();
    }

    #[test]
    fn crash_latches_permission_even_if_history_is_later_rolled_back() {
        let mut a = authorization();
        let baseline = a.crashes.clone();
        let mut changed = baseline.clone();
        changed.push(crash(956));
        assert!(a.check_and_latch(&changed, "gpu-a").is_err());
        assert!(a.check_and_latch(&baseline, "gpu-a").is_err());
        assert!(a.claim().is_err());
        fs::remove_file(a.audit_path).unwrap();
    }

    #[test]
    fn authorization_keeps_all_historical_cone_sources() {
        let a = authorization();
        let baseline = a.crashes.clone();
        let targets = [1920, 1905, 1890];
        let bins = [925, 931, 937, 943, 950, 956];
        let before = nidavellir_core::condemnation::rigid_tdr_safety_cone_floors(
            &baseline, "gpu-a", 29, &targets, &bins,
        );
        assert!(!before.is_empty());
        assert!(a.budget(&baseline, "gpu-a").is_ok());
        assert_eq!(
            nidavellir_core::condemnation::rigid_tdr_safety_cone_floors(
                &a.crashes, "gpu-a", 29, &targets, &bins
            ),
            before
        );
        assert_eq!(a.crashes, baseline);
        fs::remove_file(a.audit_path).unwrap();
    }

    #[test]
    fn audit_failure_consumes_permission_before_work_can_start() {
        let mut a = authorization();
        fs::remove_file(&a.audit_path).unwrap();
        assert!(a.claim().is_err());
        assert_eq!(a.phase, Phase::Finished);
        assert!(a.claim().is_err());
    }

    #[test]
    fn readiness_requires_idle_no_recovery_no_profile_and_no_checkpoint() {
        let mut record = SafeLoopRecord::default();
        assert!(require_ready(&record, false, false, false).is_ok());
        for (flag, applied, checkpoint) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            assert!(require_ready(&record, flag, applied, checkpoint).is_err());
        }
        record.safe_mode = true;
        assert!(require_ready(&record, false, false, false).is_err());
        record.safe_mode = false;
        record.state = SafeLoopState::Unstable;
        assert!(require_ready(&record, false, false, false).is_err());
    }

    #[test]
    fn development_scope_excludes_resume_long_manual_apply_and_other_workers() {
        for request in [
            IpcRequest::StartPowerSweep,
            IpcRequest::StartPowerSweepClean,
        ] {
            assert!(allows_tuning_request(&request));
        }
        for request in [
            IpcRequest::ResumePowerSweep,
            IpcRequest::StartPowerSweepLong,
            IpcRequest::StartMemSweep,
            IpcRequest::ApplyPowerBrokkrs,
            IpcRequest::StartBenchmark,
            IpcRequest::ApplyManualDiagnosticPoint {
                target_mhz: 1920,
                voltage_mv: 943,
            },
        ] {
            assert!(!allows_tuning_request(&request));
        }
    }

    #[test]
    fn ordinary_service_cannot_authorize_and_checkpoints_cannot_restore_permission() {
        let store = SafeLoopStore::new(
            std::env::temp_dir().join("nidavellir-disabled-development-authorization"),
        );
        assert!(authorize(&store, "Reviewed development validation")
            .unwrap_err()
            .contains("disabled"));
        assert!(budget_override(&[], "gpu-a").is_none());
        assert!(status_note().is_none());
        let mut saved =
            serde_json::to_value(nidavellir_core::ipc::PowerSweepProgress::default()).unwrap();
        saved["development_validation_note"] = json!("Forged saved authorization must be ignored");
        let progress: nidavellir_core::ipc::PowerSweepProgress =
            serde_json::from_value(saved).unwrap();
        assert!(progress.development_validation_note.is_none());
    }
}
