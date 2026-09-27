use std::any::Any;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use nidavellir_core::f2_observation::{F2QualificationCoverage, F2QualificationVerdict};
use nidavellir_core::gpu_sweep::StabilityResult;
use nidavellir_core::ipc::{
    DetectorLabPhaseStatus, DetectorLabStatus, ManualDiagnosticPointStatus,
};
use nidavellir_core::safe_loop::{SafeLoopStore, DETECTOR_LAB_PHASE};
use nidavellir_gpu_stress::{
    GpuCtx, RenderGoldens, VfPhaseReport, VfQualifierPattern, VfQualifierPhase,
};
use serde_json::json;

use crate::manual_point::{self, ManualPointStatusSlot};

const MIN_DURATION_S: u32 = 15;
const MAX_DURATION_S: u32 = 600;
const MATRIX_V27_API_DURATION_S: u32 = 120;
const MATRIX_V27_DX11_DURATION_MS: u64 = 420_000;
const MATRIX_V27_STOCK_LANE_MS: u64 = 60_000;
const MATRIX_V27_ENDURANCE_MS: u64 = 300_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetectorRecipe {
    MatrixV27,
    Dx11Resident,
    ControlV25,
    CurveV25,
    DenseV14,
}

impl DetectorRecipe {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "matrix_v27" | "matrix_v26" => Ok(Self::MatrixV27),
            "dx11_resident" => Ok(Self::Dx11Resident),
            "control_v25" | "control_v24" | "control_v23" => Ok(Self::ControlV25),
            "curve_v25" => Ok(Self::CurveV25),
            "dense_v14" => Ok(Self::DenseV14),
            _ => Err(
                "Detector Lab recipe must be matrix_v27, dx11_resident, control_v25, curve_v25, or dense_v14"
                    .into(),
            ),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::MatrixV27 => "matrix_v27",
            Self::Dx11Resident => "dx11_resident",
            Self::ControlV25 => "control_v25",
            Self::CurveV25 => "curve_v25",
            Self::DenseV14 => "dense_v14",
        }
    }

    fn pattern(self) -> VfQualifierPattern {
        match self {
            Self::MatrixV27 | Self::Dx11Resident | Self::ControlV25 | Self::CurveV25 => {
                VfQualifierPattern::V8Texture
            }
            Self::DenseV14 => VfQualifierPattern::DetectorLabDense,
        }
    }

    fn requires_full_stock_control(self) -> bool {
        matches!(self, Self::MatrixV27 | Self::ControlV25 | Self::CurveV25)
    }

    fn uses_voltage_lock(self) -> bool {
        !matches!(self, Self::CurveV25)
    }

    fn application_mode(self) -> &'static str {
        if self.uses_voltage_lock() {
            "voltage_lock"
        } else {
            "anchored_curve"
        }
    }

    fn candidate_duration_ms(self, duration_s: u32) -> u64 {
        if self == Self::MatrixV27 {
            MATRIX_V27_DX11_DURATION_MS
                .saturating_add(u64::from(MATRIX_V27_API_DURATION_S).saturating_mul(2_000))
                .saturating_add(MATRIX_V27_ENDURANCE_MS)
        } else {
            u64::from(duration_s).saturating_mul(1_000)
        }
    }
}

pub struct DetectorLabHandle {
    status: Arc<Mutex<DetectorLabStatus>>,
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    started_at: Arc<Mutex<Option<Instant>>>,
}

struct ActiveDetectorLabRun {
    cancel: Weak<AtomicBool>,
    status: Weak<Mutex<DetectorLabStatus>>,
    out_path: PathBuf,
    tdr_stop_requested: bool,
}

static ACTIVE_DETECTOR_LAB_RUN: OnceLock<Mutex<Option<ActiveDetectorLabRun>>> = OnceLock::new();

fn active_detector_lab_run() -> &'static Mutex<Option<ActiveDetectorLabRun>> {
    ACTIVE_DETECTOR_LAB_RUN.get_or_init(|| Mutex::new(None))
}

fn register_active_detector_lab_run(
    cancel: &Arc<AtomicBool>,
    status: &Arc<Mutex<DetectorLabStatus>>,
    out_path: &Path,
) {
    if let Ok(mut active) = active_detector_lab_run().lock() {
        *active = Some(ActiveDetectorLabRun {
            cancel: Arc::downgrade(cancel),
            status: Arc::downgrade(status),
            out_path: out_path.to_path_buf(),
            tdr_stop_requested: false,
        });
    }
}

fn clear_active_detector_lab_run(cancel: &Arc<AtomicBool>) {
    if let Ok(mut active) = active_detector_lab_run().lock() {
        let owns_slot = active
            .as_ref()
            .is_some_and(|run| run.cancel.as_ptr() == Arc::as_ptr(cancel));
        if owns_slot {
            *active = None;
        }
    }
}

/// Hand the first live driver-reset event to the Detector Lab worker. The worker owns the GPU
/// transaction and performs the stock reset after its current preemptible band returns; the
/// sentinel must never race it with a concurrent hardware mutation. Diagnostic runs remain
/// non-learning, so this records attribution and requests cancellation without writing blacklist.
pub(crate) fn request_active_tdr_stop(event_timestamp: &str) -> bool {
    let Some((cancel, status, out_path, first_request)) = active_detector_lab_run()
        .lock()
        .ok()
        .and_then(|mut active| {
            let run = active.as_mut()?;
            let Some(cancel) = run.cancel.upgrade() else {
                *active = None;
                return None;
            };
            let status = run.status.upgrade();
            let first_request = !run.tdr_stop_requested;
            run.tdr_stop_requested = true;
            Some((cancel, status, run.out_path.clone(), first_request))
        })
    else {
        return false;
    };

    cancel.store(true, Ordering::SeqCst);
    if first_request {
        let _ = append_journal(
            &out_path,
            json!({
                "event": "tdr_stop_requested",
                "epoch_ms": now_epoch_ms(),
                "driver_event_timestamp": event_timestamp,
                "action": "cooperative_cancel",
                "publishable": false,
            }),
        );
        if let Some(status) = status {
            update_status(&status, |current| {
                current.stage = "tdr_detected".into();
                current.result = Some("tdr".into());
                current.note = "GPU driver reset detected; stopping the owned workload and returning to stock. Windows restart will be required before another GPU mutation.".into();
            });
        }
    }
    true
}

impl Default for DetectorLabHandle {
    fn default() -> Self {
        let reboot_event = crate::tdr_sentinel::reboot_required_event();
        Self {
            status: Arc::new(Mutex::new(DetectorLabStatus {
                stage: if reboot_event.is_some() {
                    "reboot_required".into()
                } else {
                    "idle".into()
                },
                result: reboot_event.as_ref().map(|_| "tdr".into()),
                note: reboot_event.map_or_else(
                    || "Choose a verified manual point to compare detector recipes.".into(),
                    |event| {
                        format!(
                            "GPU driver reset detected at {event}. Reboot Windows before another GPU test."
                        )
                    },
                ),
                ..DetectorLabStatus::default()
            })),
            cancel: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            started_at: Arc::new(Mutex::new(None)),
        }
    }
}

