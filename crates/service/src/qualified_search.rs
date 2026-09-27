//! Pure, durable admission policy for qualification-first discovery. The caller owns all hardware,
//! evidence validation and persistence: save a successful admission BEFORE arming a candidate.

use nidavellir_core::ipc::{ForgeDiscoveryBand, ForgeDiscoverySearch};

pub const VERSION: u32 = 7;
pub const STANDARD_ATTEMPTS: u32 = 24;
pub const STANDARD_BUDGET_MS: u64 = 8 * 60 * 60 * 1_000;
/// The qualified top's single margin probe waits here for the last, reserved admission.
const MARGIN_PROBE_WAITING: &str = "margin_probe_waiting";

#[derive(Debug, Clone, Copy)]
pub struct Seed<'a> {
    pub id: &'a str,
    pub target_clock_mhz: u32,
    pub voltage_mv: u32,
    pub clock_ceiling_mhz: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    pub target_clock_mhz: u32,
    pub voltage_mv: u32,
}

/// Representative-load (PowerRender) telemetry, recovered and validated by the caller. Never a pass.
#[derive(Debug, Clone, Copy)]
pub struct PowerBoundHint {
    /// Mean voltage where the GPU's own limiter settled while drawing the board cap.
    pub measured_voltage_mv: u32,
}

/// The limiter settled at the measured voltage at a clock BELOW the target, and board power does
/// not fall as clock rises: the target cannot fit the cap above that voltage. Jump straight to the
/// lowest physical bin above it (run 1790448315552: 24 one-bin steps from 1081 ended at 937; its
/// first capped sample averaged 934). Capped steps never exercise the target pair, so skipping them
/// loses no stability evidence. Without a usable hint, keep one-bin refinement.
fn lower_power_voltage(
    band: &ForgeDiscoveryBand,
    voltage_bins: &[u32],
    hint: Option<PowerBoundHint>,
) -> Option<u32> {
    let lower = voltage_bins.iter().copied()
        .filter(|mv| *mv < band.voltage_mv && band.integrity_floor_voltage_mv.is_none_or(|floor| *mv > floor));
    let adjacent = lower.clone().max()?;
    match hint.filter(|h| h.measured_voltage_mv > 0 && h.measured_voltage_mv < adjacent) {
        Some(hint) => lower.filter(|mv| *mv > hint.measured_voltage_mv).min(),
        None => Some(adjacent),
    }
}

fn clear_power_bracket(band: &mut ForgeDiscoveryBand) {
    band.integrity_floor_voltage_mv = None;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Only a complete current-contract matrix, exact pair and confirmed stock cleanup.
    Qualified,
    /// Confirmed power limitation, no integrity/thermal/containment error, cleanup confirmed.
    PowerBound,
    IntegrityError,
    Inconclusive,
    DriverFailure,
    OperationalFailure,
    /// Discovery excursion only, with persisted neutral evidence and confirmed stock cleanup.
    ControlMismatch,
    /// Only after cooperative stop and confirmed stock. Retrying costs a new admission.
    Cancelled,
}

