//! Service-side Safe Loop runtime: the privileged half of roadmap §4.
//!
//! On startup the service runs [`run_startup_recovery`] *before* anything else
//! touches hardware — it reads the on-disk boot-flag, classifies any post-reboot
//! bugcheck, and decides whether to recede, reapply the last good profile, or
//! drop to Safe Mode. A background thread keeps a liveness heartbeat fresh.
//!
//! All the decision logic lives in `nidavellir_core::safe_loop` (pure + tested);
//! this module only does the OS I/O (event log, threads) and logging.

use std::time::Duration;

use nidavellir_core::ipc::SafeLoopStatus;
use nidavellir_core::safe_loop::{self, CrashClass, RecoveryAction, SafeLoopRecord, SafeLoopStore};
use tracing::{info, warn};

/// How often the liveness heartbeat is refreshed.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

fn retain_boot_flag_until_reapply(action: &RecoveryAction) -> bool {
    matches!(
        action,
        RecoveryAction::RecoverDiagnosticInterruption { .. }
            | RecoveryAction::BlacklistAndRecede { .. }
            | RecoveryAction::EnterSafeMode { .. }
    )
}

/// Run boot-time crash recovery and return the (persisted) updated record.
///
/// The returned [`TuningPoint`] is what *should* be (re)applied to hardware.
/// Actual application is wired in once tuning axes land (v0.3+); for now we log
/// the intent so the safety scaffold is observable end to end.
pub fn run_startup_recovery(store: &SafeLoopStore) -> SafeLoopRecord {
    let mut record = match store.load_record_checked() {
        Ok(record) => record,
        Err(error) => {
            warn!(
                "Safe Loop: persisted record is unreadable; startup fails closed at stock: {error}"
            );
            return SafeLoopRecord {
                safe_mode: true,
                state: nidavellir_core::safe_loop::SafeLoopState::SafeMode,
                ..SafeLoopRecord::default()
            };
        }
    };
    let mut boot_flag = match store.read_boot_flag_checked() {
        Ok(flag) => flag,
        Err(error) => {
            warn!("Safe Loop: boot flag is unreadable; startup fails closed at stock and preserves it: {error}");
            record.safe_mode = true;
            record.state = nidavellir_core::safe_loop::SafeLoopState::SafeMode;
            return record;
        }
    };

    // A graceful service stop (clean OS shutdown/restart, or an explicit stop) writes a one-shot
    // marker. If a boot-flag was still armed at that moment, the apply/forge it guarded was
    // interrupted by a *user-initiated restart*, not a crash — so disarm it without counting a
    // crash. The marker is always consumed here so it can never mask a later, genuine crash.
    let clean_shutdown = store.is_clean_shutdown_present();
    let clean_shutdown_consumed = if clean_shutdown {
        match store.clear_clean_shutdown() {
            Ok(()) => true,
            Err(e) => {
                warn!("Safe Loop: failed to consume clean-shutdown marker; it cannot mask an armed recovery transaction: {e}");
                false
            }
        }
    } else {
        false
    };
    if boot_flag.is_some() && clean_shutdown_consumed {
        info!(
            "Safe Loop: boot-flag was armed but the previous service stop was graceful — \
             treating as a clean interruption, not a crash"
        );
        let expected = boot_flag.as_ref().expect("checked above");
        match store.clear_boot_flag_if_matches(expected) {
            Ok(true) => boot_flag = None,
            Ok(false) => {
                warn!("Safe Loop: clean-shutdown flag ownership changed before clear; startup remains fail-closed at stock");
                record.safe_mode = true;
                record.state = nidavellir_core::safe_loop::SafeLoopState::SafeMode;
                return record;
            }
            Err(e) => {
                warn!("Safe Loop: failed to disarm boot-flag after a clean shutdown; startup remains at stock: {e}");
                record.safe_mode = true;
                record.state = nidavellir_core::safe_loop::SafeLoopState::SafeMode;
                return record;
            }
        }
    }

    let bugcheck = if let Some(flag) = boot_flag.as_ref() {
        read_last_bugcheck_class(&flag.timestamp)
    } else {
        CrashClass::Unknown
    };

    let action = safe_loop::decide_recovery(boot_flag.as_ref(), bugcheck, &record);
    let target = safe_loop::apply_recovery(&mut record, &action);

    match &action {
        RecoveryAction::Idle => {
            info!("Safe Loop: clean boot, nothing to restore");
        }
        RecoveryAction::ApplyLastValidated { point } => {
            info!("Safe Loop: clean boot, reapplying last validated profile {point:?}");
        }
        RecoveryAction::AwaitOperatorAcknowledgement { incident } => {
            warn!(
                "Safe Loop: Forge incident {} requires operator acknowledgement; staying at stock",
                incident.id
            );
        }
        RecoveryAction::RecoverDiagnosticInterruption { interrupted, class } => {
            warn!(
                "Safe Loop: Detector Lab was interrupted at {interrupted:?} ({class:?}); returning \
                 to stock without blacklisting the diagnostic point or consuming Safe Mode budget"
            );
        }
        RecoveryAction::BlacklistAndRecede {
            crashed,
            recede_to,
            class,
            count_toward_safe_mode,
        } => {
            warn!(
                "Safe Loop: boot-flag was ARMED — last apply {crashed:?} crashed ({class:?}). \
                 Blacklisting region and receding to {recede_to:?} \
                 (consecutive crashes: {}, counted toward Safe Mode: {count_toward_safe_mode})",
                record.consecutive_crashes,
            );
        }
        RecoveryAction::EnterSafeMode { .. } => {
            warn!(
                "Safe Loop: {} consecutive crashes — entering SAFE MODE (stock profile, hands off)",
                record.consecutive_crashes
            );
        }
        RecoveryAction::RemainSafeMode { .. } => {
            info!(
                "Safe Loop: clean boot while in Safe Mode — staying hands off (no new crash counted, \
                 {} on record). Use Reset all to release.",
                record.consecutive_crashes
            );
        }
    }

    let _ = target; // application is a v0.3+ concern; intent is logged above.

    // Keep an accounted crash flag armed until apply-on-boot has observed it and explicitly skipped
    // the persisted profile. Clearing it here would let the same crashing profile be reapplied later
    // in this startup sequence. Clean/idle actions have no crash profile to suppress.
    let retain_until_reapply = retain_boot_flag_until_reapply(&action);
    if !retain_until_reapply {
        if let Some(expected) = boot_flag.as_ref() {
            match store.clear_boot_flag_if_matches(expected) {
                Ok(true) | Ok(false) => {}
                Err(e) => warn!("Safe Loop: failed to clear owned boot-flag: {e}"),
            }
        }
    }
    if let Err(e) = store.save_record(&record) {
        warn!("Safe Loop: failed to persist record: {e}");
    }
    record
}