impl DetectorLabHandle {
    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn status(&self) -> DetectorLabStatus {
        let mut status = self
            .status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_else(|_| DetectorLabStatus {
                stage: "environment_error".into(),
                result: Some("environment_error".into()),
                note: "Detector Lab status is unavailable.".into(),
                ..DetectorLabStatus::default()
            });
        if status.running {
            if let Ok(started_at) = self.started_at.lock() {
                if let Some(started_at) = *started_at {
                    status.elapsed_ms = started_at.elapsed().as_millis() as u64;
                    if matches!(status.stage.as_str(), "running" | "running_matrix_v27")
                        && status.duration_ms > 0
                    {
                        status.progress_pct = (status.elapsed_ms as f32 * 100.0
                            / status.duration_ms as f32)
                            .clamp(0.0, 99.5);
                    }
                }
            }
        }
        status
    }

    pub fn start(
        &self,
        store: SafeLoopStore,
        manual_status: ManualPointStatusSlot,
        recipe: &str,
        duration_s: u32,
    ) -> Result<DetectorLabStatus, String> {
        let recipe = DetectorRecipe::parse(recipe)?;
        if let Some(event) = crate::tdr_sentinel::reboot_required_event() {
            return Err(format!(
                "GPU driver reset detected at {event}. Reboot Windows before another Detector Lab run"
            ));
        }
        if !(MIN_DURATION_S..=MAX_DURATION_S).contains(&duration_s) {
            return Err(format!(
                "Detector Lab duration must be between {MIN_DURATION_S} and {MAX_DURATION_S} seconds"
            ));
        }
        if recipe == DetectorRecipe::MatrixV27 && duration_s != MATRIX_V27_API_DURATION_S {
            return Err(format!(
                "matrix_v27 uses the production Standard policy and requires duration_s={MATRIX_V27_API_DURATION_S}"
            ));
        }
        let point = manual_status
            .lock()
            .map_err(|_| "Manual point status lock is poisoned".to_string())?
            .clone();
        let target_mhz = point
            .target_mhz
            .filter(|_| point.active && point.verified)
            .ok_or_else(|| {
                "Apply and verify a manual diagnostic point before starting Detector Lab"
                    .to_string()
            })?;
        let requested_voltage_mv = point
            .requested_voltage_mv
            .ok_or_else(|| "The active manual point has no requested voltage".to_string())?;
        let resolved_voltage_mv = point
            .resolved_voltage_mv
            .ok_or_else(|| "The active manual point has no resolved physical VF bin".to_string())?;

        self.running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "Detector Lab is already running".to_string())?;
        self.cancel.store(false, Ordering::SeqCst);

        let session_started_epoch_ms = now_epoch_ms();
        let tdr_baseline = crate::tdr_sentinel::query_latest_tdr_event();
        let out_path = match create_journal_path() {
            Ok(path) => path,
            Err(error) => {
                self.running.store(false, Ordering::SeqCst);
                return Err(error);
            }
        };
        if let Err(error) = append_journal(
            &out_path,
            json!({
                "event": "session_start",
                "epoch_ms": session_started_epoch_ms,
                "recipe": recipe.key(),
                "duration_s": duration_s,
                "target_mhz": target_mhz,
                "requested_voltage_mv": requested_voltage_mv,
                "resolved_voltage_mv": resolved_voltage_mv,
                "application_mode": recipe.application_mode(),
                "publishable": false,
            }),
        ) {
            self.running.store(false, Ordering::SeqCst);
            return Err(error);
        }

        let initial = DetectorLabStatus {
            running: true,
            recipe: Some(recipe.key().into()),
            target_mhz: Some(target_mhz),
            voltage_mv: Some(resolved_voltage_mv),
            stage: "calibrating_stock".into(),
            duration_ms: recipe.candidate_duration_ms(duration_s),
            out_path: Some(out_path.display().to_string()),
            note: "Returning to stock and capturing the golden reference.".into(),
            ..DetectorLabStatus::default()
        };
        if let Err(error) = set_status(&self.status, initial.clone()) {
            self.running.store(false, Ordering::SeqCst);
            return Err(error);
        }
        match self.started_at.lock() {
            Ok(mut timer) => *timer = Some(Instant::now()),
            Err(_) => {
                self.running.store(false, Ordering::SeqCst);
                return Err("Detector Lab timer lock is poisoned".into());
            }
        }

        let status = Arc::clone(&self.status);
        let cancel = Arc::clone(&self.cancel);
        let running = Arc::clone(&self.running);
        let started_at = Arc::clone(&self.started_at);
        register_active_detector_lab_run(&cancel, &status, &out_path);
        std::thread::spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                run_lab(
                    &store,
                    &manual_status,
                    &status,
                    &cancel,
                    &started_at,
                    &out_path,
                    recipe,
                    duration_s,
                    point,
                )
            }))
            .map_err(panic_message)
            .and_then(|result| result);
            let tdr_event = crate::tdr_sentinel::new_tdr_event_since(
                tdr_baseline.as_deref(),
                session_started_epoch_ms,
            );
            if let Some(event) = tdr_event {
                crate::tdr_sentinel::mark_gpu_reboot_required(&event);
                finish_tdr_error(
                    &store,
                    &manual_status,
                    &status,
                    &out_path,
                    &event,
                    result.as_ref().err().cloned(),
                );
            } else {
                match result {
                    Ok(()) => {}
                    Err(error) => {
                        finish_environment_error(&store, &manual_status, &status, &out_path, error)
                    }
                }
            }
            clear_active_detector_lab_run(&cancel);
            running.store(false, Ordering::SeqCst);
            if let Ok(mut timer) = started_at.lock() {
                *timer = None;
            }
            if let Ok(mut status) = status.lock() {
                status.running = false;
            }
        });

        Ok(initial)
    }

    pub fn stop(&self) -> DetectorLabStatus {
        self.cancel.store(true, Ordering::SeqCst);
        if let Ok(mut status) = self.status.lock() {
            if status.running {
                status.stage = "stopping".into();
                status.note = "Stopping after the current preemptible GPU band.".into();
            }
        }
        self.status()
    }
}