pub fn new(seeds: &[Seed<'_>], attempts_limit: u32, time_budget_ms: u64) -> ForgeDiscoverySearch {
    let valid = seeds.len() == 3
        && seeds.iter().all(|seed| {
            seed.target_clock_mhz > 0
                && seed.voltage_mv > 0
                && seed.target_clock_mhz <= seed.clock_ceiling_mhz
        })
        && ["performance", "balanced", "efficiency"]
            .iter()
            .all(|id| seeds.iter().filter(|seed| seed.id == *id).count() == 1);
    ForgeDiscoverySearch {
        version: VERSION,
        attempts_limit,
        time_budget_ms,
        stop_reason: (!valid || attempts_limit == 0 || time_budget_ms == 0)
            .then(|| "invalid_search_plan".into()),
        bands: seeds
            .iter()
            .map(|seed| ForgeDiscoveryBand {
                id: seed.id.into(),
                target_clock_mhz: seed.target_clock_mhz,
                voltage_mv: seed.voltage_mv,
                clock_ceiling_mhz: seed.clock_ceiling_mhz,
                status: if seed.id == "performance" { "pending" } else { "waiting_for_top" }.into(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

/// Update the clock and persist a budget stop before asking for another band. An admitted
/// candidate must still record its result; reaching the limit does not discard its evidence.
pub fn finalize_budget(search: &mut ForgeDiscoverySearch, elapsed_ms: u64) -> bool {
    search.elapsed_ms = search.elapsed_ms.max(elapsed_ms);
    if search.version != VERSION
        || search.stop_reason.is_some()
        || search.bands.iter().any(|band| band.status == "in_flight")
    {
        return false;
    }
    let reason = if search.attempts_used >= search.attempts_limit {
        "attempt_budget_exhausted"
    } else if search.elapsed_ms >= search.time_budget_ms {
        "time_budget_exhausted"
    } else {
        return false;
    };
    close_all(search, reason);
    true
}

pub fn next_band(search: &ForgeDiscoverySearch) -> Option<usize> {
    if search.version != VERSION
        || search.stop_reason.is_some()
        || search.bands.is_empty()
        || search.attempts_used >= search.attempts_limit
        || search.elapsed_ms >= search.time_budget_ms
        || search.bands.iter().any(|band| band.status == "in_flight")
    {
        return None;
    }
    // The top margin probe runs last on an admission reserved for it (2026-09-27), so a probe TDR
    // cannot cost the economic evidence already gathered. Time is not reserved: if the clock runs
    // out first, the top simply stays unpublished.
    let probe = search.bands.iter().position(|band| band.status == MARGIN_PROBE_WAITING);
    let start = search.next_band_index % search.bands.len();
    (search.attempts_used + u32::from(probe.is_some()) < search.attempts_limit)
        .then(|| {
            (0..search.bands.len())
                .map(|offset| (start + offset) % search.bands.len())
                .find(|&index| search.bands[index].status == "pending")
        })
        .flatten()
        .or(probe)
}

/// Counts even cancelled/failed attempts. `elapsed_ms` is cumulative across service sessions.
pub fn admit(
    search: &mut ForgeDiscoverySearch,
    band: usize,
    elapsed_ms: u64,
    required_ms: u64,
) -> Result<Candidate, String> {
    search.elapsed_ms = search.elapsed_ms.max(elapsed_ms);
    if search.version != VERSION {
        return Err("incompatible_search_version".into());
    }
    if let Some(reason) = &search.stop_reason {
        return Err(reason.clone());
    }
    if search.attempts_used >= search.attempts_limit {
        close_all(search, "attempt_budget_exhausted");
        return Err("attempt_budget_exhausted".into());
    }
    if required_ms == 0 || required_ms > search.time_budget_ms.saturating_sub(search.elapsed_ms) {
        close_all(search, "time_budget_exhausted");
        return Err("time_budget_exhausted".into());
    }
    if next_band(search) != Some(band) {
        return Err("candidate_not_admissible".into());
    }
    search.attempts_used += 1;
    search.next_band_index = (band + 1) % search.bands.len();
    let selected = &mut search.bands[band];
    selected.attempts += 1;
    selected.status = "in_flight".into();
    Ok(Candidate {
        target_clock_mhz: selected.target_clock_mhz,
        voltage_mv: selected.voltage_mv,
    })
}

fn close_band(band: &mut ForgeDiscoveryBand, reason: &str) {
    band.status = "closed".into();
    band.stop_reason = Some(reason.into());
}

fn close_all(search: &mut ForgeDiscoverySearch, reason: &str) {
    search.stop_reason = Some(reason.into());
    for band in &mut search.bands {
        if band.status != "closed" {
            close_band(band, reason);
        }
    }
}

/// The integrity budget ends exploration, but the single planned margin probe still runs last.
fn close_exploration(search: &mut ForgeDiscoverySearch, reason: &str) {
    if !search.bands.iter().any(|band| band.status == MARGIN_PROBE_WAITING) {
        return close_all(search, reason);
    }
    for band in &mut search.bands {
        if band.status != "closed" && band.status != MARGIN_PROBE_WAITING {
            close_band(band, reason);
        }
    }
}

/// Retry after a cancel/control retry: the margin probe goes back to its reserved slot.
fn requeue(band: &mut ForgeDiscoveryBand) {
    band.status = if band.id == "performance" && band.margin_probe_used {
        MARGIN_PROBE_WAITING
    } else {
        "pending"
    }
    .into();
}

fn below_economic_floor(clock_mhz: u32, performance_clock_mhz: Option<u32>) -> bool {
    performance_clock_mhz
        .is_some_and(|performance| u64::from(clock_mhz) * 100 < u64::from(performance) * 90)
}

/// Economic exploration starts only after the performance search closes with complete proof.
/// These are starting candidates, never predeclared profile results. Every pair needs its own matrix.
fn start_economic_bands(search: &mut ForgeDiscoverySearch, clock_bins: &[u32]) {
    let Some(top) = search.bands.iter().find(|b| b.id == "performance"
        && (b.status == "closed" || b.status == MARGIN_PROBE_WAITING)) else { return; };
    let (Some(clock), Some(voltage)) = (top.last_qualified_clock_mhz, top.last_qualified_voltage_mv) else {
        close_all(search, "qualified_top_unavailable");
        return;
    };
    let floor = clock_bins.iter().copied().filter(|c| *c <= clock && u64::from(*c) * 100 >= u64::from(clock) * 90).min().unwrap_or(clock);
    for band in &mut search.bands {
        if band.status != "waiting_for_top" { continue; }
        band.target_clock_mhz = if band.id == "balanced" {
            clock_bins.iter().copied().filter(|c| *c <= clock && u64::from(*c) * 100 >= u64::from(clock) * 95).min().unwrap_or(clock)
        } else { floor };
        band.voltage_mv = voltage;
        band.clock_ceiling_mhz = clock;
        band.status = "pending".into();
    }
}

/// Pure result transition. No propagated blacklist or physical stability inference is produced.
pub fn record(
    search: &mut ForgeDiscoverySearch,
    index: usize,
    outcome: Outcome,
    clock_bins: &[u32],
    voltage_bins: &[u32],
    power_hint: Option<PowerBoundHint>,
) -> Result<(), String> {
    if search.version != VERSION
        || search
            .bands
            .get(index)
            .is_none_or(|band| band.status != "in_flight")
    {
        return Err("no_admitted_candidate".into());
    }
    if outcome == Outcome::DriverFailure {
        close_all(search, "driver_failure_recovery_required");
        return Ok(());
    }
    if outcome == Outcome::OperationalFailure {
        close_all(search, "operational_failure");
        return Ok(());
    }
    let performance_clock = search
        .bands
        .iter()
        .find(|band| band.id == "performance")
        .and_then(|band| band.last_qualified_clock_mhz);
    let band = &mut search.bands[index];
    let lower_voltage = voltage_bins
        .iter()
        .copied()
        .filter(|mv| *mv < band.voltage_mv)
        .max();
    let lower_power_voltage = lower_power_voltage(band, voltage_bins, power_hint);
    match outcome {
        // Margin probe result (2026-09-27): it only decides whether the top may be published;
        // it never replaces the qualified top or descends further. By user decision its
        // integrity failure does not count toward the two-error exploration budget.
        _ if band.id == "performance" && band.margin_probe_used
            && !matches!(outcome, Outcome::ControlMismatch | Outcome::Cancelled) => {
            close_band(band, match outcome {
                Outcome::Qualified => "top_margin_proven",
                Outcome::IntegrityError => "top_margin_edge",
                _ => "top_margin_unproven",
            });
        }
        Outcome::Qualified => {
            band.power_preparation_used = false;
            clear_power_bracket(band);
            band.last_qualified_clock_mhz = Some(band.target_clock_mhz);
            band.last_qualified_voltage_mv = Some(band.voltage_mv);
            let higher_clock = clock_bins
                .iter()
                .copied()
                .filter(|clock| *clock > band.target_clock_mhz && *clock <= band.clock_ceiling_mhz)
                .min();
            if band.id != "performance"
                && below_economic_floor(band.target_clock_mhz, performance_clock)
            {
                if let Some(clock) = higher_clock {
                    band.target_clock_mhz = clock;
                    band.status = "pending".into();
                } else {
                    close_band(band, "physical_domain_exhausted");
                }
            } else if band.id == "performance" && higher_clock.is_some() {
                band.target_clock_mhz = higher_clock.unwrap();
                band.next_raise_clock = false;
                band.status = "pending".into();
            } else if band.id == "performance" && lower_voltage.is_some() {
                // Margin (2026-09-27): prove one bin below the top once, after the economic bands;
                // only then may the top be published. The qualified top used by the economic
                // bands stays where it is.
                band.margin_probe_used = true;
                band.voltage_mv = lower_voltage.unwrap();
                band.status = MARGIN_PROBE_WAITING.into();
            } else if band.id == "performance" {
                close_band(band, "physical_clock_domain_exhausted");
            } else if let Some(voltage) = lower_voltage {
                band.voltage_mv = voltage;
                band.next_raise_clock = band.id == "performance";
                band.status = "pending".into();
            } else {
                close_band(band, "physical_domain_exhausted");
            }
        }
        Outcome::PowerBound if band.id == "performance"
            && band.last_qualified_voltage_mv.is_some_and(|mv| band.voltage_mv > mv) => {
            close_band(band, "power_integrity_boundary");
        }
        Outcome::PowerBound if lower_power_voltage.is_some() => {
            band.power_preparation_used = true;
            band.voltage_mv = lower_power_voltage.unwrap();
            band.status = "pending".into();
        }
        Outcome::PowerBound => {
            // At the voltage floor, explore the next physical clock without inventing a pass.
            let lower_clock = clock_bins.iter().copied().filter(|clock| *clock < band.target_clock_mhz).max();
            if band.id == "performance" && band.last_qualified_clock_mhz.is_none() && lower_clock.is_some() {
                band.target_clock_mhz = lower_clock.unwrap();
                band.clock_ceiling_mhz = band.target_clock_mhz;
                band.power_preparation_used = false;
                clear_power_bracket(band);
                band.status = "pending".into();
            } else {
                close_band(band, "power_preparation_exhausted");
            }
        },
        Outcome::IntegrityError => {
            search.integrity_errors += 1;
            band.integrity_floor_voltage_mv = Some(band.voltage_mv);
            // Only an attributed computation fault may justify more voltage, never missing
            // residency. Do not return into a voltage already rejected by the power envelope.
            let higher_voltage = voltage_bins.iter().copied().filter(|mv| *mv > band.voltage_mv).min();
            let lower_clock = clock_bins.iter().copied().filter(|clock| *clock < band.target_clock_mhz).max();
            if band.id == "performance" && band.last_qualified_clock_mhz.is_none()
                && lower_clock.is_some() && search.integrity_errors < 2 {
                // The upper candidate could not be qualified: at its power-free voltage it failed,
                // lower voltage is less stable and higher voltage is power-bound. Descend one real
                // clock bin; after a power descent, leave the failing low-voltage bin too.
                // This is a new hypothesis, never inherited stability or a propagated blacklist.
                band.target_clock_mhz = lower_clock.unwrap();
                band.clock_ceiling_mhz = band.target_clock_mhz;
                if band.power_preparation_used {
                    if let Some(voltage) = higher_voltage { band.voltage_mv = voltage; }
                }
                band.power_preparation_used = false;
                clear_power_bracket(band);
                band.status = "pending".into();
            } else if band.id == "performance" && band.last_qualified_clock_mhz.is_some()
                && !band.power_preparation_used && higher_voltage.is_some() && search.integrity_errors < 2 {
                band.voltage_mv = higher_voltage.unwrap();
                band.status = "pending".into();
            } else {
                close_band(band, "integrity_error_region_closed");
            }
        }
        // Missing proof closes only this band (user decision 2026-09-26); the pair is never
        // claimed. Without a qualified top the economic bands cannot start, so the search stops.
        Outcome::Inconclusive => close_band(band, "evidence_incomplete_no_boundary_inferred"),
        Outcome::ControlMismatch => {
            if search.control_retries_used == 0 {
                search.control_retries_used += 1;
                requeue(band);
            } else {
                close_all(search, "control_reapplication_failed");
                return Ok(());
            }
        },
        Outcome::Cancelled => requeue(band),
        Outcome::DriverFailure | Outcome::OperationalFailure => unreachable!(),
    }
    start_economic_bands(search, clock_bins);
    if search.integrity_errors >= 2 {
        close_exploration(search, "integrity_error_budget_exhausted");
    } else {
        let elapsed_ms = search.elapsed_ms;
        finalize_budget(search, elapsed_ms);
        if search.stop_reason.is_none() && search.bands.iter().all(|band| band.status == "closed") {
            search.stop_reason = Some("all_regions_closed".into());
        }
    }
    Ok(())
}

/// Invoke only after the caller confirms stock and resolves startup recovery. Admission already
/// persisted before the interruption stays spent, including if no observation made it to disk.
pub fn resume_after_stock(search: &mut ForgeDiscoverySearch) -> Result<(), String> {
    if search.version != VERSION {
        return Err("incompatible_search_version".into());
    }
    if search.stop_reason.is_some() {
        return Ok(());
    }
    for band in &mut search.bands {
        if band.status == "in_flight" {
            requeue(band);
        }
    }
    let elapsed_ms = search.elapsed_ms;
    finalize_budget(search, elapsed_ms);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const CLOCKS: &[u32] = &[1500, 1605, 1710, 1725, 1740];
    const VOLTAGES: &[u32] = &[887, 893, 900, 906];
    fn search() -> ForgeDiscoverySearch {
        new(
            &[
                Seed {
                    id: "performance",
                    target_clock_mhz: 1710,
                    voltage_mv: 906,
                    clock_ceiling_mhz: 1740,
                },
                Seed {
                    id: "balanced",
                    target_clock_mhz: 1605,
                    voltage_mv: 906,
                    clock_ceiling_mhz: 1740,
                },
                Seed {
                    id: "efficiency",
                    target_clock_mhz: 1500,
                    voltage_mv: 906,
                    clock_ceiling_mhz: 1740,
                },
            ],
            24,
            10_000,
        )
    }
    fn step(state: &mut ForgeDiscoverySearch, outcome: Outcome) -> usize {
        let index = next_band(state).unwrap();
        admit(state, index, 0, 100).unwrap();
        record(state, index, outcome, CLOCKS, VOLTAGES, None).unwrap();
        index
    }
    #[test]
    fn highest_clock_is_explored_before_voltage_optimization_or_economics() {
        let mut state = search();
        assert_eq!(step(&mut state, Outcome::Qualified), 0);
        assert_eq!((state.bands[0].target_clock_mhz,state.bands[0].voltage_mv), (1725,906));
        assert_eq!(step(&mut state, Outcome::Qualified), 0);
        assert_eq!(step(&mut state, Outcome::Qualified), 0);
        assert_eq!(state.bands[0].last_qualified_clock_mhz, Some(1740));
        assert_eq!(state.bands[0].status, MARGIN_PROBE_WAITING, "margin probe waits for the end");
        assert_eq!(next_band(&state), Some(1));
        assert_eq!(state.bands[1].target_clock_mhz,1710);
        assert_eq!(state.bands[2].target_clock_mhz,1605);
    }
    #[test]
    fn top_margin_probe_runs_last_once_uncounted_and_never_moves_the_top() {
        for (probe, reason) in [
            (Outcome::Qualified, "top_margin_proven"),
            (Outcome::IntegrityError, "top_margin_edge"),
            (Outcome::Inconclusive, "top_margin_unproven"),
        ] {
            let mut state = search();
            state.bands[0].target_clock_mhz = 1740;
            step(&mut state, Outcome::Qualified);
            let top = &state.bands[0];
            assert_eq!((top.target_clock_mhz, top.voltage_mv, top.margin_probe_used), (1740, 900, true));
            assert_eq!(state.bands[1].voltage_mv, 906, "economics start from the top, not the probe");
            assert_eq!(step(&mut state, Outcome::Inconclusive), 1);
            assert_eq!(step(&mut state, Outcome::Inconclusive), 2);
            assert_eq!(step(&mut state, probe), 0, "the probe runs after the economic bands");
            let top = &state.bands[0];
            assert_eq!(top.stop_reason.as_deref(), Some(reason));
            assert_eq!((top.last_qualified_clock_mhz, top.last_qualified_voltage_mv), (Some(1740), Some(906)));
            assert_eq!((state.integrity_errors, next_band(&state)), (0, None));
        }
    }
    #[test]
    fn margin_probe_keeps_the_last_admission_and_survives_the_integrity_budget() {
        let mut state = search();
        state.attempts_limit = 3;
        state.bands[0].target_clock_mhz = 1740;
        step(&mut state, Outcome::Qualified);
        assert_eq!(step(&mut state, Outcome::Qualified), 1);
        assert_eq!(next_band(&state), Some(0), "last admission is reserved for the probe");
        let mut state = search();
        state.bands[0].target_clock_mhz = 1740;
        step(&mut state, Outcome::Qualified);
        step(&mut state, Outcome::IntegrityError);
        step(&mut state, Outcome::IntegrityError);
        assert_eq!((state.integrity_errors, state.stop_reason.as_deref()), (2, None));
        assert_eq!(step(&mut state, Outcome::Qualified), 0);
        assert_eq!(state.stop_reason.as_deref(), Some("integrity_error_budget_exhausted"));
    }
    #[test]
    fn power_bound_lowers_voltage_at_same_clock_without_promoting_evidence() {
        let mut state = search();
        for voltage in [900,893,887] {
            assert_eq!(step(&mut state, Outcome::PowerBound),0);
            assert_eq!(state.bands[0].voltage_mv,voltage);
            assert_eq!(state.bands[0].target_clock_mhz,1710);
            assert_eq!(state.bands[0].last_qualified_clock_mhz,None);
        }
        step(&mut state,Outcome::PowerBound);
        assert_eq!(state.bands[0].target_clock_mhz,1605);
        assert_eq!(state.bands[0].voltage_mv,887);
        assert_eq!(state.bands[0].last_qualified_clock_mhz,None);
        step(&mut state,Outcome::PowerBound);
        assert_eq!(state.bands[0].target_clock_mhz,1500);
        step(&mut state,Outcome::PowerBound);
        assert_eq!(next_band(&state),None);
        assert_eq!(state.stop_reason.as_deref(),Some("qualified_top_unavailable"));
    }
    #[test]
    fn power_hint_jumps_to_first_bin_above_measured_equilibrium_then_refines_one_bin() {
        // Run 1790448315552 stepped 1920 MHz from 1081 to 937 one bin per admission.
        let bins = [931, 937, 943, 950, 956, 962, 968, 975, 981, 987, 993, 1000, 1006, 1012,
            1018, 1025, 1031, 1037, 1043, 1050, 1056, 1062, 1068, 1075, 1081];
        let seed = |id| Seed { id, target_clock_mhz: 1920, voltage_mv: 1081, clock_ceiling_mhz: 1920 };
        let mut state = new(&[seed("performance"), seed("balanced"), seed("efficiency")], 24, 10_000);
        let capped = |state: &mut ForgeDiscoverySearch, mean: u32| {
            admit(state, 0, 0, 100).unwrap();
            let hint = Some(PowerBoundHint { measured_voltage_mv: mean });
            record(state, 0, Outcome::PowerBound, &[1905, 1920], &bins, hint).unwrap();
            state.bands[0].voltage_mv
        };
        assert_eq!(capped(&mut state, 934), 937, "one admission instead of 23");
        assert_eq!(capped(&mut state, 934), 931, "at the equilibrium: one-bin refinement");
        assert_eq!((state.bands[0].target_clock_mhz, state.attempts_used), (1920, 2));
        assert_eq!(state.bands[0].last_qualified_clock_mhz, None);
    }
    #[test]
    fn power_hint_never_lands_on_or_below_a_failed_voltage() {
        let mut state = search();
        state.bands[0].integrity_floor_voltage_mv = Some(893);
        admit(&mut state, 0, 0, 100).unwrap();
        let hint = Some(PowerBoundHint { measured_voltage_mv: 880 });
        record(&mut state, 0, Outcome::PowerBound, CLOCKS, VOLTAGES, hint).unwrap();
        assert_eq!(state.bands[0].voltage_mv, 900);
    }
    #[test]
    fn top_down_power_integrity_bracket_descends_without_retesting_failed_top() {
        let mut state = search();
        state.bands[0].target_clock_mhz = 1740;
        step(&mut state, Outcome::PowerBound);
        assert_eq!((state.bands[0].target_clock_mhz,state.bands[0].voltage_mv),(1740,900));
        step(&mut state, Outcome::IntegrityError);
        assert_eq!((state.bands[0].target_clock_mhz,state.bands[0].voltage_mv),(1725,906));
        assert_eq!(state.bands[0].clock_ceiling_mhz,1725);
        assert_eq!(state.bands[0].last_qualified_clock_mhz,None);
        assert_eq!(state.bands[1].status,"waiting_for_top");
        let mut restored: ForgeDiscoverySearch =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        resume_after_stock(&mut restored).unwrap();
        step(&mut restored, Outcome::Qualified);
        assert_eq!(restored.bands[0].last_qualified_clock_mhz,Some(1725));
        assert_eq!((restored.bands[0].voltage_mv, restored.bands[0].margin_probe_used), (900, true));
        assert_eq!(restored.bands[0].status, MARGIN_PROBE_WAITING);
        assert_eq!(next_band(&restored),Some(1));
        assert_eq!(restored.attempts_used,3);
    }
    #[test]
    fn top_down_second_integrity_error_still_stops_without_profiles() {
        let mut state = search();
        state.bands[0].target_clock_mhz = 1740;
        step(&mut state, Outcome::IntegrityError);
        assert_eq!(state.bands[0].target_clock_mhz,1725);
        assert_eq!(state.bands[0].voltage_mv,906);
        step(&mut state, Outcome::IntegrityError);
        assert_eq!(state.stop_reason.as_deref(),Some("integrity_error_budget_exhausted"));
        assert!(state.bands.iter().all(|band|band.last_qualified_clock_mhz.is_none()));
        assert_eq!(next_band(&state),None);
    }
    #[test]
    fn control_retry_is_same_pair_once_per_run_and_survives_resume() {
        let mut state = search();
        let before = (state.bands[0].target_clock_mhz,state.bands[0].voltage_mv);
        step(&mut state, Outcome::ControlMismatch);
        assert_eq!((state.bands[0].target_clock_mhz,state.bands[0].voltage_mv),before);
        assert_eq!(state.control_retries_used,1);
        assert_eq!(state.integrity_errors,0);
        assert_eq!(state.bands[0].last_qualified_clock_mhz,None);
        let mut restored: ForgeDiscoverySearch = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        resume_after_stock(&mut restored).unwrap();
        step(&mut restored, Outcome::PowerBound);
        step(&mut restored, Outcome::ControlMismatch);
        assert_eq!(restored.attempts_used,3);
        assert_eq!(restored.stop_reason.as_deref(),Some("control_reapplication_failed"));
        assert_eq!(next_band(&restored),None);
    }
    #[test]
    fn control_retry_cannot_bypass_attempt_budget_or_driver_failure() {
        let mut state = search();
        state.attempts_limit = 1;
        step(&mut state, Outcome::ControlMismatch);
        assert_eq!(state.stop_reason.as_deref(),Some("attempt_budget_exhausted"));
        let mut state = search();
        step(&mut state, Outcome::ControlMismatch);
        step(&mut state, Outcome::DriverFailure);
        assert_eq!(state.stop_reason.as_deref(),Some("driver_failure_recovery_required"));
    }
    #[test]
    fn old_bottom_up_search_cannot_resume_as_top_down() {
        let mut state = search();
        state.version = 2;
        assert_eq!(resume_after_stock(&mut state).unwrap_err(),"incompatible_search_version");
        assert_eq!(next_band(&state),None);
    }
    #[test]
    fn inconclusive_closes_only_its_band_and_never_claims_the_pair() {
        let mut state=search();
        step(&mut state,Outcome::Qualified);
        step(&mut state,Outcome::Inconclusive);
        assert_eq!(state.bands[0].last_qualified_clock_mhz,Some(1710), "1725 stays unproven");
        assert_eq!(state.bands[0].stop_reason.as_deref(),Some("evidence_incomplete_no_boundary_inferred"));
        assert_eq!((state.stop_reason.as_deref(),next_band(&state)),(None,Some(1)));
        step(&mut state,Outcome::Inconclusive);
        assert_eq!((state.bands[1].status.as_str(),next_band(&state)),("closed",Some(2)));
        let mut no_top=search();
        step(&mut no_top,Outcome::Inconclusive);
        assert_eq!(no_top.stop_reason.as_deref(),Some("qualified_top_unavailable"));
        assert_eq!(next_band(&no_top),None);
    }
    #[test]
    fn integrity_failure_does_not_approve_candidate_and_keeps_error_budget() {
        let mut state=search();
        step(&mut state,Outcome::Qualified);
        step(&mut state,Outcome::IntegrityError);
        assert_eq!(state.bands[0].last_qualified_clock_mhz,Some(1710));
        assert_eq!(next_band(&state),Some(1));
        step(&mut state,Outcome::IntegrityError);
        assert_eq!(state.integrity_errors,2);
        assert_eq!(next_band(&state),None);
    }
    #[test]
    fn clock_ascent_integrity_failure_can_try_higher_voltage_but_power_bracket_cannot() {
        let mut state=search();
        state.bands[0].voltage_mv=900;
        step(&mut state,Outcome::Qualified);
        step(&mut state,Outcome::IntegrityError);
        assert_eq!(next_band(&state),Some(0));
        assert_eq!((state.bands[0].target_clock_mhz,state.bands[0].voltage_mv),(1725,906));
        step(&mut state,Outcome::PowerBound);
        assert_eq!(state.bands[0].voltage_mv,906);
        assert_eq!(state.bands[0].stop_reason.as_deref(),Some("power_integrity_boundary"));
        assert_eq!(next_band(&state),Some(1));
    }
    #[test]
    fn driver_failure_closes_every_band_immediately() {
        let mut state = search();
        step(&mut state, Outcome::DriverFailure);
        assert_eq!(state.attempts_used, 1);
        assert_eq!(next_band(&state), None);
    }
    #[test]
    fn interrupted_admission_is_not_refunded_on_resume() {
        let mut state = search();
        admit(&mut state, 0, 250, 100).unwrap();
        assert!(next_band(&state).is_none());
        let mut restored: ForgeDiscoverySearch =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        resume_after_stock(&mut restored).unwrap();
        assert_eq!(restored.attempts_used, 1);
        assert_eq!(restored.elapsed_ms, 250);
        let index = next_band(&restored).unwrap();
        admit(&mut restored, index, 200, 100).unwrap();
        assert_eq!(restored.elapsed_ms, 250);
        assert_eq!(restored.attempts_used, 2);
    }
    #[test]
    fn admission_rejects_matrix_that_does_not_fit_before_hardware() {
        let mut state = search();
        assert!(admit(&mut state, 0, 9_950, 100).is_err());
        assert_eq!(state.attempts_used, 0);
        assert_eq!(state.stop_reason.as_deref(), Some("time_budget_exhausted"));
    }
    #[test]
    fn cancelled_candidate_retry_costs_another_attempt() {
        let mut state = search();
        state.attempts_limit = 2;
        step(&mut state, Outcome::Cancelled);
        step(&mut state, Outcome::Cancelled);
        resume_after_stock(&mut state).unwrap();
        assert_eq!(state.attempts_used, 2);
        assert_eq!(next_band(&state), None);
    }
    #[test]
    fn invalid_seed_plan_fails_closed() {
        assert_eq!(next_band(&new(&[], 24, 10_000)), None);
    }
}