/// Spawn the liveness heartbeat writer (roadmap §4.3, layer 2).
pub fn spawn_heartbeat(store: SafeLoopStore) {
    std::thread::spawn(move || loop {
        if let Err(e) = store.write_heartbeat() {
            warn!("Safe Loop: heartbeat write failed: {e}");
        }
        std::thread::sleep(HEARTBEAT_INTERVAL);
    });
}

/// Build the read-only status snapshot for the UI.
pub fn status_snapshot(store: &SafeLoopStore) -> SafeLoopStatus {
    let mut record = store.load_record_checked().unwrap_or_else(|error| {
        warn!("Safe Loop status: persisted record is unreadable; exposing fail-closed Safe Mode: {error}");
        SafeLoopRecord {
            safe_mode: true,
            state: nidavellir_core::safe_loop::SafeLoopState::SafeMode,
            ..SafeLoopRecord::default()
        }
    });
    let boot_flag_armed = match store.read_boot_flag_checked() {
        Ok(flag) => flag.is_some(),
        Err(error) => {
            warn!("Safe Loop status: boot flag is unreadable; exposing fail-closed Safe Mode: {error}");
            record.safe_mode = true;
            record.state = nidavellir_core::safe_loop::SafeLoopState::SafeMode;
            true
        }
    };
    let gpu_reboot_event = crate::tdr_sentinel::reboot_required_event();
    let ledger = nidavellir_core::condemnation::CondemnationLedger::new(store.base_dir());
    let mut condemnations = match ledger.load_all_checked() {
        Ok(events) => nidavellir_core::condemnation::effective_condemnation_events(&events),
        Err(error) => {
            warn!("Safe Loop status: condemnation ledger is unreadable; exposing fail-closed Safe Mode: {error}");
            record.safe_mode = true;
            record.state = nidavellir_core::safe_loop::SafeLoopState::SafeMode;
            Vec::new()
        }
    };
    const SENTINEL_CONDEMNATION_TAIL: usize = 100;
    if condemnations.len() > SENTINEL_CONDEMNATION_TAIL {
        let drop = condemnations.len() - SENTINEL_CONDEMNATION_TAIL;
        condemnations.drain(0..drop);
    }
    SafeLoopStatus {
        state: record.state,
        safe_mode: record.safe_mode,
        consecutive_crashes: record.consecutive_crashes,
        crash_threshold: safe_loop::SAFE_MODE_CRASH_THRESHOLD,
        boot_flag_armed,
        last_validated: record.last_validated,
        blacklist: record.blacklist,
        recent_crashes: record.crash_log,
        recovery_pending_ack: record.pending_forge_incident.is_some(),
        gpu_reboot_required: gpu_reboot_event.is_some(),
        gpu_reboot_event,
        pending_forge_incident: record.pending_forge_incident,
        condemnations,
    }
}