#[allow(clippy::too_many_arguments)]
fn run_lab(
    store: &SafeLoopStore,
    manual_status: &ManualPointStatusSlot,
    status: &Arc<Mutex<DetectorLabStatus>>,
    cancel: &AtomicBool,
    started_at: &Arc<Mutex<Option<Instant>>>,
    out_path: &Path,
    recipe: DetectorRecipe,
    duration_s: u32,
    point: ManualDiagnosticPointStatus,
) -> Result<(), String> {
    manual_point::reset_and_disarm(store)?;
    let mut calibrating = point.clone();
    calibrating.active = false;
    calibrating.verified = false;
    calibrating.note =
        "Detector Lab is capturing a stock golden before reapplying this point.".into();
    manual_point::replace_status(manual_status, calibrating)?;
    append_journal(
        out_path,
        json!({ "event": "stock_calibration_start", "epoch_ms": now_epoch_ms() }),
    )?;

    let goldens = crate::gpu_power_sweep::capture_fsgl3_render_goldens()?;
    append_journal(
        out_path,
        json!({ "event": "stock_calibration_complete", "epoch_ms": now_epoch_ms() }),
    )?;
    if cancel.load(Ordering::SeqCst) {
        return finish_stopped(manual_status, status, out_path);
    }
    if recipe.requires_full_stock_control() {
        if recipe == DetectorRecipe::MatrixV27 {
            run_stock_control_v27(status, cancel, out_path, goldens)?;
        } else {
            run_stock_control_v25(
                status,
                cancel,
                out_path,
                u64::from(duration_s) * 1_000,
                goldens,
            )?;
        }
        if cancel.load(Ordering::SeqCst) {
            return finish_stopped(manual_status, status, out_path);
        }
    }

    let target_mhz = point.target_mhz.expect("validated before worker spawn");
    let requested_voltage_mv = point
        .requested_voltage_mv
        .expect("validated before worker spawn");
    let expected_resolved_mv = point
        .resolved_voltage_mv
        .expect("validated before worker spawn");
    let (resolved_voltage_mv, offset_mhz) =
        crate::gpu_undervolt::resolve_manual_diagnostic_point(target_mhz, requested_voltage_mv)?;
    if resolved_voltage_mv != expected_resolved_mv {
        return Err(format!(
            "Physical VF bin changed during stock calibration ({expected_resolved_mv} -> {resolved_voltage_mv} mV)"
        ));
    }
    if cancel.load(Ordering::SeqCst) {
        return finish_stopped(manual_status, status, out_path);
    }
    if recipe.uses_voltage_lock() {
        manual_point::apply_resolved_point(
            store,
            target_mhz,
            resolved_voltage_mv,
            offset_mhz,
            DETECTOR_LAB_PHASE,
        )?;
    } else {
        manual_point::apply_resolved_curve_point(
            store,
            target_mhz,
            resolved_voltage_mv,
            offset_mhz,
            DETECTOR_LAB_PHASE,
        )?;
    }
    if cancel.load(Ordering::SeqCst) {
        manual_point::reset_and_disarm(store)?;
        return finish_stopped(manual_status, status, out_path);
    }
    let active = ManualDiagnosticPointStatus {
        active: true,
        target_mhz: Some(target_mhz),
        requested_voltage_mv: Some(requested_voltage_mv),
        resolved_voltage_mv: Some(resolved_voltage_mv),
        applied_at_epoch_ms: Some(now_epoch_ms()),
        verified: true,
        note: "Detector Lab owns this temporary point while the recipe is running.".into(),
    };
    manual_point::replace_status(manual_status, active)?;
    if let Ok(mut timer) = started_at.lock() {
        *timer = Some(Instant::now());
    }
    update_status(status, |current| {
        current.stage = "running".into();
        current.elapsed_ms = 0;
        current.progress_pct = 0.0;
        current.note = "Failure-seeking recipe is running on the verified manual point.".into();
    });
    append_journal(
        out_path,
        json!({ "event": "point_reapplied", "epoch_ms": now_epoch_ms() }),
    )?;

    let run_started = Instant::now();
    append_journal(
        out_path,
        json!({
            "event": "candidate_recipe_start",
            "epoch_ms": now_epoch_ms(),
            "recipe": recipe.key(),
            "application_mode": recipe.application_mode(),
            "duration_ms": recipe.candidate_duration_ms(duration_s),
            "dx11_duration_ms": (recipe == DetectorRecipe::MatrixV27)
                .then_some(MATRIX_V27_DX11_DURATION_MS),
            "vulkan_duration_ms": (recipe == DetectorRecipe::MatrixV27)
                .then_some(u64::from(MATRIX_V27_API_DURATION_S) * 1_000),
            "dx12_duration_ms": (recipe == DetectorRecipe::MatrixV27)
                .then_some(u64::from(MATRIX_V27_API_DURATION_S) * 1_000),
            "endurance_duration_ms": (recipe == DetectorRecipe::MatrixV27)
                .then_some(MATRIX_V27_ENDURANCE_MS),
        }),
    )?;
    if recipe == DetectorRecipe::MatrixV27 {
        return run_candidate_matrix_v27(
            store,
            manual_status,
            status,
            cancel,
            out_path,
            target_mhz,
            requested_voltage_mv,
            resolved_voltage_mv,
            offset_mhz,
            goldens,
            run_started,
        );
    }
    let journal_error = Mutex::new(None::<String>);
    let run = if recipe == DetectorRecipe::Dx11Resident {
        update_status(status, |current| {
            current.current_segment = Some(1);
            current.current_phase = Some("dx11-resident".into());
            current.note =
                format!("Running clock-resident DX11 v3 diagnostic for {duration_s} seconds.");
        });
        append_journal(
            out_path,
            json!({
                "event": "resident_lane_start",
                "epoch_ms": now_epoch_ms(),
                "lane": "dx11_v3",
                "duration_ms": u64::from(duration_s) * 1_000,
            }),
        )?;
        crate::gpu_power_sweep::single_dx11_qualifier_dwell_with_cancel(
            u64::from(duration_s) * 1_000,
            target_mhz,
            goldens.dx11,
            Some(cancel),
        )
    } else {
        let mut hook = |index: usize, phase: VfQualifierPhase, segment_ms: u64| {
            record_segment_start(
                status,
                cancel,
                out_path,
                &journal_error,
                "candidate",
                index,
                phase,
                segment_ms,
            );
        };
        crate::gpu_power_sweep::single_qualifier_dwell_with_cancel_and_phase_hook(
            u64::from(duration_s) * 1_000,
            target_mhz,
            recipe.pattern(),
            goldens,
            Some(cancel),
            &mut hook,
        )
    };
    if let Some(error) = journal_error.lock().ok().and_then(|slot| slot.clone()) {
        return Err(error);
    }
    if run.cancelled || cancel.load(Ordering::SeqCst) {
        manual_point::reset_and_disarm(store)?;
        return finish_stopped(manual_status, status, out_path);
    }

    let workload_result = single_dwell_result(&run);
    let coverage = run.qualification_coverage.as_ref();
    let coverage_failed =
        coverage.is_some_and(|coverage| coverage.verdict == F2QualificationVerdict::Fail);
    let detected_failure = !workload_result.is_stable() || coverage_failed;
    let voltage_reason = (!detected_failure)
        .then(|| {
            voltage_inconclusive_reason(
                recipe,
                run.volt_sample_count,
                run.volt_max_mv,
                resolved_voltage_mv,
            )
        })
        .flatten();
    let anchor_voltage_escaped = run
        .volt_max_mv
        .is_some_and(|observed_mv| observed_mv > resolved_voltage_mv);
    let coverage_reason = (!detected_failure)
        .then(|| {
            coverage.and_then(|coverage| {
                (coverage.verdict != F2QualificationVerdict::Pass).then(|| {
                    coverage
                        .reason
                        .clone()
                        .unwrap_or_else(|| "qualification_coverage_inconclusive".into())
                })
            })
        })
        .flatten();
    let inconclusive_reason = voltage_reason.or(coverage_reason);
    let phase_results = coverage
        .map(phase_statuses_from_coverage)
        .unwrap_or_default();
    let checksum_count = coverage.map_or(0, |coverage| coverage.checksum_count);
    let frames = run.render_frames.unwrap_or_else(|| {
        coverage.map_or(0, |coverage| {
            coverage
                .phase_metrics
                .iter()
                .map(|phase| phase.frame_count)
                .sum()
        })
    });
    let failure_phase = coverage.and_then(|coverage| coverage.failure_phase.clone());
    let result = if coverage_failed && workload_result.is_stable() {
        "unstable"
    } else if detected_failure {
        result_key(workload_result)
    } else if inconclusive_reason.is_some() {
        "inconclusive"
    } else {
        "stable"
    };
    append_journal(
        out_path,
        json!({
            "event": "session_result",
            "epoch_ms": now_epoch_ms(),
            "result": result,
            "workload_result": result_key(workload_result),
            "failure_phase": failure_phase,
            "frames": frames,
            "checksum_count": checksum_count,
            "inconclusive_reason": inconclusive_reason,
            "application_mode": recipe.application_mode(),
            "anchor_voltage_escaped": anchor_voltage_escaped,
            "clock_mhz": {
                "avg": run.avg_clock_mhz,
                "p5": run.p5_clock_mhz,
                "p95": run.p95_clock_mhz,
                "target": target_mhz,
            },
            "voltage_mv": {
                "min": run.volt_min_mv,
                "avg": run.volt_avg_mv,
                "max": run.volt_max_mv,
                "samples": run.volt_sample_count,
                "anchor": resolved_voltage_mv,
                "ceiling": recipe.uses_voltage_lock().then_some(resolved_voltage_mv),
            },
            "power": {
                "avg_w": run.power_w,
                "max_w": run.max_power_w,
                "p99_w": run.power_p99_w,
                "capped_fraction": run.power_capped_frac,
            },
            "qualification_coverage": coverage,
            "phase_results": phase_results,
        }),
    )?;

    let should_reset =
        detected_failure || inconclusive_reason.is_some() || !recipe.uses_voltage_lock();
    if should_reset {
        manual_point::reset_and_disarm(store)?;
        manual_point::replace_status(
            manual_status,
            stock_manual_status(if detected_failure {
                "Detector Lab rejected the point and returned the GPU to stock."
            } else if inconclusive_reason.is_some() {
                "Detector Lab could not prove the selected point was exercised authoritatively and returned the GPU to stock."
            } else {
                "Detector Lab completed the curve-envelope diagnostic and returned the GPU to stock."
            }),
        )?;
    } else {
        update_manual_note(
            manual_status,
            "Detector Lab completed without a detected error. The temporary point remains active for another comparison or Game Trace.",
        );
    }
    update_status(status, |current| {
        current.stage = "finished".into();
        current.current_segment = None;
        current.current_phase = None;
        current.result = Some(result.into());
        current.failure_phase = failure_phase;
        current.frames = frames;
        current.checksum_count = checksum_count;
        current.phase_results = phase_results;
        current.elapsed_ms = run_started.elapsed().as_millis() as u64;
        current.progress_pct = if !detected_failure && inconclusive_reason.is_none() {
            100.0
        } else {
            (current.elapsed_ms as f32 * 100.0 / current.duration_ms.max(1) as f32)
                .clamp(0.0, 100.0)
        };
        current.note = if detected_failure {
            "The recipe rejected the point; the GPU was returned to stock without writing blacklist."
                .into()
        } else if let Some(reason) = inconclusive_reason {
            format!(
                "The point was not approved because authoritative coverage was inconclusive ({reason}); the GPU was returned to stock."
            )
        } else if anchor_voltage_escaped {
            format!(
                "No detector error was observed, but the curve selected voltage above the {} mV anchor (maximum {} mV). This diagnostic pass does not validate an exact clock-voltage pair.",
                resolved_voltage_mv,
                run.volt_max_mv.unwrap_or(resolved_voltage_mv),
            )
        } else {
            "No detector error was observed. This diagnostic pass does not qualify a profile."
                .into()
        };
    });
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum MatrixV27Lane {
    Vulkan,
    Dx11,
    Dx12,
    Endurance,
}

impl MatrixV27Lane {
    const ALL: [Self; 4] = [Self::Dx11, Self::Vulkan, Self::Dx12, Self::Endurance];

    fn label(self) -> &'static str {
        match self {
            Self::Vulkan => "vulkan",
            Self::Dx11 => "dx11_v3",
            Self::Dx12 => "dx12",
            Self::Endurance => "endurance",
        }
    }

    fn duration_ms(self) -> u64 {
        match self {
            Self::Dx11 => MATRIX_V27_DX11_DURATION_MS,
            Self::Endurance => MATRIX_V27_ENDURANCE_MS,
            Self::Vulkan | Self::Dx12 => u64::from(MATRIX_V27_API_DURATION_S) * 1_000,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_candidate_matrix_v27(
    store: &SafeLoopStore,
    manual_status: &ManualPointStatusSlot,
    status: &Arc<Mutex<DetectorLabStatus>>,
    cancel: &AtomicBool,
    out_path: &Path,
    target_mhz: u32,
    requested_voltage_mv: u32,
    resolved_voltage_mv: u32,
    offset_mhz: i32,
    goldens: RenderGoldens,
    run_started: Instant,
) -> Result<(), String> {
    let total_duration_ms =
        DetectorRecipe::MatrixV27.candidate_duration_ms(MATRIX_V27_API_DURATION_S);
    let mut completed_duration_ms = 0u64;
    let mut total_frames = 0u64;
    let mut total_checksums = 0u32;
    let mut all_phase_results = Vec::new();

    for (index, lane) in MatrixV27Lane::ALL.into_iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            manual_point::reset_and_disarm(store)?;
            return finish_stopped(manual_status, status, out_path);
        }
        if index > 0 {
            manual_point::apply_resolved_point(
                store,
                target_mhz,
                resolved_voltage_mv,
                offset_mhz,
                DETECTOR_LAB_PHASE,
            )?;
            manual_point::replace_status(
                manual_status,
                ManualDiagnosticPointStatus {
                    active: true,
                    target_mhz: Some(target_mhz),
                    requested_voltage_mv: Some(requested_voltage_mv),
                    resolved_voltage_mv: Some(resolved_voltage_mv),
                    applied_at_epoch_ms: Some(now_epoch_ms()),
                    verified: true,
                    note: format!("Detector Lab matrix owns the {} lane.", lane.label()),
                },
            )?;
            append_journal(
                out_path,
                json!({
                    "event": "point_reapplied",
                    "epoch_ms": now_epoch_ms(),
                    "lane": lane.label(),
                }),
            )?;
        }

        update_status(status, |current| {
            current.stage = "running_matrix_v27".into();
            current.current_segment = Some(u32::try_from(index + 1).unwrap_or(u32::MAX));
            current.current_phase = Some(lane.label().into());
            current.note = format!("Running matrix v27 lane {}/4: {}.", index + 1, lane.label());
        });
        append_journal(
            out_path,
            json!({
                "event": "matrix_lane_start",
                "epoch_ms": now_epoch_ms(),
                "lane_index": index + 1,
                "lane": lane.label(),
                "duration_ms": lane.duration_ms(),
            }),
        )?;

        let run = match lane {
            MatrixV27Lane::Vulkan => crate::gpu_power_sweep::single_qualifier_dwell_with_cancel(
                lane.duration_ms(),
                target_mhz,
                VfQualifierPattern::V8Texture,
                goldens,
                Some(cancel),
            ),
            MatrixV27Lane::Dx11 => crate::gpu_power_sweep::single_dx11_qualifier_dwell_with_cancel(
                lane.duration_ms(),
                target_mhz,
                goldens.dx11,
                Some(cancel),
            ),
            MatrixV27Lane::Dx12 => crate::gpu_power_sweep::single_dx12_qualifier_dwell_with_cancel(
                lane.duration_ms(),
                target_mhz,
                goldens.for_dx12(),
                Some(cancel),
            ),
            MatrixV27Lane::Endurance => crate::gpu_power_sweep::single_qualifier_dwell_with_cancel(
                lane.duration_ms(),
                target_mhz,
                VfQualifierPattern::Endurance,
                goldens,
                Some(cancel),
            ),
        };

        let workload_result = single_dwell_result(&run);
        let coverage = run.qualification_coverage.as_ref();
        let coverage_failed =
            coverage.is_some_and(|coverage| coverage.verdict == F2QualificationVerdict::Fail);
        let detected_failure = !workload_result.is_stable() || coverage_failed;
        let voltage_reason = (!detected_failure)
            .then(|| {
                voltage_inconclusive_reason(
                    DetectorRecipe::MatrixV27,
                    run.volt_sample_count,
                    run.volt_max_mv,
                    resolved_voltage_mv,
                )
            })
            .flatten();
        let coverage_reason = (!detected_failure)
            .then(|| match coverage {
                Some(coverage) if coverage.verdict == F2QualificationVerdict::Pass => None,
                Some(coverage) => Some(
                    coverage
                        .reason
                        .clone()
                        .unwrap_or_else(|| "qualification_coverage_inconclusive".into()),
                ),
                None => Some("qualification_coverage_missing".into()),
            })
            .flatten();
        let inconclusive_reason = voltage_reason.or(coverage_reason);
        let lane_result = if coverage_failed && workload_result.is_stable() {
            "unstable"
        } else if detected_failure {
            result_key(workload_result)
        } else if inconclusive_reason.is_some() {
            "inconclusive"
        } else {
            "stable"
        };
        let lane_failure_phase = coverage
            .and_then(|coverage| coverage.failure_phase.clone())
            .or_else(|| (lane_result != "stable").then(|| lane.label().to_string()));
        let lane_frames = run.render_frames.unwrap_or_else(|| {
            coverage.map_or(0, |coverage| {
                coverage
                    .phase_metrics
                    .iter()
                    .map(|phase| phase.frame_count)
                    .sum()
            })
        });
        let lane_checksums = coverage.map_or(0, |coverage| {
            coverage
                .checksum_count
                .saturating_add(coverage.compute_check_count)
        });
        total_frames = total_frames.saturating_add(lane_frames);
        total_checksums = total_checksums.saturating_add(lane_checksums);
        if let Some(coverage) = coverage {
            all_phase_results.extend(phase_statuses_from_coverage(coverage).into_iter().map(
                |mut phase| {
                    phase.phase = format!("{}:{}", lane.label(), phase.phase);
                    phase
                },
            ));
        }

        let lane_journal = append_journal(
            out_path,
            json!({
                "event": "matrix_lane_result",
                "epoch_ms": now_epoch_ms(),
                "lane_index": index + 1,
                "lane": lane.label(),
                "result": lane_result,
                "workload_result": result_key(workload_result),
                "failure_phase": lane_failure_phase,
                "frames": lane_frames,
                "checksum_count": lane_checksums,
                "inconclusive_reason": inconclusive_reason,
                "clock_mhz": {
                    "avg": run.avg_clock_mhz,
                    "p5": run.p5_clock_mhz,
                    "p95": run.p95_clock_mhz,
                    "target": target_mhz,
                },
                "voltage_mv": {
                    "min": run.volt_min_mv,
                    "avg": run.volt_avg_mv,
                    "max": run.volt_max_mv,
                    "samples": run.volt_sample_count,
                    "anchor": resolved_voltage_mv,
                    "ceiling": resolved_voltage_mv,
                },
                "power": {
                    "avg_w": run.power_w,
                    "max_w": run.max_power_w,
                    "p99_w": run.power_p99_w,
                    "capped_fraction": run.power_capped_frac,
                },
                "qualification_coverage": coverage,
            }),
        );
        let reset_start_journal = append_journal(
            out_path,
            json!({
                "event": "candidate_reset_start",
                "epoch_ms": now_epoch_ms(),
                "lane": lane.label(),
            }),
        );
        let reset_result = manual_point::reset_and_disarm(store);
        let reset_complete_journal = reset_result.as_ref().ok().map(|_| {
            append_journal(
                out_path,
                json!({
                    "event": "candidate_reset_complete",
                    "epoch_ms": now_epoch_ms(),
                    "lane": lane.label(),
                }),
            )
        });
        manual_point::replace_status(
            manual_status,
            stock_manual_status("Detector Lab matrix returned the GPU to stock between lanes."),
        )?;
        lane_journal?;
        reset_start_journal?;
        reset_result?;
        if let Some(reset_complete_journal) = reset_complete_journal {
            reset_complete_journal?;
        }

        completed_duration_ms = completed_duration_ms.saturating_add(lane.duration_ms());
        update_status(status, |current| {
            current.frames = total_frames;
            current.checksum_count = total_checksums;
            current.phase_results = all_phase_results.clone();
            current.failure_phase = lane_failure_phase.clone();
            current.progress_pct = (completed_duration_ms as f32 * 100.0
                / total_duration_ms.max(1) as f32)
                .clamp(0.0, 100.0);
        });

        if run.cancelled || cancel.load(Ordering::SeqCst) {
            return finish_stopped(manual_status, status, out_path);
        }
        if lane_result != "stable" {
            append_journal(
                out_path,
                json!({
                    "event": "session_result",
                    "epoch_ms": now_epoch_ms(),
                    "result": lane_result,
                    "failure_phase": lane_failure_phase,
                    "failed_lane": lane.label(),
                    "frames": total_frames,
                    "checksum_count": total_checksums,
                }),
            )?;
            update_status(status, |current| {
                current.stage = "finished".into();
                current.current_segment = None;
                current.current_phase = None;
                current.result = Some(lane_result.into());
                current.failure_phase = lane_failure_phase;
                current.elapsed_ms = run_started.elapsed().as_millis() as u64;
                current.note = format!(
                    "matrix_v27 stopped fail-closed at {} with {}.",
                    lane.label(),
                    lane_result
                );
            });
            return Ok(());
        }
    }

    append_journal(
        out_path,
        json!({
            "event": "session_result",
            "epoch_ms": now_epoch_ms(),
            "result": "stable",
            "frames": total_frames,
            "checksum_count": total_checksums,
            "lanes_completed": MatrixV27Lane::ALL.len(),
        }),
    )?;
    update_status(status, |current| {
        current.stage = "finished".into();
        current.current_segment = None;
        current.current_phase = None;
        current.result = Some("stable".into());
        current.failure_phase = None;
        current.frames = total_frames;
        current.checksum_count = total_checksums;
        current.phase_results = all_phase_results;
        current.elapsed_ms = run_started.elapsed().as_millis() as u64;
        current.progress_pct = 100.0;
        current.note = "matrix_v27 completed DX11 residency, Vulkan, DX12 and Endurance; this diagnostic run did not publish a profile.".into();
    });
    Ok(())
}

fn run_stock_control_v27(
    status: &Arc<Mutex<DetectorLabStatus>>,
    cancel: &AtomicBool,
    out_path: &Path,
    goldens: RenderGoldens,
) -> Result<(), String> {
    update_status(status, |current| {
        current.stage = "validating_stock_matrix_v27".into();
        current.current_segment = None;
        current.current_phase = None;
        current.progress_pct = 0.0;
        current.note =
            "Running 60-second stock controls for Vulkan, DX11 v3 and DX12 before the candidate."
                .into();
    });
    append_journal(
        out_path,
        json!({
            "event": "stock_matrix_start",
            "epoch_ms": now_epoch_ms(),
            "lanes": ["vulkan", "dx11_v3", "dx12"],
            "duration_ms_per_lane": MATRIX_V27_STOCK_LANE_MS,
        }),
    )?;
    if cancel.load(Ordering::SeqCst) {
        return Ok(());
    }
    let mut on_lane_start = |index: usize, lane: &'static str| {
        update_status(status, |current| {
            current.current_segment = Some(u32::try_from(index + 1).unwrap_or(u32::MAX));
            current.current_phase = Some(lane.into());
            current.progress_pct = (index as f32 * 100.0 / 3.0).clamp(0.0, 99.5);
            current.note = format!(
                "Running stock matrix lane {}/3: {lane} (60 seconds).",
                index + 1
            );
        });
    };
    let result = crate::gpu_power_sweep::validate_v27_api_matrix_stock(
        goldens,
        MATRIX_V27_STOCK_LANE_MS,
        Some(&mut on_lane_start),
    );
    append_journal(
        out_path,
        json!({
            "event": "stock_matrix_result",
            "epoch_ms": now_epoch_ms(),
            "result": if result.is_ok() { "stable" } else { "environment_error" },
            "error": result.as_ref().err(),
        }),
    )?;
    result
}

