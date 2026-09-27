//! Explicit one-point DX11 investigation. Reuses the F2 transaction; never learns a profile.
use super::{
    F2DwellOutcome, F2DwellResult, F2Ops, F2Outcome, F2QualificationPattern, F2StressPurpose,
    PositiveOffsetVerification, RealF2Ops, UndervoltMode,
};
use nidavellir_core::safe_loop::SafeLoopStore;
use nidavellir_gpu_nvapi::{self as gpu, AnchoredPositiveOffsetPlan};
use serde_json::json;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

const TARGET_MHZ: u32 = 1830;
const ANCHOR_MV: u32 = 943;
const DWELL_MS: u64 = 420_000;
const LOAD_LEVELS: [u32; 5] = [100, 75, 50, 25, 100];
const LOAD_PHASE_MS: u64 = 30_000;
const USAGE: &str = "diagnose-f2-point or diagnose-f2-loads --confirm --reason <review reason>: one 1830 MHz / 943 mV point, 120 s stock preheat + 420 s DX11 or five 30 s duty-cycle phases (100/75/50/25/100), automatic stock cleanup; no profile/search";

// Requested duty cycle of checked work windows, not a claim about GPU utilization.
fn idle_for_work(work: Duration, duty: u32) -> Duration {
    work.mul_f64(f64::from(100 - duty) / f64::from(duty))
}

fn reason(args: &[OsString]) -> Result<&str, String> {
    if args.len() != 5 || args[2] != "--confirm" || args[3] != "--reason" {
        return Err(USAGE.into());
    }
    args[4]
        .to_str()
        .filter(|s| (8..=500).contains(&s.trim().len()))
        .ok_or_else(|| "Review reason must contain 8..=500 UTF-8 bytes".into())
}

fn append(path: &Path, mut value: serde_json::Value) -> Result<(), String> {
    value["at"] = json!(chrono::Utc::now().to_rfc3339());
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, &value).map_err(|e| e.to_string())?;
    file.write_all(b"\n")
        .and_then(|_| file.sync_data())
        .map_err(|e| e.to_string())
}

fn snapshot(
    path: &Path,
    event: &str,
    plan: &AnchoredPositiveOffsetPlan,
    full: bool,
) -> Result<(), String> {
    let mut points = Vec::new();
    let mut missing = false;
    for p in plan
        .entries
        .iter()
        .filter(|p| full || p.voltage_mv.abs_diff(ANCHOR_MV) <= 7)
    {
        let tuple = gpu::read_vf_point_snapshot(p.index);
        let offset = gpu::vf_get_point_khz(p.index);
        missing |= tuple.is_none() || offset.is_none();
        points.push(
            json!({"index":p.index,"planned_mv":p.voltage_mv,"planned_base_mhz":p.base_mhz,
            "planned_offset_mhz":p.offset_mhz,"planned_effective_mhz":p.effective_mhz,
            "base_live_tuple_mhz_mv":tuple,"read_offset_khz":offset}),
        );
    }
    append(
        path,
        json!({"event":event,"points":points,
        "voltage_lock_readback":format!("{:?}",gpu::read_core_voltage_locks())}),
    )?;
    if missing {
        Err("Diagnostic base/offset readback unavailable".into())
    } else {
        Ok(())
    }
}

struct DiagnosticOps<'a> {
    inner: RealF2Ops<'a>,
    path: PathBuf,
    stop: &'a AtomicBool,
    load_comparison: bool,
}

