use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use nidavellir_core::ipc::ManualDiagnosticPointStatus;
use nidavellir_core::safe_loop::{
    BootFlag, SafeLoopStore, TuningPoint, GAME_TRACE_DIAGNOSTIC_PHASE,
};

pub(crate) type ManualPointStatusSlot = Arc<Mutex<ManualDiagnosticPointStatus>>;

#[derive(Clone)]
pub struct ManualPointHandle {
    status: ManualPointStatusSlot,
}

impl Default for ManualPointHandle {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(ManualDiagnosticPointStatus {
                note: "No manual diagnostic point is applied.".into(),
                ..ManualDiagnosticPointStatus::default()
            })),
        }
    }
}

impl ManualPointHandle {
    pub fn status(&self) -> ManualDiagnosticPointStatus {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_else(|_| ManualDiagnosticPointStatus {
                note: "Manual point status is unavailable.".into(),
                ..ManualDiagnosticPointStatus::default()
            })
    }

    pub(crate) fn status_slot(&self) -> ManualPointStatusSlot {
        Arc::clone(&self.status)
    }

    pub fn apply(
        &mut self,
        store: &SafeLoopStore,
        target_mhz: u32,
        requested_voltage_mv: u32,
    ) -> Result<ManualDiagnosticPointStatus, String> {
        self.apply_with_mode(store, target_mhz, requested_voltage_mv, false)
    }

    pub fn apply_curve(
        &mut self,
        store: &SafeLoopStore,
        target_mhz: u32,
        requested_voltage_mv: u32,
    ) -> Result<ManualDiagnosticPointStatus, String> {
        self.apply_with_mode(store, target_mhz, requested_voltage_mv, true)
    }

    fn apply_with_mode(
        &mut self,
        store: &SafeLoopStore,
        target_mhz: u32,
        requested_voltage_mv: u32,
        curve_only: bool,
    ) -> Result<ManualDiagnosticPointStatus, String> {
        if self.status().active {
            return Err(
                "A manual diagnostic point is already active; return to stock first".into(),
            );
        }
        if !(300..=4_000).contains(&target_mhz) {
            return Err("Target clock must be between 300 and 4000 MHz".into());
        }
        if !(500..=1_250).contains(&requested_voltage_mv) {
            return Err("Requested voltage must be between 500 and 1250 mV".into());
        }

        let record = store.load_record_checked().map_err(|error| {
            format!("Safe Loop record is unreadable; manual point refused: {error}")
        })?;
        if record.safe_mode {
            return Err(
                "Safe Mode is active; recover the GPU before applying a manual point".into(),
            );
        }
        match store.read_boot_flag_checked() {
            Ok(None) => {}
            Ok(Some(_)) => {
                return Err("Safe Loop recovery is armed; return the GPU to stock before applying a manual point".into())
            }
            Err(error) => {
                return Err(format!(
                    "Safe Loop boot flag is unreadable; manual point refused: {error}"
                ))
            }
        }

        let (resolved_voltage_mv, offset_mhz) =
            crate::gpu_undervolt::resolve_manual_diagnostic_point(
                target_mhz,
                requested_voltage_mv,
            )?;

        crate::gpu_apply::generic_hardware_write_preflight(
            store,
            Some((target_mhz, resolved_voltage_mv)),
        )?;

        reset_hardware()?;
        crate::gpu_apply::clear_applied_checked()?;

        let apply_result = if curve_only {
            apply_resolved_curve_point(
                store,
                target_mhz,
                resolved_voltage_mv,
                offset_mhz,
                GAME_TRACE_DIAGNOSTIC_PHASE,
            )
        } else {
            apply_resolved_point(
                store,
                target_mhz,
                resolved_voltage_mv,
                offset_mhz,
                "manual_diagnostic_point",
            )
        };
        if let Err(error) = apply_result {
            let recovery = reset_and_disarm(store);
            return Err(match recovery {
                Ok(()) => format!("Manual point apply failed ({error}); GPU returned to stock"),
                Err(reset_error) => format!(
                    "Manual point apply failed ({error}); stock recovery also failed ({reset_error})"
                ),
            });
        }

        replace_status(&self.status, ManualDiagnosticPointStatus {
            active: true,
            target_mhz: Some(target_mhz),
            requested_voltage_mv: Some(requested_voltage_mv),
            resolved_voltage_mv: Some(resolved_voltage_mv),
            applied_at_epoch_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|duration| duration.as_millis() as u64),
            verified: true,
            note: if curve_only {
                "Temporary curve is active with a max-clock ceiling and elastic voltage. Game Trace can now monitor the real workload."
            } else {
                "Temporary point is active with a max-clock ceiling and verified voltage lock. Start Game Trace before launching the workload."
            }
            .into(),
        })?;
        Ok(self.status())
    }

    pub fn reset(&mut self, store: &SafeLoopStore) -> Result<ManualDiagnosticPointStatus, String> {
        reset_and_disarm(store)?;
        self.mark_reset();
        Ok(self.status())
    }

    pub fn mark_reset(&mut self) {
        let _ = replace_status(
            &self.status,
            ManualDiagnosticPointStatus {
                note: "GPU is at stock; no manual diagnostic point is active.".into(),
                ..ManualDiagnosticPointStatus::default()
            },
        );
    }
}

pub(crate) fn replace_status(
    slot: &ManualPointStatusSlot,
    status: ManualDiagnosticPointStatus,
) -> Result<(), String> {
    *slot
        .lock()
        .map_err(|_| "Manual point status lock is poisoned".to_string())? = status;
    Ok(())
}

