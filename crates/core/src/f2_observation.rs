//! F2 discovery/learning store — records every F2 true-undervolt attempt with its full outcome, and
//! derives the learned F2 frontier the existing GPU profile classifiers consume.
//!
//! This is OBSERVATION / LEARNING data ONLY. It is NOT profile persistence and it NEVER applies,
//! persists, or promotes a selected profile. It records what an F2 attempt did (anchor, offset, caps,
//! verifier/dwell outcome, telemetry, safety outcome) so future runs can pick up where the last left
//! off, compute the minimum stable voltage per target, and hand a learned frontier to the existing
//! `synthesize_forge_profiles` classifier (via [`to_power_sweep_point`]) WITHOUT re-implementing
//! profile scoring.
//!
//! Persistence mirrors the [`crate::safe_loop::SafeLoopStore`] conventions (reuses
//! [`crate::safe_loop::default_data_dir`], serde, BOM-tolerant reads) but is APPEND-ONLY (JSONL — one
//! observation per line) because observations accumulate across runs rather than overwriting a single
//! record. Only the confirmed F2 motor appends; a dry-run never writes.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ipc::PowerSweepPoint;
use crate::safe_loop::default_data_dir;

/// The append-only F2 observation log filename under `default_data_dir()`.
pub const F2_OBSERVATIONS_FILE: &str = "f2_observations.jsonl";
/// Current homogeneous PowerRender discovery contract.
///
/// v4 preserves mean, sustained-p99 and sampled-peak power separately, validates suspicious p99
/// steps with reset-clean repeated dwells, and carries render/voltage telemetry so frontier
/// decisions and exact apply-margin-bin calibration use confirmed sustained-power evidence.
///
/// v5 (v13 plan): every discovery dwell runs under an absolute NVML max-clock ceiling at the
/// focus target, so the measured point IS the labeled point (p95 == target; the pre-v5 thermal
/// curve shift let every pair run +15/+30 MHz above its label). v4 clocks/powers describe
/// shifted regimes and cannot seed the new frontier.
///
/// v6 (2026-07-23): every candidate additionally uses a verified graphics-domain voltage lock at
/// its physical VF bin, and stable discovery requires p5 to reach the exact target instead of
/// accepting the adjacent lower boost bin. Pre-v6 evidence did not prove the labeled clock/voltage
/// pair was exercised authoritatively.
///
/// v7 (2026-08-11): discovery permits one adjacent physical boost bin of clock elasticity while
/// retaining authoritative voltage-lock/readback at the labeled anchor. A miss larger than one bin
/// remains `ClockDrop`; pre-v7 positives used the stricter clock-residency interpretation.
/// v10 (2026-09-28): the one hot bin below the target is held again (it had returned to exact).
/// Without it, every anchor below 937 mV read as ClockDrop when hot on the test 3060 Ti.
pub const F2_DISCOVERY_CONTRACT_VERSION: u32 = 10;

/// Explicit nominal-clock envelope, not permission to increase voltage or infer a higher profile.
/// Keep requesting the nominal NVML cap; qualify measured operation through nominal + 15 MHz.
pub const F2_CLOCK_UPPER_MARGIN_MHZ: u32 = 15;
pub fn f2_clock_ceiling_mhz(target: u32) -> u32 {
    target.saturating_add(F2_CLOCK_UPPER_MARGIN_MHZ)
}
pub fn f2_clock_in_target_band(clock: u32, target: u32) -> bool {
    (target..=f2_clock_ceiling_mhz(target)).contains(&clock)
}
/// A pair holds its target one physical bin below the nominal clock too. When hot, the GPU's own
/// boost management drops one bin at the locked anchor voltage, earlier at lower voltages:
/// - 1920@937 at ~78 °C (run 1790537155912);
/// - 1920/1830/1740@931 at 71 °C (run f2-forge-1790617016985).
/// It is not instability. Since the staircase decision (2026-09-28) it counts as held everywhere:
/// discovery, residency, DX11 exposure and the Endurance hot target. A drop of two bins still
/// fails. The published nominal clock never moves.
pub const F2_HELD_BIN_BELOW_MHZ: u32 = 15;
pub fn f2_clock_held(clock: u32, target: u32) -> bool {
    (target.saturating_sub(F2_HELD_BIN_BELOW_MHZ)..=f2_clock_ceiling_mhz(target)).contains(&clock)
}
/// Current FailureSeekingGameLoop qualification contract.
///
/// v7 requires the High-FPS, Texture and Transitions qualification set and reconciles the exact
/// sustained-p95 electrical regime before a point may be applied.
///
/// v8 adds the FrameCadence phase (game-frame-scale heavy burst / short idle cycling with its own
/// stock golden) to all three patterns — evidence qualified without it cannot unlock Apply.
///
/// v9 adds VRAM-pressure and geometry/depth phases plus the fourth Memory pattern; the complete
/// qualification set is now HighFps + Texture + Transitions + Memory.
///
/// v10 rebuilds the texture path: TextureRop now samples a large VRAM-resident source with a
/// per-pixel scattered tap chain (TMU + memory controller together, cache-defeating). v9
/// positives were measured against an L2-resident source and proved optimistic on hardware —
/// they cannot unlock Apply.
///
/// v11 hardens the engine: TextureRop reverts to the L2-resident graceful silent-error detector,
/// the heavy memory sampling moves to the banded TextureStream phase (pre-hang watchdog +
/// stock-referenced degradation gate), patterns are severity-ordered, and the pre-hang stall
/// signal became a failing verdict.
///
/// v12 (v13 plan): every qualification dwell runs under an absolute NVML max-clock ceiling at
/// the focus target — the qualified point IS the point the hardware exercised (p95 == target).
/// Pre-v12 positives were exercised +15/+30 MHz above their label by the thermal curve shift
/// and cannot unlock Apply.
///
/// v13 (efficiency): HighFps dropped from the required set. Across two full HW runs it was never
/// the binding detector — every boundary and Apply rejection fired in Texture (texture-rop). The set
/// is now Texture (binding, graceful — runs FIRST so a failing bin fails after one dwell) +
/// Transitions + Memory (VRAM-dominant, hang-prone — runs LAST). Cuts one 60 s dwell per descent
/// bin and one 300 s dwell per Apply pair. Pre-v13 evidence used a different pattern set → quarantined.
/// v14 (2026-07-10): across 5 HW runs, Texture (texture-rop) was the ONLY binding boundary/Apply
/// detector; the standalone Transitions and Memory 5-min passes never rejected a candidate — 10 min
/// of redundant per-pair coverage. REQUIRED is now Texture alone; the VRAM-controller path Memory
/// covered is folded INTO the candidate-only Endurance soak as an interleaved VramPressure segment
/// under sustained worst-case load (stronger than the isolated pass ever was). Different set →
/// pre-v14 evidence quarantined; full re-forge required.
/// v15 (2026-07-13): the Texture qualifier became LOBBY-FIRST after an initial interpretation of a
/// field trace favored sustained BoostEdge residency. Follow-up comparison showed the same external
/// clock/power/utilization envelope surviving in the lobby while a real match failed, so residency
/// alone was not causal; v15 evidence must not be described as reproducing the field failure.
/// v16 (2026-07-15): MixedGame is interleaved per frame, BoostEdge/MixedGame integrity sampling is
/// sparse GPU-side, and every dwell records the exact workload/build/adapter/golden provenance that
/// produced it. Positive evidence additionally requires confirmed stock reset and boot-flag cleanup.
/// Pre-v16 positives describe a different workload and cannot unlock Apply.
/// v17 (2026-07-15): exact Apply additionally requires a native offscreen Direct3D 11 golden gate
/// on the selected NVIDIA adapter. This covers a graphics API/driver path absent from wgpu's
/// Vulkan/DX12 backends; pre-v17 positives did not exercise it and cannot unlock Apply.
/// v18 (2026-07-16): Texture becomes TextureRop-first v9 and Endurance front-loads its aggressive
/// rejection tier. DX11 and TransitionShock leave the mandatory gate because neither rejected a
/// candidate in the collected runs; v18 deployability was exact-Apply Texture v9 + Endurance.
/// v19 (2026-07-18): Texture Hop v10 increases dependent texture/ROP work, sweeps denser multi-period
/// load-release transitions and owns more of the early descent tier. Standard uses a bounded compact
/// proof; Long retains the exhaustive thermal proof. Pre-v19 positives cannot unlock Apply.
/// v20 (2026-07-18): Texture Hop v11 combines the precise golden-checked TextureRop oracle with an
/// early idle-to-CompositeGameLoad slam, then checks TextureRop again before broader coverage. Its
/// new workload fingerprint prevents pre-v20 positives from unlocking Apply.
/// v21 (2026-07-18): Texture Hop v12 turns CompositeGameLoad into a heterogeneous Texture Stack:
/// cache-bound TextureRop, VRAM-bound TextureStream, power render and scattered VRAM gather share
/// one submit with per-lane rotating goldens. The active Texture path no longer uses the banded
/// pre-hang wall-time abort; silent error, DeviceLost and TDR are valid rejection evidence.
/// v22 (2026-07-18): Texture Hop v13 introduced the measured game-like primary Texture Stack envelope
/// resident while fresh independent devices/queues run the live TextureRop canary concurrently.
/// v23 keeps the v13 field-derived concurrency but makes its coverage decision numeric-power-first,
/// lengthens Boost Edge enough for representative telemetry and separates the physical discovery
/// frontier from the qualified publication frontier. Pre-v23 positives cannot unlock Apply.
/// v24 (2026-07-23): Field Concurrency keeps one independent secondary TextureRop device resident
/// for the whole phase instead of repeatedly creating and destroying devices. Secondary
/// initialization/coverage failure is environmental Inconclusive evidence, never VF instability.
/// The new workload fingerprint and contract quarantine all positives produced by the self-failing
/// context-churn recipe.
/// v25 (2026-07-23): qualification runs under a verified voltage lock and only samples at the exact
/// target clock count as target residency. Missing voltage authority, an observed voltage above the
/// selected bin, or insufficient exact-clock residency is Inconclusive and cannot unlock Apply.
/// v26 (2026-08-04): exact Apply additionally requires same-run Vulkan, native DX11 v2 and DX12
/// API lanes before Endurance. Discovery remains Texture-only and is not multiplied by API count.
/// v27 (2026-08-04): the field-calibrated native DX11 v2 residency gate runs first for 420 seconds;
/// Vulkan and DX12 retain mode-specific durations, followed by continuous Endurance. This rejects a
/// late DX11 failure before spending time on the remaining exact-Apply proof.
/// v28 (2026-08-11): frontier Texture qualification accepts one adjacent physical boost bin of
/// runtime elasticity before declaring a margin ClockDrop. Exact-Apply keeps its zero-bin held-clock
/// rule and the same DX11/Vulkan/DX12/Endurance matrix. Pre-v28 frontier positives cannot unlock Apply.
/// v29: stock-checked secondary context and immediate peer-failure cancellation.
/// v32 (2026-09-26): representative-load contract. Only PowerRender must stay below the board
/// limit; qualification samples below target while at the limit count as held. Heavy phases too
/// short to evaluate are skipped, not refused.
/// v33 (2026-09-28): screening residency counts the one hot bin below the target as held.
/// v34 (2026-10-01): the screening runs Texture Hop r5 (load steps), 34.5 s.
pub const F2_FRONTIER_QUALIFICATION_CONTRACT_VERSION: u32 = 34;

/// v29 (2026-08-11): three consecutive, provenance-identical native DX11 v2 exact-Apply dwells that
/// finish reset-clean and off-cap with at least 95% of clocks below the requested target are a
/// structural clock drop. Their raw observations remain Inconclusive, while the aggregate gate
/// rejects the candidate without blacklisting it so the caller may perform a bounded voltage repair.
/// v30 (2026-09-15): native DX11 v3 queues one render/compute batch while hashing the previous
/// staging copy. This removes the CPU checksum gap from the load. Every completed batch still
/// has both checks; duration, exact target residency and the remaining matrix are unchanged.
/// v31 (2026-09-16): DX11 separates heavy integrity from bounded, fenced active target exposure.
/// Pre-v31 exact-Apply positives cannot qualify this different coverage contract.
/// v32: stock-checked secondary context and immediate peer-failure cancellation.
/// v35 (2026-09-26): representative-load contract, same rules as Frontier v32; DX11 exposure
/// counts power-limited active time separately (`power_limited_active_ms`).
/// v36 (2026-09-27): the DX11 lane adds a continuous light phase (one-instance frame, own stock
/// golden) that must hold the exact target for 30 s, so DX11 itself exercises the pair.
/// v37 (2026-09-27): the light phase is paced at 50% duty with two lane shares, and back-to-back
/// batches of one phase are credited as one span. Back-to-back light frames kept the GPU busy and
/// power-limited, and ~8.7 ms batches capped the phase below 30 s.
/// v38 (2026-09-28): the one hot bin below the target counts as held target time in residency,
/// DX11 exposure (the light phase included) and the Endurance hot target (user decision). The
/// exact label is no longer proven when hot; a two-bin drop still fails.
/// v39 (2026-10-01): Texture and DX12 lanes run Texture Hop r5 with ~30 idle→slam load steps each
/// (138 s); a pass that held its target only through the hot bin anchors the game margin higher.
pub const F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION: u32 = 39;

/// Backward-compatible alias for callers that expose one latest profile-publication contract.
/// Frontier qualification has an independent version because exact-Apply policy changes must not
/// invalidate already-proven descent boundaries.
pub const F2_QUALIFICATION_CONTRACT_VERSION: u32 = F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION;

/// What kind of evidence one observation contributes. Old JSONL lines default to `Legacy`: they may
/// guide discovery, but can never satisfy the current qualification gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum F2EvidenceKind {
    #[default]
    Legacy,
    Discovery,
    Qualification,
    ApplyQualification,
}

/// Reproducibility metadata for the exact executable, workload and graphics stack that produced an
/// F2 dwell. Every field is optional so observations written before provenance existed remain
/// readable; current writers fill every value the active backend exposes rather than fabricating
/// unavailable data.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct F2EvidenceProvenance {
    /// Service package version that wrote the observation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_version: Option<String>,
    /// Source revision embedded at build time (with a `-dirty` suffix for local dirty builds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_revision: Option<String>,
    /// Stable semantic fingerprint exported by the workload implementation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload_fingerprint: Option<String>,
    /// Actual wgpu backend selected for this dwell, not merely the requested backend set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_backend: Option<String>,
    /// Adapter name reported by the graphics API, useful for matching it to `gpu_key`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_name: Option<String>,
    /// Driver family/name reported by the selected graphics adapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_name: Option<String>,
    /// Driver version/details reported by the selected graphics adapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_info: Option<String>,
    /// Canonical description of the active integrity/checksum method.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum_method: Option<String>,
    /// Canonical capture configuration and stock golden values used by this dwell, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub golden_config: Option<String>,
}

impl F2EvidenceProvenance {
    /// A positive may be reused only when the executable, semantic workload, selected graphics
    /// stack and integrity oracle can all be identified. Driver APIs occasionally omit either the
    /// short driver name or the detailed version string, so one non-empty driver identifier is
    /// sufficient; every other identity component is mandatory.
    pub fn is_reproducible(&self) -> bool {
        fn present(value: &Option<String>) -> bool {
            value
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
        }

        present(&self.build_version)
            && present(&self.build_revision)
            && present(&self.workload_fingerprint)
            && present(&self.render_backend)
            && present(&self.adapter_name)
            && (present(&self.driver_name) || present(&self.driver_info))
            && present(&self.checksum_method)
            && present(&self.golden_config)
    }
}

/// Whether a stable qualification dwell exercised every required phase strongly enough to count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum F2QualificationVerdict {
    Pass,
    Fail,
    Inconclusive,
}

/// Strength of the qualification evidence. Older strengths remain readable for compatibility;
/// FSGL4 is the current provenance-complete qualifier strength required for Apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum F2QualificationStrength {
    #[default]
    Fsgl1,
    Fsgl2,
    Fsgl3,
    Fsgl4,
}

/// Deterministic workload pattern. A/B remain readable for legacy observations; current boundary
/// deployability requires [`REQUIRED_QUALIFICATION_PATTERNS`], while exact Apply additionally uses
/// [`REQUIRED_EXACT_APPLY_PATTERNS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum F2QualificationPattern {
    A,
    B,
    HighFps,
    Texture,
    Transitions,
    Memory,
    /// v14 candidate-only endurance soak: one CONTINUOUS ~15-min mixed dwell run ONLY at the exact
    /// Apply pair (never in the frontier descent). Deliberately kept OUT of
    /// [`REQUIRED_QUALIFICATION_PATTERNS`] so it tightens Apply without altering the descent /
    /// completeness gates; publishing is gated on the run-scoped exact-Apply matrix instead
    /// ([`point_has_current_exact_apply_qualification`]).
    Endurance,
    /// Legacy v15 candidate-only transition shock. Contract v19 no longer executes or requires it,
    /// but the variant remains so persisted evidence is backward-readable.
    TransitionShock,
    /// Candidate-only native Direct3D 11 v2 render/integrity gate.
    Dx11Game,
    /// Candidate-only Direct3D 12 mirror of the Vulkan Texture Hop recipe.
    Dx12Game,
}

/// The complete pattern set the current qualification contract requires at a discovery boundary.
/// Exact Apply starts with this set and adds the candidate-only API/Endurance lanes in
/// [`REQUIRED_EXACT_APPLY_PATTERNS`].
/// v13: Texture FIRST (the empirically-binding graceful silent-error detector — a failing bin fails
/// after one dwell), Memory LAST (VRAM-dominant, hang-prone). HighFps was removed (never binding).
pub const REQUIRED_QUALIFICATION_PATTERNS: [F2QualificationPattern; 1] =
    [F2QualificationPattern::Texture];

/// Complete exact-Apply proof. Texture is the explicitly selected Vulkan path; DX11 v3 and DX12
/// add independent driver/API coverage, and Endurance retains the long thermal/transient proof.
/// These candidate-only additions do not multiply the frontier descent.
pub const REQUIRED_EXACT_APPLY_PATTERNS: [F2QualificationPattern; 4] = [
    F2QualificationPattern::Dx11Game,
    F2QualificationPattern::Texture,
    F2QualificationPattern::Dx12Game,
    F2QualificationPattern::Endurance,
];

fn required_pattern_index(pattern: F2QualificationPattern) -> Option<usize> {
    REQUIRED_QUALIFICATION_PATTERNS
        .iter()
        .position(|required| *required == pattern)
}

