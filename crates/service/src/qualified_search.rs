//! Pure, durable admission policy for qualification-first discovery. The caller owns all hardware,
//! evidence validation and persistence: save a successful admission BEFORE arming a candidate.
//!
//! Staircase (user decisions 2026-09-28/29), one level at a time:
//! 1. Find the top below the power limit, then descend its voltage until the first failure.
//! 2. Games need a margin above what the matrix approves. If the top's lowest pass plus that
//!    margin exceeds the top's power-free voltage, a compensated top one or more clock bins lower
//!    must pass its margin pair (power-free voltage minus the margin) before its publication pair
//!    (the power-free voltage) is qualified.
//! 3. The -5% and -10% levels of the compensated top start at the lowest voltage the level above
//!    passed and descend in two-bin steps until the first failure. A publication pair above the
//!    tested range gets its own admission.

use nidavellir_core::ipc::{ForgeDiscoveryBand, ForgeDiscoverySearch};

pub const VERSION: u32 = 9;
pub const STANDARD_ATTEMPTS: u32 = 24;
pub const STANDARD_BUDGET_MS: u64 = 8 * 60 * 60 * 1_000;
/// Games need about this much more voltage than the matrix approves (user decision 2026-09-29).
/// 1830@856 passed the matrix, yet 1815@875 crashes Overwatch within 30 min and 1800@875 is the
/// user's daily undervolt. Godforge 1920@925, two bins above its failure, TDR'd in Overwatch after
/// ~23 min. 36 mV is six 6.25 mV bins after integer-mV truncation (37-38 mV), never five (31 mV).
pub const GAME_MARGIN_MV: u32 = 36;
/// Test-edge slope used only to predict the compensated top clock. On the test 3060 Ti the edge
/// fell 918 → 856 mV over 1920 → 1830 MHz, ~10 mV per 15 MHz bin. The margin pair verifies it.
const COMPENSATION_MV_PER_CLOCK_BIN: u32 = 10;
/// A compensated top whose margin pair fails steps one clock bin down, at most this often.
const COMPENSATION_RETRIES: u32 = 2;
/// Lower levels descend two bins per admission: their descent from the dominated start spans ~10
/// bins, and the game margin absorbs a one-bin coarser edge.
const LEVEL_STEP_BINS: usize = 2;
/// Lower clock levels after the (compensated) top, as (band id, percent of that top's clock).
const LEVELS: [(&str, u64); 2] = [("balanced", 95), ("efficiency", 90)];

fn bin_at_or_above(bins: &[u32], mv: u32) -> Option<u32> {
    bins.iter().copied().filter(|bin| *bin >= mv).min()
}

fn bin_at_or_below(bins: &[u32], mv: u32) -> Option<u32> {
    bins.iter().copied().filter(|bin| *bin <= mv).max()
}