fn run_stock_control_v25(
    status: &Arc<Mutex<DetectorLabStatus>>,
    cancel: &AtomicBool,
    out_path: &Path,
    duration_ms: u64,
    goldens: RenderGoldens,
) -> Result<(), String> {
    update_status(status, |current| {
        current.stage = "validating_stock_recipe".into();
        current.current_segment = None;
        current.current_phase = None;
        current.progress_pct = 0.0;
        current.note =
            "Running the complete v25 sequence at stock before testing the candidate.".into();
    });
    append_journal(
        out_path,
        json!({
            "event": "stock_recipe_start",
            "epoch_ms": now_epoch_ms(),
            "recipe": "control_v25",
            "duration_ms": duration_ms,
        }),
    )?;

    let ctx = GpuCtx::new()
        .map_err(|error| format!("Detector Lab full stock control init failed: {error}"))?;
    let phase_state = AtomicU8::new(VfQualifierPhase::NONE_CODE);
    let journal_error = Mutex::new(None::<String>);
    let mut hook = |index: usize, phase: VfQualifierPhase, segment_ms: u64| {
        record_segment_start(
            status,
            cancel,
            out_path,
            &journal_error,
            "stock",
            index,
            phase,
            segment_ms,
        );
    };
    let run = ctx.run_vf_qualifier_stress_with_segment_hook(
        duration_ms,
        &phase_state,
        VfQualifierPattern::V8Texture,
        Some(goldens),
        Some(cancel),
        &mut hook,
    );
    if let Some(error) = journal_error.lock().ok().and_then(|slot| slot.clone()) {
        return Err(error);
    }

    let phase_results = phase_statuses(&run.phase_reports);
    let checksum_count: u32 = phase_results.iter().map(|phase| phase.checksum_count).sum();
    let inconclusive_reason = run.inconclusive_reason.clone();
    append_journal(
        out_path,
        json!({
            "event": "stock_recipe_result",
            "epoch_ms": now_epoch_ms(),
            "result": result_key(run.result),
            "failure_phase": run.failure_phase.map(VfQualifierPhase::label),
            "frames": run.frames,
            "checksum_count": checksum_count,
            "inconclusive_reason": inconclusive_reason,
            "phase_results": phase_results,
        }),
    )?;
    if cancel.load(Ordering::SeqCst) {
        return Ok(());
    }
    if let Some(reason) = run.inconclusive_reason {
        return Err(format!(
            "Full v25 stock control was inconclusive at the environment level: {reason}; candidate was not applied"
        ));
    }
    if run.result != StabilityResult::Stable {
        return Err(format!(
            "Full v25 stock control failed during {} with {:?}; candidate was not applied",
            run.failure_phase
                .map(VfQualifierPhase::label)
                .unwrap_or("unknown phase"),
            run.result
        ));
    }
    update_status(status, |current| {
        current.stage = "reapplying_point".into();
        current.current_segment = None;
        current.current_phase = None;
        current.note =
            "Full v25 stock control passed. Reapplying the verified candidate point.".into();
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn record_segment_start(
    status: &Arc<Mutex<DetectorLabStatus>>,
    cancel: &AtomicBool,
    out_path: &Path,
    journal_error: &Mutex<Option<String>>,
    scope: &str,
    index: usize,
    phase: VfQualifierPhase,
    segment_ms: u64,
) {
    update_status(status, |current| {
        current.current_segment = Some(index as u32 + 1);
        current.current_phase = Some(phase.label().into());
    });
    if let Err(error) = append_journal(
        out_path,
        json!({
            "event": "segment_start",
            "epoch_ms": now_epoch_ms(),
            "segment": index + 1,
            "phase": phase.label(),
            "planned_duration_ms": segment_ms,
            "scope": scope,
        }),
    ) {
        if let Ok(mut slot) = journal_error.lock() {
            *slot = Some(error);
        }
        cancel.store(true, Ordering::SeqCst);
    }
}

fn finish_stopped(
    manual_status: &ManualPointStatusSlot,
    status: &Arc<Mutex<DetectorLabStatus>>,
    out_path: &Path,
) -> Result<(), String> {
    manual_point::replace_status(
        manual_status,
        stock_manual_status("Detector Lab was stopped; the GPU is at stock."),
    )?;
    append_journal(
        out_path,
        json!({ "event": "session_stopped", "epoch_ms": now_epoch_ms() }),
    )?;
    update_status(status, |current| {
        current.stage = "stopped".into();
        current.current_segment = None;
        current.current_phase = None;
        current.result = Some("stopped".into());
        current.note = "Detector Lab stopped and returned the GPU to stock.".into();
    });
    Ok(())
}

fn finish_environment_error(
    store: &SafeLoopStore,
    manual_status: &ManualPointStatusSlot,
    status: &Arc<Mutex<DetectorLabStatus>>,
    out_path: &Path,
    error: String,
) {
    let recovery = manual_point::reset_and_disarm(store);
    let recovery_ok = recovery.is_ok();
    let recovery_note = match &recovery {
        Ok(()) => "GPU returned to stock".to_string(),
        Err(reset_error) => format!("stock recovery failed: {reset_error}"),
    };
    if recovery_ok {
        let _ = manual_point::replace_status(
            manual_status,
            stock_manual_status(&format!("Detector Lab error; {recovery_note}.")),
        );
    } else if let Ok(mut manual) = manual_status.lock() {
        manual.active = true;
        manual.verified = false;
        manual.note = "Detector Lab error and automatic stock recovery failed. Use Reset GPU tuning before another test.".into();
    }
    let _ = append_journal(
        out_path,
        json!({
            "event": "environment_error",
            "epoch_ms": now_epoch_ms(),
            "error": error,
            "recovery": recovery_note,
        }),
    );
    update_status(status, |current| {
        current.stage = "environment_error".into();
        current.current_segment = None;
        current.current_phase = None;
        current.result = Some("environment_error".into());
        current.note = format!("{error} {recovery_note}.");
    });
}

fn finish_tdr_error(
    store: &SafeLoopStore,
    manual_status: &ManualPointStatusSlot,
    status: &Arc<Mutex<DetectorLabStatus>>,
    out_path: &Path,
    event: &str,
    worker_error: Option<String>,
) {
    let failure_phase = status
        .lock()
        .ok()
        .and_then(|current| current.current_phase.clone());
    let recovery = manual_point::reset_and_disarm(store);
    let recovery_note = match &recovery {
        Ok(()) => "GPU returned to stock".to_string(),
        Err(reset_error) => format!("stock recovery failed: {reset_error}"),
    };
    if recovery.is_ok() {
        let _ = manual_point::replace_status(
            manual_status,
            stock_manual_status(
                "Detector Lab observed a driver reset. Reboot Windows before another GPU test.",
            ),
        );
    } else if let Ok(mut manual) = manual_status.lock() {
        manual.active = true;
        manual.verified = false;
        manual.note = "Driver reset detected and automatic stock recovery failed. Reboot Windows before another GPU operation.".into();
    }
    let _ = append_journal(
        out_path,
        json!({
            "event": "tdr_detected",
            "epoch_ms": now_epoch_ms(),
            "result": "tdr",
            "failure_phase": failure_phase,
            "tdr_event_ts": event,
            "worker_error": worker_error,
            "reboot_required": true,
            "recovery": recovery_note,
        }),
    );
    update_status(status, |current| {
        current.stage = "reboot_required".into();
        current.failure_phase = failure_phase;
        current.current_segment = None;
        current.current_phase = None;
        current.result = Some("tdr".into());
        current.note = format!(
            "Windows recorded a GPU driver reset at {event}. {recovery_note}; reboot is required before another test."
        );
    });
}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    let detail = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|message| (*message).to_string())
        })
        .unwrap_or_else(|| "unknown panic payload".into());
    format!("Detector Lab worker panicked: {detail}")
}

