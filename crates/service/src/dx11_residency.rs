//! Bounded DX11 integrity phases and target exposure. No tuning writes or profile decisions.
use nidavellir_core::f2_observation::{
    F2ActiveTargetCoverage, F2ActiveTargetDiagnostics, F2ActiveClockPhase,
    F2ClockExcursion, F2ClockCurveSnapshot,
};
use nidavellir_core::gpu_sweep::StabilityResult;
use nidavellir_gpu_stress::{Dx11Golden, Dx11QualificationResult, Dx11Qualifier};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// (duty %, light frame, lane shares). Heavy opening/closing and heavy duty bursts stay. The light
/// phase paces one-instance frames at 50% duty like a frame-capped light game, so the pair runs
/// below the power cap (ExactApply37). Run f2-forge-1790544997509 ran it back-to-back: the GPU
/// stayed busy and was power-limited 65% of the time. It gets two shares because it is busy about
/// half the time.
const PHASES: [(u32, bool, u64); 6] =
    [(100, false, 1), (75, false, 1), (50, false, 1), (25, false, 1), (50, true, 2), (100, false, 1)];
const _: () = assert!(PHASES.len() as u32 == nidavellir_core::f2_observation::F2_DX11_PHASES);
const TARGET_EXPOSURE_MS: u64 = 30_000;
const ACTIVE_COVERAGE_MS: u64 = 60_000;
const SAMPLE_HALF_WIDTH_US: u64 = 15_000;
/// Consecutive batches of one phase are one load: their recorded gap is only the next batch's
/// submission. Duty idle and phase changes are far longer and stay separate.
const CONTIGUOUS_GAP_US: u64 = 1_000;

/// Phase end, as an offset from the lane start, splitting the lane by shares.
fn phase_end_ms(duration_ms: u64, index: usize) -> u64 {
    let shares = |phases: &[(u32, bool, u64)]| phases.iter().map(|p| p.2).sum::<u64>();
    duration_ms * shares(&PHASES[..=index]) / shares(&PHASES)
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Sample {
    pub start_us: u64,
    pub end_us: u64,
    pub clock_mhz: u32,
    pub voltage_mv: Option<u32>,
    pub temperature_c: Option<u32>,
    pub curve: Option<F2ClockCurveSnapshot>,
    /// SW power cap with power at the board limit; below-target time here counts as held.
    pub power_limited: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Work {
    start_us: u64,
    end_us: u64,
    exposure: bool,
    phase_index: usize,
}

#[derive(Default)]
pub(super) struct Evidence {
    work: Vec<Work>,
    completed: u32,
}

/// Keeps the existing total lane budget. Heavy opening/closing check integrity and power; duty
/// phases provide several work/idle ratios; the light phase exercises the target below the cap.
/// None of them changes the applied VF configuration.
pub(super) fn run(
    ctx: &Dx11Qualifier,
    duration_ms: u64,
    golden: Dx11Golden,
    cancel: Option<&AtomicBool>,
    origin: Instant,
    evidence: &mut Evidence,
    phase_changed: &mut dyn FnMut(bool),
) -> Dx11QualificationResult {
    let started = Instant::now();
    let mut total = Dx11QualificationResult {
        result: StabilityResult::Stable,
        frames: 0,
        checks: 0,
        compute_checks: 0,
        fps: 0.0,
        elapsed_ms: 0,
        timed_out: false,
        inconclusive_reason: None,
    };
    let cancelled = || cancel.is_some_and(|flag| flag.load(Ordering::SeqCst));
    for (index, (duty, light, _)) in PHASES.into_iter().enumerate() {
        phase_changed(duty == 100 && !light);
        let deadline = started + Duration::from_millis(phase_end_ms(duration_ms, index));
        let mut phase_checks = 0;
        while Instant::now() < deadline && !cancelled() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let window = if duty == 100 {
                remaining
            } else {
                remaining.min(Duration::from_millis(100))
            };
            if window.as_millis() == 0 {
                break;
            }
            let active_started = Instant::now();
            let mut work_started = None;
            let mut on_activity = |active| {
                let now = origin.elapsed().as_micros() as u64;
                if active {
                    work_started = Some(now);
                } else if let Some(start_us) = work_started.take() {
                    evidence.work.push(Work {
                        start_us,
                        end_us: now,
                        exposure: duty != 100 || light,
                        phase_index: index,
                    });
                }
            };
            let window_ms = window.as_millis() as u64;
            let run = if light {
                ctx.run_light_with_golden_observed(window_ms, golden, cancel, &mut on_activity)
            } else {
                ctx.run_with_golden_observed(window_ms, golden, cancel, &mut on_activity)
            };
            total.frames += run.frames;
            total.checks += run.checks;
            total.compute_checks += run.compute_checks;
            phase_checks += run.checks;
            total.result = run.result;
            total.timed_out |= run.timed_out;
            total.inconclusive_reason = run.inconclusive_reason;
            if !total.result.is_stable() || total.inconclusive_reason.is_some() || cancelled() {
                break;
            }
            let idle = active_started
                .elapsed()
                .mul_f64(f64::from(100 - duty) / f64::from(duty));
            let idle_end = (Instant::now() + idle).min(deadline);
            while Instant::now() < idle_end && !cancelled() {
                std::thread::sleep(
                    idle_end
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(10)),
                );
            }
        }
        if !total.result.is_stable() || total.inconclusive_reason.is_some() || cancelled() {
            break;
        }
        if phase_checks == 0 {
            total.inconclusive_reason = Some("dx11_phase_checks_missing".into());
            break;
        }
        evidence.completed += 1;
    }
    total.elapsed_ms = started.elapsed().as_millis() as u64;
    total.fps = total.frames as f64 / started.elapsed().as_secs_f64().max(f64::EPSILON);
    total
}