/// Per-phase telemetry captured during a qualification dwell. Optional values remain absent when a
/// driver/sample path could not provide them; they are never fabricated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct F2QualificationPhaseMetric {
    /// Actual retained sensor reads; absent in older records, never inferred from phase duration.
    #[serde(default)]
    pub sample_count: Option<u32>,
    /// Highest sampled clock, including excursions hidden by p95; not an instantaneous HW bound.
    #[serde(default)]
    pub clock_max: Option<u32>,
    pub phase_name: String,
    pub phase_pattern: String,
    pub duration_ms: u64,
    pub frame_count: u64,
    pub checksum_count: u32,
    pub compute_check_count: u32,
    #[serde(default)]
    pub clock_avg: Option<f32>,
    #[serde(default)]
    pub clock_p5: Option<u32>,
    #[serde(default)]
    pub clock_p50: Option<u32>,
    #[serde(default)]
    pub clock_p95: Option<u32>,
    #[serde(default)]
    pub target_residency_pct: Option<f32>,
    #[serde(default)]
    pub power_avg: Option<f32>,
    #[serde(default)]
    pub power_p95: Option<f32>,
    #[serde(default)]
    pub power_capped_fraction: Option<f32>,
    #[serde(default)]
    pub temperature_avg: Option<f32>,
    #[serde(default)]
    pub temperature_max: Option<f32>,
    pub coverage_status: String,
    /// Joint cells `[clock MHz, whole °C, samples, SW power-cap samples]`, sorted. Diagnostic only
    /// (2026-09-27): shows where a phase dropped a bin as it heated. Legacy evidence has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clock_temp: Vec<[u32; 4]>,
}

/// Compact, append-only qualification coverage summary. Phase details remain service-internal; this
/// carries the durable facts needed to decide whether a validation may qualify Apply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct F2QualificationCoverage {
    /// DX11 v4 sampled exposure inside fenced GPU-work intervals, excluding idle/CPU checksums.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_target: Option<F2ActiveTargetCoverage>,
    #[serde(default)]
    pub strength: F2QualificationStrength,
    #[serde(default)]
    pub pattern: Option<F2QualificationPattern>,
    #[serde(default)]
    pub pass_index: u32,
    pub verdict: F2QualificationVerdict,
    pub phases_completed: u32,
    pub phases_expected: u32,
    pub checksum_count: u32,
    pub sample_count: u32,
    #[serde(default)]
    pub compute_check_count: u32,
    #[serde(default)]
    pub target_residency_frac: Option<f32>,
    #[serde(default)]
    pub heavy_light_power_delta_w: Option<f32>,
    #[serde(default)]
    pub failure_phase: Option<String>,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub phase_metrics: Vec<F2QualificationPhaseMetric>,
}

/// Native DX11 lane phases: heavy, 75/50/25% duty heavy bursts, continuous light, heavy.
pub const F2_DX11_PHASES: u32 = 6;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct F2ActiveTargetCoverage {
    pub observed_active_ms: u64,
    pub target_active_ms: u64,
    /// Active time below the target band while at the board power limit (Frontier32/Apply35):
    /// held under the representative-load contract, reported apart from real target exposure.
    #[serde(default)]
    pub power_limited_active_ms: u64,
    /// Exact-target time in the paced light DX11 phase (ExactApply37): DX11 itself must exercise
    /// the pair below the power cap, as a frame-capped light game would.
    #[serde(default)]
    pub light_target_active_ms: u64,
    pub required_target_ms: u64,
    pub sample_count: u32,
    pub phases_completed: u32,
    pub upper_clock_exceeded: bool,
    /// Both full-duty phases supplied >=30 s sampled work and >=95% target-envelope exposure.
    #[serde(default)]
    pub heavy_target_proven: bool,
    /// Diagnostic detail only; old v31 evidence remains readable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<F2ActiveTargetDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct F2ActiveTargetDiagnostics {
    pub requested_max_mhz: u32,
    pub anchor_mv: u32,
    /// Independent refusal reasons, including power when the complete lane is evaluated.
    pub reasons: Vec<String>,
    pub publication_power_ceiling_w: Option<f32>,
    pub phases: Vec<F2ActiveClockPhase>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct F2ActiveClockPhase {
    pub phase_index: u32,
    pub requested_duty_pct: u32,
    /// Light frame (one instance, paced) instead of the heavy frame.
    #[serde(default)]
    pub light: bool,
    pub active_sample_count: u32,
    pub active_clock_max_mhz: Option<u32>,
    #[serde(default)]
    pub observed_active_us: u64,
    /// Held target time: the target, its +15 MHz envelope or the one hot bin below (ExactApply38).
    #[serde(default)]
    pub target_active_us: u64,
    #[serde(default)]
    pub power_limited_active_us: u64,
    /// The part of `target_active_us` spent one bin below the nominal target (diagnostic).
    #[serde(default)]
    pub one_bin_below_active_us: u64,
    pub upper_sample_count: u32,
    // Upper samples describe excursions above the nominal request, including allowed +15 MHz.
    /// Bounded sampled time support, NOT the continuous duration of an excursion.
    pub upper_observed_us: u64,
    pub first_upper: Option<F2ClockExcursion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct F2ClockExcursion {
    pub at_ms: u64,
    pub clock_mhz: u32,
    pub voltage_mv: Option<u32>,
    pub temperature_c: Option<u32>,
    /// Optional read after the telemetry query, not an atomic snapshot of the event.
    pub curve: Option<F2ClockCurveSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct F2ClockCurveSnapshot {
    pub captured_at_ms: u64,
    pub base_mhz: u32,
    pub base_mv: u32,
    pub effective_mhz: u32,
    pub effective_mv: u32,
    pub offset_khz: Option<i32>,
}

impl F2ActiveTargetCoverage {
    pub fn proves_target(&self) -> bool {
        self.heavy_target_proven && self.phases_completed == F2_DX11_PHASES && !self.upper_clock_exceeded && self.sample_count > 0
            && self.light_target_active_ms >= self.required_target_ms
            && self.diagnostics.as_ref().is_none_or(|d| d.phases.iter().all(|p|
                p.active_clock_max_mhz.map_or(p.upper_sample_count == 0, |clock| clock <= f2_clock_ceiling_mhz(d.requested_max_mhz))))
            && self.observed_active_ms >= 60_000 && self.required_target_ms == 30_000
            && self.held_active_ms() >= self.required_target_ms
            && self.held_active_ms() <= self.observed_active_ms
            && self.held_active_ms() as f64 / self.observed_active_ms as f64 >= 0.35
    }

    /// Target exposure plus power-limited time (representative-load contract). One bin below the
    /// target is held only for heavy-phase sustain, never as exposure.
    pub fn held_active_ms(&self) -> u64 {
        self.target_active_ms.saturating_add(self.power_limited_active_ms)
    }
}

/// Which F2 path produced an observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum F2ObsMode {
    /// The official autonomous unknown-GPU path (anchored, conservative offset caps).
    DefaultProgressive,
    /// The explicit operator-provided dev/known-GPU shortcut (larger bounded cap).
    ManualPrior,
    /// A same-target autonomous sweep for the minimum stable voltage.
    TargetSweep,
    /// A multi-target ladder of target sweeps.
    LadderSweep,
    /// Long current-contract qualification of the exact post-margin Apply pair selected for a profile.
    ApplyQualification,
}

/// The verifier verdict, in an owned serializable form (the service-side `PositiveOffsetVerification`
/// is crate-private, so observations carry this mirror instead).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum F2ObsVerifier {
    RaiseVerified,
    RaiseIncomplete,
    OverRaise,
    Unverifiable,
    /// No verify ran (e.g. the candidate was refused by the planner / a safety gate before any write).
    NotRun,
}

/// The dwell verdict, in an owned serializable form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum F2ObsDwell {
    Stable,
    SilentError,
    Unstable,
    DeviceLost,
    ClockDrop,
    /// Discovery completed reset-clean, but repeated PowerRender measurements did not establish a
    /// consistent sustained-p99 value for this bin.
    PowerTelemetryInconclusive,
    /// Discovery could not establish the intended operating point; see `inconclusive_reason`.
    DiscoveryInconclusive,
    /// Dwell completed reset-clean, but the qualification did not collect enough current-contract
    /// coverage to prove the point.
    QualificationInconclusive,
    /// No dwell ran (arm/apply/verify failed first, or the candidate was refused before any write).
    NotRun,
}

/// Terminal outcome of an F2 attempt — a superset spanning planner refusal, the confirmed motor's
/// failure classes, and safety-gate aborts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum F2ObsOutcome {
    /// Dwell stable, reset confirmed, boot flag cleared — a good (stable) undervolt point.
    Validated,
    /// The planner refused the candidate (offset cap / floor / non-real bin / sanity).
    RejectedByPlanner,
    /// The post-write verify did not confirm the anchored raise.
    VerifierFailed,
    /// The dwell reported a silent compute error (no device loss).
    SilentError,
    /// The dwell reported instability without a classified silent error (no device loss).
    Unstable,
    /// The dwell reported a crash / TDR / device loss.
    DeviceLost,
    /// The target did not hold yet, but the GPU was still at 99–100% of its power limit. This is a
    /// search-state observation, not a voltage-instability boundary.
    PowerBoundClockDrop,
    /// The clock sagged below tolerance under load (held the dwell but not the clock).
    ClockDrop,
    /// Discovery power telemetry could not be confirmed after the bounded repeat budget. This is
    /// neither stability evidence nor a voltage failure.
    PowerTelemetryInconclusive,
    /// Discovery lacks operating-point evidence (e.g. voltage samples); never a bad boundary.
    DiscoveryInconclusive,
    /// The qualification workload ran reset-clean, but coverage was too weak to qualify or reject the
    /// point. This is not a voltage failure and must not become a bad-boundary veto.
    QualificationInconclusive,
    /// `reset_to_stock` could not be confirmed — a SAFETY failure (boot flag retained, fail closed).
    ResetFailed,
    /// The candidate intent was blacklisted (known-bad).
    Blacklisted,
    /// A crash/TDR that Safe Loop recovered from — learning data, recovery remained safe.
    CrashOrRecovery,
    /// A safety gate (Safe Mode, armed flag, crash threshold, blacklist preflight) aborted the run.
    AbortedBySafetyGate,
}

impl F2ObsOutcome {
    /// A good point: the undervolt held stably AND the GPU reset cleanly.
    pub fn is_validated(self) -> bool {
        matches!(self, F2ObsOutcome::Validated)
    }

    /// A real instability/failure at this voltage (the undervolt did NOT hold) — distinct from a
    /// planner/safety-gate refusal that performed no write. Used to compute `first_bad` / `is_known_bad`.
    pub fn is_bad(self) -> bool {
        matches!(
            self,
            F2ObsOutcome::VerifierFailed
                | F2ObsOutcome::SilentError
                | F2ObsOutcome::Unstable
                | F2ObsOutcome::DeviceLost
                | F2ObsOutcome::ClockDrop
                | F2ObsOutcome::ResetFailed
                | F2ObsOutcome::Blacklisted
                | F2ObsOutcome::CrashOrRecovery
        )
    }

    /// A safety-critical outcome: the run MUST stop and must not continue to a deeper candidate. A
    /// `ResetFailed` left the GPU potentially un-reset (boot flag retained); a crash/recovery means the
    /// run cannot be trusted to continue. Ordinary instability (`Unstable`/`ClockDrop`/`VerifierFailed`)
    /// is NOT a safety failure when the reset was clean — it is normal learning data.
    pub fn is_safety_failure(self) -> bool {
        matches!(
            self,
            F2ObsOutcome::DeviceLost | F2ObsOutcome::ResetFailed | F2ObsOutcome::CrashOrRecovery
        )
    }
}

/// One recorded F2 attempt with its full outcome. Append-only; persisted as one JSON object per line.
/// Optional fields use `#[serde(default)]` so partial/older lines still decode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct F2Observation {
    /// Stable id for the run that produced this observation (caller-provided; groups ladder candidates).
    pub run_id: String,
    /// RFC3339 timestamp (caller-provided so pure queries/tests stay deterministic).
    pub timestamp: String,
    /// GPU identity (the NVAPI curve name), when available.
    #[serde(default)]
    pub gpu_key: Option<String>,
    #[serde(default)]
    pub evidence_kind: F2EvidenceKind,
    #[serde(default)]
    pub discovery_contract_version: Option<u32>,
    #[serde(default)]
    pub qualification_contract_version: Option<u32>,
    #[serde(default)]
    pub qualification_coverage: Option<F2QualificationCoverage>,
    /// Exact build/workload/backend provenance for the dwell. Absent on legacy lines or attempts that
    /// failed before a workload context could be created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_provenance: Option<F2EvidenceProvenance>,
    pub mode: F2ObsMode,
    pub target_mhz: u32,
    #[serde(default)]
    pub requested_start_mv: Option<u32>,
    pub anchor_mv: u32,
    pub base_mhz: u32,
    pub offset_mhz: i32,
    pub positive_offset_cap_mhz: i32,
    #[serde(default)]
    pub higher_bins_capped: u32,
    #[serde(default)]
    pub max_flatten_mhz: i32,
    #[serde(default)]
    pub lower_bins_elastic: u32,
    pub verifier_result: F2ObsVerifier,
    pub dwell_result: F2ObsDwell,
    #[serde(default)]
    pub avg_clock_mhz: Option<u32>,
    /// Sustained (p5) clock under load.
    #[serde(default)]
    pub sustained_clock_mhz: Option<u32>,
    /// Upper sustained (p95) clock under the same load. This reveals the boost regime exercised by
    /// an exact Apply pair without conflating it with the configured target.
    #[serde(default)]
    pub sustained_upper_clock_mhz: Option<u32>,
    /// Absolute sampled maximum, including ramps; distinct from sustained p95.
    #[serde(default)]
    pub max_clock_mhz: Option<u32>,
    #[serde(default)]
    pub watts: Option<u32>,
    /// Highest post-ramp power sample captured by the discovery dwell.
    #[serde(default)]
    pub max_watts: Option<u32>,
    /// Sustained high-power percentile captured from the retained post-ramp dwell samples.
    #[serde(default)]
    pub power_p99_w: Option<f32>,
    /// The discovery p99 passed the v4 adjacent-bin/repeat consistency gate.
    #[serde(default)]
    pub power_p99_confirmed: bool,
    /// Number of reset-clean PowerRender attempts used by the v4 consistency decision.
    #[serde(default)]
    pub power_p99_attempts: u32,
    /// Ramp-filtered voltage telemetry from the dwell; diagnostic only and never fabricated.
    #[serde(default)]
    pub measured_voltage_min_mv: Option<u32>,
    #[serde(default)]
    pub measured_voltage_avg_mv: Option<u32>,
    #[serde(default)]
    pub measured_voltage_max_mv: Option<u32>,
    #[serde(default)]
    pub measured_voltage_sample_count: u32,
    /// Render coverage captured by the workload itself, used to diagnose underloaded dwells.
    #[serde(default)]
    pub render_frames: Option<u64>,
    #[serde(default)]
    pub render_fps: Option<f64>,
    /// Fraction of steady-state samples where the NVIDIA power-cap flag was active.
    #[serde(default)]
    pub power_capped_frac: Option<f32>,
    #[serde(default)]
    pub max_temp_c: Option<f32>,
    /// NVML reported software or hardware thermal slowdown during the dwell.
    #[serde(default)]
    pub thermal_throttled: bool,
    /// Actual wall-clock duration of the dwell that produced this observation.
    #[serde(default)]
    pub dwell_duration_ms: Option<u64>,
    /// Number of retained steady-state clock/power samples.
    #[serde(default)]
    pub sample_count: Option<u32>,
    #[serde(default)]
    pub silent_error: bool,
    #[serde(default)]
    pub device_lost: bool,
    #[serde(default)]
    pub unstable: bool,
    #[serde(default)]
    pub clock_drop: bool,
    /// A TDR/crash that Safe Loop detected/recovered (when distinguishable).
    #[serde(default)]
    pub tdr_or_crash: bool,
    #[serde(default)]
    pub reset_to_stock_attempted: bool,
    #[serde(default)]
    pub reset_to_stock_ok: bool,
    #[serde(default)]
    pub boot_flag_cleared: bool,
    #[serde(default)]
    pub blacklisted: bool,
    pub outcome: F2ObsOutcome,
    /// Specific measurement/coverage refusal, preserved from the executed dwell or power gate.
    /// Absent in historical records; those records must not acquire an inferred reason on load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inconclusive_reason: Option<String>,
    /// Per-attempt confidence basis (0–1) when known; the frontier recomputes an aggregate confidence.
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// The known voltage bracket around the minimum stable voltage `Vmin` for a target. In a (monotone)
/// descent every Validated point is `>= Vmin` and every bad point is `< Vmin`, so
/// `first_bad_mv < Vmin <= last_good_mv`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoltageBracket {
    /// Highest voltage that FAILED (a lower bound below `Vmin`).
    pub first_bad_mv: u32,
    /// Lowest voltage that VALIDATED (an upper bound at/above `Vmin`).
    pub last_good_mv: u32,
    /// `last_good_mv - first_bad_mv` — how tightly `Vmin` is bracketed.
    pub width_mv: u32,
}

/// One learned F2 frontier entry per target: the best (lowest-voltage) validated undervolt point plus
/// the search state (first-bad, bracket, counts, confidence). This is the BRIDGE from discovery to the
/// existing profile classifiers — NOT profile selection. Persisted/reportable and serde-friendly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct F2FrontierEntry {
    pub target_mhz: u32,
    /// Best validated anchor voltage (the minimum stable voltage discovered so far).
    pub best_anchor_mv: u32,
    pub offset_mhz: i32,
    #[serde(default)]
    pub watts: Option<u32>,
    #[serde(default)]
    pub max_watts: Option<u32>,
    #[serde(default)]
    pub power_p99_w: Option<f32>,
    #[serde(default)]
    pub avg_clock_mhz: Option<u32>,
    #[serde(default)]
    pub sustained_clock_mhz: Option<u32>,
    #[serde(default)]
    pub sustained_upper_clock_mhz: Option<u32>,
    #[serde(default)]
    pub power_capped_frac: Option<f32>,
    #[serde(default)]
    pub dwell_duration_ms: Option<u64>,
    #[serde(default)]
    pub sample_count: Option<u32>,
    #[serde(default)]
    pub max_temp_c: Option<f32>,
    #[serde(default)]
    pub thermal_throttled: bool,
    /// Aggregate confidence (0–1) from repeat validations at `best_anchor_mv`.
    pub confidence: f64,
    /// Successful confirmations at this exact target/anchor point.
    #[serde(default)]
    pub validation_count: usize,
    #[serde(default)]
    pub first_bad_mv: Option<u32>,
    #[serde(default)]
    pub bracket_width_mv: Option<u32>,
    pub observation_count: usize,
    /// Timestamp of the best observation (RFC3339).
    pub last_updated: String,
    #[serde(default)]
    pub safety_notes: Option<String>,
}