fn phase_statuses(reports: &[VfPhaseReport]) -> Vec<DetectorLabPhaseStatus> {
    reports
        .iter()
        .map(|report| DetectorLabPhaseStatus {
            phase: report.phase.label().into(),
            result: result_key(report.result).into(),
            duration_ms: report.elapsed_ms,
            frames: report.frames,
            checksum_count: report.checksum_count,
        })
        .collect()
}

fn phase_statuses_from_coverage(coverage: &F2QualificationCoverage) -> Vec<DetectorLabPhaseStatus> {
    coverage
        .phase_metrics
        .iter()
        .map(|phase| DetectorLabPhaseStatus {
            phase: phase.phase_name.clone(),
            result: match phase.coverage_status.as_str() {
                "pass" => "stable".into(),
                "fail" => "unstable".into(),
                other => other.into(),
            },
            duration_ms: phase.duration_ms,
            frames: phase.frame_count,
            checksum_count: phase.checksum_count,
        })
        .collect()
}

fn single_dwell_result(run: &crate::gpu_power_sweep::SingleDwell) -> StabilityResult {
    if run.crashed {
        StabilityResult::Crash
    } else if run.silent_error {
        StabilityResult::SilentError
    } else if run.stable {
        StabilityResult::Stable
    } else {
        StabilityResult::Unstable
    }
}