impl DiagnosticOps<'_> {
    fn compare_loads(&mut self, phase: &AtomicU32) -> F2DwellResult {
        use nidavellir_core::gpu_sweep::StabilityResult;
        let start = Instant::now();
        // This diagnostic never grants qualification. Raw per-phase evidence is in its journal.
        let mut result = F2DwellResult {
            outcome: F2DwellOutcome::Inconclusive,
            inconclusive_reason: Some("diagnostic_load_comparison_not_qualification".into()),
            avg_clock_mhz: 0, p5_clock_mhz: 0, p95_clock_mhz: 0,
            max_clock_mhz: 0,
            power_w: 0.0, max_power_w: 0.0, power_p99_w: None, power_capped_frac: 0.0,
            max_temp_c: None, thermal_throttled: false,
            measured_voltage_min_mv: None, measured_voltage_avg_mv: None,
            measured_voltage_max_mv: None, measured_voltage_sample_count: 0,
            render_frames: None, render_fps: None, duration_ms: 0, sample_count: 0,
            qualification_coverage: None, evidence_provenance: None,
        };
        let F2StressPurpose::ApplyQualification(_, goldens) = self.inner.stress_purpose else {
            return result;
        };
        let operation = (|| -> Result<(), String> {
            let ctx = nidavellir_gpu_stress::Dx11Qualifier::new()?;
            for (index, duty) in LOAD_LEVELS.into_iter().enumerate() {
                if self.stop.load(Ordering::SeqCst) { break; }
                snapshot(&self.path, "phase_curve", self.inner.anchored.as_ref().unwrap(), false)?;
                append(&self.path, json!({"event":"phase_start","phase":index + 1,"duty_pct":duty}))?;
                phase.store(index as u32 + 1, Ordering::SeqCst);
                let deadline = Instant::now() + Duration::from_millis(LOAD_PHASE_MS);
                let mut frames = 0;
                let mut checks = 0;
                let mut compute_checks = 0;
                while Instant::now() < deadline && !self.stop.load(Ordering::SeqCst) {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    let window = if duty == 100 { remaining } else { remaining.min(Duration::from_millis(100)) };
                    if window.as_millis() == 0 { break; }
                    let active_start = Instant::now();
                    let run = ctx.run_with_golden(window.as_millis() as u64, goldens.dx11, Some(self.stop));
                    let active = active_start.elapsed();
                    frames += run.frames;
                    checks += run.checks;
                    compute_checks += run.compute_checks;
                    result.outcome = match run.result {
                        StabilityResult::Stable => F2DwellOutcome::Inconclusive,
                        StabilityResult::SilentError => F2DwellOutcome::SilentError,
                        StabilityResult::Unstable => F2DwellOutcome::Unstable,
                        StabilityResult::Crash => F2DwellOutcome::DeviceLost,
                    };
                    if !run.result.is_stable() || run.inconclusive_reason.is_some() {
                        result.inconclusive_reason = run.inconclusive_reason.clone();
                        append(&self.path, json!({"event":"phase_failure","phase":index+1,
                            "result":format!("{:?}",run.result),"reason":run.inconclusive_reason}))?;
                        self.stop.store(true, Ordering::SeqCst);
                        break;
                    }
                    // Each window drains and checks every submitted batch before idling.
                    let idle_end = (Instant::now() + idle_for_work(active, duty)).min(deadline);
                    while Instant::now() < idle_end && !self.stop.load(Ordering::SeqCst) {
                        std::thread::sleep(idle_end.saturating_duration_since(Instant::now()).min(Duration::from_millis(10)));
                    }
                }
                phase.store(0, Ordering::SeqCst);
                append(&self.path, json!({"event":"phase_end","phase":index+1,"duty_pct":duty,
                    "frames":frames,"checks":checks,"compute_checks":compute_checks,"cancelled":self.stop.load(Ordering::SeqCst)}))?;
            }
            Ok(())
        })();
        if let Err(error) = operation {
            self.stop.store(true, Ordering::SeqCst);
            tracing::error!("Load diagnostic failed: {error}");
        }
        result.duration_ms = start.elapsed().as_millis() as u64;
        result
    }
}

impl Drop for DiagnosticOps<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let reset = self.inner.reset_to_stock();
            tracing::error!("Point diagnostic panicked; best-effort stock reset: {reset:?}; recovery flag retained");
        }
    }
}