/// The `n`-th value strictly below `from` (n >= 1), or the lowest one when fewer exist.
fn nth_below(values: &[u32], from: u32, n: usize) -> Option<u32> {
    let mut lower: Vec<u32> = values.iter().copied().filter(|v| *v < from).collect();
    lower.sort_unstable_by(|a, b| b.cmp(a));
    lower.get(n.saturating_sub(1)).or(lower.last()).copied()
}

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
    let mut bands: Vec<ForgeDiscoveryBand> = seeds
        .iter()
        .map(|seed| ForgeDiscoveryBand {
            id: seed.id.into(),
            target_clock_mhz: seed.target_clock_mhz,
            voltage_mv: seed.voltage_mv,
            clock_ceiling_mhz: seed.clock_ceiling_mhz,
            status: if seed.id == "performance" { "pending" } else { "waiting_for_top" }.into(),
            ..Default::default()
        })
        .collect();
    if let Some(top) = bands.iter().position(|band| band.id == "performance") {
        bands.insert(top + 1, ForgeDiscoveryBand {
            id: "compensated".into(),
            status: "waiting_for_top".into(),
            ..Default::default()
        });
    }
    ForgeDiscoverySearch {
        version: VERSION,
        attempts_limit,
        time_budget_ms,
        stop_reason: (!valid || attempts_limit == 0 || time_budget_ms == 0)
            .then(|| "invalid_search_plan".into()),
        bands,
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

/// Starts the next level once the one above closes. The compensated top follows the top; the
/// -5%/-10% levels follow the compensated top, relative to its clock, and start at the lowest
/// voltage the level above passed. That start is dominated (lower clock, same voltage), so it must
/// pass; it still gets its own matrix. Nothing here is a predeclared profile result.
fn start_next_level(search: &mut ForgeDiscoverySearch, clock_bins: &[u32], voltage_bins: &[u32]) {
    let Some(mut previous) = search.bands.iter().position(|b| b.id == "performance") else { return };
    let top = &search.bands[previous];
    if top.status != "closed" {
        return;
    }
    let (Some(top_clock), Some(top_lowest)) = (top.last_qualified_clock_mhz, top.last_qualified_voltage_mv) else {
        close_all(search, "qualified_top_unavailable");
        return;
    };
    let power_free = top.first_qualified_voltage_mv.unwrap_or(top_lowest);
    if let Some(comp) = search.bands.iter().position(|b| b.id == "compensated") {
        if search.bands[comp].status == "waiting_for_top" {
            let publication = bin_at_or_above(voltage_bins, top_lowest + GAME_MARGIN_MV);
            let margin = bin_at_or_below(voltage_bins, power_free.saturating_sub(GAME_MARGIN_MV));
            // Predict how far the clock must drop for the margin to fit under the power-free voltage.
            let clock = publication.filter(|mv| *mv > power_free).and_then(|mv| {
                let bins_down = (mv - power_free).div_ceil(COMPENSATION_MV_PER_CLOCK_BIN).max(1);
                nth_below(clock_bins, top_clock, bins_down as usize)
            });
            let band = &mut search.bands[comp];
            band.clock_ceiling_mhz = top_clock;
            match (clock, margin) {
                _ if publication.is_some_and(|mv| mv <= power_free) => {
                    band.target_clock_mhz = top_clock;
                    band.voltage_mv = top_lowest;
                    close_band(band, "compensation_not_needed");
                }
                (Some(clock), Some(margin)) => {
                    band.target_clock_mhz = clock;
                    band.voltage_mv = margin;
                    band.status = "pending".into();
                    return;
                }
                _ => {
                    band.target_clock_mhz = top_clock;
                    band.voltage_mv = top_lowest;
                    close_band(band, "compensated_top_unavailable");
                }
            }
        }
        if search.bands[comp].status != "closed" {
            return;
        }
        previous = comp;
    }
    let prev = &search.bands[previous];
    let level_top = prev.last_qualified_clock_mhz.unwrap_or(prev.target_clock_mhz);
    let (mut prev_clock, mut prev_voltage) = (level_top, prev.last_qualified_voltage_mv.unwrap_or(prev.voltage_mv));
    for (id, percent) in LEVELS {
        let Some(next) = search.bands.iter().position(|b| b.id == id) else { continue };
        let band = &mut search.bands[next];
        match band.status.as_str() {
            "closed" => {
                prev_clock = band.last_qualified_clock_mhz.unwrap_or(band.target_clock_mhz);
                prev_voltage = band.last_qualified_voltage_mv.unwrap_or(band.voltage_mv);
                continue;
            }
            "waiting_for_top" => {}
            _ => return,
        }
        let clock = clock_bins.iter().copied()
            .filter(|c| *c <= level_top && u64::from(*c) * 100 >= u64::from(level_top) * percent)
            .min()
            .filter(|c| *c < prev_clock);
        band.voltage_mv = prev_voltage;
        band.target_clock_mhz = clock.unwrap_or(prev_clock);
        band.clock_ceiling_mhz = band.target_clock_mhz;
        if clock.is_some() {
            band.status = "pending".into();
            return;
        }
        // No bin strictly below the previous level: pass its clock/voltage through to the next one.
        close_band(band, "clock_level_exhausted");
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
    match outcome {
        Outcome::OperationalFailure => close_all(search, "operational_failure"),
        Outcome::ControlMismatch if search.control_retries_used > 0 => {
            close_all(search, "control_reapplication_failed")
        }
        Outcome::ControlMismatch | Outcome::Cancelled => {
            search.control_retries_used += u32::from(outcome == Outcome::ControlMismatch);
            search.bands[index].status = "pending".into();
        }
        _ if search.bands[index].publishing => record_publication(search, index, outcome),
        _ if search.bands[index].id == "compensated" => {
            record_compensated(search, index, outcome, clock_bins, voltage_bins)
        }
        _ => record_level(search, index, outcome, clock_bins, voltage_bins, power_hint),
    }
    finish_transition(search, clock_bins, voltage_bins);
    Ok(())
}

/// A publication pair sits at or above a voltage that already passed at a higher-or-equal clock,
/// so it must pass; a failure there is inconsistent evidence and stops the search.
fn record_publication(search: &mut ForgeDiscoverySearch, index: usize, outcome: Outcome) {
    search.bands[index].publishing = false;
    match outcome {
        Outcome::Qualified => close_band(&mut search.bands[index], "published"),
        Outcome::IntegrityError => {
            search.integrity_errors += 1;
            close_all(search, "dominated_pair_failed");
        }
        Outcome::DriverFailure => close_all(search, "driver_failure_recovery_required"),
        _ => close_band(&mut search.bands[index], "publication_unproven"),
    }
}

/// The compensated top. Its margin pair must pass at the predicted clock before its publication
/// pair (the margin pair plus the game margin, at most the top's power-free voltage) is qualified.
/// A failure means the prediction was too high: one clock bin lower, a bounded number of times. A
/// TDR here pauses like a level edge, and Resume retries the lower clock.
fn record_compensated(
    search: &mut ForgeDiscoverySearch,
    index: usize,
    outcome: Outcome,
    clock_bins: &[u32],
    voltage_bins: &[u32],
) {
    let top = search.bands.iter().find(|band| band.id == "performance");
    let power_free = top.and_then(|band| band.first_qualified_voltage_mv);
    let top_lowest = top.and_then(|band| band.last_qualified_voltage_mv);
    let band = &mut search.bands[index];
    if outcome == Outcome::Qualified {
        band.first_qualified_voltage_mv = Some(band.voltage_mv);
        band.last_qualified_clock_mhz = Some(band.target_clock_mhz);
        band.last_qualified_voltage_mv = Some(band.voltage_mv);
        match bin_at_or_above(voltage_bins, band.voltage_mv + GAME_MARGIN_MV)
            .filter(|mv| power_free.is_none_or(|limit| *mv <= limit))
        {
            Some(publication) => {
                band.voltage_mv = publication;
                band.publishing = true;
                band.status = "pending".into();
            }
            None => close_band(band, "compensated_top_unavailable"),
        }
        return;
    }
    match clock_bins.iter().copied().filter(|clock| *clock < band.target_clock_mhz).max() {
        Some(clock) if band.attempts <= COMPENSATION_RETRIES => {
            band.target_clock_mhz = clock;
            band.status = "pending".into();
        }
        _ => {
            // The lower levels then start from the top's lowest pass, which dominates them.
            if let Some(mv) = top_lowest {
                band.voltage_mv = mv;
            }
            close_band(band, "compensated_top_unavailable");
        }
    }
}

/// The top and the lower levels. A level first qualifies its clock, then descends until its
/// first failure: the expected staircase edge, which does not spend the integrity budget.
fn record_level(
    search: &mut ForgeDiscoverySearch,
    index: usize,
    outcome: Outcome,
    clock_bins: &[u32],
    voltage_bins: &[u32],
    power_hint: Option<PowerBoundHint>,
) {
    let band = &search.bands[index];
    let descending = band.last_qualified_clock_mhz == Some(band.target_clock_mhz)
        && band.last_qualified_voltage_mv.is_some_and(|mv| band.voltage_mv < mv);
    // A lower level's first pair repeats a voltage the level above passed, so it must pass.
    let dominated_start = band.id != "performance" && band.last_qualified_clock_mhz.is_none();
    let step = if band.id == "performance" { 1 } else { LEVEL_STEP_BINS };
    match outcome {
        // The boot is dirty after a TDR (2026-07-22), so the caller stops this session. The
        // search stays open: after reboot and acknowledgement, Resume continues.
        Outcome::DriverFailure if descending => {
            return finish_level(search, index, "tdr_edge", voltage_bins)
        }
        Outcome::DriverFailure => return close_all(search, "driver_failure_recovery_required"),
        Outcome::IntegrityError if dominated_start => {
            search.integrity_errors += 1;
            return close_all(search, "dominated_pair_failed");
        }
        Outcome::IntegrityError if descending => {
            return finish_level(search, index, "integrity_edge", voltage_bins)
        }
        // Missing proof ends only this level's descent; the pair is never claimed.
        Outcome::Inconclusive if descending => {
            return finish_level(search, index, "evidence_boundary", voltage_bins)
        }
        _ => {}
    }
    let band = &mut search.bands[index];
    let lower_power_voltage = lower_power_voltage(band, voltage_bins, power_hint);
    match outcome {
        Outcome::Qualified => {
            if band.last_qualified_clock_mhz != Some(band.target_clock_mhz) {
                band.first_qualified_voltage_mv = Some(band.voltage_mv);
            }
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
                return;
            }
            // Staircase: keep descending this clock until its first failure.
            match nth_below(voltage_bins, band.voltage_mv, step) {
                Some(voltage) => {
                    band.voltage_mv = voltage;
                    band.status = "pending".into();
                }
                None => finish_level(search, index, "voltage_floor_reached", voltage_bins),
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
        _ => unreachable!("record handles operational, control, cancel and driver outcomes"),
    }
}

/// A level's first failure or voltage floor. Its publication pair sits the game margin above the
/// lowest pass. When the descent did not test it, the level qualifies it next (it is dominated by
/// the level above); otherwise the level closes.
fn finish_level(search: &mut ForgeDiscoverySearch, index: usize, reason: &str, voltage_bins: &[u32]) {
    let band = &mut search.bands[index];
    let publication = band
        .last_qualified_voltage_mv
        .and_then(|lowest| bin_at_or_above(voltage_bins, lowest + GAME_MARGIN_MV))
        .filter(|mv| {
            band.id != "performance" && band.first_qualified_voltage_mv.is_some_and(|start| *mv > start)
        });
    match publication {
        Some(mv) => {
            band.voltage_mv = mv;
            band.publishing = true;
            band.status = "pending".into();
        }
        None => close_band(band, reason),
    }
}

fn finish_transition(search: &mut ForgeDiscoverySearch, clock_bins: &[u32], voltage_bins: &[u32]) {
    if search.stop_reason.is_some() {
        return;
    }
    start_next_level(search, clock_bins, voltage_bins);
    if search.integrity_errors >= 2 {
        close_all(search, "integrity_error_budget_exhausted");
    } else if search.stop_reason.is_none() {
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
    /// The test 3060 Ti grid: 15 MHz clock bins and 6.25 mV voltage bins (integer mV).
    fn staircase_at(power_free_mv: u32) -> (ForgeDiscoverySearch, Vec<u32>, Vec<u32>) {
        let seed = |id| Seed { id, target_clock_mhz: 1920, voltage_mv: power_free_mv, clock_ceiling_mhz: 1920 };
        let state = new(&[seed("performance"), seed("balanced"), seed("efficiency")], 24, 10_000);
        let volts = (0..=30u32).map(|bin| (81_250 + bin * 625) / 100).collect();
        (state, (1695..=1920).step_by(15).collect(), volts)
    }
    fn staircase() -> (ForgeDiscoverySearch, Vec<u32>, Vec<u32>) {
        staircase_at(937)
    }
    fn walk(state: &mut ForgeDiscoverySearch, clocks: &[u32], volts: &[u32], outcome: Outcome) -> (usize, u32, u32) {
        let index = next_band(state).unwrap();
        let pair = admit(state, index, 0, 100).map(|c| (index, c.target_clock_mhz, c.voltage_mv)).unwrap();
        record(state, index, outcome, clocks, volts, None).unwrap();
        pair
    }
    fn walk_all(state: &mut ForgeDiscoverySearch, clocks: &[u32], volts: &[u32], outcomes: &[Outcome]) -> Vec<(usize, u32, u32)> {
        outcomes.iter().map(|outcome| walk(state, clocks, volts, *outcome)).collect()
    }
    #[test]
    fn compensated_staircase_publishes_every_level_with_the_game_margin() {
        use Outcome::*;
        let (mut state, clocks, volts) = staircase();
        let trace = walk_all(&mut state, &clocks, &volts, &[
            Qualified, Qualified, Qualified, Qualified, IntegrityError,
            Qualified, Qualified,
            Qualified, Qualified, Qualified, Qualified, Qualified, IntegrityError,
            Qualified, Qualified, Qualified, IntegrityError, Qualified,
        ]);
        assert_eq!(trace, [
            // Top: 918 + 36 mV exceeds the power-free 937, so the top itself is not publishable.
            (0, 1920, 937), (0, 1920, 931), (0, 1920, 925), (0, 1920, 918), (0, 1920, 912),
            // Compensated top two bins lower: margin pair first, then its publication pair.
            (1, 1890, 900), (1, 1890, 937),
            // -5% of 1890 from the dominated start, two bins per step; 850 + 36 mV = 887 was tested.
            (2, 1800, 900), (2, 1800, 887), (2, 1800, 875), (2, 1800, 862), (2, 1800, 850), (2, 1800, 837),
            // -10%: 825 + 36 mV = 862 is above its start, so it gets its own admission.
            (3, 1710, 850), (3, 1710, 837), (3, 1710, 825), (3, 1710, 812), (3, 1710, 862),
        ]);
        let reasons: Vec<_> = state.bands.iter().map(|b| b.stop_reason.as_deref().unwrap()).collect();
        assert_eq!(reasons, ["integrity_edge", "published", "integrity_edge", "published"]);
        assert_eq!(state.bands[1].last_qualified_voltage_mv, Some(900), "publication keeps the lowest pass");
        assert_eq!((state.integrity_errors, state.stop_reason.as_deref()), (0, Some("all_regions_closed")));
    }
    #[test]
    fn compensation_is_skipped_when_the_top_already_holds_the_game_margin() {
        use Outcome::*;
        let (mut state, clocks, volts) = staircase_at(956);
        walk_all(&mut state, &clocks, &volts, &[Qualified; 7]);
        walk(&mut state, &clocks, &volts, IntegrityError);
        assert_eq!(state.bands[1].stop_reason.as_deref(), Some("compensation_not_needed"));
        assert_eq!((state.bands[2].target_clock_mhz, state.bands[2].voltage_mv), (1830, 918));
    }
    #[test]
    fn compensated_top_steps_down_on_a_failed_margin_pair_and_gives_up_after_two_retries() {
        use Outcome::*;
        let (mut state, clocks, volts) = staircase();
        walk_all(&mut state, &clocks, &volts, &[Qualified, Qualified, Qualified, Qualified, IntegrityError]);
        let mut paused = state.clone();
        assert_eq!(walk_all(&mut state, &clocks, &volts, &[IntegrityError, Inconclusive, IntegrityError]),
            [(1, 1890, 900), (1, 1875, 900), (1, 1860, 900)]);
        assert_eq!(state.bands[1].stop_reason.as_deref(), Some("compensated_top_unavailable"));
        assert_eq!(state.integrity_errors, 0, "the prediction failing is not an integrity budget event");
        // The lower levels then start from the top's lowest pass, which dominates them.
        assert_eq!((state.bands[2].target_clock_mhz, state.bands[2].voltage_mv), (1770, 918));
        // A TDR on the margin pair pauses with the search open, one clock bin lower.
        walk(&mut paused, &clocks, &volts, DriverFailure);
        assert_eq!((paused.stop_reason.as_deref(), paused.bands[1].status.as_str()), (None, "pending"));
        assert_eq!(paused.bands[1].target_clock_mhz, 1875);
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
        // The lowest pass is 937: 937 + 36 mV rounds to 975, 38 mV over the power-free 937, so the
        // compensated top is four clock bins lower (1860), verified at its margin pair 900.
        assert_eq!(walk(&mut restored, &clocks, &volts, Outcome::Qualified), (1, 1860, 900));
        walk(&mut restored, &clocks, &volts, Outcome::Qualified);
        let mut dominated = restored.clone();
        assert_eq!(walk(&mut restored, &clocks, &volts, Outcome::IntegrityError), (2, 1770, 900));
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
    fn a_failed_publication_pair_is_inconsistent_evidence() {
        use Outcome::*;
        let (mut state, clocks, volts) = staircase();
        walk_all(&mut state, &clocks, &volts, &[Qualified, Qualified, Qualified, Qualified, IntegrityError, Qualified]);
        assert!(state.bands[1].publishing);
        walk(&mut state, &clocks, &volts, IntegrityError);
        assert_eq!((state.stop_reason.as_deref(), state.integrity_errors), (Some("dominated_pair_failed"), 1));
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
        // This tiny grid has no bin 36 mV above 906, so no compensated top; and no bin lies in
        // [95% of 1710, 1710), so balanced passes through to efficiency.
        assert_eq!(state.bands[1].stop_reason.as_deref(),Some("compensated_top_unavailable"));
        assert_eq!(state.bands[2].stop_reason.as_deref(),Some("clock_level_exhausted"));
        assert_eq!((state.stop_reason.as_deref(),next_band(&state)),(None,Some(3)));
        assert_eq!((state.bands[3].target_clock_mhz,state.bands[3].voltage_mv),(1605,906));
        step(&mut state,Outcome::Inconclusive);
        assert_eq!((state.bands[3].status.as_str(),next_band(&state)),("closed",None));
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
        assert_eq!(next_band(&state),Some(3));
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
        assert_eq!(next_band(&state),Some(3));
        assert_eq!((state.bands[3].target_clock_mhz,state.bands[3].voltage_mv),(1605,900));
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