fn contiguous(work: &[Work]) -> Vec<Work> {
    let mut spans: Vec<Work> = Vec::with_capacity(work.len());
    for w in work {
        match spans.last_mut() {
            Some(last) if last.phase_index == w.phase_index
                && w.start_us.saturating_sub(last.end_us) <= CONTIGUOUS_GAP_US =>
                last.end_us = last.end_us.max(w.end_us),
            _ => spans.push(*w),
        }
    }
    spans
}

/// Sensor reads are sequential, not atomic. Credit only reads wholly inside a submitted-work span
/// (one batch, or back-to-back batches of one phase). Their time support is clipped to that span,
/// adjacent sample midpoints and ±15 ms. Consequently duty idle, phase crossings, sensor gaps and
/// duplicate reads cannot manufacture target exposure. A batch that finishes before the previous
/// checksum is hashed idles inside its interval (at most one checksum). Per-batch clipping capped
/// short light batches at ~18 s of credit per 70 s (run f2-forge-1790544997509). This is a bounded
/// sampled estimate, not continuous HW tracing.
pub(super) fn coverage(
    samples: &[Sample],
    evidence: &Evidence,
    target: u32,
    anchor: u32,
) -> F2ActiveTargetCoverage {
    let mut active_us = 0;
    let mut target_us = 0;
    let mut limited_us = 0;
    let mut count = 0;
    let mut window_index = 0;
    let upper_clock_exceeded = samples.iter().any(|s| s.clock_mhz > nidavellir_core::f2_observation::f2_clock_ceiling_mhz(target));
    let spans = contiguous(&evidence.work);
    let mut phases: Vec<_> = PHASES.iter().enumerate().map(|(index, (duty, light, _))| F2ActiveClockPhase {
        phase_index: index as u32 + 1, requested_duty_pct: *duty, light: *light,
        active_sample_count: 0, active_clock_max_mhz: None,
        observed_active_us: 0, target_active_us: 0, power_limited_active_us: 0, one_bin_below_active_us: 0,
        upper_sample_count: 0, upper_observed_us: 0, first_upper: None,
    }).collect();
    for (index, sample) in samples.iter().enumerate() {
        while window_index < spans.len() && spans[window_index].end_us < sample.end_us {
            window_index += 1;
        }
        let Some(work) = spans.get(window_index) else {
            break;
        };
        if sample.start_us < work.start_us || sample.end_us > work.end_us {
            continue;
        }

        let center = sample.start_us + (sample.end_us - sample.start_us) / 2;
        let previous_mid = index
            .checked_sub(1)
            .map(|i| (samples[i].end_us + sample.start_us) / 2)
            .unwrap_or(0);
        let next_mid = samples
            .get(index + 1)
            .map(|s| (sample.end_us + s.start_us) / 2)
            .unwrap_or(u64::MAX);
        let left = work
            .start_us
            .max(center.saturating_sub(SAMPLE_HALF_WIDTH_US))
            .max(previous_mid);
        let right = work
            .end_us
            .min(center.saturating_add(SAMPLE_HALF_WIDTH_US))
            .min(next_mid);
        let credit = right.saturating_sub(left);
        let phase = &mut phases[work.phase_index];
        phase.active_sample_count += 1;
        phase.active_clock_max_mhz = Some(phase.active_clock_max_mhz.unwrap_or(0).max(sample.clock_mhz));
        if sample.clock_mhz > target {
            phase.upper_sample_count += 1;
            phase.upper_observed_us += credit;
            phase.first_upper.get_or_insert(F2ClockExcursion {
                at_ms: sample.start_us / 1000, clock_mhz: sample.clock_mhz,
                voltage_mv: sample.voltage_mv, temperature_c: sample.temperature_c,
                curve: sample.curve,
            });
        }
        let in_band = nidavellir_core::f2_observation::f2_clock_in_target_band(sample.clock_mhz, target);
        let limited = sample.clock_mhz < target && sample.power_limited;
        let one_below = !in_band && !limited
            && nidavellir_core::f2_observation::f2_clock_held(sample.clock_mhz, target);
        if credit > 0 && sample.voltage_mv.is_some_and(|mv| (500..=anchor).contains(&mv)) {
            phase.observed_active_us += credit;
            if in_band { phase.target_active_us += credit; }
            if limited { phase.power_limited_active_us += credit; }
            if one_below { phase.one_bin_below_active_us += credit; }
        }
        // Clock containment covers all work even when voltage telemetry is unavailable.
        // Target exposure still requires the original middle-phase voltage authority.
        if !work.exposure || sample.voltage_mv.is_none_or(|mv| !(500..=anchor).contains(&mv)) {
            continue;
        }
        if credit == 0 {
            continue;
        }
        active_us += credit;
        count += 1;
        if in_band {
            target_us += credit;
        }
        if limited {
            limited_us += credit;
        }
    }
    let mut coverage = F2ActiveTargetCoverage {
        observed_active_ms: active_us / 1000,
        target_active_ms: target_us / 1000,
        power_limited_active_ms: limited_us / 1000,
        light_target_active_ms: phases.iter().filter(|p| p.light).map(|p| p.target_active_us).sum::<u64>() / 1000,
        required_target_ms: TARGET_EXPOSURE_MS,
        sample_count: count,
        phases_completed: evidence.completed,
        upper_clock_exceeded,
        heavy_target_proven: phases.iter().filter(|p| p.requested_duty_pct == 100 && !p.light).all(|p| {
            let held = p.target_active_us + p.power_limited_active_us + p.one_bin_below_active_us;
            p.observed_active_us >= 30_000_000 && held as f64 / p.observed_active_us as f64 >= 0.95
        }),
        diagnostics: None,
    };
    let reasons = refusals(&coverage).into_iter().map(str::to_owned).collect();
    coverage.diagnostics = Some(F2ActiveTargetDiagnostics {
        requested_max_mhz: target, anchor_mv: anchor, reasons,
        publication_power_ceiling_w: None, phases,
    });
    coverage
}