/// Current UTC timestamp as RFC3339 (the project's timestamp convention; see `safe_loop`). Kept here so
/// service callers need not depend on `chrono` directly.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// A run id unique per F2 run (groups all candidate observations of one sweep/ladder run). Format:
/// `<prefix>-<unix_millis>`.
pub fn new_run_id(prefix: &str) -> String {
    format!("{prefix}-{}", chrono::Utc::now().timestamp_millis())
}

/// Pure, BOM-tolerant JSONL parser: one observation per line, blank/malformed lines skipped (so a
/// truncated final line from a crash never invalidates the whole log).
pub fn parse_observations(data: &str) -> Vec<F2Observation> {
    data.lines()
        .map(|l| l.trim_start_matches('\u{feff}').trim())
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<F2Observation>(l).ok())
        .collect()
}

/// Confidence (0–1) for a learned frontier point given how many times that exact point validated.
/// Deliberately simple and SEPARATE from the F1b Wilson trial model (this is F2 learning, not profile
/// scoring): one clean validation already clears the balanced classifier threshold (0.85), and repeats
/// raise it toward a 0.99 ceiling. Pure + monotone non-decreasing in `validated_count`.
pub fn frontier_confidence(validated_count: usize) -> f64 {
    if validated_count == 0 {
        return 0.0;
    }
    (1.0 - 0.15_f64.powf(validated_count as f64)).min(0.99)
}

/// Confidence from the evidence actually collected at one exact point. Standard's proven 15 s
/// dwell with at least 100 retained samples contributes one evidence unit (0.85 confidence); shorter
/// dwells contribute less, while longer and independently repeated clean passes mature toward 0.99.
pub fn frontier_confidence_from_evidence(validations: &[&F2Observation]) -> f64 {
    let evidence_units = validations.iter().fold(0.0, |sum, obs| {
        let duration = obs.dwell_duration_ms.unwrap_or(15_000) as f64 / 15_000.0;
        let sample_quality = (obs.sample_count.unwrap_or(100) as f64 / 100.0).min(1.0);
        sum + duration * sample_quality
    });
    if evidence_units <= 0.0 {
        0.0
    } else {
        (1.0 - 0.15_f64.powf(evidence_units)).min(0.99)
    }
}

/// The last good (minimum stable) anchor for a target: the LOWEST-voltage Validated observation. This
/// is the best undervolt found so far (lower voltage = deeper undervolt). `None` if none validated.
pub fn last_good_for_target(obs: &[F2Observation], target_mhz: u32) -> Option<&F2Observation> {
    let first_bad_mv = first_bad_for_target(obs, target_mhz).map(|o| o.anchor_mv);
    obs.iter()
        .filter(|o| o.target_mhz == target_mhz && o.outcome.is_validated())
        // A later failure at V invalidates V and every deeper/lower-voltage point, even if an older
        // run once validated there. Only clean points strictly above the nearest known failure remain.
        .filter(|o| first_bad_mv.is_none_or(|bad| o.anchor_mv > bad))
        .min_by_key(|o| o.anchor_mv)
}

/// Positive evidence is reusable only after the candidate transaction proved that the GPU returned
/// to stock and the Safe Loop boot flag was cleared. Missing legacy fields deserialize as `false`,
/// so incomplete observations fail closed.
fn has_proven_cleanup(o: &F2Observation) -> bool {
    o.reset_to_stock_ok && o.boot_flag_cleared
}

fn has_reproducible_provenance(o: &F2Observation) -> bool {
    o.evidence_provenance
        .as_ref()
        .is_some_and(F2EvidenceProvenance::is_reproducible)
}

/// True when an observation can define the current PowerRender discovery frontier. Discovery v5
/// requires confirmed p99 evidence and proven cleanup, so older, telemetry-inconclusive or
/// transaction-incomplete positives cannot seed the current frontier. Negative observations remain
/// version-independent and conservative.
pub fn is_current_discovery_evidence(o: &F2Observation) -> bool {
    o.evidence_kind == F2EvidenceKind::Discovery
        && o.discovery_contract_version == Some(F2_DISCOVERY_CONTRACT_VERSION)
        && o.max_clock_mhz.is_some_and(|clock| clock > 0 && clock <= f2_clock_ceiling_mhz(o.target_mhz))
        && o.power_p99_confirmed
        && has_proven_cleanup(o)
        && has_reproducible_provenance(o)
}

/// True only for a fully-covered pass from the current qualification contract. Legacy positives and
/// passes from older qualifiers or without proven cleanup can guide a start point but can never
/// unlock Apply.
pub fn is_current_qualification_pass(o: &F2Observation) -> bool {
    o.evidence_kind == F2EvidenceKind::Qualification
        && o.qualification_contract_version == Some(F2_FRONTIER_QUALIFICATION_CONTRACT_VERSION)
        && o.max_clock_mhz.is_some_and(|clock| clock > 0 && clock <= f2_clock_ceiling_mhz(o.target_mhz))
        && o.outcome.is_validated()
        && has_proven_cleanup(o)
        && has_reproducible_provenance(o)
        && o.qualification_coverage.as_ref().is_some_and(|coverage| {
            coverage.strength == F2QualificationStrength::Fsgl4
                && coverage
                    .pattern
                    .is_some_and(is_required_qualification_pattern)
                && coverage.verdict == F2QualificationVerdict::Pass
        })
}

/// True only for a fully-covered pass at the exact post-margin Apply pair under the current
/// qualification contract. Kept separate from frontier qualification so an Apply failure caused by
/// a higher boost regime does not rewrite the learned voltage boundary.
pub fn is_current_apply_qualification_pass(o: &F2Observation) -> bool {
    is_current_apply_qualification_evidence(o)
        && o.qualification_coverage
            .as_ref()
            .and_then(|coverage| coverage.pattern)
            .is_some_and(is_required_qualification_pattern)
}

/// A pass holds its target only through the GPU's own relief when, in some lane, more than this
/// share of its critical-phase samples sat at the hot bin (2026-10-01). It stays a pass; it only
/// anchors the game margin one clock bin's worth of voltage higher (never inconclusive).
pub const F2_HOT_BIN_RELIEF_SHARE: f32 = 0.5;
/// Field concurrency and the r5 load steps carry the transient that TDR'd 1815@887. DX11 lanes have
/// neither: their variable phase spreads clocks under the power limit for reasons unrelated to it.
const F2_CRITICAL_PHASES: [&str; 2] = ["field-concurrency", "load-step"];
const F2_HOT_BIN_RELIEF_MIN_SAMPLES: u32 = 20;

/// One lane's share of critical-phase samples at the hot bin (target − 15 MHz) versus at or above
/// the target. None when the lane has too few critical samples to judge.
pub fn f2_critical_hot_bin_share(metrics: &[F2QualificationPhaseMetric], target_mhz: u32) -> Option<f32> {
    let (mut hot, mut held) = (0u32, 0u32);
    for metric in metrics.iter().filter(|m| F2_CRITICAL_PHASES.contains(&m.phase_name.as_str())) {
        for &[clock, _, samples, _] in &metric.clock_temp {
            if clock >= target_mhz {
                held += samples;
            } else if clock + F2_HELD_BIN_BELOW_MHZ >= target_mhz {
                hot += samples;
            }
        }
    }
    (hot + held >= F2_HOT_BIN_RELIEF_MIN_SAMPLES).then(|| hot as f32 / (hot + held) as f32)
}

/// True when any current exact-Apply pass lane of this pair held its critical phases mostly at the
/// hot bin (run 1790850465550: 1815@843 ran 69% of DX12 field concurrency at 1800).
pub fn f2_pair_has_hot_bin_relief(
    obs: &[F2Observation],
    run_id: &str,
    target_mhz: u32,
    anchor_mv: u32,
    gpu_key: &str,
) -> bool {
    obs.iter()
        .filter(|o| {
            o.run_id == run_id
                && o.gpu_key.as_deref() == Some(gpu_key)
                && o.target_mhz == target_mhz
                && o.anchor_mv == anchor_mv
                && is_current_apply_qualification_pass(o)
        })
        .filter_map(|o| f2_critical_hot_bin_share(&o.qualification_coverage.as_ref()?.phase_metrics, target_mhz))
        .any(|share| share > F2_HOT_BIN_RELIEF_SHARE)
}

fn is_current_apply_qualification_evidence(o: &F2Observation) -> bool {
    o.evidence_kind == F2EvidenceKind::ApplyQualification
        && o.qualification_contract_version == Some(F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION)
        && o.max_clock_mhz.is_some_and(|clock| clock > 0 && clock <= f2_clock_ceiling_mhz(o.target_mhz))
        && o.outcome.is_validated()
        && has_proven_cleanup(o)
        && has_reproducible_provenance(o)
        && o.qualification_coverage.as_ref().is_some_and(|coverage| {
            coverage.strength == F2QualificationStrength::Fsgl4
                && coverage.pattern.is_some()
                && (coverage.pattern != Some(F2QualificationPattern::Dx11Game)
                    || coverage.active_target.as_ref().is_some_and(F2ActiveTargetCoverage::proves_target))
                && coverage.verdict == F2QualificationVerdict::Pass
        })
}

fn is_required_qualification_pattern(pattern: F2QualificationPattern) -> bool {
    required_pattern_index(pattern).is_some()
}

/// Lowest-voltage stable point proven by the current, homogeneous discovery contract. Negative
/// observations remain version-independent and invalidate the same/deeper voltage as before.
pub fn last_discovery_good_for_target(
    obs: &[F2Observation],
    target_mhz: u32,
) -> Option<&F2Observation> {
    let first_bad_mv = first_bad_for_target(obs, target_mhz).map(|o| o.anchor_mv);
    obs.iter()
        .filter(|o| {
            o.target_mhz == target_mhz
                && o.outcome.is_validated()
                && is_current_discovery_evidence(o)
        })
        .filter(|o| first_bad_mv.is_none_or(|bad| o.anchor_mv > bad))
        .min_by_key(|o| o.anchor_mv)
}

fn same_run_and_gpu(a: &F2Observation, b: &F2Observation) -> bool {
    a.run_id == b.run_id && a.gpu_key == b.gpu_key
}

/// A reset-clean `ClockDrop` is performance evidence, not an unconditional silicon failure. If the
/// same run later qualifies either the same target at a strictly lower voltage or a harder clock at
/// the same/lower voltage, that stronger physical observation contradicts the drop as a
/// downward-voltage boundary. Require the dominating pair to have both current Discovery and current
/// Qualification evidence so an orphan/stale pass cannot reopen a descent.
fn clock_drop_is_dominated_by_qualified_pair(obs: &[F2Observation], drop: &F2Observation) -> bool {
    matches!(drop.outcome, F2ObsOutcome::ClockDrop)
        && obs.iter().any(|qualification| {
            ((qualification.target_mhz == drop.target_mhz
                && qualification.anchor_mv < drop.anchor_mv)
                || (qualification.target_mhz > drop.target_mhz
                    && qualification.anchor_mv <= drop.anchor_mv))
                && same_run_and_gpu(qualification, drop)
                && is_current_qualification_pass(qualification)
                && obs.iter().any(|discovery| {
                    discovery.target_mhz == qualification.target_mhz
                        && discovery.anchor_mv == qualification.anchor_mv
                        && same_run_and_gpu(discovery, qualification)
                        && discovery.outcome.is_validated()
                        && is_current_discovery_evidence(discovery)
                })
        })
}

fn is_voltage_boundary_failure(obs: &[F2Observation], candidate: &F2Observation) -> bool {
    candidate.outcome.is_bad()
        && !(candidate.outcome == F2ObsOutcome::ClockDrop && candidate.discovery_contract_version == Some(F2_DISCOVERY_CONTRACT_VERSION))
        && !candidate.inconclusive_reason.as_deref().is_some_and(|r| r.starts_with("control_failure"))
        && !clock_drop_is_dominated_by_qualified_pair(obs, candidate)
}

/// The first effective bad anchor for a target: the HIGHEST-voltage real-failure observation — the
/// shallowest undervolt that already failed (the closest failure below the validated region).
/// Reset-clean `ClockDrop` evidence contradicted by a harder qualified pair in the same run is not a
/// voltage boundary. `None` if no effective bad observation remains.
pub fn first_bad_for_target(obs: &[F2Observation], target_mhz: u32) -> Option<&F2Observation> {
    obs.iter()
        .filter(|o| {
            o.target_mhz == target_mhz
                && o.evidence_kind != F2EvidenceKind::ApplyQualification
                && is_voltage_boundary_failure(obs, o)
        })
        .max_by_key(|o| o.anchor_mv)
}

/// The known voltage bracket for a target: `Vmin` lies in `(first_bad_mv, last_good_mv]`. Returns
/// `None` unless BOTH a validated and a bad point exist AND `last_good > first_bad` (a consistent,
/// monotone descent — if a failure sits at/above the lowest validated point the data is inconsistent
/// and no bracket is claimed).
pub fn bracket_for_target(obs: &[F2Observation], target_mhz: u32) -> Option<VoltageBracket> {
    let last_good_mv = last_good_for_target(obs, target_mhz)?.anchor_mv;
    let first_bad_mv = first_bad_for_target(obs, target_mhz)?.anchor_mv;
    if last_good_mv > first_bad_mv {
        Some(VoltageBracket {
            first_bad_mv,
            last_good_mv,
            width_mv: last_good_mv - first_bad_mv,
        })
    } else {
        None
    }
}

/// True if `(target, anchor_mv)` is already known bad: there is a prior real-failure observation at
/// that voltage OR ANY HIGHER voltage for the target. Conservative — a failure at voltage `V` (too low
/// to hold the clock) implies `V` and everything LOWER is at least as risky.
pub fn is_known_bad(obs: &[F2Observation], target_mhz: u32, anchor_mv: u32) -> bool {
    obs.iter().any(|o| {
        o.target_mhz == target_mhz
            && o.evidence_kind != F2EvidenceKind::ApplyQualification
            && is_voltage_boundary_failure(obs, o)
            && o.anchor_mv >= anchor_mv
    })
}

/// The validated descent baseline for chained same-target descent: the DEEPEST (lowest-voltage,
/// largest-offset) prior `Validated` observation for `target_mhz` that left the GPU clean — reset
/// confirmed, boot flag cleared, and no instability/crash flags — optionally scoped to `gpu_key` (a
/// baseline learned on a DIFFERENT GPU must never bound this GPU's descent). This is the cross-run
/// RESUME point: the official target sweep measures a candidate's per-step increase against this
/// baseline's `offset_mhz` instead of stock `+0`, so a descent already validated to `+15` may reach a
/// `+30` candidate in one bounded step. Returns `None` when no such point exists (the descent then
/// starts from stock baseline `0` — unchanged first-run behavior). A no-write planner/safety-gate abort
/// (`RejectedByPlanner` / `AbortedBySafetyGate`) is NOT validated, so it never becomes a baseline. The
/// ABSOLUTE offset cap still bounds each candidate independently; this only relaxes the per-step delta.
/// Pure.
pub fn validated_descent_baseline<'a>(
    obs: &'a [F2Observation],
    target_mhz: u32,
    gpu_key: Option<&str>,
) -> Option<&'a F2Observation> {
    obs.iter()
        .filter(|o| o.target_mhz == target_mhz && o.outcome.is_validated())
        // Defensive: a Validated point already implies clean cleanup, but a hand-edited/older log line
        // could disagree — require the clean flags explicitly before trusting it as a resume baseline.
        .filter(|o| o.reset_to_stock_ok && o.boot_flag_cleared)
        .filter(|o| !o.device_lost && !o.unstable && !o.silent_error && !o.clock_drop)
        .filter(|o| match gpu_key {
            Some(k) => o.gpu_key.as_deref() == Some(k),
            None => true,
        })
        .min_by_key(|o| o.anchor_mv)
}

/// Crash-proximity margin: a synthesized boundary must sit at least this far (≈ two physical VF
/// bins on modern NVIDIA curves) above the highest crash/TDR anchor observed for the target. A
/// crash means the silent-error threshold above it went undetected — the immediately adjacent bin
/// cannot be trusted just because it happened to pass. Generic rule; never derived from any
/// specific GPU's known points.
pub const F2_CRASH_PROXIMITY_MIN_MV: u32 = 12;

/// Highest crash/TDR/device-loss anchor recorded for a target, if any. Pure.
pub fn crash_floor_for_target(obs: &[F2Observation], target_mhz: u32) -> Option<u32> {
    obs.iter()
        .filter(|o| {
            o.target_mhz == target_mhz
                && (o.device_lost || matches!(o.outcome, F2ObsOutcome::DeviceLost))
        })
        .map(|o| o.anchor_mv)
        .max()
}

fn frontier_entry_from_best(
    obs: &[F2Observation],
    target_mhz: u32,
    best: &F2Observation,
) -> F2FrontierEntry {
    let evidence_at_best: Vec<&F2Observation> = obs
        .iter()
        .filter(|o| {
            o.target_mhz == target_mhz
                && o.outcome.is_validated()
                && o.anchor_mv == best.anchor_mv
                && (is_current_discovery_evidence(o) || is_current_qualification_pass(o))
        })
        .collect();
    let qualification_count = REQUIRED_QUALIFICATION_PATTERNS
        .into_iter()
        .filter(|pattern| {
            evidence_at_best.iter().any(|o| {
                is_current_qualification_pass(o)
                    && o.qualification_coverage.as_ref().and_then(|c| c.pattern) == Some(*pattern)
            })
        })
        .count();
    let observation_count = obs.iter().filter(|o| o.target_mhz == target_mhz).count();
    let bracket = bracket_for_target(obs, target_mhz);
    F2FrontierEntry {
        target_mhz,
        best_anchor_mv: best.anchor_mv,
        offset_mhz: best.offset_mhz,
        watts: best.watts,
        max_watts: best.max_watts,
        power_p99_w: best.power_p99_w,
        avg_clock_mhz: best.avg_clock_mhz,
        sustained_clock_mhz: best.sustained_clock_mhz,
        sustained_upper_clock_mhz: best.sustained_upper_clock_mhz,
        power_capped_frac: best.power_capped_frac,
        dwell_duration_ms: best.dwell_duration_ms,
        sample_count: best.sample_count,
        max_temp_c: best.max_temp_c,
        thermal_throttled: best.thermal_throttled,
        confidence: frontier_confidence_from_evidence(&evidence_at_best),
        validation_count: qualification_count,
        first_bad_mv: first_bad_for_target(obs, target_mhz).map(|o| o.anchor_mv),
        bracket_width_mv: bracket.map(|b| b.width_mv),
        observation_count,
        last_updated: best.timestamp.clone(),
        safety_notes: None,
    }
}

fn required_exact_apply_pattern_index(pattern: F2QualificationPattern) -> Option<usize> {
    REQUIRED_EXACT_APPLY_PATTERNS
        .iter()
        .position(|required| *required == pattern)
}