/// Explicitly release the pending Forge incident latch while preserving blacklist and history.
/// `expected_id` (auto-resume) refuses when the latch no longer holds the incident it decided on.
pub fn acknowledge_forge_incident(
    store: &SafeLoopStore,
    expected_id: Option<&str>,
) -> Result<bool, String> {
    let mut record = store.load_record_checked().map_err(|error| {
        format!("Safe Loop record is unreadable; acknowledgement refused: {error}")
    })?;
    if expected_id.is_some_and(|id| {
        record.pending_forge_incident.as_ref().map(|incident| incident.id.as_str()) != Some(id)
    }) {
        return Err("o incidente pendente mudou; reconhecimento automático recusado".into());
    }
    #[cfg(windows)]
    if let Some(incident) = record.pending_forge_incident.as_ref().filter(|incident| {
        incident.kind == nidavellir_core::safe_loop::ForgeIncidentKind::CandidateCrash
    }) {
        let run_id = incident.run_id.as_deref().ok_or_else(|| {
            "CandidateCrash sem run_id exato; reconhecimento recusado para preservar o cone TDR"
                .to_string()
        })?;
        let gpu_key = incident.gpu_key.as_deref().ok_or_else(|| {
            "CandidateCrash sem GPU exata; reconhecimento recusado para preservar o cone TDR"
                .to_string()
        })?;
        let target_mhz = incident.target_mhz.ok_or_else(|| {
            "CandidateCrash sem target exato; reconhecimento recusado para preservar o cone TDR"
                .to_string()
        })?;
        let anchor_mv = incident.anchor_mv.ok_or_else(|| {
            "CandidateCrash sem bin VF exato; reconhecimento recusado para preservar o cone TDR"
                .to_string()
        })?;
        crate::gpu_power_sweep::ensure_reconciled_candidate_crash_condemnation(
            store,
            gpu_key,
            Some(run_id),
            target_mhz,
            anchor_mv,
            &incident.message,
        )
        .map_err(|e| {
            format!(
                "CandidateCrash não pôde ser persistido no ledger rígido; incidente continua pendente: {e}"
            )
        })?;
    }
    #[cfg(not(windows))]
    if record
        .pending_forge_incident
        .as_ref()
        .is_some_and(|incident| {
            incident.kind == nidavellir_core::safe_loop::ForgeIncidentKind::CandidateCrash
        })
    {
        return Err(
            "CandidateCrash acknowledgement is available only on Windows with a durable ledger"
                .into(),
        );
    }
    let acknowledged = record.acknowledge_forge_incident().is_some();
    if acknowledged {
        store
            .save_record(&record)
            .map_err(|e| format!("persist Forge incident acknowledgement: {e}"))?;
    }
    Ok(acknowledged)
}

