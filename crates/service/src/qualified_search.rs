//! Pure, durable admission policy for qualification-first discovery. The caller owns all hardware,
//! evidence validation and persistence: save a successful admission BEFORE arming a candidate.
//!
//! Staircase (user decision 2026-09-28): find the top below the power limit, then descend its
//! voltage until the first failure. The next clock level (-5%, then -10%) starts at the lowest
//! voltage the previous level passed (one bin above that failure) and descends again. Levels run
//! one at a time.

use nidavellir_core::ipc::{ForgeDiscoveryBand, ForgeDiscoverySearch};

pub const VERSION: u32 = 8;
pub const STANDARD_ATTEMPTS: u32 = 24;
pub const STANDARD_BUDGET_MS: u64 = 8 * 60 * 60 * 1_000;
/// Lower clock levels after the top, as (band id, percent of the qualified top clock).
const LEVELS: [(&str, u64); 2] = [("balanced", 95), ("efficiency", 90)];

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
    let start = search.next_band_index % search.bands.len();
    (0..search.bands.len())
        .map(|offset| (start + offset) % search.bands.len())
        .find(|&index| search.bands[index].status == "pending")
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

pub fn close_all(search: &mut ForgeDiscoverySearch, reason: &str) {
    search.stop_reason = Some(reason.into());
    for band in &mut search.bands {
        if band.status != "closed" {
            close_band(band, reason);
        }
    }
}

/// A lower clock level starts only after the previous level closes, at the lowest voltage that level
/// passed. That start is dominated by the previous level's pair (lower clock, same voltage), so it
/// must pass; it still gets its own matrix. Nothing here is a predeclared profile result.
fn start_next_level(search: &mut ForgeDiscoverySearch, clock_bins: &[u32]) {
    let Some(mut previous) = search.bands.iter().position(|b| b.id == "performance") else { return };
    let top = &search.bands[previous];
    if top.status != "closed" {
        return;
    }
    let Some(top_clock) = top.last_qualified_clock_mhz.filter(|_| top.last_qualified_voltage_mv.is_some()) else {
        close_all(search, "qualified_top_unavailable");
        return;
    };
    for (id, percent) in LEVELS {
        let Some(next) = search.bands.iter().position(|b| b.id == id) else { continue };
        match search.bands[next].status.as_str() {
            "closed" => { previous = next; continue; }
            "waiting_for_top" => {}
            _ => return,
        }
        let prev = &search.bands[previous];
        let voltage = prev.last_qualified_voltage_mv.unwrap_or(prev.voltage_mv);
        let prev_clock = prev.last_qualified_clock_mhz.unwrap_or(prev.target_clock_mhz);
        let clock = clock_bins.iter().copied()
            .filter(|c| *c <= top_clock && u64::from(*c) * 100 >= u64::from(top_clock) * percent)
            .min()
            .filter(|c| *c < prev_clock);
        let band = &mut search.bands[next];
        band.voltage_mv = voltage;
        band.target_clock_mhz = clock.unwrap_or(prev_clock);
        band.clock_ceiling_mhz = band.target_clock_mhz;
        if clock.is_some() {
            band.status = "pending".into();
            return;
        }
        // No bin strictly below the previous level: pass its clock/voltage through to the next one.
        close_band(band, "clock_level_exhausted");
        previous = next;
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
    let band = &search.bands[index];
    // The level already passed its clock at a higher voltage and now tests a lower one: its first
    // failure is the expected edge of the staircase, not an anomaly.
    let descending = band.last_qualified_clock_mhz == Some(band.target_clock_mhz)
        && band.last_qualified_voltage_mv.is_some_and(|mv| band.voltage_mv < mv);
    // A lower level's first pair repeats a voltage the higher clock passed, so it must pass.
    let dominated_start = band.id != "performance" && band.last_qualified_clock_mhz.is_none();
    if outcome == Outcome::DriverFailure {
        if descending {
            // The boot is dirty after a TDR (2026-07-22), so the caller stops this session. The
            // search stays open: after reboot and acknowledgement, Resume starts the next level.
            close_band(&mut search.bands[index], "tdr_edge");
            start_next_level(search, clock_bins);
            finish_transition(search);
        } else {
            close_all(search, "driver_failure_recovery_required");
        }
        return Ok(());
    }
    if outcome == Outcome::OperationalFailure {
        close_all(search, "operational_failure");
        return Ok(());
    }
    if outcome == Outcome::IntegrityError && dominated_start {
        search.integrity_errors += 1;
        close_all(search, "dominated_pair_failed");
        return Ok(());
    }
    let band = &mut search.bands[index];
    let lower_voltage = voltage_bins
        .iter()
        .copied()
        .filter(|mv| *mv < band.voltage_mv)
        .max();
    let lower_power_voltage = lower_power_voltage(band, voltage_bins, power_hint);
    match outcome {
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
            if band.id == "performance" && higher_clock.is_some() {
                band.target_clock_mhz = higher_clock.unwrap();
                band.next_raise_clock = false;
                band.status = "pending".into();
            } else if let Some(voltage) = lower_voltage {
                // Staircase: keep descending this clock until its first failure.
                band.voltage_mv = voltage;
                band.status = "pending".into();
            } else {
                close_band(band, "voltage_floor_reached");
            }
        }
        // The level's edge. Expected, so it does not spend the integrity budget.
        Outcome::IntegrityError if descending => close_band(band, "integrity_edge"),
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
        // Missing proof ends only this level's descent; the pair is never claimed, and the next
        // level starts one bin above it.
        Outcome::Inconclusive if descending => close_band(band, "evidence_boundary"),
        // Missing proof closes only this band (user decision 2026-09-26); the pair is never
        // claimed. Without a qualified top the lower levels cannot start and the run ends with no
        // profile (run f2-forge-1790544997509), so an unqualified top first descends one clock bin
        // at the same voltage, once per run. That is a new hypothesis, not an inferred boundary.
        Outcome::Inconclusive => {
            let lower_clock = clock_bins.iter().copied().filter(|clock| *clock < band.target_clock_mhz).max();
            match lower_clock {
                Some(clock) if band.id == "performance" && band.last_qualified_clock_mhz.is_none()
                    && !band.inconclusive_descent_used => {
                    band.inconclusive_descent_used = true;
                    band.target_clock_mhz = clock;
                    band.clock_ceiling_mhz = clock;
                    band.power_preparation_used = false;
                    clear_power_bracket(band);
                    band.status = "pending".into();
                }
                _ => close_band(band, "evidence_incomplete_no_boundary_inferred"),
            }
        }
        Outcome::ControlMismatch => {
            if search.control_retries_used == 0 {
                search.control_retries_used += 1;
                band.status = "pending".into();
            } else {
                close_all(search, "control_reapplication_failed");
                return Ok(());
            }
        },
        Outcome::Cancelled => band.status = "pending".into(),
        Outcome::DriverFailure | Outcome::OperationalFailure => unreachable!(),
    }
    start_next_level(search, clock_bins);
    finish_transition(search);
    Ok(())
}