fn discovery_frontier_candidates(
    obs: &[F2Observation],
    target_mhz: u32,
) -> impl Iterator<Item = &F2Observation> {
    let crash_floor = crash_floor_for_target(obs, target_mhz);
    let first_bad_mv = first_bad_for_target(obs, target_mhz).map(|bad| bad.anchor_mv);
    obs.iter().filter(move |o| {
        o.target_mhz == target_mhz
            && o.outcome.is_validated()
            && is_current_discovery_evidence(o)
            && first_bad_mv.is_none_or(|bad_mv| o.anchor_mv > bad_mv)
            && crash_floor.is_none_or(|crash_mv| {
                o.anchor_mv >= crash_mv.saturating_add(F2_CRASH_PROXIMITY_MIN_MV)
            })
    })
}

/// Build one physical discovery frontier entry for a target. This deliberately preserves the
/// deepest current Discovery pass even when its qualification is still inconclusive; callers that
/// publish profiles must use [`qualified_frontier_entry_for_target`] instead.
pub fn frontier_entry_for_target(
    obs: &[F2Observation],
    target_mhz: u32,
) -> Option<F2FrontierEntry> {
    let best = discovery_frontier_candidates(obs, target_mhz).min_by_key(|o| o.anchor_mv)?;
    Some(frontier_entry_from_best(obs, target_mhz, best))
}

/// Build the deepest frontier entry that is publishable under the current qualification contract.
/// An inconclusive deeper Discovery point remains physical boundary knowledge, but it cannot replace
/// the nearest shallower pair that completed every currently required qualification pattern.
pub fn qualified_frontier_entry_for_target(
    obs: &[F2Observation],
    target_mhz: u32,
) -> Option<F2FrontierEntry> {
    let best = discovery_frontier_candidates(obs, target_mhz)
        .filter(|candidate| {
            REQUIRED_QUALIFICATION_PATTERNS.into_iter().all(|pattern| {
                obs.iter().any(|qualification| {
                    qualification.target_mhz == target_mhz
                        && qualification.anchor_mv == candidate.anchor_mv
                        && is_current_qualification_pass(qualification)
                        && qualification
                            .qualification_coverage
                            .as_ref()
                            .and_then(|c| c.pattern)
                            == Some(pattern)
                })
            })
        })
        .min_by_key(|o| o.anchor_mv)?;
    Some(frontier_entry_from_best(obs, target_mhz, best))
}

/// Build the full learned F2 frontier: one entry per target that has a Validated observation, sorted by
/// target. Pure — this is the discovery→classifier bridge input.
pub fn learned_frontier(obs: &[F2Observation]) -> Vec<F2FrontierEntry> {
    let mut targets: Vec<u32> = obs.iter().map(|o| o.target_mhz).collect();
    targets.sort_unstable();
    targets.dedup();
    targets
        .into_iter()
        .filter_map(|t| frontier_entry_for_target(obs, t))
        .collect()
}

/// Build a frontier using observations from one exact physical GPU only.
pub fn learned_frontier_for_gpu(obs: &[F2Observation], gpu_key: &str) -> Vec<F2FrontierEntry> {
    let scoped: Vec<F2Observation> = obs
        .iter()
        .filter(|o| o.gpu_key.as_deref() == Some(gpu_key))
        .cloned()
        .collect();
    learned_frontier(&scoped)
}

/// Build the profile-publication projection for one exact physical GPU. The physical discovery
/// frontier remains available through [`learned_frontier_for_gpu`].
pub fn qualified_frontier_for_gpu(obs: &[F2Observation], gpu_key: &str) -> Vec<F2FrontierEntry> {
    let scoped: Vec<F2Observation> = obs
        .iter()
        .filter(|o| o.gpu_key.as_deref() == Some(gpu_key))
        .cloned()
        .collect();
    let mut targets: Vec<u32> = scoped.iter().map(|o| o.target_mhz).collect();
    targets.sort_unstable();
    targets.dedup();
    targets
        .into_iter()
        .filter_map(|target| qualified_frontier_entry_for_target(&scoped, target))
        .collect()
}

/// Project a measured, qualified frontier whose voltage never decreases as target clock rises.
/// For each target, selects the lowest-voltage current Discovery candidate that has current
/// qualification at that exact pair and is not below the previously selected anchor. Equal-voltage
/// plateaus are valid. If no measured candidate satisfies the monotonic floor, the target is omitted;
/// this projection never fabricates or relabels a voltage point.
pub fn monotonic_qualified_frontier_for_gpu(
    obs: &[F2Observation],
    gpu_key: &str,
) -> Vec<F2FrontierEntry> {
    let scoped: Vec<F2Observation> = obs
        .iter()
        .filter(|o| o.gpu_key.as_deref() == Some(gpu_key))
        .cloned()
        .collect();
    let mut targets: Vec<u32> = scoped.iter().map(|o| o.target_mhz).collect();
    targets.sort_unstable();
    targets.dedup();

    let mut minimum_anchor_mv = None;
    let mut frontier = Vec::new();
    for target_mhz in targets {
        let best = discovery_frontier_candidates(&scoped, target_mhz)
            .filter(|candidate| {
                minimum_anchor_mv.is_none_or(|minimum| candidate.anchor_mv >= minimum)
            })
            .filter(|candidate| {
                REQUIRED_QUALIFICATION_PATTERNS.into_iter().all(|pattern| {
                    scoped.iter().any(|qualification| {
                        qualification.target_mhz == target_mhz
                            && qualification.anchor_mv == candidate.anchor_mv
                            && is_current_qualification_pass(qualification)
                            && qualification
                                .qualification_coverage
                                .as_ref()
                                .and_then(|coverage| coverage.pattern)
                                == Some(pattern)
                    })
                })
            })
            .min_by_key(|candidate| candidate.anchor_mv);
        if let Some(best) = best {
            minimum_anchor_mv = Some(best.anchor_mv);
            frontier.push(frontier_entry_from_best(&scoped, target_mhz, best));
        }
    }
    frontier
}

/// Return the first physical voltage-order inversion in a frontier. As target clock rises, the
/// minimum qualified voltage may stay flat or rise, but it must never fall. Equal-voltage plateaus
/// remain valid measured evidence; a higher clock at a lower voltage means at least one target is
/// stale/contradictory and profile synthesis must fail closed until discovery reconciles it.
pub fn frontier_voltage_order_violation(
    frontier: &[F2FrontierEntry],
) -> Option<(u32, u32, u32, u32)> {
    frontier.iter().find_map(|lower| {
        frontier.iter().find_map(|higher| {
            (higher.target_mhz > lower.target_mhz && higher.best_anchor_mv < lower.best_anchor_mv)
                .then_some((
                    lower.target_mhz,
                    lower.best_anchor_mv,
                    higher.target_mhz,
                    higher.best_anchor_mv,
                ))
        })
    })
}

/// Bridge ONE learned frontier entry to the canonical telemetry point the existing GPU profile
/// classifiers (`synthesize_forge_profiles`) consume, paired with its confidence. F2 RAISES a
/// lower-voltage bin (true undervolt), so the apply axis `vf_table_voltage_mv` is that LOWER anchor bin
/// (the opposite direction from F1's down-cap), the point is non-power-bound (`power_capped_frac = 0`),
/// and `stable = true`. Every field the classifier does not need is left default. Pure — builds DATA
/// only; it never selects, applies, persists, or promotes a profile.
pub fn to_power_sweep_point(entry: &F2FrontierEntry) -> (PowerSweepPoint, f64) {
    let clock = entry.avg_clock_mhz.unwrap_or(entry.target_mhz);
    let power = entry.watts.unwrap_or(0) as f32;
    let max_power = entry.max_watts.unwrap_or(entry.watts.unwrap_or(0)) as f32;
    let power_p99 = entry
        .power_p99_w
        .filter(|power| power.is_finite() && *power > 0.0);
    let sustained_clock = entry.sustained_clock_mhz.unwrap_or(clock);
    let perf_per_watt = if let Some(power_p99) = power_p99 {
        sustained_clock as f64 / power_p99 as f64
    } else {
        0.0
    };
    let point = PowerSweepPoint {
        voltage_mv: entry.best_anchor_mv,
        clock_mhz: clock,
        offset_mhz: entry.offset_mhz,
        power_w: power,
        max_power_w: max_power,
        power_p99_w: power_p99,
        // In F1, a power-bound point means the voltage probe did not reveal a useful tuning
        // boundary, so synthesis excludes it. In F2 the anchored target was explicitly sustained;
        // being at the cap is valid Cmax evidence and must not disqualify the forged point.
        power_capped_frac: 0.0,
        stable: true,
        perf_per_watt,
        vf_table_voltage_mv: Some(entry.best_anchor_mv),
        boundary_voltage_mv: Some(entry.best_anchor_mv),
        apply_margin_mv: Some(0),
        p5_clock_mhz: entry.sustained_clock_mhz,
        p95_clock_mhz: entry.sustained_upper_clock_mhz,
        max_temp_c: entry.max_temp_c,
        thermal_throttled: entry.thermal_throttled,
        target_clock_mhz: Some(entry.target_mhz),
        confidence: Some(entry.confidence),
        validation_count: Some(entry.validation_count as u32),
        ..Default::default()
    };
    (point, entry.confidence)
}

/// Highest sustained-p99 discovery observation for one exact target/apply anchor. Only current v5,
/// p99-confirmed, reset-clean, thermally valid PowerRender evidence is eligible. A power-bound clock
/// drop remains valid power/clock telemetry for the apply bin, but it is never promoted into
/// stability evidence. Repeated measurements deliberately choose the largest measured p99 so a
/// profile is calibrated conservatively without inventing a monotonic correction.
pub fn current_discovery_observation_at_anchor<'a>(
    obs: &'a [F2Observation],
    target_mhz: u32,
    anchor_mv: u32,
    gpu_key: &str,
) -> Option<&'a F2Observation> {
    obs.iter()
        .filter(|o| {
            o.target_mhz == target_mhz
                && o.anchor_mv == anchor_mv
                && o.gpu_key.as_deref() == Some(gpu_key)
                && matches!(
                    o.outcome,
                    F2ObsOutcome::Validated | F2ObsOutcome::PowerBoundClockDrop
                )
                && is_current_discovery_evidence(o)
                && o.reset_to_stock_ok
                && o.boot_flag_cleared
                && !o.thermal_throttled
                && o.avg_clock_mhz.is_some()
                && o.sustained_clock_mhz.is_some()
                && o.watts.is_some_and(|watts| watts > 0)
                && o.max_watts.is_some_and(|watts| watts > 0)
                && o.power_p99_w
                    .is_some_and(|power| power.is_finite() && power > 0.0)
        })
        .max_by(|a, b| {
            a.power_p99_w
                .unwrap_or(0.0)
                .total_cmp(&b.power_p99_w.unwrap_or(0.0))
        })
}

/// Sustained-clock tolerance (MHz) mirroring the service classifier's `F2_CLOCK_DROP_TOL_MHZ`:
/// one hot boost bin at the anchor voltage (see [`F2_HELD_BIN_BELOW_MHZ`]).
pub const F2_APPLY_CLOCK_HOLD_TOL_MHZ: u32 = F2_HELD_BIN_BELOW_MHZ;

/// True when an Apply-qualification observation's power/clock telemetry is trustworthy. A
/// thermal-slowdown flag only invalidates it when the slowdown actually backed the card OFF the
/// qualified point — i.e. the sustained (p5) clock sagged below target beyond tolerance. When the
/// card HELD >= target despite the flag (a momentary memory-junction hotspot at a cool core temp),
/// the point ran at its real operating clock/power, so the reading stands. Fails closed when the
/// sustained clock is unknown. Mirrors the held-clock rule in `classify_f2_stress_dwell`; power
/// discovery/calibration keep the stricter unconditional `!thermal_throttled`.
fn apply_qual_reading_trustworthy(o: &F2Observation, _target_mhz: u32) -> bool {
    // v33 qualification already verifies heavy phases individually. Aggregate p5 includes idle
    // and cannot diagnose thermal clock loss independently of those phase measurements.
    is_current_apply_qualification_evidence(o)
}

/// Highest sustained p99 measured by the complete, reset-clean current required set at one exact Apply
/// anchor in one run. Partial sets, failed/inconclusive passes, old qualification contracts, and
/// thermal slowdowns that sagged the sustained clock are excluded; a thermal-slowdown flag that
/// still HELD the target clock is trusted (see `apply_qual_reading_trustworthy`). The maximum across
/// all approved patterns is returned so profile presentation cannot understate power already observed
/// during its deployability soak (accepting a held-throttled reading can only raise it).
fn apply_qualification_p99_at_anchor(
    obs: &[F2Observation],
    run_id: Option<&str>,
    target_mhz: u32,
    anchor_mv: u32,
    gpu_key: &str,
) -> Option<f32> {
    let mut runs = std::collections::BTreeMap::<
        &str,
        ([bool; REQUIRED_QUALIFICATION_PATTERNS.len()], f32),
    >::new();
    for observation in obs.iter().filter(|o| {
        run_id.is_none_or(|expected| o.run_id == expected)
            && o.target_mhz == target_mhz
            && o.anchor_mv == anchor_mv
            && o.gpu_key.as_deref() == Some(gpu_key)
            && is_current_apply_qualification_pass(o)
            && o.reset_to_stock_ok
            && o.boot_flag_cleared
            && apply_qual_reading_trustworthy(o, target_mhz)
            && o.power_p99_w
                .is_some_and(|power| power.is_finite() && power > 0.0)
    }) {
        let entry = runs
            .entry(observation.run_id.as_str())
            .or_insert(([false; REQUIRED_QUALIFICATION_PATTERNS.len()], 0.0));
        let Some(index) = observation
            .qualification_coverage
            .as_ref()
            .and_then(|coverage| coverage.pattern)
            .and_then(required_pattern_index)
        else {
            continue;
        };
        entry.0[index] = true;
        let power = observation.power_p99_w.unwrap_or(0.0);
        entry.1 = entry.1.max(power);
    }
    runs.into_values()
        .filter(|(seen, _)| seen.iter().all(|present| *present))
        .map(|(_, power)| power)
        .max_by(f32::total_cmp)
}

/// Highest sustained p99 from the complete approved current set produced by one exact Forge run.
pub fn current_apply_qualification_p99_at_anchor(
    obs: &[F2Observation],
    run_id: &str,
    target_mhz: u32,
    anchor_mv: u32,
    gpu_key: &str,
) -> Option<f32> {
    apply_qualification_p99_at_anchor(obs, Some(run_id), target_mhz, anchor_mv, gpu_key)
}

/// Highest sustained p99 from a complete exact-Apply gate: Vulkan, DX11 v3, DX12 and continuous
/// Endurance must have passed reset-clean in the same run.
fn complete_apply_gate_p99_at_anchor(
    obs: &[F2Observation],
    run_id: Option<&str>,
    target_mhz: u32,
    anchor_mv: u32,
    gpu_key: &str,
) -> Option<f32> {
    let mut runs = std::collections::BTreeMap::<
        &str,
        ([bool; REQUIRED_EXACT_APPLY_PATTERNS.len()], f32),
    >::new();
    for observation in obs.iter().filter(|o| {
        run_id.is_none_or(|expected| o.run_id == expected)
            && o.target_mhz == target_mhz
            && o.anchor_mv == anchor_mv
            && o.gpu_key.as_deref() == Some(gpu_key)
            && is_current_apply_qualification_evidence(o)
            && o.reset_to_stock_ok
            && o.boot_flag_cleared
            && apply_qual_reading_trustworthy(o, target_mhz)
            && o.power_p99_w
                .is_some_and(|power| power.is_finite() && power > 0.0)
    }) {
        let Some(pattern) = observation
            .qualification_coverage
            .as_ref()
            .and_then(|coverage| coverage.pattern)
        else {
            continue;
        };
        let entry = runs
            .entry(observation.run_id.as_str())
            .or_insert(([false; REQUIRED_EXACT_APPLY_PATTERNS.len()], 0.0));
        if let Some(index) = required_exact_apply_pattern_index(pattern) {
            entry.0[index] = true;
        } else {
            continue;
        }
        entry.1 = entry.1.max(observation.power_p99_w.unwrap_or(0.0));
    }
    runs.into_values()
        .filter(|(seen, _)| seen.iter().all(|present| *present))
        .map(|(_, power)| power)
        .max_by(f32::total_cmp)
}

pub fn current_complete_apply_gate_p99_at_anchor(
    obs: &[F2Observation],
    run_id: &str,
    target_mhz: u32,
    anchor_mv: u32,
    gpu_key: &str,
) -> Option<f32> {
    complete_apply_gate_p99_at_anchor(obs, Some(run_id), target_mhz, anchor_mv, gpu_key)
}

pub fn highest_complete_apply_gate_p99_at_anchor(
    obs: &[F2Observation],
    target_mhz: u32,
    anchor_mv: u32,
    gpu_key: &str,
) -> Option<f32> {
    complete_apply_gate_p99_at_anchor(obs, None, target_mhz, anchor_mv, gpu_key)
}

/// Highest sustained p95 clock reached by a complete, reset-clean current set at one exact Apply pair.
/// Missing telemetry in any required pattern fails closed.
pub fn current_apply_qualification_p95_clock_at_anchor(
    obs: &[F2Observation],
    run_id: &str,
    target_mhz: u32,
    anchor_mv: u32,
    gpu_key: &str,
) -> Option<u32> {
    let mut seen = [false; REQUIRED_QUALIFICATION_PATTERNS.len()];
    let mut highest = 0u32;
    for observation in obs.iter().filter(|o| {
        o.run_id == run_id
            && o.target_mhz == target_mhz
            && o.anchor_mv == anchor_mv
            && o.gpu_key.as_deref() == Some(gpu_key)
            && is_current_apply_qualification_pass(o)
            && o.reset_to_stock_ok
            && o.boot_flag_cleared
            && apply_qual_reading_trustworthy(o, target_mhz)
    }) {
        let clock = observation
            .sustained_upper_clock_mhz
            .filter(|clock| *clock > 0)?;
        let Some(index) = observation
            .qualification_coverage
            .as_ref()
            .and_then(|coverage| coverage.pattern)
            .and_then(required_pattern_index)
        else {
            continue;
        };
        seen[index] = true;
        highest = highest.max(clock);
    }
    (seen.iter().all(|present| *present) && highest > 0).then_some(highest)
}