#[derive(serde::Deserialize)]
struct BugcheckEvent {
    timestamp: String,
    message: String,
}

/// A historical BSOD cannot classify a newer interrupted transaction. Unknown still takes
/// the conservative recovery path; this correlation check never certifies a candidate as safe.
fn classify_transaction_bugcheck(
    armed_at: &str,
    event: Option<&BugcheckEvent>,
    now: chrono::DateTime<chrono::Utc>,
) -> CrashClass {
    let Some(event) = event else { return CrashClass::Unknown; };
    let (Ok(armed), Ok(reported)) = (
        chrono::DateTime::parse_from_rfc3339(armed_at),
        chrono::DateTime::parse_from_rfc3339(&event.timestamp),
    ) else { return CrashClass::Unknown; };
    if reported < armed || reported > now || armed > now {
        return CrashClass::Unknown;
    }
    safe_loop::parse_bugcheck_code(&event.message)
        .map(safe_loop::classify_bugcheck)
        .unwrap_or(CrashClass::Unknown)
}

fn read_last_bugcheck_class(armed_at: &str) -> CrashClass {
    let event = read_last_bugcheck_event();
    let class = classify_transaction_bugcheck(armed_at, event.as_ref(), chrono::Utc::now());
    info!("Safe Loop: transaction bugcheck classification {class:?} (armed {armed_at}, latest report {:?})", event.as_ref().map(|event| &event.timestamp));
    class
}

#[cfg(windows)]
fn read_last_bugcheck_event() -> Option<BugcheckEvent> {
    // Event 1001 from the WER-SystemErrorReporting provider is the "computer
    // rebooted from a bugcheck" record; its message carries the stop code.
    let ps = "[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); \
              Get-WinEvent -FilterHashtable @{LogName='System'; \
              ProviderName='Microsoft-Windows-WER-SystemErrorReporting'; Id=1001} \
              -MaxEvents 1 -ErrorAction SilentlyContinue | ForEach-Object { \
              @{timestamp=$_.TimeCreated.ToUniversalTime().ToString('o'); message=$_.Message} \
              | ConvertTo-Json -Compress }";
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", ps])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    serde_json::from_str(&text).ok()
}