fn result_key(result: StabilityResult) -> &'static str {
    match result {
        StabilityResult::Stable => "stable",
        StabilityResult::SilentError => "silent_error",
        StabilityResult::Unstable => "unstable",
        StabilityResult::Crash => "crash",
    }
}

fn voltage_inconclusive_reason(
    recipe: DetectorRecipe,
    sample_count: u32,
    max_mv: Option<u32>,
    resolved_voltage_mv: u32,
) -> Option<String> {
    if sample_count < 3 {
        return Some("voltage_telemetry_low".into());
    }
    let Some(observed_mv) = max_mv else {
        return Some("voltage_telemetry_missing".into());
    };
    if recipe.uses_voltage_lock() && observed_mv > resolved_voltage_mv {
        return Some(format!(
            "voltage_ceiling_exceeded:{observed_mv}>{resolved_voltage_mv}"
        ));
    }
    None
}

fn update_status(
    status: &Arc<Mutex<DetectorLabStatus>>,
    update: impl FnOnce(&mut DetectorLabStatus),
) {
    if let Ok(mut status) = status.lock() {
        update(&mut status);
    }
}

fn set_status(
    status: &Arc<Mutex<DetectorLabStatus>>,
    next: DetectorLabStatus,
) -> Result<(), String> {
    *status
        .lock()
        .map_err(|_| "Detector Lab status lock is poisoned".to_string())? = next;
    Ok(())
}