/// True when the CURRENT run's complete candidate-only stress gate validated cleanly at this exact
/// `(target_mhz, apply_mv)` pair on this GPU: Vulkan Texture Hop, DX11 v3, DX12 and Endurance.
/// These gates only TIGHTEN Apply — they are not part of [`REQUIRED_QUALIFICATION_PATTERNS`] and
/// never touch the frontier descent. They still share the current qualification contract and full
/// reproducibility/cleanup requirements; the publish gate is also run_id-scoped. Fail closed:
/// legacy, incomplete or absent evidence reads `false` and can never publish.
pub fn point_has_current_exact_apply_qualification(
    obs: &[F2Observation],
    run_id: &str,
    target_mhz: u32,
    apply_mv: u32,
    gpu_key: &str,
) -> bool {
    point_has_n_current_exact_apply_qualifications(obs, run_id, target_mhz, apply_mv, gpu_key, 1)
}

/// True when at least `required_matrices` complete exact-Apply matrices were persisted by the
/// CURRENT run at this exact pair/GPU. Evidence is consumed in append order and must complete the
/// declared DX11 → Vulkan → DX12 → Endurance sequence; duplicated/retried lanes inside one ladder
/// cannot masquerade as another matrix. Zero is rejected because every publish/re-proof path must
/// require positive evidence.
pub fn point_has_n_current_exact_apply_qualifications(
    obs: &[F2Observation],
    run_id: &str,
    target_mhz: u32,
    apply_mv: u32,
    gpu_key: &str,
    required_matrices: u32,
) -> bool {
    if required_matrices == 0 {
        return false;
    }
    let mut next_pattern = 0usize;
    let mut complete_matrices = 0u32;
    for o in obs.iter().filter(|o| {
        o.run_id == run_id
            && o.target_mhz == target_mhz
            && o.anchor_mv == apply_mv
            && o.gpu_key.as_deref() == Some(gpu_key)
            && is_current_apply_qualification_evidence(o)
    }) {
        let Some(pattern) = o
            .qualification_coverage
            .as_ref()
            .and_then(|coverage| coverage.pattern)
        else {
            continue;
        };
        if pattern == REQUIRED_EXACT_APPLY_PATTERNS[next_pattern] {
            next_pattern += 1;
            if next_pattern == REQUIRED_EXACT_APPLY_PATTERNS.len() {
                complete_matrices = complete_matrices.saturating_add(1);
                if complete_matrices >= required_matrices {
                    return true;
                }
                next_pattern = 0;
            }
        } else if pattern == REQUIRED_EXACT_APPLY_PATTERNS[0] {
            // A new DX11 lane starts a new matrix; abandon any incomplete prior ladder. Repeated
            // DX11 retries simply keep the cursor at the first completed lane.
            next_pattern = 1;
        }
    }
    false
}

/// WORST-CASE measured power (max of p99 and peak) across ALL of the CURRENT run's validated
/// ApplyQualification dwells at one exact Apply pair — INCLUDING the v14 Endurance soak and the
/// v15 TransitionShock, which are the honest worst-load measurements. The off-cap invariant must
/// consume this, not just the calm PowerRender basis: a cool-ambient run measures PowerRender
/// 10-15 W below a warm day at the same point, letting an at-cap-under-game-load clock slip past
/// the headroom ceiling (the 2026-07-10 run published the known-TDR 1920@918 exactly this way —
/// PowerRender 174 W, Endurance peak 189 W). Strictly conservative: only ever RAISES the basis.
pub fn worst_current_apply_qualification_power_at_anchor(
    obs: &[F2Observation],
    run_id: &str,
    target_mhz: u32,
    apply_mv: u32,
    gpu_key: &str,
) -> Option<f32> {
    obs.iter()
        .filter(|o| {
            o.run_id == run_id
                && o.target_mhz == target_mhz
                && o.anchor_mv == apply_mv
                && o.gpu_key.as_deref() == Some(gpu_key)
                && is_current_apply_qualification_evidence(o)
        })
        .flat_map(|o| {
            o.power_p99_w
                .into_iter()
                .chain(o.max_watts.map(|peak| peak as f32))
        })
        .filter(|power| power.is_finite() && *power > 0.0)
        .max_by(f32::total_cmp)
}

/// Highest sustained p99 across every complete current-contract run for one exact Apply pair.
/// This restores the conservative published wattage when a qualified Forge snapshot is reloaded.
pub fn highest_apply_qualification_p99_at_anchor(
    obs: &[F2Observation],
    target_mhz: u32,
    anchor_mv: u32,
    gpu_key: &str,
) -> Option<f32> {
    apply_qualification_p99_at_anchor(obs, None, target_mhz, anchor_mv, gpu_key)
}

/// Failure-phase telemetry: counts of qualification dwells that failed inside a named phase,
/// keyed by `(target_mhz, anchor_mv, pattern, failure_phase)`. This is the data source for
/// evidence-driven pattern weighting and adaptive apply margins — over time it shows WHICH
/// stress phase actually predicts instability on this hardware. Pure; counts persisted
/// evidence only.
pub fn qualification_failure_histogram(
    obs: &[F2Observation],
) -> std::collections::BTreeMap<(u32, u32, String, String), u32> {
    let mut histogram = std::collections::BTreeMap::new();
    for observation in obs {
        let Some(coverage) = observation.qualification_coverage.as_ref() else {
            continue;
        };
        // `failure_phase` is recorded only when a phase actually failed.
        let Some(phase) = coverage.failure_phase.clone() else {
            continue;
        };
        let pattern = coverage
            .pattern
            .map(|pattern| format!("{pattern:?}"))
            .unwrap_or_else(|| "legacy".to_string());
        *histogram
            .entry((
                observation.target_mhz,
                observation.anchor_mv,
                pattern,
                phase,
            ))
            .or_insert(0u32) += 1;
    }
    histogram
}

/// Bridge a whole learned frontier to the `(PowerSweepPoint, confidence)` pairing the existing
/// classifier consumes. Pure; builds data only.
pub fn frontier_to_points(entries: &[F2FrontierEntry]) -> Vec<(PowerSweepPoint, f64)> {
    entries.iter().map(to_power_sweep_point).collect()
}

/// File-backed, append-only F2 observation log. Mirrors [`crate::safe_loop::SafeLoopStore`]'s
/// path/serde conventions (reuses [`default_data_dir`]) but APPENDS one JSON line per observation
/// (JSONL) because observations accumulate. Reads tolerate a leading BOM and skip malformed lines.
/// NEVER written during a dry-run — only the confirmed F2 motor appends.
#[derive(Debug, Clone)]
pub struct F2ObservationStore {
    base: PathBuf,
}

impl F2ObservationStore {
    /// The machine-wide store under `default_data_dir()` (`%ProgramData%/Nidavellir`).
    pub fn system() -> Self {
        Self {
            base: default_data_dir(),
        }
    }

    /// A store rooted at an explicit base directory (used by tests).
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    /// The JSONL log path.
    pub fn path(&self) -> PathBuf {
        self.base.join(F2_OBSERVATIONS_FILE)
    }