pub(super) fn refusal(c: &F2ActiveTargetCoverage) -> Option<&'static str> {
    refusals(c).into_iter().next()
}

fn refusals(c: &F2ActiveTargetCoverage) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if c.upper_clock_exceeded {
        reasons.push("dx11_upper_clock_exceeded");
    }
    if c.phases_completed != nidavellir_core::f2_observation::F2_DX11_PHASES { reasons.push("dx11_phases_incomplete"); }
    if c.observed_active_ms < ACTIVE_COVERAGE_MS { reasons.push("dx11_active_telemetry_low"); }
    let held = c.held_active_ms();
    if c.required_target_ms != TARGET_EXPOSURE_MS || held < TARGET_EXPOSURE_MS || held > c.observed_active_ms
        || c.observed_active_ms == 0 || c.sample_count == 0
        || held as f64 / (c.observed_active_ms as f64) < 0.35 {
        reasons.push("dx11_target_unexercised");
    }
    if !c.heavy_target_proven { reasons.push("heavy_clock_not_sustained"); }
    if c.light_target_active_ms < TARGET_EXPOSURE_MS { reasons.push("dx11_light_target_unexercised"); }
    reasons
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(start_us: u64, clock: u32) -> Sample {
        Sample {
            start_us,
            end_us: start_us + 1000,
            clock_mhz: clock,
            voltage_mv: Some(900),
            temperature_c: Some(65),
            curve: None,
            power_limited: false,
        }
    }
    #[test]
    fn power_limited_drops_are_held_but_reported_apart_from_target_exposure() {
        let evidence = Evidence { completed: 6, work: vec![
            Work { start_us: 0, end_us: 40_000_000, exposure: false, phase_index: 0 },
            Work { start_us: 40_000_000, end_us: 120_000_000, exposure: true, phase_index: 2 },
            Work { start_us: 120_000_000, end_us: 160_000_000, exposure: false, phase_index: 5 },
            Work { start_us: 160_000_000, end_us: 200_000_000, exposure: true, phase_index: 4 },
        ]};
        // Run 1790466472114, DX11 at 1920@937: the SW power cap was set on every sample and the
        // limiter clamped full-duty and duty-cycled bursts onto stock points (p50 1695 MHz); only
        // ~1.7 s ran at target. That is power limiting, not an unexercised or unstable pair. The
        // paced light phase (ExactApply37) is what exercises the exact target in DX11.
        let mut reads: Vec<_> = (0..200_000_000).step_by(30_000).map(|t| {
            let mut s = sample(t, 1695);
            s.power_limited = t < 160_000_000;
            if (40_000_000..42_000_000).contains(&t) || t >= 160_000_000 { s.clock_mhz = 1920; }
            s
        }).collect();
        let held = coverage(&reads, &evidence, 1920, 900);
        assert!(held.proves_target(), "{held:?}");
        assert!(held.light_target_active_ms >= 30_000);
        let duty = &held.diagnostics.as_ref().unwrap().phases[2];
        assert!(duty.target_active_us < 3_000_000 && duty.power_limited_active_us > 70_000_000);
        for s in &mut reads { s.power_limited = false; }
        let refused = coverage(&reads, &evidence, 1920, 900);
        assert!(!refused.proves_target());
        assert!(refused.diagnostics.unwrap().reasons.iter().any(|r| r == "heavy_clock_not_sustained"));
    }
    #[test]
    fn excursions_report_active_phase_context_without_idle_or_gap_inflation() {
        let evidence = Evidence { completed: 6, work: vec![
            Work { start_us: 0, end_us: 100_000, exposure: false, phase_index: 0 },
            Work { start_us: 200_000, end_us: 300_000, exposure: true, phase_index: 2 },
        ]};
        let mut heavy = sample(50_000, 1815);
        heavy.voltage_mv = None; // Missing rail data cannot hide a containment violation.
        let mut variable = sample(250_000, 1830);
        variable.curve = Some(F2ClockCurveSnapshot { captured_at_ms: 255, base_mhz: 1680,
            base_mv: 900, effective_mhz: 1830, effective_mv: 900, offset_khz: Some(150_000) });
        let c = coverage(&[heavy, sample(150_000, 2100), sample(199_500, 2200), variable],
            &evidence, 1800, 900);
        assert!(c.upper_clock_exceeded);
        assert_eq!(c.target_active_ms, 0);
        let d = c.diagnostics.unwrap();
        assert_eq!(d.phases[0].active_clock_max_mhz, Some(1815));
        assert_eq!(d.phases[0].upper_sample_count, 1);
        assert_eq!(d.phases[0].upper_observed_us, 30_000);
        assert_eq!(d.phases[2].active_clock_max_mhz, Some(1830));
        assert_eq!(d.phases[2].upper_sample_count, 1);
        assert_eq!(d.phases[2].upper_observed_us, 30_000);
        let event = d.phases[2].first_upper.unwrap();
        assert_eq!((event.at_ms, event.voltage_mv, event.temperature_c), (250, Some(900), Some(65)));
        assert_eq!(event.curve, variable.curve);
        assert_eq!(d.reasons, ["dx11_upper_clock_exceeded", "dx11_active_telemetry_low", "dx11_target_unexercised",
            "heavy_clock_not_sustained", "dx11_light_target_unexercised"]);
        let restored: F2ActiveTargetDiagnostics = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(restored, d);
    }
    #[test]
    fn idle_heavy_and_fence_crossing_samples_cannot_prove_the_target() {
        let evidence = Evidence {
            completed: 6,
            work: vec![
                Work {
                    start_us: 0,
                    end_us: 100_000,
                    exposure: false,
                    phase_index: 0,
                },
                Work {
                    start_us: 200_000,
                    end_us: 300_000,
                    exposure: true,
                    phase_index: 1,
                },
            ],
        };
        let reads = [
            sample(50_000, 1800),
            sample(150_000, 1800),
            sample(199_500, 1800),
            sample(250_000, 1785),
            sample(350_000, 1800),
        ];
        let c = coverage(&reads, &evidence, 1800, 900);
        assert_eq!(c.sample_count, 1);
        assert_eq!(c.target_active_ms, 0);
        assert_eq!(c.observed_active_ms, 30);
    }
    #[test]
    fn missing_voltage_and_sensor_gaps_do_not_manufacture_time() {
        let evidence = Evidence {
            completed: 6,
            work: vec![Work {
                start_us: 0,
                end_us: 10_000_000,
                exposure: true,
                phase_index: 1,
            }],
        };
        let mut missing = sample(2_000_000, 1800);
        missing.voltage_mv = None;
        let mut above = sample(3_000_000, 1800);
        above.voltage_mv = Some(906);
        let c = coverage(
            &[
                sample(1_000_000, 1800),
                missing,
                above,
                sample(9_000_000, 1800),
            ],
            &evidence,
            1800,
            900,
        );
        assert_eq!(c.target_active_ms, 60);
        assert_eq!(c.observed_active_ms, 60);
        assert_eq!(refusal(&c), Some("dx11_active_telemetry_low"));
    }
    #[test]
    fn exposure_requires_duration_fraction_complete_phases_and_no_upper_excursion() {
        let pass = F2ActiveTargetCoverage {
            observed_active_ms: 80_000,
            target_active_ms: 30_000,
            power_limited_active_ms: 0,
            light_target_active_ms: 30_000,
            required_target_ms: 30_000,
            sample_count: 3000,
            phases_completed: 6,
            upper_clock_exceeded: false, heavy_target_proven: true,
            diagnostics: None,
        };
        assert_eq!(refusal(&pass), None);
        let light_short = F2ActiveTargetCoverage { light_target_active_ms: 29_999, ..pass.clone() };
        assert_eq!(refusal(&light_short), Some("dx11_light_target_unexercised"));
        for (active, target, phases, upper, reason) in [
            (80_000, 29_999, 6, false, "dx11_target_unexercised"),
            (100_000, 30_000, 6, false, "dx11_target_unexercised"),
            (80_000, 30_000, 5, false, "dx11_phases_incomplete"),
            (80_000, 30_000, 6, true, "dx11_upper_clock_exceeded"),
        ] {
            let c = F2ActiveTargetCoverage {
                observed_active_ms: active,
                target_active_ms: target,
                phases_completed: phases,
                upper_clock_exceeded: upper,
                ..pass.clone()
            };
            assert_eq!(refusal(&c), Some(reason));
        }
    }
    #[test]
    fn heavy_target_proof_cannot_be_replaced_by_light_phase_visits() {
        let evidence = Evidence { completed: 6, work: vec![
            Work { start_us: 0, end_us: 40_000_000, exposure: false, phase_index: 0 },
            Work { start_us: 40_000_000, end_us: 120_000_000, exposure: true, phase_index: 2 },
            Work { start_us: 120_000_000, end_us: 160_000_000, exposure: false, phase_index: 5 },
            Work { start_us: 160_000_000, end_us: 200_000_000, exposure: true, phase_index: 4 },
        ]};
        let mut reads: Vec<_> = (0..200_000_000).step_by(30_000).map(|t| sample(t,1740)).collect();
        assert!(coverage(&reads,&evidence,1740,900).proves_target());
        for (i,s) in reads.iter_mut().enumerate() { if i % 2 == 0 { s.clock_mhz=1755; } }
        let envelope = coverage(&reads,&evidence,1740,900);
        assert!(envelope.proves_target(), "nominal/+15 mixture must credit actual heavy work");
        assert!(!envelope.upper_clock_exceeded);
        // Heavy sustain holds one bin below target, never two: 1740/1755 cannot qualify 1770.
        assert!(!coverage(&reads,&evidence,1770,900).proves_target(), "peaks cannot qualify a clock two bins up");
        for s in &mut reads { if s.start_us < 40_000_000 { s.clock_mhz=1680; } }
        let refused=coverage(&reads,&evidence,1740,900);
        assert!(!refused.proves_target());
        assert_eq!(refusal(&refused),Some("heavy_clock_not_sustained"));
        assert!(refused.target_active_ms >= 30_000);
    }

    #[test]
    fn reconstructed_work_trace_passes_but_idle_only_target_trace_does_not() {
        let evidence = Evidence {
            completed: 6,
            work: (0..1000)
                .map(|i| Work {
                    start_us: i * 200_000,
                    end_us: i * 200_000 + 100_000,
                    exposure: true,
                    phase_index: 1,
                })
                .collect(),
        };
        let active: Vec<_> = (0..200_000_000)
            .step_by(30_000)
            .map(|t| sample(t, 1800))
            .collect();
        let accepted = coverage(&active, &evidence, 1800, 900);
        assert!(!accepted.proves_target(), "middle-only exposure must not prove heavy load: {accepted:?}");
        assert_eq!(refusal(&accepted), Some("heavy_clock_not_sustained"));
        assert!(
            accepted.observed_active_ms <= 100_000,
            "cannot credit more than submitted work"
        );
        let idle_only: Vec<_> = active
            .iter()
            .map(|s| Sample {
                clock_mhz: if s.start_us % 200_000 < 100_000 {
                    1650
                } else {
                    1800
                },
                ..*s
            })
            .collect();
        let refused = coverage(&idle_only, &evidence, 1800, 900);
        assert_eq!(refused.target_active_ms, 0);
        assert_eq!(refusal(&refused), Some("dx11_target_unexercised"));
        let mut excursion = active;
        excursion[1].clock_mhz = 1815;
        let within_band = coverage(&excursion, &evidence, 1800, 900);
        assert!(!within_band.upper_clock_exceeded);
        assert_eq!(refusal(&within_band), Some("heavy_clock_not_sustained"));
        excursion[1].clock_mhz = 1830;
        let refused = coverage(&excursion, &evidence, 1800, 900);
        assert_eq!(refusal(&refused), Some("dx11_upper_clock_exceeded"));
    }

    #[test]
    fn paced_light_batches_credit_their_load_but_never_duty_idle() {
        // Run f2-forge-1790544997509: light batches lasted ~8.7 ms, and per-batch clipping credited
        // ~15 s of a phase at target. Paced: 12 back-to-back batches (~105 ms), then duty idle.
        let work = (0..120_000_000).step_by(220_000).flat_map(|t| (0..12).map(move |b| Work {
            start_us: t + b * 8_800, end_us: t + b * 8_800 + 8_700, exposure: true, phase_index: 4,
        }));
        let evidence = Evidence { completed: 6, work: work.collect() };
        let busy_ms: u64 = contiguous(&evidence.work).iter().map(|w| w.end_us - w.start_us).sum::<u64>() / 1000;
        let reads: Vec<_> = (0..120_000_000).step_by(30_500).map(|t| sample(t, 1920)).collect();
        let light = coverage(&reads, &evidence, 1920, 900).light_target_active_ms;
        assert!((45_000..=busy_ms).contains(&light), "light {light} ms of {busy_ms} ms load");

        let w = |start_us, end_us, phase_index| Work { start_us, end_us, exposure: true, phase_index };
        let spans = contiguous(&[w(0, 8_700, 4), w(8_800, 17_500, 4), w(17_500, 20_000, 5), w(60_000, 70_000, 5)]);
        assert_eq!(spans.iter().map(|s| (s.start_us, s.end_us)).collect::<Vec<_>>(),
            [(0, 17_500), (17_500, 20_000), (60_000, 70_000)], "phase changes and idle stay apart");
    }

    #[test]
    fn light_phase_gets_two_of_seven_lane_shares() {
        let ends: Vec<_> = (0..PHASES.len()).map(|i| phase_end_ms(420_000, i)).collect();
        assert_eq!(ends, [60_000, 120_000, 180_000, 240_000, 360_000, 420_000]);
    }
}