fn update_manual_note(status: &ManualPointStatusSlot, note: &str) {
    if let Ok(mut status) = status.lock() {
        status.note = note.into();
    }
}

fn stock_manual_status(note: &str) -> ManualDiagnosticPointStatus {
    ManualDiagnosticPointStatus {
        note: note.into(),
        ..ManualDiagnosticPointStatus::default()
    }
}

fn create_journal_path() -> Result<PathBuf, String> {
    let data_dir = nidavellir_core::safe_loop::default_data_dir();
    fs::create_dir_all(&data_dir)
        .map_err(|error| format!("Detector Lab could not create the data directory: {error}"))?;
    Ok(data_dir.join(format!("detector-lab-{}.jsonl", now_epoch_ms())))
}

fn append_journal(path: &Path, value: serde_json::Value) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("Detector Lab journal open failed: {error}"))?;
    serde_json::to_writer(&mut file, &value)
        .map_err(|error| format!("Detector Lab journal encode failed: {error}"))?;
    file.write_all(b"\n")
        .map_err(|error| format!("Detector Lab journal write failed: {error}"))?;
    file.sync_data()
        .map_err(|error| format!("Detector Lab journal sync failed: {error}"))
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detector_recipes_are_an_explicit_allowlist() {
        assert!(matches!(
            DetectorRecipe::parse("matrix_v27").unwrap(),
            DetectorRecipe::MatrixV27
        ));
        assert!(matches!(
            DetectorRecipe::parse("matrix_v26").unwrap(),
            DetectorRecipe::MatrixV27
        ));
        assert!(matches!(
            DetectorRecipe::parse("control_v25").unwrap(),
            DetectorRecipe::ControlV25
        ));
        assert!(matches!(
            DetectorRecipe::parse("control_v24").unwrap(),
            DetectorRecipe::ControlV25
        ));
        assert!(matches!(
            DetectorRecipe::parse("control_v23").unwrap(),
            DetectorRecipe::ControlV25
        ));
        assert!(matches!(
            DetectorRecipe::parse("curve_v25").unwrap(),
            DetectorRecipe::CurveV25
        ));
        assert!(matches!(
            DetectorRecipe::parse("dense_v14").unwrap(),
            DetectorRecipe::DenseV14
        ));
        assert!(DetectorRecipe::parse("custom").is_err());
        assert!(DetectorRecipe::ControlV25.requires_full_stock_control());
        assert!(DetectorRecipe::CurveV25.requires_full_stock_control());
        assert!(DetectorRecipe::MatrixV27.requires_full_stock_control());
        assert!(!DetectorRecipe::DenseV14.requires_full_stock_control());
        assert!(DetectorRecipe::ControlV25.uses_voltage_lock());
        assert!(DetectorRecipe::MatrixV27.uses_voltage_lock());
        assert!(!DetectorRecipe::CurveV25.uses_voltage_lock());
        assert_eq!(
            DetectorRecipe::CurveV25.application_mode(),
            "anchored_curve"
        );
        assert_eq!(
            DetectorRecipe::MatrixV27.candidate_duration_ms(MATRIX_V27_API_DURATION_S),
            960_000
        );
    }

    #[test]
    fn matrix_status_reports_live_progress_inside_a_lane() {
        let handle = DetectorLabHandle::default();
        *handle.status.lock().unwrap() = DetectorLabStatus {
            running: true,
            stage: "running_matrix_v27".into(),
            duration_ms: 1_000,
            ..DetectorLabStatus::default()
        };
        *handle.started_at.lock().unwrap() =
            Some(Instant::now() - std::time::Duration::from_millis(500));

        let snapshot = handle.status();

        assert!(snapshot.elapsed_ms >= 500);
        assert!((50.0..=60.0).contains(&snapshot.progress_pct));
    }

    #[test]
    fn active_tdr_handoff_requests_cooperative_stop_without_learning() {
        let cancel = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(DetectorLabStatus {
            running: true,
            stage: "running".into(),
            ..DetectorLabStatus::default()
        }));
        let path = std::env::temp_dir().join(format!(
            "nid-detector-tdr-handoff-{}-{}.jsonl",
            std::process::id(),
            now_epoch_ms()
        ));
        register_active_detector_lab_run(&cancel, &status, &path);

        assert!(request_active_tdr_stop("2026-08-04T20:27:11Z"));
        assert!(cancel.load(Ordering::SeqCst));
        let snapshot = status.lock().unwrap().clone();
        assert_eq!(snapshot.stage, "tdr_detected");
        assert_eq!(snapshot.result.as_deref(), Some("tdr"));
        let journal = fs::read_to_string(&path).unwrap();
        assert!(journal.contains("tdr_stop_requested"));
        assert!(journal.contains("cooperative_cancel"));

        clear_active_detector_lab_run(&cancel);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn result_keys_match_ipc_contract() {
        assert_eq!(result_key(StabilityResult::Stable), "stable");
        assert_eq!(result_key(StabilityResult::SilentError), "silent_error");
        assert_eq!(result_key(StabilityResult::Unstable), "unstable");
        assert_eq!(result_key(StabilityResult::Crash), "crash");
    }

    #[test]
    fn curve_voltage_escape_is_fidelity_metadata_not_an_inconclusive_workload() {
        assert_eq!(
            voltage_inconclusive_reason(DetectorRecipe::ControlV25, 10, Some(900), 875),
            Some("voltage_ceiling_exceeded:900>875".into())
        );
        assert_eq!(
            voltage_inconclusive_reason(DetectorRecipe::CurveV25, 10, Some(900), 875),
            None
        );
        assert_eq!(
            voltage_inconclusive_reason(DetectorRecipe::CurveV25, 2, Some(875), 875),
            Some("voltage_telemetry_low".into())
        );
        assert_eq!(
            voltage_inconclusive_reason(DetectorRecipe::CurveV25, 10, None, 875),
            Some("voltage_telemetry_missing".into())
        );
    }

    #[test]
    fn panic_payload_is_preserved_for_the_journal() {
        let error = panic_message(Box::new("device lost after field concurrency"));
        assert!(error.contains("device lost after field concurrency"));
    }

    #[test]
    fn segment_checkpoint_records_scope_before_gpu_work() {
        let path = std::env::temp_dir().join(format!(
            "nidavellir-detector-segment-{}-{}.jsonl",
            std::process::id(),
            now_epoch_ms()
        ));
        let status = Arc::new(Mutex::new(DetectorLabStatus::default()));
        let cancel = AtomicBool::new(false);
        let journal_error = Mutex::new(None);

        record_segment_start(
            &status,
            &cancel,
            &path,
            &journal_error,
            "candidate",
            2,
            VfQualifierPhase::TextureRop,
            1_250,
        );

        let current = status.lock().unwrap().clone();
        assert_eq!(current.current_segment, Some(3));
        assert_eq!(current.current_phase.as_deref(), Some("texture-rop"));
        assert!(!cancel.load(Ordering::SeqCst));
        assert!(journal_error.lock().unwrap().is_none());
        let row: serde_json::Value =
            serde_json::from_str(std::fs::read_to_string(&path).unwrap().trim()).unwrap();
        assert_eq!(row["event"], "segment_start");
        assert_eq!(row["scope"], "candidate");
        assert_eq!(row["segment"], 3);
        assert_eq!(row["planned_duration_ms"], 1_250);
        let _ = std::fs::remove_file(path);
    }
}