    fn ensure_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.base)
    }

    /// Append one observation as a JSON line. Best-effort (mirrors the project's non-fsync convention);
    /// creates the file/dir on first write.
    pub fn append(&self, obs: &F2Observation) -> std::io::Result<()> {
        self.ensure_dir()?;
        let line = serde_json::to_string(obs)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path())?;
        writeln!(f, "{line}")
    }

    /// Load every well-formed observation (missing file → empty; malformed lines skipped).
    pub fn load_all(&self) -> Vec<F2Observation> {
        match std::fs::read_to_string(self.path()) {
            Ok(data) => parse_observations(&data),
            Err(_) => Vec::new(),
        }
    }

    /// Strict load for safety-critical decisions. A missing log is an empty history, but every
    /// other read error and every malformed non-empty JSONL line is returned to the caller.
    pub fn load_all_checked(&self) -> std::io::Result<Vec<F2Observation>> {
        let path = self.path();
        let data = match std::fs::read_to_string(&path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(std::io::Error::new(
                    error.kind(),
                    format!(
                        "failed to read F2 observation log {}: {error}",
                        path.display()
                    ),
                ));
            }
        };

        let mut observations = Vec::new();
        for (line_index, line) in data.lines().enumerate() {
            let line = line.trim_start_matches('\u{feff}').trim();
            if line.is_empty() {
                continue;
            }
            observations.push(
                serde_json::from_str::<F2Observation>(line).map_err(|error| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "invalid F2 observation JSONL at {} line {}: {error}",
                            path.display(),
                            line_index + 1
                        ),
                    )
                })?,
            );
        }
        Ok(observations)
    }

    /// All observations for a target.
    pub fn query_by_target(&self, target_mhz: u32) -> Vec<F2Observation> {
        self.load_all()
            .into_iter()
            .filter(|o| o.target_mhz == target_mhz)
            .collect()
    }

    /// Observations for one target on one exact physical GPU.
    pub fn query_by_target_for_gpu(&self, target_mhz: u32, gpu_key: &str) -> Vec<F2Observation> {
        self.load_all()
            .into_iter()
            .filter(|o| o.target_mhz == target_mhz && o.gpu_key.as_deref() == Some(gpu_key))
            .collect()
    }

    /// The learned frontier over the entire log.
    pub fn learned_frontier(&self) -> Vec<F2FrontierEntry> {
        learned_frontier(&self.load_all())
    }

    /// Learned frontier isolated to one exact physical GPU.
    pub fn learned_frontier_for_gpu(&self, gpu_key: &str) -> Vec<F2FrontierEntry> {
        learned_frontier_for_gpu(&self.load_all(), gpu_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hot_bin_share_reads_only_critical_phases_of_one_lane() {
        let metric = |name: &str, cells: &[(u32, u32)]| F2QualificationPhaseMetric {
            sample_count: None, clock_max: None, phase_name: name.into(), phase_pattern: String::new(),
            duration_ms: 0, frame_count: 0, checksum_count: 0, compute_check_count: 0, clock_avg: None,
            clock_p5: None, clock_p50: None, clock_p95: None, target_residency_pct: None,
            power_avg: None, power_p95: None, power_capped_fraction: None, temperature_avg: None,
            temperature_max: None, coverage_status: "pass".into(),
            clock_temp: cells.iter().map(|&(clock, n)| [clock, 66, n, 0]).collect(),
        };
        // Run 1790850465550, 1815@843 DX12 lane: field concurrency 1394 samples at 1800, 624 at 1815.
        let dx12 = [
            metric("texture-rop", &[(1800, 327), (1815, 905)]),
            metric("field-concurrency", &[(1800, 1394), (1815, 624)]),
        ];
        let share = f2_critical_hot_bin_share(&dx12, 1815).unwrap();
        assert!((share - 0.69).abs() < 0.01 && share > F2_HOT_BIN_RELIEF_SHARE);
        // Its Texture lane held 1815 exactly; a two-bin drop is not the hot bin; DX11 has no
        // critical phase; too few samples prove nothing.
        assert_eq!(f2_critical_hot_bin_share(&[metric("field-concurrency", &[(1815, 1978)])], 1815), Some(0.0));
        assert_eq!(f2_critical_hot_bin_share(&[metric("load-step", &[(1785, 50), (1815, 50)])], 1815), Some(0.0));
        assert_eq!(f2_critical_hot_bin_share(&[metric("dx11-game", &[(1800, 500)])], 1815), None);
        assert_eq!(f2_critical_hot_bin_share(&[metric("load-step", &[(1800, 19)])], 1815), None);
    }

    fn reproducible_provenance() -> F2EvidenceProvenance {
        F2EvidenceProvenance {
            build_version: Some("0.1.0".into()),
            build_revision: Some("d449b63-dirty".into()),
            workload_fingerprint: Some("vf-qualifier-v16-texture".into()),
            render_backend: Some("dx12".into()),
            adapter_name: Some("NVIDIA GeForce RTX 3060 Ti".into()),
            driver_name: Some("NVIDIA".into()),
            driver_info: Some("32.0.15.7688".into()),
            checksum_method: Some("gpu-reduction-sparse-readback".into()),
            golden_config: Some("capture_ms=2000;power=1;boost=2;texrop=3".into()),
        }
    }

    fn obs(target: u32, anchor: u32, outcome: F2ObsOutcome) -> F2Observation {
        F2Observation {
            inconclusive_reason: None,
            run_id: "run-test".into(),
            timestamp: "2026-06-21T00:00:00Z".into(),
            gpu_key: Some("RTX 3060 Ti".into()),
            evidence_kind: F2EvidenceKind::Discovery,
            discovery_contract_version: Some(F2_DISCOVERY_CONTRACT_VERSION),
            qualification_contract_version: None,
            qualification_coverage: None,
            evidence_provenance: Some(reproducible_provenance()),
            mode: F2ObsMode::TargetSweep,
            target_mhz: target,
            requested_start_mv: None,
            anchor_mv: anchor,
            base_mhz: target.saturating_sub(15),
            offset_mhz: 15,
            positive_offset_cap_mhz: 30,
            higher_bins_capped: 26,
            max_flatten_mhz: 150,
            lower_bins_elastic: 40,
            verifier_result: if outcome.is_validated() {
                F2ObsVerifier::RaiseVerified
            } else {
                F2ObsVerifier::Unverifiable
            },
            dwell_result: if outcome.is_validated() {
                F2ObsDwell::Stable
            } else {
                F2ObsDwell::Unstable
            },
            avg_clock_mhz: Some(target + 15),
            sustained_clock_mhz: Some(target + 15),
            sustained_upper_clock_mhz: Some(target + 15),
            max_clock_mhz: Some(target + 15),
            watts: Some(180),
            max_watts: Some(188),
            power_p99_w: Some(186.0),
            power_p99_confirmed: true,
            power_p99_attempts: 1,
            measured_voltage_min_mv: Some(anchor),
            measured_voltage_avg_mv: Some(anchor),
            measured_voltage_max_mv: Some(anchor),
            measured_voltage_sample_count: 1,
            render_frames: Some(900),
            render_fps: Some(60.0),
            power_capped_frac: Some(0.0),
            max_temp_c: Some(68.0),
            thermal_throttled: false,
            dwell_duration_ms: Some(15_000),
            sample_count: Some(300),
            silent_error: false,
            device_lost: false,
            unstable: !outcome.is_validated(),
            clock_drop: false,
            tdr_or_crash: false,
            reset_to_stock_attempted: true,
            reset_to_stock_ok: true,
            boot_flag_cleared: true,
            blacklisted: false,
            outcome,
            confidence: outcome.is_validated().then_some(0.86),
            notes: None,
        }
    }

    fn qualification_pass(o: F2Observation) -> F2Observation {
        qualification_pass_with_pattern(o, F2QualificationPattern::Texture)
    }

    fn qualification_pass_with_pattern(
        mut o: F2Observation,
        pattern: F2QualificationPattern,
    ) -> F2Observation {
        o.evidence_kind = F2EvidenceKind::Qualification;
        o.discovery_contract_version = None;
        o.qualification_contract_version = Some(F2_FRONTIER_QUALIFICATION_CONTRACT_VERSION);
        o.qualification_coverage = Some(F2QualificationCoverage {
            active_target: None,
            strength: F2QualificationStrength::Fsgl4,
            pattern: Some(pattern),
            pass_index: match pattern {
                F2QualificationPattern::A => 1,
                F2QualificationPattern::B => 2,
                F2QualificationPattern::HighFps => 1,
                F2QualificationPattern::Texture => 2,
                F2QualificationPattern::Transitions => 3,
                F2QualificationPattern::Memory => 4,
                F2QualificationPattern::Endurance => 5,
                F2QualificationPattern::TransitionShock => 6,
                F2QualificationPattern::Dx11Game => 7,
                F2QualificationPattern::Dx12Game => 8,
            },
            verdict: F2QualificationVerdict::Pass,
            phases_completed: 8,
            phases_expected: 8,
            checksum_count: 8,
            sample_count: 100,
            compute_check_count: 1,
            target_residency_frac: Some(1.0),
            heavy_light_power_delta_w: Some(20.0),
            failure_phase: None,
            retry_count: 0,
            reason: None,
            phase_metrics: Vec::new(),
        });
        o
    }

    fn apply_qualification_pass(
        o: F2Observation,
        pattern: F2QualificationPattern,
    ) -> F2Observation {
        let mut o = qualification_pass_with_pattern(o, pattern);
        o.evidence_kind = F2EvidenceKind::ApplyQualification;
        o.qualification_contract_version = Some(F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION);
        o.mode = F2ObsMode::ApplyQualification;
        if pattern == F2QualificationPattern::Dx11Game {
            o.qualification_coverage.as_mut().unwrap().active_target = Some(F2ActiveTargetCoverage {
                observed_active_ms: 80_000, target_active_ms: 40_000, power_limited_active_ms: 0, light_target_active_ms: 30_000, required_target_ms: 30_000,
                sample_count: 3000, phases_completed: 6, upper_clock_exceeded: false, heavy_target_proven: true,
                diagnostics: None,
            });
        }
        o
    }

    #[test]
    fn clock_envelope_requires_absolute_peak_and_never_promotes_nominal_target() {
        let discovery = obs(1800,900,F2ObsOutcome::Validated);
        let frontier = qualification_pass(discovery.clone());
        let apply = apply_qualification_pass(discovery.clone(),F2QualificationPattern::Texture);
        assert!(is_current_discovery_evidence(&discovery));
        assert!(is_current_qualification_pass(&frontier));
        assert!(is_current_apply_qualification_pass(&apply));
        for original in [discovery,frontier,apply] {
            let restored: F2Observation = serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
            assert_eq!(restored.target_mhz,1800);
            assert_eq!(restored.max_clock_mhz,Some(1815));
            for peak in [None,Some(0),Some(1816),Some(1830)] {
                let mut invalid=original.clone();
                invalid.max_clock_mhz=peak;
                invalid.sustained_upper_clock_mhz=Some(1800);
                assert!(!is_current_discovery_evidence(&invalid));
                assert!(!is_current_qualification_pass(&invalid));
                assert!(!is_current_apply_qualification_pass(&invalid));
            }
        }
    }
    #[test]
    fn dx11_active_proof_is_required_after_serialization() {
        let mut pass = apply_qualification_pass(obs(1800, 900, F2ObsOutcome::Validated), F2QualificationPattern::Dx11Game);
        assert!(is_current_apply_qualification_evidence(&pass));
        pass.qualification_coverage.as_mut().unwrap().active_target = None;
        let restored: F2Observation = serde_json::from_str(&serde_json::to_string(&pass).unwrap()).unwrap();
        assert!(!is_current_apply_qualification_evidence(&restored), "a Pass label cannot replace active exposure proof");
        let coverage = F2ActiveTargetCoverage {
            observed_active_ms:80_000, target_active_ms:40_000, power_limited_active_ms: 0, light_target_active_ms: 30_000, required_target_ms:30_000,
            sample_count:3000, phases_completed:6, upper_clock_exceeded: false, heavy_target_proven: true,
            diagnostics: None,
        };
        let loaded: F2ActiveTargetCoverage = serde_json::from_str(&serde_json::to_string(&coverage).unwrap()).unwrap();
        assert!(loaded.proves_target());
        let mut legacy = serde_json::to_value(&coverage).unwrap();
        legacy.as_object_mut().unwrap().remove("diagnostics");
        let legacy: F2ActiveTargetCoverage = serde_json::from_value(legacy).unwrap();
        assert!(legacy.proves_target(), "diagnostic additions do not erase existing v31 evidence");
        for bad in [
            F2ActiveTargetCoverage {target_active_ms:0,..loaded.clone()},
            F2ActiveTargetCoverage {target_active_ms:90_000,..loaded.clone()},
            F2ActiveTargetCoverage {sample_count:0,..loaded.clone()},
            F2ActiveTargetCoverage {required_target_ms:1,..loaded.clone()},
            F2ActiveTargetCoverage {upper_clock_exceeded: true, heavy_target_proven: true,..loaded.clone()},
            F2ActiveTargetCoverage {light_target_active_ms: 29_999,..loaded.clone()},
            F2ActiveTargetCoverage {phases_completed: 5,..loaded.clone()},
        ] { assert!(!bad.proves_target()); }
    }

    fn legacy_fsgl2_qualification_pass(o: F2Observation) -> F2Observation {
        let mut o = qualification_pass(o);
        o.qualification_coverage.as_mut().unwrap().strength = F2QualificationStrength::Fsgl2;
        o
    }

    #[test]
    fn interleaved_qualification_failure_selects_shallower_qualified_point() {
        // Mirrors the interleaved Cmax descent: PowerRender validates 975→968→962, qualification
        // PASSES at 975 and 968, then FAILS at 962. The deeper failure records a bad observation, so
        // the frontier must select 968 (the deepest QUALIFIED bin), never the rejected 962, and report
        // it as qualified. This is the downstream invariant the per-bin interleaved discovery relies on.
        let t = 1935;
        let mut v = vec![
            obs(t, 975, F2ObsOutcome::Validated),
            qualification_pass(obs(t, 975, F2ObsOutcome::Validated)),
            obs(t, 968, F2ObsOutcome::Validated),
            qualification_pass(obs(t, 968, F2ObsOutcome::Validated)),
            obs(t, 962, F2ObsOutcome::Validated),
        ];
        let mut deeper_qual_fail = obs(t, 962, F2ObsOutcome::Unstable);
        deeper_qual_fail.evidence_kind = F2EvidenceKind::Qualification;
        deeper_qual_fail.discovery_contract_version = None;
        deeper_qual_fail.qualification_contract_version =
            Some(F2_FRONTIER_QUALIFICATION_CONTRACT_VERSION);
        v.push(deeper_qual_fail);

        let entry = frontier_entry_for_target(&v, t).expect("a qualified frontier point exists");
        assert_eq!(
            entry.best_anchor_mv, 968,
            "deepest QUALIFIED bin, not the rejected 962"
        );
        assert!(
            entry.validation_count >= 1,
            "selected point carries its qualification pass"
        );
        assert_eq!(
            entry.first_bad_mv,
            Some(962),
            "the rejected deeper bin bounds the frontier"
        );
    }

    #[test]
    fn outcome_classification() {
        assert!(F2ObsOutcome::Validated.is_validated());
        for o in [
            F2ObsOutcome::VerifierFailed,
            F2ObsOutcome::SilentError,
            F2ObsOutcome::Unstable,
            F2ObsOutcome::DeviceLost,
            F2ObsOutcome::ClockDrop,
            F2ObsOutcome::ResetFailed,
            F2ObsOutcome::Blacklisted,
            F2ObsOutcome::CrashOrRecovery,
        ] {
            assert!(o.is_bad(), "{o:?} should be bad");
        }
        // Planner/gate refusals performed no write → NOT "bad" (no instability learned).
        assert!(!F2ObsOutcome::RejectedByPlanner.is_bad());
        assert!(!F2ObsOutcome::AbortedBySafetyGate.is_bad());
        assert!(!F2ObsOutcome::QualificationInconclusive.is_bad());
        // Only reset-failure and unrecovered crash are SAFETY failures.
        assert!(F2ObsOutcome::ResetFailed.is_safety_failure());
        assert!(F2ObsOutcome::DeviceLost.is_safety_failure());
        assert!(F2ObsOutcome::CrashOrRecovery.is_safety_failure());
        assert!(!F2ObsOutcome::Unstable.is_safety_failure());
        assert!(!F2ObsOutcome::ClockDrop.is_safety_failure());
    }

    #[test]
    fn last_good_is_lowest_validated() {
        let v = vec![
            obs(1800, 975, F2ObsOutcome::Validated),
            obs(1800, 968, F2ObsOutcome::Validated),
            obs(1800, 962, F2ObsOutcome::Validated),
            obs(1800, 956, F2ObsOutcome::Unstable),
        ];
        assert_eq!(last_good_for_target(&v, 1800).unwrap().anchor_mv, 962);
        // A target with no validated point → None.
        assert!(last_good_for_target(&v, 1815).is_none());
    }

    #[test]
    fn later_failure_invalidates_same_or_deeper_old_validations() {
        let v = vec![
            obs(1800, 975, F2ObsOutcome::Validated),
            obs(1800, 968, F2ObsOutcome::Validated),
            obs(1800, 968, F2ObsOutcome::SilentError),
        ];
        assert_eq!(last_good_for_target(&v, 1800).unwrap().anchor_mv, 975);
    }

    #[test]
    fn first_bad_is_highest_failure() {
        let v = vec![
            obs(1800, 962, F2ObsOutcome::Validated),
            obs(1800, 956, F2ObsOutcome::Unstable),
            obs(1800, 950, F2ObsOutcome::DeviceLost),
        ];
        // The shallowest (highest-voltage) failure brackets Vmin from below.
        assert_eq!(first_bad_for_target(&v, 1800).unwrap().anchor_mv, 956);
    }

    #[test]
    fn bracket_only_when_consistent() {
        let consistent = vec![
            obs(1800, 962, F2ObsOutcome::Validated),
            obs(1800, 956, F2ObsOutcome::Unstable),
        ];
        let b = bracket_for_target(&consistent, 1800).unwrap();
        assert_eq!((b.first_bad_mv, b.last_good_mv, b.width_mv), (956, 962, 6));
        // Inconsistent: a failure at/above the lowest validated point → no bracket claimed.
        let inconsistent = vec![
            obs(1800, 962, F2ObsOutcome::Validated),
            obs(1800, 968, F2ObsOutcome::Unstable),
        ];
        assert!(bracket_for_target(&inconsistent, 1800).is_none());
    }

    #[test]
    fn current_clock_drop_and_contaminated_failure_are_not_nominal_instability() {
        let mut candidate=obs(1800,875,F2ObsOutcome::ClockDrop);
        assert!(first_bad_for_target(&[candidate.clone()],1800).is_none());
        candidate.outcome=F2ObsOutcome::SilentError;
        candidate.inconclusive_reason=Some("control_failure_outside_requested_pair".into());
        assert!(!is_known_bad(&[candidate.clone()],1800,875));
        candidate.inconclusive_reason=None;
        assert!(is_known_bad(&[candidate],1800,875));
    }

    #[test]
    fn is_known_bad_is_conservative_downward() {
        let v = vec![obs(1800, 956, F2ObsOutcome::Unstable)];
        // The exact failed voltage and anything LOWER is known bad.
        assert!(is_known_bad(&v, 1800, 956));
        assert!(is_known_bad(&v, 1800, 950));
        // A HIGHER voltage is not implied bad by a lower-voltage failure.
        assert!(!is_known_bad(&v, 1800, 962));
        // Different target is unaffected.
        assert!(!is_known_bad(&v, 1815, 956));
    }

    #[test]
    fn harder_qualified_lower_voltage_reopens_only_clock_drop_boundary() {
        let mut clock_drop = obs(1815, 968, F2ObsOutcome::ClockDrop);
        clock_drop.discovery_contract_version = Some(F2_DISCOVERY_CONTRACT_VERSION - 1);
        assert_eq!(
            first_bad_for_target(std::slice::from_ref(&clock_drop), 1815).map(|bad| bad.anchor_mv),
            Some(968)
        );

        let harder_discovery = obs(1935, 906, F2ObsOutcome::Validated);
        let harder_qualification = qualification_pass(harder_discovery.clone());
        let reconciled = [
            clock_drop,
            harder_discovery.clone(),
            harder_qualification.clone(),
        ];
        assert!(first_bad_for_target(&reconciled, 1815).is_none());
        assert!(!is_known_bad(&reconciled, 1815, 900));

        let silent_error = obs(1815, 968, F2ObsOutcome::SilentError);
        let physical_failure = [silent_error, harder_discovery, harder_qualification];
        assert_eq!(
            first_bad_for_target(&physical_failure, 1815).map(|bad| bad.anchor_mv),
            Some(968),
            "a harder pass must never erase direct instability evidence"
        );
    }

    #[test]
    fn same_target_lower_qualified_pair_reopens_only_clock_drop_boundary() {
        let mut clock_drop = obs(1815, 968, F2ObsOutcome::ClockDrop);
        clock_drop.discovery_contract_version = Some(F2_DISCOVERY_CONTRACT_VERSION - 1);
        let lower_discovery = obs(1815, 962, F2ObsOutcome::Validated);
        let lower_qualification = qualification_pass(lower_discovery.clone());
        let reconciled = [clock_drop.clone(), lower_discovery, lower_qualification];
        assert!(first_bad_for_target(&reconciled, 1815).is_none());
        assert!(!is_known_bad(&reconciled, 1815, 950));

        let equal_discovery = obs(1815, 968, F2ObsOutcome::Validated);
        let equal_qualification = qualification_pass(equal_discovery.clone());
        let not_strictly_lower = [clock_drop, equal_discovery, equal_qualification];
        assert_eq!(
            first_bad_for_target(&not_strictly_lower, 1815).map(|bad| bad.anchor_mv),
            Some(968),
            "same-target domination requires a strictly lower voltage"
        );
    }

    #[test]
    fn validated_descent_baseline_picks_deepest_clean_validated() {
        let v = vec![
            obs(1800, 975, F2ObsOutcome::Validated),
            obs(1800, 968, F2ObsOutcome::Validated),
            obs(1800, 956, F2ObsOutcome::Unstable), // a real failure is not a baseline
        ];
        // Deepest (lowest-voltage) clean validated point — its offset is the resume baseline.
        let b = validated_descent_baseline(&v, 1800, None).unwrap();
        assert_eq!(b.anchor_mv, 968);
        // A target with no validated point → None (descent then starts from stock 0).
        assert!(validated_descent_baseline(&v, 1815, None).is_none());
    }

    #[test]
    fn validated_descent_baseline_ignores_no_write_aborts_and_other_gpus() {
        // A no-write safety-gate abort is NOT a baseline even at the lowest voltage.
        let mut aborted = obs(1800, 950, F2ObsOutcome::AbortedBySafetyGate);
        aborted.reset_to_stock_ok = true;
        aborted.boot_flag_cleared = true;
        aborted.unstable = false;
        let v = vec![obs(1800, 975, F2ObsOutcome::Validated), aborted];
        assert_eq!(
            validated_descent_baseline(&v, 1800, None)
                .unwrap()
                .anchor_mv,
            975
        );
        // A validated point on a DIFFERENT GPU must not bound this GPU's descent.
        let mut other_gpu = obs(1800, 962, F2ObsOutcome::Validated);
        other_gpu.gpu_key = Some("RTX 4090".into());
        let v2 = vec![obs(1800, 975, F2ObsOutcome::Validated), other_gpu];
        assert_eq!(
            validated_descent_baseline(&v2, 1800, Some("RTX 3060 Ti"))
                .unwrap()
                .anchor_mv,
            975
        );
        // Unfiltered (gpu_key None) sees both → deepest wins.
        assert_eq!(
            validated_descent_baseline(&v2, 1800, None)
                .unwrap()
                .anchor_mv,
            962
        );
    }

    #[test]
    fn validated_descent_baseline_rejects_dirty_cleanup_flags() {
        // A record claiming Validated but with an un-cleared boot flag is not trusted as a baseline.
        let mut dirty = obs(1800, 962, F2ObsOutcome::Validated);
        dirty.boot_flag_cleared = false;
        let v = vec![obs(1800, 975, F2ObsOutcome::Validated), dirty];
        assert_eq!(
            validated_descent_baseline(&v, 1800, None)
                .unwrap()
                .anchor_mv,
            975
        );
    }

    #[test]
    fn frontier_confidence_monotone() {
        assert_eq!(frontier_confidence(0), 0.0);
        assert!(frontier_confidence(1) >= 0.85); // a single clean validation clears the balanced gate
        assert!(frontier_confidence(2) > frontier_confidence(1));
        assert!(frontier_confidence(100) <= 0.99);
    }

    #[test]
    fn confidence_comes_from_dwell_duration_and_independent_passes() {
        let mut short = obs(1800, 975, F2ObsOutcome::Validated);
        short.dwell_duration_ms = Some(10_000);
        short.sample_count = Some(100);
        let standard = obs(1800, 975, F2ObsOutcome::Validated);
        let mut long = obs(1800, 975, F2ObsOutcome::Validated);
        long.dwell_duration_ms = Some(35_000);
        let short_conf = frontier_confidence_from_evidence(&[&short]);
        let standard_conf = frontier_confidence_from_evidence(&[&standard]);
        let long_conf = frontier_confidence_from_evidence(&[&long, &long, &long]);
        assert!(short_conf < standard_conf);
        assert!(standard_conf >= 0.85);
        assert!(long_conf > standard_conf);
        assert!(long_conf <= 0.99);
    }

    #[test]
    fn learned_frontier_is_scoped_to_exact_gpu_key() {
        let a = obs(1800, 975, F2ObsOutcome::Validated);
        let mut b = obs(1950, 1000, F2ObsOutcome::Validated);
        b.gpu_key = Some("GPU-B-UUID".into());
        let fr = learned_frontier_for_gpu(&[a, b], "RTX 3060 Ti");
        assert_eq!(
            fr.iter().map(|e| e.target_mhz).collect::<Vec<_>>(),
            vec![1800]
        );
    }

    #[test]
    fn learned_frontier_picks_best_per_target() {
        let v = vec![
            obs(1800, 975, F2ObsOutcome::Validated),
            obs(1800, 968, F2ObsOutcome::Validated),
            obs(1800, 962, F2ObsOutcome::Validated),
            obs(1800, 956, F2ObsOutcome::Unstable),
            obs(1815, 980, F2ObsOutcome::Validated),
            obs(1830, 990, F2ObsOutcome::Unstable), // no validated → excluded
        ];
        let fr = learned_frontier(&v);
        // Two targets have a validated point (1800, 1815); 1830 has none → excluded.
        assert_eq!(
            fr.iter().map(|e| e.target_mhz).collect::<Vec<_>>(),
            vec![1800, 1815]
        );
        let e1800 = &fr[0];
        assert_eq!(e1800.best_anchor_mv, 962); // lowest validated
        assert_eq!(e1800.first_bad_mv, Some(956));
        assert_eq!(e1800.bracket_width_mv, Some(6));
        assert_eq!(e1800.observation_count, 4);
        assert_eq!(e1800.validation_count, 0);
        assert!(e1800.confidence >= 0.85);
    }

    #[test]
    fn learned_frontier_counts_only_current_qualification_passes() {
        let discovery = obs(1800, 962, F2ObsOutcome::Validated);
        let mut inconclusive = qualification_pass(discovery.clone());
        inconclusive.outcome = F2ObsOutcome::QualificationInconclusive;
        inconclusive
            .qualification_coverage
            .as_mut()
            .unwrap()
            .verdict = F2QualificationVerdict::Inconclusive;
        let mut old_pass = qualification_pass(discovery.clone());
        old_pass.qualification_contract_version =
            Some(F2_FRONTIER_QUALIFICATION_CONTRACT_VERSION - 1);
        let current_pass = qualification_pass(discovery.clone());

        let entry =
            frontier_entry_for_target(&[discovery, inconclusive, old_pass, current_pass], 1800)
                .unwrap();
        assert_eq!(entry.best_anchor_mv, 962);
        assert_eq!(entry.validation_count, 1);
    }

    #[test]
    fn qualified_frontier_keeps_shallower_pass_when_deeper_point_is_inconclusive() {
        let target = 1860;
        let qualified_discovery = obs(target, 937, F2ObsOutcome::Validated);
        let qualified = qualification_pass(qualified_discovery.clone());
        let deeper_discovery = obs(target, 931, F2ObsOutcome::Validated);
        let mut deeper_inconclusive = qualification_pass(deeper_discovery.clone());
        deeper_inconclusive.outcome = F2ObsOutcome::QualificationInconclusive;
        deeper_inconclusive
            .qualification_coverage
            .as_mut()
            .unwrap()
            .verdict = F2QualificationVerdict::Inconclusive;

        let observations = [
            qualified_discovery,
            qualified,
            deeper_discovery,
            deeper_inconclusive,
        ];
        assert_eq!(
            frontier_entry_for_target(&observations, target)
                .unwrap()
                .best_anchor_mv,
            931,
            "the physical discovery frontier preserves the deeper unproven observation"
        );
        let publishable = qualified_frontier_entry_for_target(&observations, target).unwrap();
        assert_eq!(publishable.best_anchor_mv, 937);
        assert_eq!(publishable.validation_count, 1);
    }

    #[test]
    fn qualified_frontier_for_gpu_excludes_targets_without_current_qualification() {
        let qualified_discovery = obs(1860, 937, F2ObsOutcome::Validated);
        let qualified = qualification_pass(qualified_discovery.clone());
        let unqualified = obs(1785, 937, F2ObsOutcome::Validated);
        let frontier = qualified_frontier_for_gpu(
            &[qualified_discovery, qualified, unqualified],
            "RTX 3060 Ti",
        );
        assert_eq!(
            frontier
                .iter()
                .map(|entry| (entry.target_mhz, entry.best_anchor_mv))
                .collect::<Vec<_>>(),
            vec![(1860, 937)]
        );
    }

    #[test]
    fn monotonic_qualified_frontier_uses_measured_shallower_alternative() {
        let low = obs(1695, 887, F2ObsOutcome::Validated);
        let high_deep = obs(1710, 881, F2ObsOutcome::Validated);
        let high_monotonic = obs(1710, 887, F2ObsOutcome::Validated);
        let observations = [
            low.clone(),
            qualification_pass(low),
            high_deep.clone(),
            qualification_pass(high_deep),
            high_monotonic.clone(),
            qualification_pass(high_monotonic),
        ];

        let frontier = monotonic_qualified_frontier_for_gpu(&observations, "RTX 3060 Ti");
        assert_eq!(
            frontier
                .iter()
                .map(|entry| (entry.target_mhz, entry.best_anchor_mv))
                .collect::<Vec<_>>(),
            vec![(1695, 887), (1710, 887)]
        );
    }

    #[test]
    fn monotonic_qualified_frontier_allows_measured_plateau() {
        let low = obs(1695, 887, F2ObsOutcome::Validated);
        let high = obs(1710, 887, F2ObsOutcome::Validated);
        let observations = [
            low.clone(),
            qualification_pass(low),
            high.clone(),
            qualification_pass(high),
        ];

        let frontier = monotonic_qualified_frontier_for_gpu(&observations, "RTX 3060 Ti");
        assert_eq!(
            frontier
                .iter()
                .map(|entry| (entry.target_mhz, entry.best_anchor_mv))
                .collect::<Vec<_>>(),
            vec![(1695, 887), (1710, 887)]
        );
    }

    #[test]
    fn monotonic_qualified_frontier_omits_target_without_measured_alternative() {
        let low = obs(1695, 887, F2ObsOutcome::Validated);
        let inverted_only = obs(1710, 881, F2ObsOutcome::Validated);
        let later = obs(1725, 893, F2ObsOutcome::Validated);
        let observations = [
            low.clone(),
            qualification_pass(low),
            inverted_only.clone(),
            qualification_pass(inverted_only),
            later.clone(),
            qualification_pass(later),
        ];

        let frontier = monotonic_qualified_frontier_for_gpu(&observations, "RTX 3060 Ti");
        assert_eq!(
            frontier
                .iter()
                .map(|entry| (entry.target_mhz, entry.best_anchor_mv))
                .collect::<Vec<_>>(),
            vec![(1695, 887), (1725, 893)],
            "a missing compatible measurement is omitted, never synthesized"
        );
    }

    #[test]
    fn frontier_voltage_order_allows_plateau_but_rejects_inversion() {
        let entry = |target_mhz, best_anchor_mv| {
            let discovery = obs(target_mhz, best_anchor_mv, F2ObsOutcome::Validated);
            let qualification = qualification_pass(discovery.clone());
            qualified_frontier_entry_for_target(&[discovery, qualification], target_mhz).unwrap()
        };
        let plateau = [entry(1815, 975), entry(1860, 975), entry(1875, 981)];
        assert_eq!(frontier_voltage_order_violation(&plateau), None);

        let inverted = [entry(1815, 975), entry(1875, 975), entry(1935, 906)];
        assert_eq!(
            frontier_voltage_order_violation(&inverted),
            Some((1815, 975, 1935, 906))
        );
    }

    #[test]
    fn learned_frontier_ignores_legacy_strengths_for_apply_qualification() {
        let discovery = obs(1800, 962, F2ObsOutcome::Validated);
        let mut fsgl1 = qualification_pass(discovery.clone());
        fsgl1.qualification_coverage.as_mut().unwrap().strength = F2QualificationStrength::Fsgl1;
        fsgl1.qualification_coverage.as_mut().unwrap().pattern = None;
        let fsgl2 = legacy_fsgl2_qualification_pass(discovery.clone());
        let current_fsgl3 = qualification_pass(discovery.clone());

        let entry =
            frontier_entry_for_target(&[discovery, fsgl1, fsgl2, current_fsgl3], 1800).unwrap();
        assert_eq!(entry.best_anchor_mv, 962);
        assert_eq!(entry.validation_count, 1);
    }

    #[test]
    fn learned_frontier_counts_distinct_v7_patterns() {
        let discovery = obs(1800, 962, F2ObsOutcome::Validated);
        let texture_1 =
            qualification_pass_with_pattern(discovery.clone(), F2QualificationPattern::Texture);
        let texture_2 =
            qualification_pass_with_pattern(discovery.clone(), F2QualificationPattern::Texture);
        let only_one =
            frontier_entry_for_target(&[discovery.clone(), texture_1, texture_2], 1800).unwrap();
        assert_eq!(only_one.validation_count, 1);

        // v14: only Texture is REQUIRED. Non-required patterns (Transitions/Memory, now folded into
        // the candidate-only composite Endurance) must NOT inflate the frontier validation count.
        let texture =
            qualification_pass_with_pattern(discovery.clone(), F2QualificationPattern::Texture);
        let transitions =
            qualification_pass_with_pattern(discovery.clone(), F2QualificationPattern::Transitions);
        let memory =
            qualification_pass_with_pattern(discovery.clone(), F2QualificationPattern::Memory);
        let complete =
            frontier_entry_for_target(&[discovery, texture, transitions, memory], 1800).unwrap();
        assert_eq!(complete.validation_count, 1);
    }

    #[test]
    fn apply_qualification_is_current_but_does_not_rewrite_frontier_failure_bracket() {
        let discovery = obs(1800, 881, F2ObsOutcome::Validated);
        let apply_pass = apply_qualification_pass(
            obs(1800, 893, F2ObsOutcome::Validated),
            F2QualificationPattern::Texture,
        );
        assert!(is_current_apply_qualification_pass(&apply_pass));
        assert!(!is_current_qualification_pass(&apply_pass));

        let mut apply_failure = apply_qualification_pass(
            obs(1800, 893, F2ObsOutcome::SilentError),
            F2QualificationPattern::Texture,
        );
        apply_failure
            .qualification_coverage
            .as_mut()
            .unwrap()
            .verdict = F2QualificationVerdict::Fail;
        let observations = [discovery, apply_pass, apply_failure];
        assert_eq!(
            last_discovery_good_for_target(&observations, 1800)
                .unwrap()
                .anchor_mv,
            881
        );
        assert!(first_bad_for_target(&observations, 1800).is_none());
    }

    #[test]
    fn frontier_v28_and_exact_apply_v29_are_independently_current() {
        let discovery = obs(1800, 881, F2ObsOutcome::Validated);
        let frontier = qualification_pass(discovery.clone());
        assert_eq!(
            F2_QUALIFICATION_CONTRACT_VERSION,
            F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION
        );
        assert_eq!(
            frontier.qualification_contract_version,
            Some(F2_FRONTIER_QUALIFICATION_CONTRACT_VERSION)
        );
        assert!(is_current_qualification_pass(&frontier));

        let exact = apply_qualification_pass(discovery.clone(), F2QualificationPattern::Texture);
        assert_eq!(
            exact.qualification_contract_version,
            Some(F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION)
        );
        assert!(is_current_apply_qualification_pass(&exact));

        let exact_dx11 = apply_qualification_pass(discovery, F2QualificationPattern::Dx11Game);
        assert!(is_current_apply_qualification_evidence(&exact_dx11));

        let mut pre_v29_exact = exact.clone();
        pre_v29_exact.qualification_contract_version =
            Some(F2_FRONTIER_QUALIFICATION_CONTRACT_VERSION);
        assert!(!is_current_apply_qualification_pass(&pre_v29_exact));

        let mut exact_version_on_frontier = frontier;
        exact_version_on_frontier.qualification_contract_version =
            Some(F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION);
        assert!(!is_current_qualification_pass(&exact_version_on_frontier));
    }

    #[test]
    fn bridge_builds_classifier_compatible_points() {
        let mut capped = obs(1800, 962, F2ObsOutcome::Validated);
        capped.power_capped_frac = Some(1.0);
        let v = vec![capped];
        let fr = learned_frontier(&v);
        let (p, conf) = to_power_sweep_point(&fr[0]);
        // F2 retains the cap evidence in its frontier, but its sustained point must remain eligible
        // for synthesis: F1's "power-bound probe" exclusion has different semantics.
        assert_eq!(fr[0].power_capped_frac, Some(1.0));
        assert!(p.stable);
        assert_eq!(p.power_capped_frac, 0.0);
        assert_eq!(p.vf_table_voltage_mv, Some(962));
        assert_eq!(p.boundary_voltage_mv, Some(962));
        assert_eq!(p.apply_margin_mv, Some(0));
        assert_eq!(p.target_clock_mhz, Some(1800));
        assert_eq!(p.clock_mhz, 1815);
        assert_eq!(p.p5_clock_mhz, Some(1815));
        assert_eq!(p.p95_clock_mhz, Some(1815));
        assert_eq!(p.power_w, 180.0);
        assert_eq!(p.max_power_w, 188.0);
        assert_eq!(p.power_p99_w, Some(186.0));
        assert_eq!(p.max_temp_c, Some(68.0));
        assert_eq!(p.confidence, Some(conf));
        assert_eq!(p.validation_count, Some(0));
        assert!(p.perf_per_watt > 0.0);
        assert!(conf >= 0.85); // passes the balanced confidence gate
                               // Whole-frontier bridge preserves count.
        assert_eq!(frontier_to_points(&fr).len(), 1);
    }

    #[test]
    fn current_positive_evidence_requires_proven_cleanup() {
        let discovery = obs(1800, 962, F2ObsOutcome::Validated);
        assert!(is_current_discovery_evidence(&discovery));

        let mut discovery_reset_failed = discovery.clone();
        discovery_reset_failed.reset_to_stock_ok = false;
        assert!(!is_current_discovery_evidence(&discovery_reset_failed));

        let mut discovery_flag_retained = discovery.clone();
        discovery_flag_retained.boot_flag_cleared = false;
        assert!(!is_current_discovery_evidence(&discovery_flag_retained));

        let qualification = qualification_pass(discovery.clone());
        assert!(is_current_qualification_pass(&qualification));

        let mut qualification_reset_failed = qualification.clone();
        qualification_reset_failed.reset_to_stock_ok = false;
        assert!(!is_current_qualification_pass(&qualification_reset_failed));

        let mut qualification_flag_retained = qualification.clone();
        qualification_flag_retained.boot_flag_cleared = false;
        assert!(!is_current_qualification_pass(&qualification_flag_retained));

        let apply = apply_qualification_pass(discovery, F2QualificationPattern::Texture);
        assert!(is_current_apply_qualification_pass(&apply));

        let mut apply_reset_failed = apply.clone();
        apply_reset_failed.reset_to_stock_ok = false;
        assert!(!is_current_apply_qualification_pass(&apply_reset_failed));

        let mut apply_flag_retained = apply;
        apply_flag_retained.boot_flag_cleared = false;
        assert!(!is_current_apply_qualification_pass(&apply_flag_retained));
    }

    #[test]
    fn current_positive_evidence_requires_reproducible_provenance() {
        let discovery = obs(1800, 962, F2ObsOutcome::Validated);
        assert!(is_current_discovery_evidence(&discovery));

        let mut missing = discovery.clone();
        missing.evidence_provenance = None;
        assert!(!is_current_discovery_evidence(&missing));

        let mut unknown_build = discovery.clone();
        unknown_build
            .evidence_provenance
            .as_mut()
            .unwrap()
            .build_revision = None;
        assert!(!is_current_discovery_evidence(&unknown_build));

        let mut missing_driver = qualification_pass(discovery.clone());
        let provenance = missing_driver.evidence_provenance.as_mut().unwrap();
        provenance.driver_name = None;
        provenance.driver_info = None;
        assert!(!is_current_qualification_pass(&missing_driver));

        let mut missing_checksum =
            apply_qualification_pass(discovery, F2QualificationPattern::Texture);
        missing_checksum
            .evidence_provenance
            .as_mut()
            .unwrap()
            .checksum_method = Some(" ".into());
        assert!(!is_current_apply_qualification_pass(&missing_checksum));
    }

    #[test]
    fn missing_legacy_cleanup_fields_default_false_and_cannot_qualify() {
        let discovery = obs(1800, 962, F2ObsOutcome::Validated);
        let mut legacy_discovery = serde_json::to_value(discovery).unwrap();
        let discovery_object = legacy_discovery.as_object_mut().unwrap();
        discovery_object.remove("reset_to_stock_ok");
        discovery_object.remove("boot_flag_cleared");
        let legacy_discovery: F2Observation = serde_json::from_value(legacy_discovery).unwrap();
        assert!(!legacy_discovery.reset_to_stock_ok);
        assert!(!legacy_discovery.boot_flag_cleared);
        assert!(!is_current_discovery_evidence(&legacy_discovery));

        let qualification = qualification_pass(obs(1800, 962, F2ObsOutcome::Validated));
        let mut legacy_qualification = serde_json::to_value(qualification).unwrap();
        let qualification_object = legacy_qualification.as_object_mut().unwrap();
        qualification_object.remove("reset_to_stock_ok");
        qualification_object.remove("boot_flag_cleared");
        let legacy_qualification: F2Observation =
            serde_json::from_value(legacy_qualification).unwrap();
        assert!(!legacy_qualification.reset_to_stock_ok);
        assert!(!legacy_qualification.boot_flag_cleared);
        assert!(!is_current_qualification_pass(&legacy_qualification));
    }

    #[test]
    fn discovery_v7_rejects_v6_and_unconfirmed_positive_evidence() {
        let mut old = obs(1800, 962, F2ObsOutcome::Validated);
        old.discovery_contract_version = Some(3);
        assert!(!is_current_discovery_evidence(&old));
        assert!(last_discovery_good_for_target(&[old.clone()], 1800).is_none());
        assert!(learned_frontier(&[old]).is_empty());

        let mut unconfirmed = obs(1800, 962, F2ObsOutcome::Validated);
        unconfirmed.power_p99_confirmed = false;
        assert!(!is_current_discovery_evidence(&unconfirmed));
        assert!(learned_frontier(&[unconfirmed]).is_empty());

        let mut legacy = obs(1800, 962, F2ObsOutcome::Validated);
        legacy.evidence_kind = F2EvidenceKind::Legacy;
        legacy.discovery_contract_version = None;
        assert!(!is_current_discovery_evidence(&legacy));
        assert!(last_discovery_good_for_target(&[legacy], 1800).is_none());
    }

    #[test]
    fn apply_anchor_power_uses_highest_current_thermal_safe_p99() {
        let mut lower_p99 = obs(1800, 975, F2ObsOutcome::Validated);
        lower_p99.max_watts = Some(205);
        lower_p99.power_p99_w = Some(188.0);
        let mut higher_p99 = lower_p99.clone();
        higher_p99.max_watts = Some(198);
        higher_p99.power_p99_w = Some(196.0);
        higher_p99.watts = Some(190);
        higher_p99.outcome = F2ObsOutcome::PowerBoundClockDrop;
        let mut throttled = higher_p99.clone();
        throttled.max_watts = Some(200);
        throttled.power_p99_w = Some(200.0);
        throttled.thermal_throttled = true;
        let mut old = higher_p99.clone();
        old.max_watts = Some(199);
        old.power_p99_w = Some(199.0);
        old.discovery_contract_version = Some(F2_DISCOVERY_CONTRACT_VERSION - 1);

        let observations = [lower_p99, higher_p99, throttled, old];
        let selected =
            current_discovery_observation_at_anchor(&observations, 1800, 975, "RTX 3060 Ti")
                .unwrap();
        assert_eq!(selected.watts, Some(190));
        assert_eq!(selected.max_watts, Some(198));
        assert_eq!(selected.power_p99_w, Some(196.0));
        assert!(!selected.thermal_throttled);
    }

    #[test]
    fn apply_qualification_power_requires_complete_clean_set_and_uses_highest_p99() {
        let mut high_fps = apply_qualification_pass(
            obs(1830, 862, F2ObsOutcome::Validated),
            F2QualificationPattern::HighFps,
        );
        high_fps.run_id = "apply-v7".into();
        high_fps.power_p99_w = Some(172.25);
        let mut texture = apply_qualification_pass(
            obs(1830, 862, F2ObsOutcome::Validated),
            F2QualificationPattern::Texture,
        );
        texture.run_id = "apply-v7".into();
        texture.power_p99_w = Some(172.587);
        // v14: Texture is the sole REQUIRED pattern, so it carries the sustained p95 clock the
        // publish gate reads (Transitions/Memory are folded into candidate-only Endurance).
        texture.sustained_upper_clock_mhz = Some(1890);
        let mut transitions = apply_qualification_pass(
            obs(1830, 862, F2ObsOutcome::Validated),
            F2QualificationPattern::Transitions,
        );
        transitions.run_id = "apply-v7".into();
        transitions.power_p99_w = Some(173.125);
        transitions.sustained_upper_clock_mhz = Some(1890);
        let mut memory = apply_qualification_pass(
            obs(1830, 862, F2ObsOutcome::Validated),
            F2QualificationPattern::Memory,
        );
        memory.run_id = "apply-v7".into();
        memory.power_p99_w = Some(172.9);
        let mut endurance = apply_qualification_pass(
            obs(1830, 862, F2ObsOutcome::Validated),
            F2QualificationPattern::Endurance,
        );
        endurance.run_id = "apply-v7".into();
        endurance.power_p99_w = Some(181.5);
        let mut dx11 = apply_qualification_pass(
            obs(1830, 862, F2ObsOutcome::Validated),
            F2QualificationPattern::Dx11Game,
        );
        dx11.run_id = "apply-v7".into();
        dx11.power_p99_w = Some(176.0);
        let mut dx12 = apply_qualification_pass(
            obs(1830, 862, F2ObsOutcome::Validated),
            F2QualificationPattern::Dx12Game,
        );
        dx12.run_id = "apply-v7".into();
        dx12.power_p99_w = Some(177.0);
        // Thermal slowdown that ALSO sagged the sustained clock below tolerance stays excluded
        // (fail-closed). The held-clock case (throttle but clock >= target) is trusted now and is
        // covered by `apply_qualification_held_thermal_reading_is_trusted_but_sag_is_excluded`.
        let mut throttled = texture.clone();
        throttled.power_p99_w = Some(180.0);
        throttled.thermal_throttled = true;
        throttled.sustained_clock_mhz = Some(1830 - F2_APPLY_CLOCK_HOLD_TOL_MHZ - 1);
        let mut other_run_high_fps = high_fps.clone();
        other_run_high_fps.run_id = "other-run".into();
        other_run_high_fps.power_p99_w = Some(189.0);
        let mut other_run_texture = texture.clone();
        other_run_texture.run_id = "other-run".into();
        other_run_texture.power_p99_w = Some(190.0);
        let mut other_run_transitions = transitions.clone();
        other_run_transitions.run_id = "other-run".into();
        other_run_transitions.power_p99_w = Some(191.0);
        let mut other_run_memory = memory.clone();
        other_run_memory.run_id = "other-run".into();
        other_run_memory.power_p99_w = Some(190.5);
        let mut other_run_endurance = endurance.clone();
        other_run_endurance.run_id = "other-run".into();
        other_run_endurance.power_p99_w = Some(192.0);
        let mut other_run_dx11 = dx11.clone();
        other_run_dx11.run_id = "other-run".into();
        other_run_dx11.power_p99_w = Some(191.5);
        let mut other_run_dx12 = dx12.clone();
        other_run_dx12.run_id = "other-run".into();
        other_run_dx12.power_p99_w = Some(191.8);

        throttled.qualification_coverage.as_mut().unwrap().verdict = F2QualificationVerdict::Inconclusive;
        throttled.run_id = "incomplete-thermal-run".into();
        let observations = [
            high_fps.clone(),
            texture,
            transitions,
            memory,
            dx11,
            dx12,
            endurance,
            throttled,
            other_run_high_fps,
            other_run_texture,
            other_run_transitions,
            other_run_memory,
            other_run_dx11,
            other_run_dx12,
            other_run_endurance,
        ];
        assert_eq!(
            current_apply_qualification_p99_at_anchor(
                &observations,
                "apply-v7",
                1830,
                862,
                "RTX 3060 Ti"
            ),
            Some(172.587)
        );
        assert_eq!(
            current_apply_qualification_p95_clock_at_anchor(
                &observations,
                "apply-v7",
                1830,
                862,
                "RTX 3060 Ti"
            ),
            Some(1890)
        );
        assert_eq!(
            highest_apply_qualification_p99_at_anchor(&observations, 1830, 862, "RTX 3060 Ti"),
            Some(190.0),
            "restored snapshots use the highest complete approved run"
        );
        assert_eq!(
            current_complete_apply_gate_p99_at_anchor(
                &observations,
                "apply-v7",
                1830,
                862,
                "RTX 3060 Ti"
            ),
            Some(181.5),
            "complete gate power includes the harsher Endurance p99"
        );
        assert_eq!(
            highest_complete_apply_gate_p99_at_anchor(&observations, 1830, 862, "RTX 3060 Ti"),
            Some(192.0),
            "restored snapshots preserve the worst complete-gate run"
        );
        assert_eq!(
            current_apply_qualification_p99_at_anchor(
                &[high_fps],
                "apply-v7",
                1830,
                862,
                "RTX 3060 Ti"
            ),
            None,
            "one approved pattern alone cannot publish soak power"
        );
    }

    #[test]
    fn apply_qualification_held_thermal_reading_is_trusted_but_sag_is_excluded() {
        // A complete v7 triad whose dwells thermal-throttled but HELD the target clock (sustained
        // p5 >= target - tol) is now trusted: the exact-Apply point ran at its real operating
        // clock/power despite a momentary hotspot, so p95/p99 publish rather than fail closed. This
        // is the fix for a power-bound top point (e.g. 1935 @ 200 W cap) that hotspot-throttles on
        // every 5-min soak and would otherwise leave a whole run with zero applicable profiles.
        let held = |pattern: F2QualificationPattern, p99: f32| -> F2Observation {
            let mut o = apply_qualification_pass(obs(1935, 956, F2ObsOutcome::Validated), pattern);
            o.run_id = "held-thermal".into();
            o.gpu_key = Some("RTX 4070".into());
            o.thermal_throttled = true;
            o.sustained_clock_mhz = Some(1935); // held at target despite the flag
            o.sustained_upper_clock_mhz = Some(1965);
            o.power_p99_w = Some(p99);
            o
        };
        let held_set = [
            held(F2QualificationPattern::HighFps, 199.0),
            held(F2QualificationPattern::Texture, 199.7),
            held(F2QualificationPattern::Transitions, 199.1),
            held(F2QualificationPattern::Memory, 199.3),
        ];
        assert_eq!(
            current_apply_qualification_p99_at_anchor(
                &held_set,
                "held-thermal",
                1935,
                956,
                "RTX 4070"
            ),
            Some(199.7),
            "held-clock thermal readings publish (highest p99), never understating power"
        );
        assert_eq!(
            current_apply_qualification_p95_clock_at_anchor(
                &held_set,
                "held-thermal",
                1935,
                956,
                "RTX 4070"
            ),
            Some(1965),
            "held-clock thermal readings publish the sustained upper clock"
        );

        // Sag the sustained clock below tolerance on one pattern → that pattern is untrustworthy,
        // the triad is incomplete, and both gates fail closed.
        let mut sagged_set = held_set.clone();
        sagged_set[1].sustained_clock_mhz = Some(1935 - F2_APPLY_CLOCK_HOLD_TOL_MHZ - 1);
        assert!(current_apply_qualification_p99_at_anchor(&sagged_set, "held-thermal", 1935, 956, "RTX 4070").is_some(),
            "aggregate idle downclock cannot override proven heavy phases");
        sagged_set[1].qualification_coverage.as_mut().unwrap().verdict = F2QualificationVerdict::Inconclusive;
        assert_eq!(
            current_apply_qualification_p99_at_anchor(
                &sagged_set,
                "held-thermal",
                1935,
                956,
                "RTX 4070"
            ),
            None,
            "a thermal slowdown that sagged the clock still fails closed"
        );
        assert_eq!(
            current_apply_qualification_p95_clock_at_anchor(
                &sagged_set,
                "held-thermal",
                1935,
                956,
                "RTX 4070"
            ),
            None,
            "a thermal slowdown that sagged the clock still fails closed"
        );
    }

    #[test]
    fn exact_apply_matrix_is_run_scoped_and_fails_closed() {
        let pass =
            |run: &str, pattern: F2QualificationPattern, outcome: F2ObsOutcome| -> F2Observation {
                let mut o = apply_qualification_pass(obs(1935, 956, outcome), pattern);
                o.run_id = run.into();
                o.gpu_key = Some("RTX 4070".into());
                o
            };
        let matrix = [
            pass(
                "R1",
                F2QualificationPattern::Dx11Game,
                F2ObsOutcome::Validated,
            ),
            pass(
                "R1",
                F2QualificationPattern::Texture,
                F2ObsOutcome::Validated,
            ),
            pass(
                "R1",
                F2QualificationPattern::Dx12Game,
                F2ObsOutcome::Validated,
            ),
            pass(
                "R1",
                F2QualificationPattern::Endurance,
                F2ObsOutcome::Validated,
            ),
        ];
        assert!(point_has_current_exact_apply_qualification(
            &matrix, "R1", 1935, 956, "RTX 4070"
        ));
        assert!(!point_has_n_current_exact_apply_qualifications(
            &matrix, "R1", 1935, 956, "RTX 4070", 2
        ));
        let mut duplicated_lane = matrix.to_vec();
        duplicated_lane.push(matrix[0].clone());
        assert!(
            !point_has_n_current_exact_apply_qualifications(
                &duplicated_lane,
                "R1",
                1935,
                956,
                "RTX 4070",
                2,
            ),
            "a duplicated lane is not a second complete matrix"
        );
        let retried_lanes = matrix
            .iter()
            .flat_map(|observation| [observation.clone(), observation.clone()])
            .collect::<Vec<_>>();
        assert!(point_has_current_exact_apply_qualification(
            &retried_lanes,
            "R1",
            1935,
            956,
            "RTX 4070",
        ));
        assert!(
            !point_has_n_current_exact_apply_qualifications(
                &retried_lanes,
                "R1",
                1935,
                956,
                "RTX 4070",
                2,
            ),
            "lane retries within one ordered ladder are still only one complete matrix"
        );
        let mut duplicate_matrix = matrix.to_vec();
        duplicate_matrix.extend(matrix.clone());
        assert!(point_has_current_exact_apply_qualification(
            &duplicate_matrix,
            "R1",
            1935,
            956,
            "RTX 4070"
        ));
        assert!(point_has_n_current_exact_apply_qualifications(
            &duplicate_matrix,
            "R1",
            1935,
            956,
            "RTX 4070",
            2,
        ));
        assert!(!point_has_n_current_exact_apply_qualifications(
            &duplicate_matrix,
            "R1",
            1935,
            956,
            "RTX 4070",
            0,
        ));
        let mut missing_dx12_driver = matrix.clone();
        let dx12_provenance = missing_dx12_driver[2].evidence_provenance.as_mut().unwrap();
        dx12_provenance.driver_name = None;
        dx12_provenance.driver_info = None;
        assert!(!point_has_current_exact_apply_qualification(
            &missing_dx12_driver,
            "R1",
            1935,
            956,
            "RTX 4070"
        ));
        let mut missing_provenance = matrix.clone();
        missing_provenance[0].evidence_provenance = None;
        assert!(!point_has_current_exact_apply_qualification(
            &missing_provenance,
            "R1",
            1935,
            956,
            "RTX 4070"
        ));
        // Run-scoped: the same evidence never publishes a different run.
        assert!(!point_has_current_exact_apply_qualification(
            &matrix, "R2", 1935, 956, "RTX 4070"
        ));
        // Removed legacy gates cannot publish a current point by themselves.
        let shock_only = [pass(
            "R1",
            F2QualificationPattern::TransitionShock,
            F2ObsOutcome::Validated,
        )];
        assert!(!point_has_current_exact_apply_qualification(
            &shock_only,
            "R1",
            1935,
            956,
            "RTX 4070"
        ));
        // Fail closed: every individual API/Endurance lane is mandatory.
        for missing_index in 0..REQUIRED_EXACT_APPLY_PATTERNS.len() {
            let incomplete = matrix
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != missing_index)
                .map(|(_, observation)| observation.clone())
                .collect::<Vec<_>>();
            assert!(!point_has_current_exact_apply_qualification(
                &incomplete,
                "R1",
                1935,
                956,
                "RTX 4070"
            ));
        }
        let mut inconclusive_dx12 = matrix.clone();
        inconclusive_dx12[2].outcome = F2ObsOutcome::QualificationInconclusive;
        inconclusive_dx12[2]
            .qualification_coverage
            .as_mut()
            .unwrap()
            .verdict = F2QualificationVerdict::Inconclusive;
        assert!(!point_has_current_exact_apply_qualification(
            &inconclusive_dx12,
            "R1",
            1935,
            956,
            "RTX 4070"
        ));
        // A non-validated endurance dwell (silent error mid-soak) rejects the point.
        let mut failed = pass(
            "R1",
            F2QualificationPattern::Endurance,
            F2ObsOutcome::SilentError,
        );
        failed.silent_error = true;
        let with_failed_endurance = [
            matrix[0].clone(),
            matrix[1].clone(),
            matrix[2].clone(),
            failed,
        ];
        assert!(!point_has_current_exact_apply_qualification(
            &with_failed_endurance,
            "R1",
            1935,
            956,
            "RTX 4070"
        ));
    }

    #[test]
    fn worst_apply_power_includes_endurance_and_is_run_scoped() {
        let mut calm = apply_qualification_pass(
            obs(1920, 918, F2ObsOutcome::Validated),
            F2QualificationPattern::Texture,
        );
        calm.run_id = "R1".into();
        calm.gpu_key = Some("RTX 4070".into());
        calm.power_p99_w = Some(174.0);
        calm.max_watts = Some(175);
        // The endurance soak measured the honest worst load — its PEAK must win the basis.
        let mut soak = apply_qualification_pass(
            obs(1920, 918, F2ObsOutcome::Validated),
            F2QualificationPattern::Endurance,
        );
        soak.run_id = "R1".into();
        soak.gpu_key = Some("RTX 4070".into());
        soak.power_p99_w = Some(188.0);
        soak.max_watts = Some(189);
        let set = [calm, soak];
        assert_eq!(
            worst_current_apply_qualification_power_at_anchor(&set, "R1", 1920, 918, "RTX 4070"),
            Some(189.0)
        );
        let mut missing_provenance = set.clone();
        missing_provenance[1].evidence_provenance = None;
        assert_eq!(
            worst_current_apply_qualification_power_at_anchor(
                &missing_provenance,
                "R1",
                1920,
                918,
                "RTX 4070"
            ),
            Some(175.0),
            "an untraceable soak cannot raise the published power basis"
        );
        // Run-scoped: another run's evidence never feeds this run's off-cap basis.
        assert_eq!(
            worst_current_apply_qualification_power_at_anchor(&set, "R2", 1920, 918, "RTX 4070"),
            None
        );
    }

    #[test]
    fn frontier_boundary_respects_crash_proximity_margin() {
        // Crash at V taints the adjacent bin: with a device-loss at 906 mV, a validated 912 mV
        // (one bin up) cannot become the boundary; 918 mV (~two bins) can.
        let mut crash = obs(1920, 906, F2ObsOutcome::DeviceLost);
        crash.device_lost = true;
        let near = obs(1920, 912, F2ObsOutcome::Validated);
        let clear = obs(1920, 918, F2ObsOutcome::Validated);
        assert_eq!(crash_floor_for_target(&[crash.clone()], 1920), Some(906));
        let entry = frontier_entry_for_target(&[crash.clone(), near.clone(), clear], 1920).unwrap();
        assert_eq!(entry.best_anchor_mv, 918);
        // Only the tainted bin available -> no boundary at all.
        assert!(frontier_entry_for_target(&[crash, near], 1920).is_none());
    }

    #[test]
    fn failure_histogram_counts_failed_phases_per_point_and_pattern() {
        let mut fail_a = apply_qualification_pass(
            obs(1935, 956, F2ObsOutcome::Unstable),
            F2QualificationPattern::HighFps,
        );
        fail_a
            .qualification_coverage
            .as_mut()
            .unwrap()
            .failure_phase = Some("frame-cadence".into());
        let mut fail_b = fail_a.clone();
        fail_b
            .qualification_coverage
            .as_mut()
            .unwrap()
            .failure_phase = Some("frame-cadence".into());
        let mut fail_other = apply_qualification_pass(
            obs(1935, 950, F2ObsOutcome::Unstable),
            F2QualificationPattern::Memory,
        );
        fail_other
            .qualification_coverage
            .as_mut()
            .unwrap()
            .failure_phase = Some("vram-pressure".into());
        // A clean pass (no failure_phase) contributes nothing.
        let clean = apply_qualification_pass(
            obs(1935, 962, F2ObsOutcome::Validated),
            F2QualificationPattern::Texture,
        );

        let histogram = qualification_failure_histogram(&[fail_a, fail_b, fail_other, clean]);
        assert_eq!(
            histogram
                .get(&(1935, 956, "HighFps".into(), "frame-cadence".into()))
                .copied(),
            Some(2)
        );
        assert_eq!(
            histogram
                .get(&(1935, 950, "Memory".into(), "vram-pressure".into()))
                .copied(),
            Some(1)
        );
        assert_eq!(histogram.len(), 2);
    }

    #[test]
    fn jsonl_parse_is_bom_tolerant_and_skips_malformed() {
        let good = serde_json::to_string(&obs(1800, 962, F2ObsOutcome::Validated)).unwrap();
        let data = format!("\u{feff}{good}\n\n{{not valid json}}\n{good}\n");
        let parsed = parse_observations(&data);
        assert_eq!(parsed.len(), 2); // both good lines; blank + malformed skipped
        assert_eq!(parsed[0].target_mhz, 1800);
    }

    #[test]
    fn evidence_provenance_round_trips_and_legacy_lines_default_to_none() {
        let mut current = obs(1800, 962, F2ObsOutcome::Validated);
        current.evidence_provenance = Some(reproducible_provenance());
        let encoded = serde_json::to_string(&current).unwrap();
        let decoded: F2Observation = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, current);

        let mut legacy = serde_json::to_value(current).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("evidence_provenance");
        let decoded_legacy: F2Observation = serde_json::from_value(legacy).unwrap();
        assert_eq!(decoded_legacy.evidence_provenance, None);
    }

    #[test]
    fn discovery_measurement_refusal_round_trips_without_becoming_a_boundary() {
        let mut current = obs(1710, 875, F2ObsOutcome::DiscoveryInconclusive);
        current.dwell_result = F2ObsDwell::DiscoveryInconclusive;
        current.inconclusive_reason = Some("voltage_telemetry_low".into());
        current.measured_voltage_sample_count = 1;
        let decoded: F2Observation = serde_json::from_str(
            &serde_json::to_string(&current).unwrap(),
        ).unwrap();
        assert_eq!(decoded, current);
        assert!(first_bad_for_target(std::slice::from_ref(&decoded), 1710).is_none());
        assert!(last_good_for_target(&[decoded], 1710).is_none());

        let mut legacy = serde_json::to_value(obs(1710, 875,
            F2ObsOutcome::PowerTelemetryInconclusive)).unwrap();
        legacy.as_object_mut().unwrap().remove("inconclusive_reason");
        let decoded: F2Observation = serde_json::from_value(legacy).unwrap();
        assert_eq!(decoded.outcome, F2ObsOutcome::PowerTelemetryInconclusive);
        assert_eq!(decoded.inconclusive_reason, None);
    }

    #[test]
    fn store_append_accumulates_and_queries() {
        // Unique temp base so the JSONL append round-trips without colliding with other tests.
        let base = std::env::temp_dir().join(format!("nidav-f2-obs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let store = F2ObservationStore::new(&base);
        assert!(store.load_all().is_empty()); // missing file → empty
        store
            .append(&obs(1800, 968, F2ObsOutcome::Validated))
            .unwrap();
        store
            .append(&obs(1800, 962, F2ObsOutcome::Validated))
            .unwrap();
        store
            .append(&obs(1815, 980, F2ObsOutcome::Unstable))
            .unwrap();
        // Append accumulates (does NOT overwrite).
        assert_eq!(store.load_all().len(), 3);
        assert_eq!(store.query_by_target(1800).len(), 2);
        assert_eq!(
            store
                .learned_frontier()
                .iter()
                .map(|e| e.target_mhz)
                .collect::<Vec<_>>(),
            vec![1800]
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn checked_store_load_treats_missing_as_empty_and_accepts_bom() {
        let base = std::env::temp_dir().join(format!(
            "nidav-f2-obs-checked-missing-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let store = F2ObservationStore::new(&base);
        assert!(store.load_all_checked().unwrap().is_empty());

        std::fs::create_dir_all(&base).unwrap();
        let good = serde_json::to_string(&obs(1800, 962, F2ObsOutcome::Validated)).unwrap();
        std::fs::write(store.path(), format!("\u{feff}{good}\n\n")).unwrap();
        assert_eq!(store.load_all_checked().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn checked_store_load_rejects_malformed_nonempty_line_with_context() {
        let base = std::env::temp_dir().join(format!(
            "nidav-f2-obs-checked-invalid-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let store = F2ObservationStore::new(&base);
        std::fs::create_dir_all(&base).unwrap();
        let good = serde_json::to_string(&obs(1800, 962, F2ObsOutcome::Validated)).unwrap();
        std::fs::write(store.path(), format!("{good}\n{{not valid json}}\n")).unwrap();

        let error = store.load_all_checked().unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        let message = error.to_string();
        assert!(message.contains(&store.path().display().to_string()));
        assert!(message.contains("line 2"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn checked_store_load_propagates_non_missing_io_error() {
        let base = std::env::temp_dir().join(format!(
            "nidav-f2-obs-checked-io-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let store = F2ObservationStore::new(&base);
        std::fs::create_dir_all(store.path()).unwrap();

        let error = store.load_all_checked().unwrap_err();
        assert_ne!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(error
            .to_string()
            .contains(&store.path().display().to_string()));
        let _ = std::fs::remove_dir_all(&base);
    }
}