pub(crate) fn apply_resolved_point(
    store: &SafeLoopStore,
    target_mhz: u32,
    resolved_voltage_mv: u32,
    offset_mhz: i32,
    source: &str,
) -> Result<(), String> {
    // Detector Lab calls this for every lane re-apply. Re-read current reboot/Safe Loop/ledger/cone
    // state immediately before each write instead of trusting the session-start decision.
    crate::gpu_apply::generic_hardware_write_preflight(
        store,
        Some((target_mhz, resolved_voltage_mv)),
    )?;
    arm_manual_point(store, target_mhz, resolved_voltage_mv, offset_mhz, source)?;
    crate::gpu_undervolt::apply_anchored_undervolt(target_mhz, resolved_voltage_mv)
}

pub(crate) fn apply_resolved_curve_point(
    store: &SafeLoopStore,
    target_mhz: u32,
    resolved_voltage_mv: u32,
    offset_mhz: i32,
    source: &str,
) -> Result<(), String> {
    crate::gpu_apply::generic_hardware_write_preflight(
        store,
        Some((target_mhz, resolved_voltage_mv)),
    )?;
    arm_manual_point(store, target_mhz, resolved_voltage_mv, offset_mhz, source)?;
    crate::gpu_undervolt::apply_anchored_curve_only(target_mhz, resolved_voltage_mv)
}

fn arm_manual_point(
    store: &SafeLoopStore,
    target_mhz: u32,
    resolved_voltage_mv: u32,
    offset_mhz: i32,
    source: &str,
) -> Result<(), String> {
    let intent = TuningPoint::from_axes([
        ("gpu_freq_mhz", target_mhz as i64),
        ("gpu_vf_bin_mv", resolved_voltage_mv as i64),
        ("gpu_offset_mhz", offset_mhz as i64),
    ]);
    store
        .arm_boot_flag(&BootFlag::new(intent, source))
        .map_err(|error| format!("Manual point: failed to arm Safe Loop before write: {error}"))
}

pub(crate) fn reset_hardware() -> Result<(), String> {
    let clock_error = nidavellir_core::nvml_gpu::reset_core_clock_lock().err();
    let vf_error = nidavellir_gpu_nvapi::reset_all().err();
    if clock_error.is_some() || vf_error.is_some() {
        return Err(format!(
            "clock-cap={}; VF/global={}",
            clock_error.as_deref().unwrap_or("ok"),
            vf_error.as_deref().unwrap_or("ok")
        ));
    }
    Ok(())
}

pub(crate) fn reset_and_disarm(store: &SafeLoopStore) -> Result<(), String> {
    let record = store.load_record_checked();
    let boot_flag = store.read_boot_flag_checked();
    reset_hardware()?;
    crate::gpu_apply::clear_applied_checked()?;
    record.map_err(|error| {
        format!(
            "GPU reset completed but Safe Loop record is unreadable; recovery state remains armed: {error}"
        )
    })?;
    let boot_flag = boot_flag.map_err(|error| {
        format!(
            "GPU reset completed but Safe Loop boot flag is unreadable and remains armed: {error}"
        )
    })?;
    let Some(expected) = boot_flag.as_ref() else {
        return Ok(());
    };
    match store.clear_boot_flag_if_matches(expected) {
        Ok(true) => Ok(()),
        Ok(false) => Err(
            "GPU reset completed, but boot-flag ownership changed; the newer transaction remains armed"
                .into(),
        ),
        Err(error) => Err(format!(
            "GPU reset completed but its owned Safe Loop flag could not be cleared: {error}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_point_starts_inactive() {
        let status = ManualPointHandle::default().status();
        assert!(!status.active);
        assert!(!status.verified);
        assert!(status.target_mhz.is_none());
    }

    #[test]
    fn mark_reset_clears_the_selected_point() {
        let mut handle = ManualPointHandle::default();
        replace_status(
            &handle.status,
            ManualDiagnosticPointStatus {
                active: true,
                target_mhz: Some(1800),
                ..ManualDiagnosticPointStatus::default()
            },
        )
        .unwrap();
        handle.mark_reset();
        assert!(!handle.status().active);
        assert!(handle.status().target_mhz.is_none());
    }

    #[test]
    fn manual_point_shared_preflight_blocks_pending_and_cone_but_allows_first_above() {
        use nidavellir_core::safe_loop::{ForgeIncident, ForgeIncidentKind, SafeLoopRecord};
        let mut pending = SafeLoopRecord::default();
        assert!(pending.record_forge_incident(ForgeIncident::new(
            ForgeIncidentKind::RuntimeFailure,
            Some("run-pending".into()),
            Some("gpu-a".into()),
            None,
            None,
            "pending recovery",
        )));
        let mut cone = nidavellir_core::condemnation::CondemnedPairs::default();
        cone.rigid.push((1800, 881));
        assert!(crate::gpu_power_sweep::f2_apply_preflight_from_sources(
            &pending, &cone, 1800, 893,
        )
        .unwrap_err()
        .contains("acknowledgement"));

        let clean = SafeLoopRecord::default();
        assert!(
            crate::gpu_power_sweep::f2_apply_preflight_from_sources(&clean, &cone, 1800, 875,)
                .is_err()
        );
        assert!(
            crate::gpu_power_sweep::f2_apply_preflight_from_sources(&clean, &cone, 1800, 893,)
                .is_ok()
        );

        let mut quarantine = nidavellir_core::condemnation::CondemnedPairs::default();
        quarantine.quarantine.push((1800, 893));
        assert!(crate::gpu_power_sweep::f2_apply_preflight_from_sources(
            &clean,
            &quarantine,
            1800,
            893,
        )
        .is_err(), "hardware Apply treats the quarantined exact pair as prohibited");
        assert!(crate::gpu_power_sweep::f2_apply_preflight_from_sources(
            &clean,
            &quarantine,
            1800,
            900,
        )
        .is_ok());
    }
}
