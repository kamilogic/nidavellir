//! Pure, durable admission policy for qualification-first discovery. The caller owns all hardware,
//! evidence validation and persistence: save a successful admission BEFORE arming a candidate.
//!
//! Staircase (user decisions 2026-09-28/29), one level at a time:
//! 1. Find the top below the power limit, then descend its voltage until the first failure.
//! 2. Games need a margin above what the matrix approves. If the top's lowest pass plus that
//!    margin exceeds the top's power-free voltage, the Godforge level is measured one boost step
//!    lower at a time (search 15, user decision 2026-10-24): at each clock, the highest voltage
//!    that holds it within the power limit and below stock, then its game-margin pair that far
//!    under it. The first clock whose pair passes is Godforge's; it descends to its edge there and
//!    publishes edge + margin, like the lower levels. No slope prediction, no rounding.
//! 3. The -5% and -10% levels of the compensated top start at the lowest voltage the level above
//!    passed and descend in two-bin steps until the first failure. A publication pair the descent
//!    did not test, above its range or between two of its steps, gets its own admission.
//! 4. Three distinct profiles (user decision 2026-09-30): a lower level must end two steps below
//!    its start, else it retries one clock bin lower (at most two); one that never went below its
//!    start publishes nothing.
//! 5. Hot-bin relief (user decision 2026-10-01): a pass whose critical phases held the target
//!    mostly one bin below it keeps the descent going, but anchors the game margin one clock
//!    bin's worth of voltage higher. It never turns a result inconclusive.
//! 6. An unqualified top that dropped its clock at the requested voltage descends, once per run,
//!    to the highest clock bin the GPU held there (user decision 2026-10-21).
//! 7. Every planned pair stays below the stock curve (user decision 2026-10-22). A lower clock
//!    at the voltage passed above can sit at or over the stock clock there; then the stock curve
//!    already runs it with less voltage, and the plan refuses it. The compensated top publishes at
//!    the highest voltage that is power-free AND still below stock at its clock, and a lower level
//!    starts no higher than that.
//! 8. Steps are physical (user decision 2026-10-23): clocks move by GPU Boost's 15 MHz step and
//!    voltages by mV, whatever the spacing of a series' curve points. A lower level's edge is one
//!    bin exact: the bin its 10 mV step skipped above the first failure is tested on its own, and
//!    a crash there is an edge like any other.

use nidavellir_core::ipc::{ForgeDiscoveryBand, ForgeDiscoverySearch};

pub const VERSION: u32 = 15;
/// Room for the distinct-profile clock retries, each level's edge refinement, a publication pair
/// the descent skipped and the Godforge level's power measurements and descent; the time budget
/// remains the hard cap.
pub const STANDARD_ATTEMPTS: u32 = 44;
/// The measured Godforge level adds about an hour to a 3060 Ti run (6.4 h on 0.8.13); a run
/// that recovers from edge crashes needs the rest.
pub const STANDARD_BUDGET_MS: u64 = 9 * 60 * 60 * 1_000;
/// Games need about this much more voltage than the matrix approves (user decisions 2026-09-29 and
/// 2026-10-23). 1830@856 passed the matrix, yet 1815@875 crashes Overwatch within 30 min and 1800@875
/// is the user's daily undervolt. Godforge 1920@925, two bins above its failure, TDR'd in Overwatch
/// after ~23 min. Two clean runs then put the 1800 MHz edge between 825 (failed) and 831 (passed):
/// six bins over 831 published 868, as close to that edge as the 1815@875 crash, while 875 is seven.
/// 43 mV is seven 6.25 mV bins after integer-mV truncation (43-44 mV), never six (37-38 mV). It is
/// a voltage, so 5 mV curves (RTX 40/50) get the same margin, 45 mV up.
pub const GAME_MARGIN_MV: u32 = 43;
/// GPU Boost moves clocks in 15 MHz steps on every series, even where curve points sit closer (an
/// RTX 5070's are ~7.5 MHz apart), so the search moves a clock by at least this much.
const BOOST_STEP_MHZ: u32 = 15;
/// A hot-bin pass is evidence one boost step lower, worth this much voltage at the target: on the
/// test 3060 Ti the edge fell 918 → 856 mV over 1920 → 1830 MHz, ~10 mV per boost step. Profile
/// synthesis applies the identical anchor rule.
pub const HOT_BIN_RELIEF_MV: u32 = 10;

fn relief_mv(hot_bin: bool) -> u32 {
    if hot_bin { HOT_BIN_RELIEF_MV } else { 0 }
}
/// The Godforge level starts one boost step under the top and steps one more down per failed
/// game-margin pair, at most this often (60 MHz in all).
const COMPENSATION_RETRIES: u32 = 3;
/// Lower levels descend about this much per admission, two bins on 6.25 mV and 5 mV curves alike:
/// their descent from the dominated start spans ~10 bins. The bin a step skips above the first
/// failure is then tested on its own, so the edge is one bin exact (user decision 2026-10-23).
const LEVEL_STEP_MV: u32 = 10;
/// A lower level is a distinct profile only this far below its start: with the game margin its
/// publication then sits ~25 mV (~8 W on the test 3060 Ti) under the level above's.
const DISTINCT_LEVEL_MV: u32 = 25;
/// A level that is not distinct retries one boost step lower, at most this often. Keep the profile
/// clock floors (Brokkr's 92%, Deep Calm 87% of Godforge) below the lowest retry.
const LEVEL_CLOCK_RETRIES: u32 = 2;
/// Lower clock levels after the (compensated) top, as (band id, percent of that top's clock).
const LEVELS: [(&str, u64); 2] = [("balanced", 95), ("efficiency", 90)];

fn bin_at_or_above(bins: &[u32], mv: u32) -> Option<u32> {
    bins.iter().copied().filter(|bin| *bin >= mv).min()
}

fn bin_at_or_below(bins: &[u32], mv: u32) -> Option<u32> {
    bins.iter().copied().filter(|bin| *bin <= mv).max()
}

/// The highest clock bin at least one boost step under `clock`.
fn step_below(clock_bins: &[u32], clock: u32) -> Option<u32> {
    clock_bins.iter().copied().filter(|bin| bin + BOOST_STEP_MHZ <= clock).max()
}

/// The F2 plan raises its anchor, so the stock clock at the pair's voltage must stay below the
/// pair's clock. Field report 2026-10-22: an RTX 4070 SUPER reached its curve's 1100 mV limit at
/// ~200 of 220 W, and v12 published the compensated top 2685@1100 over a 2715 MHz stock point.
fn undervolts(stock: &[(usize, u32, u32)], clock: u32, mv: u32) -> bool {
    crate::gpu_undervolt::select_exact_apply_anchor_bin(stock, clock, mv).is_some()
}