impl F2Ops for DiagnosticOps<'_> {
    fn arm_boot_flag(&mut self) -> Result<(), String> {
        crate::gpu_apply::generic_hardware_write_preflight(
            self.inner.store,
            Some((TARGET_MHZ, ANCHOR_MV)),
        )?;
        snapshot(
            &self.path,
            "before_apply",
            self.inner.anchored.as_ref().unwrap(),
            true,
        )?;
        self.inner.arm_boot_flag()
    }
    fn apply_positive_offset(&mut self) -> Result<(), String> {
        self.inner.apply_positive_offset()?;
        snapshot(
            &self.path,
            "after_apply",
            self.inner.anchored.as_ref().unwrap(),
            true,
        )
    }
    fn verify(&mut self) -> PositiveOffsetVerification {
        self.inner.verify()
    }
    fn dwell(&mut self) -> F2DwellResult {
        let plan = self.inner.anchored.clone().unwrap();
        let path = self.path.clone();
        let stop = self.stop;
        let done = AtomicBool::new(false);
        let phase = AtomicU32::new(0);
        let load_comparison = self.load_comparison;
        std::thread::scope(|scope| {
            let done_ref = &done;
            let phase_ref = &phase;
            let observer = scope.spawn(move || -> Result<(), String> {
                let (sampler, _) = nidavellir_core::nvml_gpu::NvmlSampler::init(0).map_err(|error| {
                    stop.store(true, Ordering::SeqCst);
                    error
                })?;
                let start = Instant::now();
                let mut tick = 0;
                while !done_ref.load(Ordering::SeqCst) {
                    let phase_before = phase_ref.load(Ordering::SeqCst);
                    let sample = sampler.sample();
                    let voltage = gpu::read_core_voltage_mv();
                    let sample_phase = if phase_before == phase_ref.load(Ordering::SeqCst) { phase_before } else { 0 };
                    let event = crate::tdr_sentinel::reboot_required_event();
                    if event.is_some() || sample.temp_c.is_none_or(|t| t >= 80)
                        || (load_comparison && (voltage.is_none_or(|mv| !(500..=ANCHOR_MV).contains(&mv))
                            || sample.core_mhz.is_none() || sample.power_w.is_none())) {
                        stop.store(true, Ordering::SeqCst);
                    }
                    let result = append(&path, json!({"event":"sample","elapsed_ms":start.elapsed().as_millis(),
                        "clock_mhz":sample.core_mhz,"temperature_c":sample.temp_c,"power_w":sample.power_w,
                        "util_pct":sample.util_pct,"throttle_bits":sample.throttle_bits,
                        "tdr":event,"phase":sample_phase,"voltage_mv":voltage})).and_then(|_| {
                            if tick % (if load_comparison {100} else {10}) == 0 { snapshot(&path, "during_dwell", &plan, false) } else { Ok(()) }
                        });
                    if let Err(e) = result { stop.store(true, Ordering::SeqCst); return Err(e); }
                    tick += 1;
                    std::thread::park_timeout(Duration::from_millis(if load_comparison {100} else {1000}));
                }
                Ok(())
            });
            // Wake and join even during unwinding; never delay stock reset by a sleeping observer.
            struct Wake<'a>(&'a AtomicBool, std::thread::Thread);
            impl Drop for Wake<'_> {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::SeqCst);
                    self.1.unpark();
                }
            }
            let wake = Wake(&done, observer.thread().clone());
            let mut result = if load_comparison { self.compare_loads(&phase) } else { self.inner.dwell() };
            drop(wake);
            if !matches!(observer.join(), Ok(Ok(()))) && matches!(result.outcome, F2DwellOutcome::Stable | F2DwellOutcome::Inconclusive) {
                result.outcome = F2DwellOutcome::Inconclusive;
                result.qualification_coverage = None;
            }
            result
        })
    }
    fn reset_to_stock(&mut self) -> Result<(), String> {
        // Diagnostics must never prevent the actual cleanup.
        let before = snapshot(
            &self.path,
            "before_reset",
            self.inner.anchored.as_ref().unwrap(),
            true,
        );
        let reset = self.inner.reset_to_stock();
        let after = snapshot(
            &self.path,
            "after_reset",
            self.inner.anchored.as_ref().unwrap(),
            true,
        );
        for error in [before.err(), after.err()].into_iter().flatten() {
            tracing::error!("Diagnostic snapshot: {error}");
        }
        reset
    }
    fn clear_boot_flag(&mut self) -> Result<(), String> {
        self.inner.clear_boot_flag()
    }
    fn blacklist_point(&mut self, crash: bool) -> Result<(), String> {
        self.inner.blacklist_point(crash)
    }
}