fn finish_transition(search: &mut ForgeDiscoverySearch) {
    if search.integrity_errors >= 2 {
        close_all(search, "integrity_error_budget_exhausted");
    } else {
        let elapsed_ms = search.elapsed_ms;
        finalize_budget(search, elapsed_ms);
        if search.stop_reason.is_none() && search.bands.iter().all(|band| band.status == "closed") {
            search.stop_reason = Some("all_regions_closed".into());
        }
    }
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
            band.status = "pending".into();
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
        let top = &state.bands[0];
        assert_eq!(top.last_qualified_clock_mhz, Some(1740));
        assert_eq!((top.target_clock_mhz, top.voltage_mv, top.status.as_str()), (1740, 900, "pending"),
            "the top descends its own voltage before any lower level starts");
        assert_eq!((next_band(&state), state.bands[1].status.as_str()), (Some(0), "waiting_for_top"));
    }
    /// A real 3060 Ti-like grid: 15 MHz clock bins and ~6 mV voltage bins.
    fn staircase() -> (ForgeDiscoverySearch, Vec<u32>, Vec<u32>) {
        let seed = |id| Seed { id, target_clock_mhz: 1920, voltage_mv: 937, clock_ceiling_mhz: 1920 };
        let state = new(&[seed("performance"), seed("balanced"), seed("efficiency")], 24, 10_000);
        (state, (1695..=1920).step_by(15).collect(), vec![900, 906, 912, 918, 925, 931, 937])
    }
    fn walk(state: &mut ForgeDiscoverySearch, clocks: &[u32], volts: &[u32], outcome: Outcome) -> (usize, u32, u32) {
        let index = next_band(state).unwrap();
        let pair = admit(state, index, 0, 100).map(|c| (index, c.target_clock_mhz, c.voltage_mv)).unwrap();
        record(state, index, outcome, clocks, volts, None).unwrap();
        pair
    }
    #[test]
    fn staircase_descends_each_level_to_its_first_failure_and_restarts_one_bin_above() {
        use Outcome::*;
        let (mut state, clocks, volts) = staircase();
        let trace: Vec<_> = [Qualified, Qualified, IntegrityError, Qualified, Qualified, IntegrityError,
            Qualified, Qualified, Inconclusive]
            .into_iter().map(|outcome| walk(&mut state, &clocks, &volts, outcome)).collect();
        assert_eq!(trace, [(0, 1920, 937), (0, 1920, 931), (0, 1920, 925),
            (1, 1830, 931), (1, 1830, 925), (1, 1830, 918),
            (2, 1740, 925), (2, 1740, 918), (2, 1740, 912)],
            "-5%/-10% levels start at the lowest voltage the level above passed");
        let edges: Vec<_> = state.bands.iter().map(|b| (b.stop_reason.as_deref().unwrap(),
            b.last_qualified_clock_mhz.unwrap(), b.last_qualified_voltage_mv.unwrap())).collect();
        assert_eq!(edges, [("integrity_edge", 1920, 931), ("integrity_edge", 1830, 925),
            ("evidence_boundary", 1740, 918)]);
        assert_eq!((state.integrity_errors, state.stop_reason.as_deref()), (0, Some("all_regions_closed")),
            "expected edges do not spend the integrity budget");
    }
    #[test]
    fn tdr_edge_keeps_the_search_open_for_resume_but_a_dominated_start_failure_stops_it() {
        let (mut state, clocks, volts) = staircase();
        walk(&mut state, &clocks, &volts, Outcome::Qualified);
        walk(&mut state, &clocks, &volts, Outcome::DriverFailure);
        assert_eq!(state.bands[0].stop_reason.as_deref(), Some("tdr_edge"));
        assert_eq!(state.stop_reason, None, "Resume after reboot continues with the next level");
        let mut restored: ForgeDiscoverySearch = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        resume_after_stock(&mut restored).unwrap();
        let mut dominated = restored.clone();
        assert_eq!(walk(&mut restored, &clocks, &volts, Outcome::IntegrityError), (1, 1830, 937));
        assert_eq!((restored.stop_reason.as_deref(), restored.integrity_errors), (Some("dominated_pair_failed"), 1));
        assert_eq!(next_band(&restored), None);
        walk(&mut dominated, &clocks, &volts, Outcome::DriverFailure);
        assert_eq!(dominated.stop_reason.as_deref(), Some("driver_failure_recovery_required"));
        // Before any pair qualified, a TDR still ends the search.
        let (mut top, clocks, volts) = staircase();
        walk(&mut top, &clocks, &volts, Outcome::DriverFailure);
        assert_eq!(top.stop_reason.as_deref(), Some("driver_failure_recovery_required"));
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
        assert_eq!((restored.bands[0].voltage_mv, restored.bands[0].status.as_str()), (900, "pending"));
        assert_eq!(next_band(&restored),Some(0));
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
        // No bin lies in [95% of 1710, 1710), so balanced passes through to efficiency.
        assert_eq!(state.bands[1].stop_reason.as_deref(),Some("clock_level_exhausted"));
        assert_eq!((state.stop_reason.as_deref(),next_band(&state)),(None,Some(2)));
        assert_eq!((state.bands[2].target_clock_mhz,state.bands[2].voltage_mv),(1605,906));
        step(&mut state,Outcome::Inconclusive);
        assert_eq!((state.bands[2].status.as_str(),next_band(&state)),("closed",None));
        // An unqualified top descends one clock bin at the same voltage, once; the next
        // Inconclusive still ends the search without a top.
        let mut no_top=search();
        assert_eq!(step(&mut no_top,Outcome::Inconclusive),0);
        let top=&no_top.bands[0];
        assert_eq!((top.target_clock_mhz,top.clock_ceiling_mhz,top.voltage_mv),(1605,1605,906));
        assert_eq!((top.last_qualified_clock_mhz,no_top.integrity_errors),(None,0));
        assert_eq!(no_top.bands[1].status,"waiting_for_top");
        step(&mut no_top,Outcome::Inconclusive);
        assert_eq!(no_top.stop_reason.as_deref(),Some("qualified_top_unavailable"));
        assert_eq!(next_band(&no_top),None);
        // The descent is per run: it survives Resume and a later lower-clock Inconclusive closes.
        let mut resumed=search();
        step(&mut resumed,Outcome::Inconclusive);
        let mut resumed:ForgeDiscoverySearch=serde_json::from_str(&serde_json::to_string(&resumed).unwrap()).unwrap();
        resume_after_stock(&mut resumed).unwrap();
        step(&mut resumed,Outcome::PowerBound);
        step(&mut resumed,Outcome::Inconclusive);
        assert_eq!(resumed.stop_reason.as_deref(),Some("qualified_top_unavailable"));
    }
    #[test]
    fn integrity_failure_does_not_approve_candidate_and_keeps_error_budget() {
        let mut state=search();
        step(&mut state,Outcome::Qualified);
        step(&mut state,Outcome::IntegrityError);
        assert_eq!(state.bands[0].last_qualified_clock_mhz,Some(1710));
        assert_eq!(next_band(&state),Some(2));
        step(&mut state,Outcome::IntegrityError);
        assert_eq!(state.integrity_errors,2);
        assert_eq!(state.stop_reason.as_deref(),Some("dominated_pair_failed"));
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
        assert_eq!(next_band(&state),Some(2));
        assert_eq!((state.bands[2].target_clock_mhz,state.bands[2].voltage_mv),(1605,900));
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