#[cfg(not(windows))]
fn read_last_bugcheck_event() -> Option<BugcheckEvent> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_bugcheck_rejects_old_bsod_after_a_later_interruption() {
        let now = "2026-09-10T19:42:44Z".parse::<chrono::DateTime<chrono::Utc>>().unwrap();
        let event = BugcheckEvent { timestamp: "2026-08-04T23:29:22.8620461Z".into(), message: "The bugcheck was: 0x00000116".into() };
        assert_eq!(classify_transaction_bugcheck("2026-08-29T00:00:00Z", Some(&event), now), CrashClass::Unknown);
        assert_eq!(classify_transaction_bugcheck("2026-08-29T00:00:00Z", None, now), CrashClass::Unknown);
    }

    #[test]
    fn transaction_bugcheck_accepts_current_report_with_timezone_conversion() {
        let now = "2026-09-10T19:42:44Z".parse::<chrono::DateTime<chrono::Utc>>().unwrap();
        let event = BugcheckEvent { timestamp: "2026-09-10T16:37:20-03:00".into(), message: "The bugcheck was: 0x00000116".into() };
        assert_eq!(classify_transaction_bugcheck("2026-09-10T19:30:00Z", Some(&event), now), CrashClass::OcInstability);
    }

    #[test]
    fn transaction_bugcheck_refuses_invalid_or_future_timestamps_and_messages() {
        let now = "2026-09-10T19:42:44Z".parse::<chrono::DateTime<chrono::Utc>>().unwrap();
        for (armed, reported, message) in [
            ("invalid", "2026-09-10T19:37:20Z", "0x00000116"),
            ("2026-09-10T19:30:00Z", "invalid", "0x00000116"),
            ("2026-09-10T19:30:00Z", "2026-09-11T19:37:20Z", "0x00000116"),
            ("2026-09-11T19:30:00Z", "2026-09-11T19:37:20Z", "0x00000116"),
            ("2026-09-10T19:30:00Z", "2026-09-10T19:37:20Z", "No bugcheck code"),
        ] {
            let event = BugcheckEvent { timestamp: reported.into(), message: message.into() };
            assert_eq!(classify_transaction_bugcheck(armed, Some(&event), now), CrashClass::Unknown);
        }
    }

    use nidavellir_core::condemnation::{
        CondemnationEvent, CondemnationLedger, CondemnationSeverity, KIND_REHABILITATED,
    };
    use nidavellir_core::safe_loop::TuningPoint;

    #[cfg(windows)]
    fn pending_candidate_crash(store: &SafeLoopStore) {
        use nidavellir_core::safe_loop::{ForgeIncident, ForgeIncidentKind, SafeLoopRecord};
        let mut record = SafeLoopRecord::default();
        assert!(record.record_forge_incident(ForgeIncident::new(
            ForgeIncidentKind::CandidateCrash,
            Some("run-ack".into()),
            Some("gpu-ack".into()),
            Some(1800),
            Some(875),
            "exact candidate crash",
        )));
        store.save_record(&record).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn candidate_crash_ack_persists_rigid_v29_before_releasing_latch() {
        let base = std::env::temp_dir().join(format!(
            "nidavellir-candidate-ack-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SafeLoopStore::new(&base);
        pending_candidate_crash(&store);

        // Auto-resume's compare-and-acknowledge refuses a latch that changed under it.
        assert!(acknowledge_forge_incident(&store, Some("another-incident")).is_err());
        assert!(store.load_record().pending_forge_incident.is_some());
        assert_eq!(acknowledge_forge_incident(&store, None), Ok(true));
        assert!(store.load_record().pending_forge_incident.is_none());
        let effective = nidavellir_core::condemnation::effective_condemnation_events(
            &CondemnationLedger::new(&base).load_all(),
        );
        assert!(effective.iter().any(|event| {
            event.kind == nidavellir_core::condemnation::KIND_CANDIDATE_CRASH
                && event.severity == CondemnationSeverity::Rigid
                && event.run_id.as_deref() == Some("run-ack")
                && event.gpu_key.as_deref() == Some("gpu-ack")
                && event.target_mhz == 1800
                && event.vf_bin_mv == 875
                && event.qualification_contract_version
                    == Some(
                        nidavellir_core::f2_observation::
                            F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION,
                    )
        }));
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(windows)]
    #[test]
    fn candidate_crash_ack_keeps_latch_when_ledger_append_fails() {
        let base = std::env::temp_dir().join(format!(
            "nidavellir-candidate-ack-failure-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(base.join(nidavellir_core::condemnation::CONDEMNATION_LEDGER_FILE))
            .unwrap();
        let store = SafeLoopStore::new(&base);
        pending_candidate_crash(&store);

        let error = acknowledge_forge_incident(&store, None).unwrap_err();
        assert!(error.contains("continua pendente"), "{error}");
        assert!(store.load_record().pending_forge_incident.is_some());

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn crash_recovery_retains_flag_until_reapply_can_skip_bad_profile() {
        let point = TuningPoint::stock();
        assert!(retain_boot_flag_until_reapply(
            &RecoveryAction::BlacklistAndRecede {
                crashed: point.clone(),
                recede_to: point.clone(),
                class: CrashClass::Unknown,
                count_toward_safe_mode: true,
            }
        ));
        assert!(retain_boot_flag_until_reapply(
            &RecoveryAction::EnterSafeMode {
                stock: point.clone()
            }
        ));
        assert!(retain_boot_flag_until_reapply(
            &RecoveryAction::RecoverDiagnosticInterruption {
                interrupted: point.clone(),
                class: CrashClass::Unknown,
            }
        ));
        assert!(!retain_boot_flag_until_reapply(&RecoveryAction::Idle));
        assert!(!retain_boot_flag_until_reapply(
            &RecoveryAction::ApplyLastValidated { point }
        ));
    }

    #[test]
    fn startup_preserves_corrupt_boot_flag_and_fails_closed() {
        let base = std::env::temp_dir().join(format!(
            "nidavellir-startup-corrupt-flag-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SafeLoopStore::new(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(store.boot_flag_path(), "{ truncated").unwrap();

        let recovered = run_startup_recovery(&store);
        assert!(recovered.safe_mode);
        assert!(store.is_boot_flag_armed());
        assert!(store.read_boot_flag_checked().is_err());
        let status = status_snapshot(&store);
        assert!(status.safe_mode);
        assert!(status.boot_flag_armed);
        assert_eq!(
            status.state,
            nidavellir_core::safe_loop::SafeLoopState::SafeMode
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn startup_does_not_overwrite_corrupt_safe_loop_record() {
        let base = std::env::temp_dir().join(format!(
            "nidavellir-startup-corrupt-record-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SafeLoopStore::new(&base);
        std::fs::create_dir_all(&base).unwrap();
        let original = "{ truncated";
        std::fs::write(store.record_path(), original).unwrap();

        let recovered = run_startup_recovery(&store);
        assert!(recovered.safe_mode);
        assert_eq!(
            std::fs::read_to_string(store.record_path()).unwrap(),
            original
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn status_snapshot_fails_closed_when_condemnation_ledger_is_corrupt() {
        let base = std::env::temp_dir().join(format!(
            "nidavellir-safe-status-corrupt-ledger-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join(nidavellir_core::condemnation::CONDEMNATION_LEDGER_FILE),
            "{ truncated\n",
        )
        .unwrap();
        let status = status_snapshot(&SafeLoopStore::new(&base));
        assert!(status.safe_mode);
        assert_eq!(
            status.state,
            nidavellir_core::safe_loop::SafeLoopState::SafeMode
        );
        assert!(status.condemnations.is_empty());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn status_snapshot_exposes_only_recent_effective_condemnations() {
        let base = std::env::temp_dir().join(format!(
            "nidavellir-safe-status-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let store = SafeLoopStore::new(&base);
        let ledger = CondemnationLedger::new(&base);
        for index in 0..105u32 {
            ledger
                .append(&CondemnationEvent {
                    timestamp: format!("2026-07-16T00:00:{:02}Z", index % 60),
                    gpu_key: Some("gpu-test".into()),
                    severity: CondemnationSeverity::Quarantine,
                    kind: "apply-gate-silent-error".into(),
                    target_mhz: 1700 + index,
                    vf_bin_mv: 800 + index,
                    run_id: Some("run-test".into()),
                    qualification_contract_version: Some(17),
                    note: None,
                    rehabilitated: false,
                })
                .unwrap();
        }
        let first_visible = CondemnationEvent {
            timestamp: "2026-07-16T01:00:00Z".into(),
            gpu_key: Some("gpu-test".into()),
            severity: CondemnationSeverity::Rigid,
            kind: KIND_REHABILITATED.into(),
            target_mhz: 1804,
            vf_bin_mv: 904,
            run_id: Some("run-test".into()),
            qualification_contract_version: Some(17),
            note: None,
            rehabilitated: true,
        };
        ledger.append(&first_visible).unwrap();

        let status = status_snapshot(&store);
        assert_eq!(status.condemnations.len(), 100);
        assert!(status
            .condemnations
            .iter()
            .all(|event| !event.rehabilitated));
        assert!(!status
            .condemnations
            .iter()
            .any(|event| event.target_mhz == 1804 && event.vf_bin_mv == 904));
        assert_eq!(status.condemnations.last().unwrap().target_mhz, 1803);
        let _ = std::fs::remove_dir_all(&base);
    }
}