/// The highest voltage bin at or below `limit` that still undervolts `clock`.
fn undervolt_bin(voltage_bins: &[u32], stock: &[(usize, u32, u32)], clock: u32, limit: u32) -> Option<u32> {
    voltage_bins.iter().copied().filter(|mv| *mv <= limit && undervolts(stock, clock, *mv)).max()
}

/// A lower level whose start moved under the voltage the level above passed, to stay below the
/// stock curve: that first pair is a new test, not a dominated one, and it hands no pass down.
pub fn moved_below_stock(band: &ForgeDiscoveryBand) -> bool {
    band.clock_retries_used == 0
        && band.last_qualified_clock_mhz.is_none()
        && band.level_start_voltage_mv.is_some_and(|start| band.voltage_mv < start)
}

/// Whether a lower level's descent qualified `mv` at its current clock. Its 10 mV steps from the
/// first pass skip every other bin, so seven bins over a lowest pass on a step land between two
/// steps. ponytail: rebuilt from the step rule, not stored; a power jump mid-descent (rare) costs
/// at worst one dominated admission, or a profile at the next tested voltage above.
fn descent_tested(band: &ForgeDiscoveryBand, voltage_bins: &[u32], mv: u32) -> bool {
    let mut step = band.first_qualified_voltage_mv;
    while let Some(voltage) = step.filter(|voltage| *voltage > mv) {
        step = bin_at_or_below(voltage_bins, voltage.saturating_sub(LEVEL_STEP_MV));
    }
    step == Some(mv)
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

/// The sustained (p5) clock a confirmed ClockDrop held at the candidate's own voltage, recovered
/// and validated by the caller. Never a pass.
#[derive(Debug, Clone, Copy)]
pub struct ClockDropHint {
    pub held_clock_mhz: u32,
    /// The stock clock of that voltage's VF point: an undervolt pair must target above it.
    pub stock_clock_mhz: u32,
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
    /// Qualified, but a lane's critical phases held the target mostly at the hot bin below it.
    QualifiedHotBin,
    /// Confirmed power limitation, no integrity/thermal/containment error, cleanup confirmed.
    PowerBound,
    /// A power measurement only (the Godforge level's): the pair held its clock within the power
    /// limit. No matrix ran, so it proves nothing about stability.
    PowerFree,
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
/// pass; it still gets its own matrix. When that voltage would not undervolt the lower clock, the
/// level starts at the highest one that does. Nothing here is a predeclared profile result.
fn start_next_level(
    search: &mut ForgeDiscoverySearch,
    clock_bins: &[u32],
    voltage_bins: &[u32],
    stock: &[(usize, u32, u32)],
) {
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
    let top_relief = relief_mv(top.lowest_hot_bin);
    if let Some(comp) = search.bands.iter().position(|b| b.id == "compensated") {
        if search.bands[comp].status == "waiting_for_top" {
            let publication = bin_at_or_above(voltage_bins, top_lowest + top_relief + GAME_MARGIN_MV);
            search.bands[comp].clock_ceiling_mhz = top_clock;
            if publication.is_some_and(|mv| mv <= power_free) {
                let band = &mut search.bands[comp];
                band.target_clock_mhz = top_clock;
                band.voltage_mv = top_lowest;
                close_band(band, "compensation_not_needed");
            } else {
                // One boost step under the top, the top's power-free voltage holds the clock too.
                let clock = step_below(clock_bins, top_clock);
                let allowed = clock.and_then(|clock| undervolt_bin(voltage_bins, stock, clock, power_free));
                match clock.zip(allowed) {
                    Some((clock, mv)) => {
                        search.bands[comp].target_clock_mhz = clock;
                        search.bands[comp].allowed_voltage_mv = Some(mv);
                        measure_or_margin(search, comp, voltage_bins, stock);
                    }
                    None => godforge_unavailable(search, comp),
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
                prev_voltage = band.last_qualified_voltage_mv
                    .or(band.level_start_voltage_mv.filter(|_| moved_below_stock(band)))
                    .unwrap_or(band.voltage_mv);
                continue;
            }
            "waiting_for_top" => {}
            _ => return,
        }
        let clock = clock_bins.iter().copied()
            .filter(|c| *c <= level_top && u64::from(*c) * 100 >= u64::from(level_top) * percent)
            .min()
            .filter(|c| *c < prev_clock);
        let start = clock.and_then(|clock| undervolt_bin(voltage_bins, stock, clock, prev_voltage));
        band.voltage_mv = start.unwrap_or(prev_voltage);
        band.level_start_voltage_mv = Some(prev_voltage);
        band.target_clock_mhz = clock.unwrap_or(prev_clock);
        band.clock_ceiling_mhz = band.target_clock_mhz;
        if start.is_some() {
            band.status = "pending".into();
            return;
        }
        // No bin strictly below the previous level: pass its clock/voltage through to the next one.
        close_band(band, "clock_level_exhausted");
    }
}

/// Pure result transition. No propagated blacklist or physical stability inference is produced.
/// `stock` is the run's stock VF curve as the F2 plan reads it: (index, mV, MHz).
#[allow(clippy::too_many_arguments)]
pub fn record(
    search: &mut ForgeDiscoverySearch,
    index: usize,
    outcome: Outcome,
    clock_bins: &[u32],
    voltage_bins: &[u32],
    stock: &[(usize, u32, u32)],
    power_hint: Option<PowerBoundHint>,
    clock_hint: Option<ClockDropHint>,
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
        Outcome::PowerFree if !search.bands[index].measuring_power => {
            return Err("power_measurement_not_requested".into());
        }
        _ if search.bands[index].publishing => record_publication(search, index, outcome),
        _ if search.bands[index].id == "compensated" => {
            record_compensated(search, index, outcome, clock_bins, voltage_bins, stock)
        }
        _ => record_level(search, index, outcome, clock_bins, voltage_bins, stock, power_hint, clock_hint),
    }
    finish_transition(search, clock_bins, voltage_bins, stock);
    Ok(())
}

/// A publication pair sits at or above a voltage that already passed at a higher-or-equal clock,
/// so it must pass; a failure there is inconsistent evidence and stops the search.
fn record_publication(search: &mut ForgeDiscoverySearch, index: usize, outcome: Outcome) {
    search.bands[index].publishing = false;
    match outcome {
        Outcome::Qualified | Outcome::QualifiedHotBin => close_band(&mut search.bands[index], "published"),
        Outcome::IntegrityError => {
            search.integrity_errors += 1;
            close_all(search, "dominated_pair_failed");
        }
        Outcome::DriverFailure => close_all(search, "driver_failure_recovery_required"),
        _ => close_band(&mut search.bands[index], "publication_unproven"),
    }
}

/// The Godforge level (search 15). At each clock it first measures, with power measurements only,
/// the highest voltage that holds the clock within the power limit and below the stock curve. Its
/// game-margin pair then sits the margin under that voltage: a pass makes this clock Godforge's,
/// and the level descends to its edge like a lower level. A failed pair steps one boost step down,
/// a bounded number of times; a TDR there pauses like a level edge, and Resume tries the lower clock.
fn record_compensated(
    search: &mut ForgeDiscoverySearch,
    index: usize,
    outcome: Outcome,
    clock_bins: &[u32],
    voltage_bins: &[u32],
    stock: &[(usize, u32, u32)],
) {
    let band = &mut search.bands[index];
    if band.measuring_power {
        match outcome {
            Outcome::PowerFree | Outcome::Qualified | Outcome::QualifiedHotBin => {
                band.allowed_voltage_mv = Some(band.voltage_mv);
                measure_or_margin(search, index, voltage_bins, stock);
            }
            // More voltage at a lower clock than a pair the top passed: a crash is no edge there.
            Outcome::DriverFailure => close_all(search, "driver_failure_recovery_required"),
            // Over the power limit, short of the clock or anything else: the last voltage holds.
            _ => margin_pair(search, index, voltage_bins),
        }
        return;
    }
    if band.last_qualified_clock_mhz == Some(band.target_clock_mhz)
        || matches!(outcome, Outcome::Qualified | Outcome::QualifiedHotBin)
    {
        return record_level(search, index, outcome, clock_bins, voltage_bins, stock, None, None);
    }
    step_godforge_down(search, index, clock_bins, voltage_bins, stock);
}

/// The next power measurement, one bin above the highest voltage that held the clock while it
/// still undervolts it; past that, the game-margin pair.
fn measure_or_margin(search: &mut ForgeDiscoverySearch, index: usize, voltage_bins: &[u32], stock: &[(usize, u32, u32)]) {
    let band = &mut search.bands[index];
    let clock = band.target_clock_mhz;
    let higher = band.allowed_voltage_mv
        .and_then(|mv| voltage_bins.iter().copied().filter(|bin| *bin > mv).min())
        .filter(|mv| undervolts(stock, clock, *mv));
    match higher {
        Some(mv) => {
            band.measuring_power = true;
            band.voltage_mv = mv;
            band.status = "pending".into();
        }
        None => margin_pair(search, index, voltage_bins),
    }
}

/// The game-margin pair: the margin under the highest voltage that held the clock.
fn margin_pair(search: &mut ForgeDiscoverySearch, index: usize, voltage_bins: &[u32]) {
    let band = &mut search.bands[index];
    band.measuring_power = false;
    match band.allowed_voltage_mv.and_then(|mv| bin_at_or_below(voltage_bins, mv.saturating_sub(GAME_MARGIN_MV))) {
        Some(mv) => {
            band.voltage_mv = mv;
            band.status = "pending".into();
        }
        None => godforge_unavailable(search, index),
    }
}

/// One boost step down, where what held the clock holds too (re-checked against the stock curve).
fn step_godforge_down(
    search: &mut ForgeDiscoverySearch,
    index: usize,
    clock_bins: &[u32],
    voltage_bins: &[u32],
    stock: &[(usize, u32, u32)],
) {
    let band = &mut search.bands[index];
    let lower = step_below(clock_bins, band.target_clock_mhz);
    let allowed = lower.zip(band.allowed_voltage_mv)
        .and_then(|(clock, mv)| undervolt_bin(voltage_bins, stock, clock, mv));
    match lower.zip(allowed) {
        Some((clock, mv)) if band.clock_retries_used < COMPENSATION_RETRIES => {
            band.clock_retries_used += 1;
            band.target_clock_mhz = clock;
            band.allowed_voltage_mv = Some(mv);
            band.first_qualified_voltage_mv = None;
            band.last_qualified_clock_mhz = None;
            band.last_qualified_voltage_mv = None;
            band.lowest_hot_bin = false;
            measure_or_margin(search, index, voltage_bins, stock);
        }
        _ => godforge_unavailable(search, index),
    }
}

/// No Godforge clock passed: the lower levels start from the top's lowest pass, which dominates
/// them, relative to the top's clock.
fn godforge_unavailable(search: &mut ForgeDiscoverySearch, index: usize) {
    let top = search.bands.iter().find(|band| band.id == "performance");
    let (clock, lowest) = (top.and_then(|band| band.last_qualified_clock_mhz), top.and_then(|band| band.last_qualified_voltage_mv));
    let band = &mut search.bands[index];
    band.target_clock_mhz = clock.unwrap_or(band.target_clock_mhz);
    band.voltage_mv = lowest.unwrap_or(band.voltage_mv);
    band.measuring_power = false;
    band.first_qualified_voltage_mv = None;
    band.last_qualified_clock_mhz = None;
    band.last_qualified_voltage_mv = None;
    close_band(band, "compensated_top_unavailable");
}

/// The top and the lower levels. A level first qualifies its clock, then descends until its
/// first failure: the expected staircase edge, which does not spend the integrity budget.
#[allow(clippy::too_many_arguments)]
fn record_level(
    search: &mut ForgeDiscoverySearch,
    index: usize,
    outcome: Outcome,
    clock_bins: &[u32],
    voltage_bins: &[u32],
    stock: &[(usize, u32, u32)],
    power_hint: Option<PowerBoundHint>,
    clock_hint: Option<ClockDropHint>,
) {
    // An edge refinement settles its level's edge with any result; nothing descends past it.
    if let Some(edge) = search.bands[index].refining_edge.take() {
        let band = &mut search.bands[index];
        let reason = match outcome {
            Outcome::Qualified | Outcome::QualifiedHotBin => {
                band.last_qualified_voltage_mv = Some(band.voltage_mv);
                band.lowest_hot_bin = outcome == Outcome::QualifiedHotBin;
                edge
            }
            Outcome::DriverFailure => "tdr_edge".into(),
            Outcome::IntegrityError => "integrity_edge".into(),
            _ => "evidence_boundary".into(),
        };
        return finish_level(search, index, &reason, clock_bins, voltage_bins, stock);
    }
    let band = &search.bands[index];
    let descending = band.last_qualified_clock_mhz == Some(band.target_clock_mhz)
        && band.last_qualified_voltage_mv.is_some_and(|mv| band.voltage_mv < mv);
    // A lower level's first pair repeats a voltage the level above passed, so it must pass,
    // unless it moved under that voltage to stay below the stock curve.
    let dominated_start = band.id != "performance" && band.last_qualified_clock_mhz.is_none()
        && !moved_below_stock(band);
    match outcome {
        // The boot is dirty after a TDR (2026-07-22), so the caller stops this session. The
        // search stays open: after reboot and acknowledgement, Resume continues.
        Outcome::DriverFailure if descending => {
            return finish_level(search, index, "tdr_edge", clock_bins, voltage_bins, stock)
        }
        Outcome::DriverFailure => return close_all(search, "driver_failure_recovery_required"),
        Outcome::IntegrityError if dominated_start => {
            search.integrity_errors += 1;
            return close_all(search, "dominated_pair_failed");
        }
        Outcome::IntegrityError if descending => {
            return finish_level(search, index, "integrity_edge", clock_bins, voltage_bins, stock)
        }
        // Missing proof ends only this level's descent; the pair is never claimed.
        Outcome::Inconclusive if descending => {
            return finish_level(search, index, "evidence_boundary", clock_bins, voltage_bins, stock)
        }
        _ => {}
    }
    let band = &mut search.bands[index];
    let lower_power_voltage = lower_power_voltage(band, voltage_bins, power_hint);
    match outcome {
        Outcome::Qualified | Outcome::QualifiedHotBin => {
            if band.last_qualified_clock_mhz != Some(band.target_clock_mhz) {
                band.first_qualified_voltage_mv = Some(band.voltage_mv);
            }
            band.power_preparation_used = false;
            clear_power_bracket(band);
            band.last_qualified_clock_mhz = Some(band.target_clock_mhz);
            band.last_qualified_voltage_mv = Some(band.voltage_mv);
            band.lowest_hot_bin = outcome == Outcome::QualifiedHotBin;
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
            // Staircase: keep descending this clock until its first failure, one bin at the top
            // and a 10 mV step on the lower levels.
            let next = if band.id == "performance" {
                nth_below(voltage_bins, band.voltage_mv, 1)
            } else {
                bin_at_or_below(voltage_bins, band.voltage_mv.saturating_sub(LEVEL_STEP_MV))
            };
            match next {
                Some(voltage) => {
                    band.voltage_mv = voltage;
                    band.status = "pending".into();
                }
                None => finish_level(search, index, "voltage_floor_reached", clock_bins, voltage_bins, stock),
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
            // At the voltage floor, explore the next boost step without inventing a pass.
            let lower_clock = step_below(clock_bins, band.target_clock_mhz);
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
            let lower_clock = step_below(clock_bins, band.target_clock_mhz);
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
        // profile (run f2-forge-1790544997509), so an unqualified top first descends at the same
        // voltage, once per run. That is a new hypothesis, not an inferred boundary. After a
        // ClockDrop it goes to the highest bin the GPU held there, since the bins in between
        // dropped too: an RTX 5070 held ~2910 MHz at 1085 mV under 2992, and one 7 MHz bin lower
        // ended the run (field report 2026-10-21). Otherwise one bin. Either stays above the
        // voltage's stock clock: 2880@1090 under a 2887 stock point was no undervolt and stopped
        // the run as untestable (same report, 0.8.6).
        Outcome::Inconclusive => {
            let floor = clock_hint.map_or(0, |hint| hint.stock_clock_mhz);
            let below = |limit: u32| {
                clock_bins.iter().copied()
                    .filter(|clock| clock + BOOST_STEP_MHZ <= band.target_clock_mhz && *clock <= limit && *clock > floor)
                    .max()
            };
            let lower_clock = clock_hint.and_then(|hint| below(hint.held_clock_mhz)).or_else(|| below(u32::MAX));
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
/// lowest pass. A lower level short of two steps below its start would repeat (almost) the level
/// above's voltage with less clock, so it first retries one clock bin lower from its lowest pass
/// (dominated there, and only while that still undervolts the lower clock); one that never went
/// below its start publishes nothing. When the descent did not test the publication pair, the
/// level qualifies it next, unless it would not undervolt; otherwise the level closes. The
/// Godforge level publishes within the voltage it measured to hold its clock: hot-bin relief can
/// push the publication over it, and then the level steps one boost step down.
fn finish_level(
    search: &mut ForgeDiscoverySearch,
    index: usize,
    reason: &str,
    clock_bins: &[u32],
    voltage_bins: &[u32],
    stock: &[(usize, u32, u32)],
) {
    let band = &mut search.bands[index];
    let lowest = band.last_qualified_voltage_mv;
    if band.id != "performance" {
        // The bin a 10 mV step skipped between the lowest pass and this edge gets its own test
        // first, so the edge is one bin exact; Safe Loop recovery carries a crash there.
        let skipped = lowest.and_then(|lowest| {
            voltage_bins.iter().copied().filter(|mv| *mv > band.voltage_mv && *mv < lowest).max()
        });
        if let Some(mv) = skipped.filter(|_| band.refining_edge.is_none()) {
            band.refining_edge = Some(reason.into());
            band.voltage_mv = mv;
            band.status = "pending".into();
            return;
        }
    }
    if band.id != "performance" && band.id != "compensated" {
        let start = band.level_start_voltage_mv;
        let distinct = lowest.zip(start).is_some_and(|(lowest, start)| lowest + DISTINCT_LEVEL_MV <= start);
        let lower_clock = step_below(clock_bins, band.target_clock_mhz)
            .filter(|clock| lowest.is_some_and(|mv| undervolts(stock, *clock, mv)));
        if !distinct && band.clock_retries_used < LEVEL_CLOCK_RETRIES {
            if let (Some(clock), Some(mv)) = (lower_clock, lowest) {
                band.clock_retries_used += 1;
                band.target_clock_mhz = clock;
                band.clock_ceiling_mhz = clock;
                band.voltage_mv = mv;
                band.first_qualified_voltage_mv = None;
                band.last_qualified_clock_mhz = None;
                band.last_qualified_voltage_mv = None;
                band.lowest_hot_bin = false;
                band.status = "pending".into();
                return;
            }
        }
        if lowest.zip(start).is_none_or(|(lowest, start)| lowest >= start) {
            return close_band(band, "no_distinct_profile");
        }
    }
    let relief = relief_mv(band.lowest_hot_bin);
    let publication = lowest
        .and_then(|lowest| bin_at_or_above(voltage_bins, lowest + relief + GAME_MARGIN_MV))
        .filter(|mv| band.id != "performance" && !descent_tested(band, voltage_bins, *mv));
    if band.id == "compensated" && publication.zip(band.allowed_voltage_mv).is_some_and(|(mv, allowed)| mv > allowed) {
        return step_godforge_down(search, index, clock_bins, voltage_bins, stock);
    }
    match publication {
        // The stock curve already runs this clock below the game margin: no profile here.
        Some(mv) if !undervolts(stock, band.target_clock_mhz, mv) => close_band(band, "publication_at_stock"),
        Some(mv) => {
            band.voltage_mv = mv;
            band.publishing = true;
            band.status = "pending".into();
        }
        None => close_band(band, reason),
    }
}

fn finish_transition(
    search: &mut ForgeDiscoverySearch,
    clock_bins: &[u32],
    voltage_bins: &[u32],
    stock: &[(usize, u32, u32)],
) {
    if search.stop_reason.is_some() {
        return;
    }
    start_next_level(search, clock_bins, voltage_bins, stock);
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
        record(state, index, outcome, CLOCKS, VOLTAGES, &low_stock(VOLTAGES), None, None).unwrap();
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
        let state = new(&[seed("performance"), seed("balanced"), seed("efficiency")], STANDARD_ATTEMPTS, 10_000);
        let volts = (0..=30u32).map(|bin| (81_250 + bin * 625) / 100).collect();
        (state, (1695..=1920).step_by(15).collect(), volts)
    }
    fn staircase() -> (ForgeDiscoverySearch, Vec<u32>, Vec<u32>) {
        staircase_at(937)
    }
    /// The top passes 937 → 906 at 1920 and fails 900. Godforge: 943 is over the power limit at
    /// 1905 and its margin pair 893 fails; at 1890, 943 is over the limit again, 893 passes, the
    /// edge is 893 (881 and the refined 887 fail) and 893 + 43 mV = 937 is published.
    fn godforge_1890(state: &mut ForgeDiscoverySearch, clocks: &[u32], volts: &[u32]) -> Vec<(usize, u32, u32)> {
        use Outcome::*;
        walk_all(state, clocks, volts, &[
            Qualified, Qualified, Qualified, Qualified, Qualified, Qualified, IntegrityError,
            PowerBound, IntegrityError, PowerBound, Qualified, IntegrityError, IntegrityError, Qualified,
        ])
    }
    /// A stock curve far below every target: every pair undervolts.
    fn low_stock(volts: &[u32]) -> Vec<(usize, u32, u32)> {
        volts.iter().enumerate().map(|(index, mv)| (index, *mv, 0)).collect()
    }
    fn walk(state: &mut ForgeDiscoverySearch, clocks: &[u32], volts: &[u32], outcome: Outcome) -> (usize, u32, u32) {
        walk_on(state, clocks, volts, &low_stock(volts), outcome)
    }
    fn walk_on(state: &mut ForgeDiscoverySearch, clocks: &[u32], volts: &[u32], stock: &[(usize, u32, u32)],
        outcome: Outcome) -> (usize, u32, u32) {
        let index = next_band(state).unwrap();
        let pair = admit(state, index, 0, 100).map(|c| (index, c.target_clock_mhz, c.voltage_mv)).unwrap();
        record(state, index, outcome, clocks, volts, stock, None, None).unwrap();
        pair
    }
    fn walk_all(state: &mut ForgeDiscoverySearch, clocks: &[u32], volts: &[u32], outcomes: &[Outcome]) -> Vec<(usize, u32, u32)> {
        outcomes.iter().map(|outcome| walk(state, clocks, volts, *outcome)).collect()
    }
    #[test]
    fn godforge_is_measured_one_boost_step_at_a_time_and_publishes_edge_plus_margin() {
        use Outcome::*;
        let (mut state, clocks, volts) = staircase();
        let trace = walk_all(&mut state, &clocks, &volts, &[
            Qualified, Qualified, Qualified, Qualified, IntegrityError,
            PowerFree, PowerBound, IntegrityError,
            PowerFree, PowerBound, Qualified, Qualified, IntegrityError, Qualified, Qualified,
        ]);
        assert_eq!(trace, [
            // Top: 918 + 43 mV exceeds the power-free 937, so the top itself is not publishable.
            (0, 1920, 937), (0, 1920, 931), (0, 1920, 925), (0, 1920, 918), (0, 1920, 912),
            // One boost step down, 937 holds too; 943 holds within the power limit and 950 does
            // not (power checks only). The margin pair 943 - 43 mV = 900 fails.
            (1, 1905, 943), (1, 1905, 950), (1, 1905, 900),
            // 1890: 950 holds, 956 does not, and 950 - 43 mV = 906 passes. The level descends to
            // its edge, refines the skipped 887 (passes) and publishes 887 + 43 mV = 931.
            (1, 1890, 950), (1, 1890, 956), (1, 1890, 906), (1, 1890, 893), (1, 1890, 881), (1, 1890, 887),
            (1, 1890, 931),
        ]);
        let godforge = &state.bands[1];
        assert_eq!(godforge.stop_reason.as_deref(), Some("published"));
        assert_eq!((godforge.last_qualified_clock_mhz, godforge.last_qualified_voltage_mv), (Some(1890), Some(887)));
        assert_eq!((godforge.allowed_voltage_mv, godforge.clock_retries_used), (Some(950), 1));
        // -5% of Godforge's clock, from its lowest pass (dominated there).
        let balanced = &state.bands[2];
        assert_eq!((balanced.target_clock_mhz, balanced.voltage_mv, balanced.status.as_str()), (1800, 887, "pending"));
        assert_eq!(state.integrity_errors, 0, "a failed margin pair is no integrity budget event");
        // A power measurement is never a result for a pair that asked for its matrix.
        let index = next_band(&state).unwrap();
        admit(&mut state, index, 0, 100).unwrap();
        assert_eq!(record(&mut state, index, PowerFree, &clocks, &volts, &low_stock(&volts), None, None),
            Err("power_measurement_not_requested".into()));
    }
    #[test]
    fn the_user_s_game_tested_profiles_come_out_of_the_0_8_11_edges() {
        use Outcome::*;
        // Run f2-forge-1792720262819 (3060 Ti, 2026-10-23): 1890 passed 893 and failed 887, 1800
        // passed 831 and failed 818, 1710 passed 781 and failed 768; 825 at 1800 failed on 02/10.
        // Every level publishes its edge + 43 mV: 1890@937, 1800@875 and 1710@825, the set the
        // user runs in games. 875 and 825 sit between two 10 mV steps, so each gets its own test.
        let (mut state, clocks, _) = staircase();
        let volts: Vec<u32> = (0..=40u32).map(|bin| (75_000 + bin * 625) / 100).collect();
        let trace = walk_all(&mut state, &clocks, &volts, &[
            Qualified, Qualified, Qualified, Qualified, Qualified, Qualified, IntegrityError,
            PowerBound, IntegrityError, PowerBound, Qualified, IntegrityError, IntegrityError, Qualified,
            Qualified, Qualified, Qualified, Qualified, Qualified, Qualified, IntegrityError, IntegrityError, Qualified,
            Qualified, Qualified, Qualified, Qualified, Qualified, IntegrityError, IntegrityError, Qualified,
        ]);
        assert_eq!(trace[7..14], [
            (1, 1905, 943), (1, 1905, 893), (1, 1890, 943), (1, 1890, 893), (1, 1890, 881), (1, 1890, 887),
            (1, 1890, 937),
        ]);
        assert_eq!(trace[19..23], [(2, 1800, 831), (2, 1800, 818), (2, 1800, 825), (2, 1800, 875)]);
        assert_eq!(trace[27..], [(3, 1710, 781), (3, 1710, 768), (3, 1710, 775), (3, 1710, 825)]);
        let reasons: Vec<_> = state.bands.iter().map(|b| b.stop_reason.as_deref().unwrap()).collect();
        assert_eq!(reasons, ["integrity_edge", "published", "published", "published"]);
    }
    #[test]
    fn compensation_is_skipped_when_the_top_already_holds_the_game_margin() {
        use Outcome::*;
        let (mut state, clocks, volts) = staircase_at(962);
        walk_all(&mut state, &clocks, &volts, &[Qualified; 8]);
        walk(&mut state, &clocks, &volts, IntegrityError);
        assert_eq!(state.bands[1].stop_reason.as_deref(), Some("compensation_not_needed"));
        assert_eq!((state.bands[2].target_clock_mhz, state.bands[2].voltage_mv), (1830, 918));
    }
    #[test]
    fn godforge_steps_down_on_a_failed_margin_pair_and_gives_up_after_three_retries() {
        use Outcome::*;
        let (mut state, clocks, volts) = staircase();
        walk_all(&mut state, &clocks, &volts, &[Qualified, Qualified, Qualified, Qualified, IntegrityError]);
        let mut paused = state.clone();
        let mut crashed = state.clone();
        assert_eq!(walk_all(&mut state, &clocks, &volts, &[
            PowerBound, IntegrityError, PowerBound, Inconclusive, PowerBound, IntegrityError, PowerBound, IntegrityError,
        ]), [
            (1, 1905, 943), (1, 1905, 893), (1, 1890, 943), (1, 1890, 893),
            (1, 1875, 943), (1, 1875, 893), (1, 1860, 943), (1, 1860, 893),
        ]);
        assert_eq!(state.bands[1].stop_reason.as_deref(), Some("compensated_top_unavailable"));
        assert_eq!(state.integrity_errors, 0, "a failed margin pair is no integrity budget event");
        // The lower levels then start from the top's lowest pass, relative to the top's clock.
        assert_eq!((state.bands[2].target_clock_mhz, state.bands[2].voltage_mv), (1830, 918));
        // A TDR on the margin pair pauses with the search open, one boost step lower.
        walk_all(&mut paused, &clocks, &volts, &[PowerBound, DriverFailure]);
        assert_eq!((paused.stop_reason.as_deref(), paused.bands[1].status.as_str()), (None, "pending"));
        assert_eq!((paused.bands[1].target_clock_mhz, paused.bands[1].measuring_power), (1890, true));
        // More voltage at a lower clock than a pair the top passed: a crash there ends the search.
        walk(&mut crashed, &clocks, &volts, DriverFailure);
        assert_eq!(crashed.stop_reason.as_deref(), Some("driver_failure_recovery_required"));
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
        // The lowest pass is 937: 937 + 43 mV rounds to 981, over the power-free 937, so Godforge
        // is measured one boost step down.
        assert_eq!(walk(&mut restored, &clocks, &volts, Outcome::PowerBound), (1, 1905, 943));
        assert_eq!(walk_all(&mut restored, &clocks, &volts, &[
            Outcome::Qualified, Outcome::IntegrityError, Outcome::IntegrityError, Outcome::Qualified,
        ]), [(1, 1905, 893), (1, 1905, 881), (1, 1905, 887), (1, 1905, 937)]);
        let mut dominated = restored.clone();
        assert_eq!(walk(&mut restored, &clocks, &volts, Outcome::IntegrityError), (2, 1815, 893));
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
        walk_all(&mut state, &clocks, &volts, &[
            Qualified, Qualified, Qualified, Qualified, IntegrityError, PowerBound, Qualified, IntegrityError, IntegrityError,
        ]);
        assert!(state.bands[1].publishing);
        walk(&mut state, &clocks, &volts, IntegrityError);
        assert_eq!((state.stop_reason.as_deref(), state.integrity_errors), (Some("dominated_pair_failed"), 1));
    }
    #[test]
    fn a_level_short_of_two_steps_retries_one_clock_bin_lower() {
        use Outcome::*;
        // Run 1790761502529 with the power fix: the top holds 906, Godforge publishes 1890@937 and
        // the -5% level TDRs on its first step below its start, then on the bin that step skipped.
        let (mut state, clocks, volts) = staircase();
        let mut trace = godforge_1890(&mut state, &clocks, &volts);
        trace.extend(walk_all(&mut state, &clocks, &volts, &[Qualified, DriverFailure, DriverFailure]));
        assert_eq!(state.stop_reason, None, "a TDR edge pauses with the search open");
        let balanced = &state.bands[2];
        assert_eq!((balanced.target_clock_mhz, balanced.voltage_mv, balanced.clock_retries_used), (1785, 893, 1));
        assert_eq!((balanced.level_start_voltage_mv, balanced.last_qualified_voltage_mv), (Some(893), None));
        trace.extend(walk_all(&mut state, &clocks, &volts, &[
            Qualified, Qualified, Qualified, IntegrityError, Qualified, Qualified,
        ]));
        assert_eq!(trace[14..], [
            (2, 1800, 893), (2, 1800, 881), (2, 1800, 887),
            // One boost step lower from the lowest pass (dominated there). The refined edge 862 is
            // more than 25 mV under the start, so 862 + 43 mV = 905 → 906 is published.
            (2, 1785, 893), (2, 1785, 881), (2, 1785, 868), (2, 1785, 856), (2, 1785, 862), (2, 1785, 906),
        ]);
        assert_eq!(state.bands[2].stop_reason.as_deref(), Some("published"));
        let efficiency = &state.bands[3];
        assert_eq!((efficiency.target_clock_mhz, efficiency.voltage_mv, efficiency.level_start_voltage_mv),
            (1710, 862, Some(862)));
    }
    #[test]
    fn retries_stop_after_two_clock_bins_and_publish_only_a_real_descent() {
        use Outcome::*;
        let (mut state, clocks, volts) = staircase();
        godforge_1890(&mut state, &clocks, &volts);
        let mut never = state.clone();
        let trace = walk_all(&mut state, &clocks, &volts, &[
            Qualified, IntegrityError, IntegrityError, Qualified, IntegrityError, IntegrityError,
            Qualified, Qualified, IntegrityError, Qualified, Qualified,
        ]);
        assert_eq!(trace, [
            (2, 1800, 893), (2, 1800, 881), (2, 1800, 887), (2, 1785, 893), (2, 1785, 881), (2, 1785, 887),
            // Out of retries short of 25 mV: the refined 875 + 43 mV = 918 still sits under
            // Godforge's 937.
            (2, 1770, 893), (2, 1770, 881), (2, 1770, 868), (2, 1770, 875), (2, 1770, 918),
        ]);
        assert_eq!((state.bands[2].stop_reason.as_deref(), state.bands[2].clock_retries_used), (Some("published"), 2));
        assert_eq!(state.integrity_errors, 0, "edges never spend the integrity budget");
        // Never below the start: no profile of its own; the -10% level starts from that lowest pass.
        walk_all(&mut never, &clocks, &volts, &[
            Qualified, IntegrityError, IntegrityError, Qualified, IntegrityError, IntegrityError,
            Qualified, IntegrityError, IntegrityError,
        ]);
        assert_eq!(never.bands[2].stop_reason.as_deref(), Some("no_distinct_profile"));
        assert_eq!((never.bands[3].target_clock_mhz, never.bands[3].voltage_mv), (1710, 893));
    }
    #[test]
    fn a_retry_start_that_proves_nothing_never_claims_a_pair() {
        use Outcome::*;
        let (mut state, clocks, volts) = staircase();
        godforge_1890(&mut state, &clocks, &volts);
        walk_all(&mut state, &clocks, &volts, &[Qualified, DriverFailure, DriverFailure]);
        let mut power = state.clone();
        // Missing proof at the retry's dominated start closes the level without a pass.
        assert_eq!(walk(&mut state, &clocks, &volts, Inconclusive), (2, 1785, 893));
        assert_eq!(state.bands[2].stop_reason.as_deref(), Some("evidence_incomplete_no_boundary_inferred"));
        assert_eq!(state.bands[2].last_qualified_voltage_mv, None);
        assert_eq!((state.bands[3].target_clock_mhz, state.bands[3].voltage_mv), (1710, 893));
        // A power-bound start only prepares a lower voltage, still unproven.
        walk(&mut power, &clocks, &volts, PowerBound);
        let band = &power.bands[2];
        assert_eq!((band.voltage_mv, band.status.as_str(), band.last_qualified_voltage_mv), (887, "pending", None));
    }
    #[test]
    fn a_hot_bin_anchor_raises_the_publication_without_stopping_the_descent() {
        use Outcome::*;
        let (mut state, clocks, volts) = staircase();
        godforge_1890(&mut state, &clocks, &volts);
        // The -5% level's passes below 881 held 1800 mostly at the 1785 hot bin.
        let trace = walk_all(&mut state, &clocks, &volts, &[
            Qualified, Qualified, QualifiedHotBin, QualifiedHotBin, IntegrityError, IntegrityError, Qualified,
        ]);
        assert_eq!(trace, [
            (2, 1800, 893), (2, 1800, 881), (2, 1800, 868), (2, 1800, 856), (2, 1800, 843), (2, 1800, 850),
            // 856 + 10 + 43 mV = 909 → 912; a plain anchor would publish 900.
            (2, 1800, 912),
        ]);
        assert!(state.bands[2].lowest_hot_bin);
        assert_eq!((state.bands[2].stop_reason.as_deref(), state.integrity_errors), (Some("published"), 0));
    }
    #[test]
    fn a_hot_bin_edge_that_puts_godforge_over_its_voltage_steps_one_boost_step_down() {
        use Outcome::*;
        let (mut state, clocks, volts) = staircase();
        walk_all(&mut state, &clocks, &volts, &[Qualified, Qualified, Qualified, Qualified, Qualified, Qualified, IntegrityError]);
        // At 1890 the edge 893 held mostly at the hot bin: 893 + 10 + 43 mV = 946 → 950, over the
        // 937 that holds the clock within the power limit, so 1875 is measured next.
        assert_eq!(walk_all(&mut state, &clocks, &volts, &[
            PowerBound, IntegrityError, PowerBound, QualifiedHotBin, IntegrityError, IntegrityError,
            PowerBound, Qualified, IntegrityError, IntegrityError, Qualified,
        ]), [
            (1, 1905, 943), (1, 1905, 893), (1, 1890, 943), (1, 1890, 893), (1, 1890, 881), (1, 1890, 887),
            (1, 1875, 943), (1, 1875, 893), (1, 1875, 881), (1, 1875, 887), (1, 1875, 937),
        ]);
        assert_eq!(state.bands[1].stop_reason.as_deref(), Some("published"));
        assert!(!state.bands[1].lowest_hot_bin, "the step down drops the hot-bin anchor of 1890");
        // A hot-bin top anchor: 906 + 10 + 43 mV = 959 → 962, over 937, so Godforge is measured.
        let (mut hot_top, clocks, volts) = staircase();
        walk_all(&mut hot_top, &clocks, &volts, &[
            Qualified, Qualified, Qualified, Qualified, Qualified, QualifiedHotBin, IntegrityError,
        ]);
        let godforge = &hot_top.bands[1];
        assert_eq!((godforge.target_clock_mhz, godforge.voltage_mv, godforge.measuring_power), (1905, 943, true));
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
            record(state, 0, Outcome::PowerBound, &[1905, 1920], &bins, &low_stock(&bins), hint, None).unwrap();
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
        record(&mut state, 0, Outcome::PowerBound, CLOCKS, VOLTAGES, &low_stock(VOLTAGES), hint, None).unwrap();
        assert_eq!(state.bands[0].voltage_mv, 900);
    }
    #[test]
    fn a_top_that_dropped_its_clock_descends_to_the_bin_it_held_once() {
        // Field report 2026-10-21, RTX 5070 at 74 °C: 2992@1085 held p5 2910, and one 7 MHz bin
        // lower (2985) dropped the same way.
        let clocks = [2850, 2865, 2880, 2895, 2902, 2910, 2917, 2932, 2947, 2962, 2977, 2985, 2992];
        let volts = [1070, 1075, 1080, 1085, 1090, 1095];
        let seed = |id| Seed { id, target_clock_mhz: 2992, voltage_mv: 1085, clock_ceiling_mhz: 2992 };
        let fresh = || new(&[seed("performance"), seed("balanced"), seed("efficiency")], 30, 10_000);
        let dropped = |state: &mut ForgeDiscoverySearch, held: Option<u32>| {
            admit(state, 0, 0, 100).unwrap();
            let hint = held.map(|held_clock_mhz| ClockDropHint { held_clock_mhz, stock_clock_mhz: 2872 });
            record(state, 0, Outcome::Inconclusive, &clocks, &volts, &low_stock(&volts), None, hint).unwrap();
            let top = &state.bands[0];
            (top.target_clock_mhz, top.clock_ceiling_mhz, top.voltage_mv)
        };
        let mut state = fresh();
        assert_eq!(dropped(&mut state, Some(2910)), (2910, 2910, 1085));
        assert_eq!(state.bands[0].last_qualified_clock_mhz, None, "a new hypothesis, never a pass");
        dropped(&mut state, Some(2830));
        assert_eq!(state.stop_reason.as_deref(), Some("qualified_top_unavailable"), "once per run");
        // RTX 50 curve points sit ~7.5 MHz apart; the search still moves one 15 MHz boost step.
        assert_eq!(dropped(&mut fresh(), None).0, 2977, "without a measurement: one boost step");
        assert_eq!(dropped(&mut fresh(), Some(2800)).0, 2977, "held below every bin: one boost step");
        // 0.8.6 at 1090 mV: the GPU held 2887, no more than that voltage's stock 2887 clock.
        let mut stock_level = fresh();
        admit(&mut stock_level, 0, 0, 100).unwrap();
        let hint = Some(ClockDropHint { held_clock_mhz: 2887, stock_clock_mhz: 2887 });
        record(&mut stock_level, 0, Outcome::Inconclusive, &clocks, &volts, &low_stock(&volts), None, hint).unwrap();
        assert_eq!(stock_level.bands[0].target_clock_mhz, 2977, "never a pair at or under stock: one boost step");
    }
    #[test]
    fn every_pair_stays_below_the_stock_curve_of_a_gpu_at_its_voltage_limit() {
        use Outcome::*;
        // Field report 2026-10-22, RTX 4070 SUPER at 220 W: its top reached the curve's own 1100 mV
        // limit at ~200 W, where stock runs 2715 MHz, and v12 published the compensated top at
        // 2685@1100, which the plan refused. Stock: 15 MHz per 10 mV under 2715@1100, 2760 at most.
        let clocks: Vec<u32> = (2400..=2760).step_by(15).collect();
        let volts: Vec<u32> = (850..=1150).step_by(5).collect();
        let stock_clock = |mv: u32| if mv <= 1100 {
            2715 - 15 * (1100 - mv).div_ceil(10)
        } else {
            (2715 + 15 * ((mv - 1100) / 10)).min(2760)
        };
        let stock: Vec<_> = volts.iter().enumerate().map(|(index, &mv)| (index, mv, stock_clock(mv))).collect();
        let seed = |id| Seed { id, target_clock_mhz: 2760, voltage_mv: 1100, clock_ceiling_mhz: 2760 };
        let mut state = new(&[seed("performance"), seed("balanced"), seed("efficiency")], 30, 10_000);
        let go = |state: &mut ForgeDiscoverySearch, outcomes: &[Outcome]| {
            outcomes.iter().map(|outcome| walk_on(state, &clocks, &volts, &stock, *outcome)).collect::<Vec<_>>()
        };
        assert_eq!(go(&mut state, &[Inconclusive, Qualified, Qualified, Inconclusive]),
            [(0, 2760, 1100), (0, 2745, 1100), (0, 2745, 1095), (0, 2745, 1090)]);
        // 1095 + 43 mV → 1140 sits over the power-free 1100, so Godforge is measured one boost step
        // down. 1105 does not hold 2730 (the GPU's own 1100 mV limit); its margin pair is 1055.
        let mut retried = state.clone();
        assert_eq!(go(&mut state, &[Inconclusive, IntegrityError, IntegrityError, IntegrityError, Qualified]),
            [(1, 2730, 1105), (1, 2730, 1055), (1, 2715, 1050), (1, 2700, 1040), (1, 2685, 1030)]);
        // Each step down re-checks the stock curve, which runs 2715 at 1100, 2700 at 1090-1095 and
        // 2685 at 1080-1085: the highest undervolting voltages are 1095, 1085 and 1075.
        assert_eq!(go(&mut state, &[IntegrityError, IntegrityError, Qualified]),
            [(1, 2685, 1020), (1, 2685, 1025), (1, 2685, 1075)]);
        assert_eq!(state.bands[1].stop_reason.as_deref(), Some("published"));
        go(&mut retried, &[Inconclusive, IntegrityError]);
        assert_eq!((retried.bands[1].target_clock_mhz, retried.bands[1].voltage_mv), (2715, 1050));
        // -5% (2565) at 1030 would sit over its 2610 MHz stock point: it starts at 995 (2550).
        let balanced = &state.bands[2];
        assert_eq!((balanced.target_clock_mhz, balanced.voltage_mv, balanced.level_start_voltage_mv),
            (2565, 995, Some(1030)));
        // That moved start is a new test, not a dominated pair: failing it closes only this level.
        let mut failed = state.clone();
        go(&mut failed, &[IntegrityError]);
        assert_eq!((failed.stop_reason.as_deref(), failed.bands[2].stop_reason.as_deref()),
            (None, Some("integrity_error_region_closed")));
        let efficiency = &failed.bands[3];
        assert_eq!((efficiency.target_clock_mhz, efficiency.voltage_mv, efficiency.level_start_voltage_mv),
            (2430, 905, Some(1030)), "the next level starts from the voltage that passed above");
        // Its edge (985 failed, then the skipped 990) leaves 995 + 43 mV = 1040 over the stock
        // curve again: no profile.
        go(&mut state, &[Qualified, IntegrityError, IntegrityError]);
        assert_eq!(state.bands[2].stop_reason.as_deref(), Some("publication_at_stock"));
        assert_eq!((state.bands[3].target_clock_mhz, state.bands[3].voltage_mv), (2430, 905));
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
        // This tiny grid has no bin 43 mV above 906, so no compensated top; and no bin lies in
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
        // Godforge measures one boost step down; this tiny grid has no margin pair under 906.
        assert_eq!((next_band(&state), state.bands[1].target_clock_mhz, state.bands[1].voltage_mv), (Some(1), 1605, 906));
        step(&mut state, Outcome::PowerFree);
        assert_eq!(state.bands[1].stop_reason.as_deref(), Some("compensated_top_unavailable"));
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