pub(crate) fn run(args: &[OsString]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    let reason = reason(args)?;
    let load_comparison = args[1] == "diagnose-f2-loads";
    let store = SafeLoopStore::system();
    crate::start_supervised_cli_sentinel(&store)?;
    crate::development_validation::enable();
    crate::development_validation::authorize_point_diagnostic(
        &store,
        reason,
        (TARGET_MHZ, ANCHOR_MV),
    )?;
    struct Finish;
    impl Drop for Finish {
        fn drop(&mut self) {
            crate::development_validation::finish("point diagnostic ended", None);
        }
    }
    let _finish = Finish;
    crate::development_validation::claim()?;
    crate::gpu_apply::generic_hardware_write_preflight(&store, Some((TARGET_MHZ, ANCHOR_MV)))?;
    let path = crate::development_validation::audit_path()
        .ok_or("Missing audit path")?
        .with_extension("point.jsonl");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    println!("POINT_DIAGNOSTIC_JOURNAL={}", path.display());
    append(
        &path,
        json!({"event":"start","target_mhz":TARGET_MHZ,"anchor_mv":ANCHOR_MV,
            "dwell_ms":if load_comparison {LOAD_PHASE_MS * LOAD_LEVELS.len() as u64} else {DWELL_MS},
            "load_comparison":load_comparison,"publishable":false}),
    )?;
    let stop = AtomicBool::new(false);
    let goldens = crate::gpu_power_sweep::capture_fsgl3_render_goldens()?;
    let warm = crate::gpu_power_sweep::single_dx11_qualifier_dwell_with_cancel(
        120_000,
        TARGET_MHZ,
        goldens.dx11,
        Some(&stop),
    );
    append(
        &path,
        json!({"event":"stock_preheat","coverage":warm.qualification_coverage,"max_temp_c":warm.max_temp_c}),
    )?;
    if warm.cancelled
        || warm.crashed
        || warm.silent_error
        || warm.thermal_throttled
        || warm.max_temp_c.is_none_or(|t| t >= 80.0)
        || warm
            .qualification_coverage
            .as_ref()
            .is_none_or(|c| c.checksum_count == 0 || c.compute_check_count == 0)
    {
        return Err("Stock preheat did not complete cleanly; candidate not applied".into());
    }
    let ceiling = crate::gpu_power_sweep::f2_stock_clock_ceiling(&gpu::read_vf_curve_modern())?;
    let inputs = super::f2_forge_inputs(ceiling).ok_or("Missing sane base curve")?;
    let index =
        super::select_exact_apply_anchor_bin(&inputs.sane_base_curve, TARGET_MHZ, ANCHOR_MV)
            .ok_or("Exact anchor unavailable")?;
    let plan = gpu::plan_bounded_anchored_positive_offset(
        &inputs.sane_base_curve,
        index,
        TARGET_MHZ,
        0,
        &inputs.limits,
    )?;
    let mut ops = DiagnosticOps {
        inner: RealF2Ops {
            store: &store,
            curve: inputs.sane_base_curve,
            candidate: plan.anchor,
            anchored: Some(plan),
            mode: UndervoltMode::Anchored,
            limits: inputs.limits,
            target_mhz: TARGET_MHZ,
            prev_offset_mhz: 0,
            dwell_ms: DWELL_MS,
            stress_purpose: F2StressPurpose::ApplyQualification(
                F2QualificationPattern::Dx11Game,
                goldens,
            ),
            cancel: Some(&stop),
        },
        path: path.clone(),
        stop: &stop,
        load_comparison,
    };
    let report = super::run_confirmed_f2_step(&mut ops);
    append(
        &path,
        json!({"event":"result","outcome":format!("{:?}",report.outcome),
        "coverage":report.qualification_coverage,"reset_ok":report.reset_ok,"boot_flag_cleared":report.boot_flag_cleared,
        "report":format!("{report:#?}"),"publishable":false}),
    )?;
    println!(
        "POINT_DIAGNOSTIC_RESULT={:?}; reset={:?}; journal={}",
        report.outcome,
        report.reset_ok,
        path.display()
    );
    if !matches!(
        report.outcome,
        F2Outcome::Validated | F2Outcome::Inconclusive
    ) || report.reset_ok != Some(true)
        || !report.boot_flag_cleared
    {
        return Err(format!(
            "Point diagnostic ended with {:?}; review journal before any further work",
            report.outcome
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }
    #[test]
    fn comparison_pacing_preserves_work_and_has_finite_idle_windows() {
        let work = Duration::from_millis(100);
        assert_eq!(idle_for_work(work, 100), Duration::ZERO);
        assert_eq!(idle_for_work(work, 50), work);
        assert_eq!(idle_for_work(work, 25), Duration::from_millis(300));
        for duty in LOAD_LEVELS {
            assert!((25..=100).contains(&duty));
            let elapsed = work + idle_for_work(work, duty);
            assert!((work.as_secs_f64() / elapsed.as_secs_f64() - f64::from(duty) / 100.0).abs() < 1e-6);
        }
        assert!(reason(&args(&["service", "diagnose-f2-loads", "--confirm", "--reason", "bounded comparison approved"])).is_ok());
    }
    #[test]
    fn diagnostic_requires_explicit_confirmation_reason_and_no_extra_flags() {
        assert!(reason(&args(&[
            "service",
            "diagnose-f2-point",
            "--confirm",
            "--reason",
            "reviewed run"
        ]))
        .is_ok());
        for a in [
            args(&["service", "diagnose-f2-point"]),
            args(&[
                "service",
                "diagnose-f2-point",
                "--confirm",
                "--reason",
                "short",
            ]),
            args(&[
                "service",
                "diagnose-f2-point",
                "--confirm",
                "--reason",
                "reviewed run",
                "--repeat",
            ]),
            args(&[
                "service",
                "diagnose-f2-point",
                "--confirm",
                "--duration",
                "420",
            ]),
        ] {
            assert!(reason(&a).is_err());
        }
    }
}
