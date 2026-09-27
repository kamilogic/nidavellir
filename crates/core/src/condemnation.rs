//! Durable condemnation ledger — the append-only memory of REAL failures that must survive every
//! operational reset.
//!
//! `safe_loop.json` is operational state and older reset implementations replaced it wholesale. The
//! 2026-07-15 reset proved the gap: the
//! 1890@900 Endurance failure learned on 2026-07-10 was wiped with it, and the pair was re-attempted
//! and PASSED a single ladder the next day — a coin-flip point one interruption away from being
//! published. This ledger is the fix: hard failures are appended here and NEVER removed by any
//! reset path (current positive-learning reset deliberately preserves this file and Safe Loop
//! negatives). Only an explicit manual
//! rehabilitation entry (itself an append) can lift one.
//!
//! Severity model (agreed 2026-07-16):
//! - **Rigid** — a real-world hard failure: field TDR, machine crash with a candidate armed,
//!   device-lost, driver reset. The exact pair and everything at-or-below it at the same clock is
//!   refused (`anchor <= floor`). Auto-reuse is never allowed; manual rehabilitation only.
//! - **Quarantine** — a synthetic gate failure at the exact Apply pair (Texture/Endurance
//!   SilentError at exact-Apply). Everything STRICTLY below it at the same clock is refused
//!   (`anchor < floor`); the pair itself may be re-attempted, but publishing it requires the
//!   stricter re-proof protocol: two independent full-gate passes, or one pass under a STRONGER
//!   qualification contract than the one that condemned it.
//! - Descent-time 60 s boundary failures are NOT ledgered — they remain operational blacklist
//!   knowledge in `safe_loop.json` and may be invalidated by contract/fingerprint changes.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::safe_loop::default_data_dir;

/// JSONL file name under the data dir. NEVER add this file to any reset/cleanup path.
pub const CONDEMNATION_LEDGER_FILE: &str = "condemnation_ledger.jsonl";

pub const KIND_FIELD_TDR: &str = "field-tdr";
pub const KIND_CANDIDATE_CRASH: &str = "candidate-crash";
pub const KIND_DEVICE_LOST: &str = "device-lost";
pub const KIND_APPLY_GATE_SILENT: &str = "apply-gate-silent-error";
pub const KIND_REHABILITATED: &str = "rehabilitated";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CondemnationSeverity {
    Rigid,
    Quarantine,
}

/// One append-only ledger line. A `rehabilitated: true` entry cancels every prior entry at the
/// SAME (gpu_key, target_mhz, vf_bin_mv) — it is a manual operator action, never automatic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CondemnationEvent {
    pub timestamp: String,
    /// Exact physical GPU. `None` is treated conservatively: it matches every GPU.
    pub gpu_key: Option<String>,
    pub severity: CondemnationSeverity,
    /// What condemned the pair (`KIND_*`), e.g. "field-tdr", "apply-gate-silent-error".
    pub kind: String,
    pub target_mhz: u32,
    pub vf_bin_mv: u32,
    #[serde(default)]
    pub run_id: Option<String>,
    /// Qualification contract in force when the failure happened. A later approval under a
    /// STRICTLY stronger contract counts as full re-proof for a Quarantine entry.
    #[serde(default)]
    pub qualification_contract_version: Option<u32>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub rehabilitated: bool,
}

/// Filesystem-backed append-only ledger. Same JSONL conventions as `F2ObservationStore`, but an
/// append is flushed to stable storage before success is reported because this ledger is recovery
/// authority after a reboot. The best-effort reader skips malformed lines; safety paths use the
/// checked reader below.
#[derive(Debug, Clone)]
pub struct CondemnationLedger {
    base: PathBuf,
}

impl CondemnationLedger {
    /// The machine-wide ledger under `default_data_dir()` (`%ProgramData%/Nidavellir`).
    pub fn system() -> Self {
        Self {
            base: default_data_dir(),
        }
    }

    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    pub fn path(&self) -> PathBuf {
        self.base.join(CONDEMNATION_LEDGER_FILE)
    }

    pub fn append(&self, event: &CondemnationEvent) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.base)?;
        let line = serde_json::to_string(event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path())?;
        writeln!(f, "{line}")?;
        f.sync_data()
    }

    pub fn load_all(&self) -> Vec<CondemnationEvent> {
        match std::fs::read_to_string(self.path()) {
            Ok(data) => data
                .lines()
                .filter_map(|l| serde_json::from_str(l.trim_start_matches('\u{feff}')).ok())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Strict safety-path loader. A missing ledger is an empty ledger; every other read error or
    /// malformed non-empty JSONL line is returned so callers can fail closed instead of treating
    /// unavailable condemnation evidence as safe.
    pub fn load_all_checked(&self) -> std::io::Result<Vec<CondemnationEvent>> {
        let path = self.path();
        let data = match std::fs::read_to_string(&path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(std::io::Error::new(
                    error.kind(),
                    format!("read condemnation ledger {}: {error}", path.display()),
                ))
            }
        };
        data.lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(index, line)| {
                serde_json::from_str(line.trim_start_matches('\u{feff}')).map_err(|error| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "invalid condemnation ledger {} line {}: {error}",
                            path.display(),
                            index + 1
                        ),
                    )
                })
            })
            .collect()
    }

    /// The effective condemned pairs for one GPU — the only thing refusal logic needs.
    pub fn condemned_pairs(&self, gpu_key: &str) -> CondemnedPairs {
        condemned_pairs(&self.load_all(), gpu_key)
    }
}

/// The effective (non-rehabilitated) condemned pairs, split by severity, for refusal math.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CondemnedPairs {
    /// Refuse `anchor <= floor` over these (clock, mv) pairs.
    pub rigid: Vec<(u32, u32)>,
    /// Refuse `anchor < floor` over these — the exact pair stays re-attemptable under re-proof.
    pub quarantine: Vec<(u32, u32)>,
    /// Highest condemning contract version per quarantined exact pair (for the re-proof rule).
    quarantine_contracts: Vec<(u32, u32, Option<u32>)>,
}

/// The EXPERIMENTAL clean-run (organic) view: only condemnations produced by `run_id` itself.
/// Writes always go to the global ledger — a clean run reads its own failures back (they block
/// and steer repairs within the run) but never historical ones, so algorithm versions can be
/// compared on organically clean searches. Production reads use [`condemned_pairs`].
pub fn condemned_pairs_for_run(
    events: &[CondemnationEvent],
    gpu_key: &str,
    run_id: &str,
) -> CondemnedPairs {
    let scoped: Vec<CondemnationEvent> = events
        .iter()
        .filter(|e| e.run_id.as_deref() == Some(run_id))
        .cloned()
        .collect();
    condemned_pairs(&scoped, gpu_key)
}

/// Events that apply to `gpu_key` (entries without a key match conservatively), with later
/// `rehabilitated` entries cancelling every prior entry at the same exact pair.
pub fn condemned_pairs(events: &[CondemnationEvent], gpu_key: &str) -> CondemnedPairs {
    let mut out = CondemnedPairs::default();
    for (i, e) in events.iter().enumerate() {
        if e.gpu_key.as_deref().is_some_and(|k| k != gpu_key) || e.rehabilitated {
            continue;
        }
        let cancelled = events[i + 1..].iter().any(|r| {
            r.rehabilitated
                && r.target_mhz == e.target_mhz
                && r.vf_bin_mv == e.vf_bin_mv
                && r.gpu_key.as_deref().is_none_or(|k| k == gpu_key)
        });
        if cancelled {
            continue;
        }
        let pair = (e.target_mhz, e.vf_bin_mv);
        match e.severity {
            CondemnationSeverity::Rigid => out.rigid.push(pair),
            CondemnationSeverity::Quarantine => {
                out.quarantine.push(pair);
                out.quarantine_contracts.push((
                    e.target_mhz,
                    e.vf_bin_mv,
                    e.qualification_contract_version,
                ));
            }
        }
    }
    out
}

/// Effective ledger events for read-only audit/UI surfaces. Rehabilitation entries are never
/// exposed as active condemnations, and a later rehabilitation cancels every earlier event at the
/// same exact GPU/pair. Ordering remains append order so callers can retain the newest bounded tail.
pub fn effective_condemnation_events(events: &[CondemnationEvent]) -> Vec<CondemnationEvent> {
    events
        .iter()
        .enumerate()
        .filter(|(_, event)| !event.rehabilitated)
        .filter(|(index, event)| {
            !events[index + 1..].iter().any(|later| {
                later.rehabilitated
                    && later.target_mhz == event.target_mhz
                    && later.vf_bin_mv == event.vf_bin_mv
                    && later.gpu_key == event.gpu_key
            })
        })
        .map(|(_, event)| event.clone())
        .collect()
}

/// Conservative downward safety cone derived from current-contract Rigid failures.
///
/// `targets_descending` and `voltage_bins_ascending` describe physical bins. Starting at each
/// effective condemned pair, every lower clock may relieve at most one physical voltage bin per
/// physical clock bin. Overlapping cones retain the highest (safest) floor. Events outside either
/// supplied bin set cannot define a physical cone and are ignored.
pub fn rigid_tdr_safety_cone_floors(
    events: &[CondemnationEvent],
    gpu_key: &str,
    min_contract: u32,
    targets_descending: &[u32],
    voltage_bins_ascending: &[u32],
) -> Vec<(u32, u32)> {
    let mut floors = std::collections::BTreeMap::<u32, u32>::new();

    for (event_index, event) in events.iter().enumerate() {
        if event.rehabilitated
            || event.severity != CondemnationSeverity::Rigid
            || event.gpu_key.as_deref().is_some_and(|key| key != gpu_key)
            || event
                .qualification_contract_version
                .is_none_or(|contract| contract < min_contract)
        {
            continue;
        }

        let rehabilitated = events[event_index + 1..].iter().any(|later| {
            later.rehabilitated
                && later.target_mhz == event.target_mhz
                && later.vf_bin_mv == event.vf_bin_mv
                && later
                    .gpu_key
                    .as_deref()
                    .is_none_or(|key| key == gpu_key)
        });
        if rehabilitated {
            continue;
        }

        let Some(clock_index) = targets_descending
            .iter()
            .position(|&clock| clock == event.target_mhz)
        else {
            continue;
        };
        let Some(voltage_index) = voltage_bins_ascending
            .iter()
            .position(|&mv| mv == event.vf_bin_mv)
        else {
            continue;
        };

        for (clock_relief, &target_mhz) in targets_descending[clock_index..].iter().enumerate() {
            let floor_mv = voltage_bins_ascending[voltage_index.saturating_sub(clock_relief)];
            floors
                .entry(target_mhz)
                .and_modify(|current| *current = (*current).max(floor_mv))
                .or_insert(floor_mv);
        }
    }

    floors.into_iter().rev().collect()
}

/// Monotone V/F floor envelope over condemned (clock, mv) pairs — the same math as the field
/// floor: running-max condemned voltage by clock, ceil-interpolated between condemned clocks
/// (conservative chord above the convex true boundary), held flat beyond the highest clock.
/// `None` below the lowest condemned clock or with no pairs.
pub fn vf_floor_envelope(pairs: &[(u32, u32)], clock_mhz: u32) -> Option<u32> {
    let mut pts: Vec<(i64, i64)> = pairs
        .iter()
        .map(|&(c, v)| (i64::from(c), i64::from(v)))
        .collect();
    pts.sort_unstable();
    let mut env: Vec<(i64, i64)> = Vec::new();
    let mut worst = i64::MIN;
    for (clock, mv) in pts {
        worst = worst.max(mv);
        match env.last_mut() {
            Some(last) if last.0 == clock => last.1 = worst,
            _ => env.push((clock, worst)),
        }
    }
    let clock = i64::from(clock_mhz);
    if clock < env.first()?.0 {
        return None;
    }
    let mut floor = env[0].1;
    for pair in env.windows(2) {
        let ((c0, v0), (c1, v1)) = (pair[0], pair[1]);
        if clock >= c1 {
            floor = v1;
        } else if clock > c0 {
            // Ceil interpolation — round toward the safe side.
            floor = v0 + ((v1 - v0) * (clock - c0) + (c1 - c0) - 1) / (c1 - c0);
            break;
        } else {
            break;
        }
    }
    u32::try_from(floor).ok()
}

impl CondemnedPairs {
    /// True when the ledger refuses this candidate outright: at-or-below the Rigid floor, or
    /// STRICTLY below the Quarantine floor (the quarantined pair itself stays attemptable).
    pub fn refuses(&self, clock_mhz: u32, anchor_mv: u32) -> bool {
        vf_floor_envelope(&self.rigid, clock_mhz).is_some_and(|floor| anchor_mv <= floor)
            || vf_floor_envelope(&self.quarantine, clock_mhz).is_some_and(|floor| anchor_mv < floor)
    }

    /// Full-gate passes required to PUBLISH this exact pair: 2 when an effective quarantine
    /// condemned it under the current-or-stronger contract (one stochastic pass proved
    /// insufficient — 1890@900 failed 2026-07-10 and passed a single ladder 2026-07-16); 1 when
    /// clean or when the current contract is strictly stronger than the condemning one.
    pub fn required_apply_passes(
        &self,
        clock_mhz: u32,
        anchor_mv: u32,
        current_contract: u32,
    ) -> u32 {
        let quarantined = self.quarantine_contracts.iter().any(|&(c, v, contract)| {
            c == clock_mhz
                && v == anchor_mv
                && contract.is_none_or(|condemned_under| condemned_under >= current_contract)
        });
        if quarantined {
            2
        } else {
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        severity: CondemnationSeverity,
        clock: u32,
        mv: u32,
        contract: Option<u32>,
    ) -> CondemnationEvent {
        CondemnationEvent {
            timestamp: "2026-07-16T00:00:00Z".into(),
            gpu_key: Some("gpu-a".into()),
            severity,
            kind: KIND_APPLY_GATE_SILENT.into(),
            target_mhz: clock,
            vf_bin_mv: mv,
            run_id: None,
            qualification_contract_version: contract,
            note: None,
            rehabilitated: false,
        }
    }

    #[test]
    fn ledger_append_load_round_trip_and_skips_malformed() {
        let dir = std::env::temp_dir().join(format!("nida-ledger-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ledger = CondemnationLedger::new(&dir);
        ledger
            .append(&event(CondemnationSeverity::Rigid, 1845, 856, Some(17)))
            .unwrap();
        ledger
            .append(&event(
                CondemnationSeverity::Quarantine,
                1890,
                900,
                Some(16),
            ))
            .unwrap();
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(ledger.path())
            .unwrap();
        writeln!(f, "{{ truncated garbage").unwrap();
        assert_eq!(ledger.load_all().len(), 2);
        let pairs = ledger.condemned_pairs("gpu-a");
        assert_eq!(pairs.rigid, vec![(1845, 856)]);
        assert_eq!(pairs.quarantine, vec![(1890, 900)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checked_ledger_load_treats_missing_file_as_empty() {
        let dir = std::env::temp_dir().join(format!(
            "nida-ledger-checked-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let ledger = CondemnationLedger::new(&dir);

        assert_eq!(ledger.load_all_checked().unwrap(), Vec::new());
    }

    #[test]
    fn checked_ledger_load_rejects_invalid_non_empty_line_with_context() {
        let dir = std::env::temp_dir().join(format!(
            "nida-ledger-checked-invalid-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let ledger = CondemnationLedger::new(&dir);
        ledger
            .append(&event(CondemnationSeverity::Rigid, 1845, 856, Some(29)))
            .unwrap();
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(ledger.path())
            .unwrap();
        writeln!(file, "{{ truncated garbage").unwrap();

        let error = ledger.load_all_checked().unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("line 2"), "{error}");
        assert!(
            error.to_string().contains(CONDEMNATION_LEDGER_FILE),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checked_ledger_load_propagates_non_not_found_io_error() {
        let dir = std::env::temp_dir().join(format!(
            "nida-ledger-checked-io-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let ledger = CondemnationLedger::new(&dir);
        std::fs::create_dir_all(ledger.path()).unwrap();

        let error = ledger.load_all_checked().unwrap_err();
        assert_ne!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(
            error.to_string().contains(CONDEMNATION_LEDGER_FILE),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rigid_refuses_at_or_below_quarantine_only_strictly_below() {
        let events = [
            event(CondemnationSeverity::Rigid, 1845, 856, Some(17)),
            event(CondemnationSeverity::Quarantine, 1890, 900, Some(17)),
        ];
        let pairs = condemned_pairs(&events, "gpu-a");
        // Rigid: the exact pair and below are refused; one bin above is allowed.
        assert!(pairs.refuses(1845, 856));
        assert!(pairs.refuses(1845, 850));
        assert!(!pairs.refuses(1845, 862));
        // Quarantine: strictly below refused; the exact pair stays attemptable.
        assert!(pairs.refuses(1890, 893));
        assert!(!pairs.refuses(1890, 900));
        // Monotone hold above the highest condemned clock (higher clock needs >= voltage).
        assert!(pairs.refuses(1920, 850)); // rigid floor held flat at 856
                                           // Below the lowest condemned clock there is no evidence.
        assert!(!pairs.refuses(1740, 793));
    }

    #[test]
    fn envelope_interpolates_ceil_between_condemned_clocks() {
        let pairs = [(1815u32, 862u32), (1875, 887)];
        // Midpoint 1845: ceil(862 + 25 * 30/60) = 875 (exact); a non-exact division rounds UP.
        assert_eq!(vf_floor_envelope(&pairs, 1845), Some(875));
        assert_eq!(vf_floor_envelope(&pairs, 1830), Some(869)); // 868.25 → ceil 869
        assert_eq!(vf_floor_envelope(&pairs, 1800), None);
        assert_eq!(vf_floor_envelope(&pairs, 1920), Some(887));
    }

    #[test]
    fn rehabilitation_cancels_only_prior_entries_at_the_exact_pair() {
        let mut rehab = event(CondemnationSeverity::Rigid, 1845, 856, None);
        rehab.rehabilitated = true;
        rehab.kind = KIND_REHABILITATED.into();
        let events = [
            event(CondemnationSeverity::Rigid, 1845, 856, Some(17)),
            event(CondemnationSeverity::Rigid, 1920, 918, Some(17)),
            rehab,
        ];
        let pairs = condemned_pairs(&events, "gpu-a");
        assert_eq!(pairs.rigid, vec![(1920, 918)]);
    }

    #[test]
    fn effective_events_hide_rehabilitated_pairs_and_rehabilitation_records() {
        let old = event(CondemnationSeverity::Rigid, 1845, 856, Some(17));
        let kept = event(CondemnationSeverity::Quarantine, 1890, 900, Some(17));
        let mut rehab = old.clone();
        rehab.rehabilitated = true;
        rehab.kind = KIND_REHABILITATED.into();
        let effective = effective_condemnation_events(&[old, kept.clone(), rehab]);
        assert_eq!(effective, vec![kept]);
    }

    #[test]
    fn rigid_tdr_cone_relaxes_one_physical_voltage_bin_per_lower_clock_bin() {
        let targets = [1935, 1920, 1905, 1890, 1875, 1860, 1845, 1830, 1815, 1800];
        let voltage_bins = [
            800, 806, 812, 818, 825, 831, 837, 843, 850, 856, 862, 868, 875, 881, 887, 893, 900,
            906, 912, 918, 925, 931, 937,
        ];
        let events = [event(CondemnationSeverity::Rigid, 1920, 931, Some(29))];

        assert_eq!(
            rigid_tdr_safety_cone_floors(&events, "gpu-a", 29, &targets, &voltage_bins),
            vec![
                (1920, 931),
                (1905, 925),
                (1890, 918),
                (1875, 912),
                (1860, 906),
                (1845, 900),
                (1830, 893),
                (1815, 887),
                (1800, 881),
            ]
        );
    }

    #[test]
    fn rigid_tdr_cone_combines_overlaps_by_the_highest_floor() {
        let targets = [1920, 1905, 1890, 1875, 1860];
        let voltage_bins = [881, 887, 893, 900, 906, 912, 918, 925, 931, 937];
        let first = event(CondemnationSeverity::Rigid, 1920, 931, Some(29));
        let second = event(CondemnationSeverity::Rigid, 1890, 937, Some(30));

        assert_eq!(
            rigid_tdr_safety_cone_floors(
                &[first, second.clone(), second],
                "gpu-a",
                29,
                &targets,
                &voltage_bins,
            ),
            vec![
                (1920, 931),
                (1905, 925),
                (1890, 937),
                (1875, 931),
                (1860, 925),
            ]
        );
    }

    #[test]
    fn rigid_tdr_cone_ignores_old_other_gpu_rehabilitated_and_non_rigid_events() {
        let targets = [1920, 1905, 1890, 1875];
        let voltage_bins = [900, 906, 912, 918, 925, 931];
        let old_contract = event(CondemnationSeverity::Rigid, 1920, 931, Some(28));
        let mut other_gpu = event(CondemnationSeverity::Rigid, 1905, 925, Some(29));
        other_gpu.gpu_key = Some("gpu-b".into());
        let active_then_rehabilitated = event(CondemnationSeverity::Rigid, 1890, 918, Some(29));
        let mut rehabilitation = active_then_rehabilitated.clone();
        rehabilitation.rehabilitated = true;
        rehabilitation.kind = KIND_REHABILITATED.into();
        let legacy = event(CondemnationSeverity::Rigid, 1875, 912, None);
        let quarantine = event(CondemnationSeverity::Quarantine, 1920, 931, Some(29));

        assert!(rigid_tdr_safety_cone_floors(
            &[
                old_contract,
                other_gpu,
                active_then_rehabilitated,
                rehabilitation,
                legacy,
                quarantine,
            ],
            "gpu-a",
            29,
            &targets,
            &voltage_bins,
        )
        .is_empty());
    }

    #[test]
    fn rigid_tdr_cone_uses_positions_for_irregular_physical_bins() {
        let targets = [1980, 1920, 1875, 1800];
        let voltage_bins = [800, 807, 819, 844, 870, 905, 931];
        let valid = event(CondemnationSeverity::Rigid, 1920, 931, Some(29));
        let absent_clock = event(CondemnationSeverity::Rigid, 1905, 931, Some(29));
        let absent_voltage = event(CondemnationSeverity::Rigid, 1920, 925, Some(29));

        assert_eq!(
            rigid_tdr_safety_cone_floors(
                &[valid, absent_clock, absent_voltage],
                "gpu-a",
                29,
                &targets,
                &voltage_bins,
            ),
            vec![(1920, 931), (1875, 905), (1800, 870)]
        );
    }

    #[test]
    fn other_gpu_entries_are_ignored_but_keyless_entries_apply_conservatively() {
        let mut other = event(CondemnationSeverity::Rigid, 1845, 856, None);
        other.gpu_key = Some("gpu-b".into());
        let mut keyless = event(CondemnationSeverity::Rigid, 1815, 837, None);
        keyless.gpu_key = None;
        let pairs = condemned_pairs(&[other, keyless], "gpu-a");
        assert_eq!(pairs.rigid, vec![(1815, 837)]);
    }

    #[test]
    fn run_scoped_view_sees_only_its_own_run() {
        let mut historical = event(CondemnationSeverity::Rigid, 1920, 918, Some(17));
        historical.run_id = Some("f2-forge-old".into());
        let mut seeded = event(CondemnationSeverity::Quarantine, 1890, 900, Some(17));
        seeded.run_id = None; // historical seed without a run id
        let mut own = event(CondemnationSeverity::Quarantine, 1875, 887, Some(17));
        own.run_id = Some("f2-forge-new".into());
        let events = [historical, seeded, own];
        let scoped = condemned_pairs_for_run(&events, "gpu-a", "f2-forge-new");
        assert!(scoped.rigid.is_empty());
        assert_eq!(scoped.quarantine, vec![(1875, 887)]);
        // The production view still sees everything.
        let global = condemned_pairs(&events, "gpu-a");
        assert_eq!(global.rigid, vec![(1920, 918)]);
        assert_eq!(global.quarantine.len(), 2);
    }

    #[test]
    fn wire_format_is_pinned() {
        // The ledger is append-only and read across releases — this line IS the wire format.
        // If this test breaks, old ledgers stop parsing: never change the format, extend it
        // with #[serde(default)] fields only.
        let line = r#"{"timestamp":"2026-07-16T02:09:03Z","gpu_key":"nvml:GPU-x","severity":"rigid","kind":"candidate-crash","target_mhz":1845,"vf_bin_mv":856,"run_id":"f2-forge-1","qualification_contract_version":17,"note":"n","rehabilitated":false}"#;
        let e: CondemnationEvent = serde_json::from_str(line).unwrap();
        assert_eq!(e.severity, CondemnationSeverity::Rigid);
        assert_eq!((e.target_mhz, e.vf_bin_mv), (1845, 856));
        assert_eq!(e.qualification_contract_version, Some(17));
    }

    #[test]
    fn quarantine_requires_double_proof_unless_contract_is_stronger() {
        let events = [event(CondemnationSeverity::Quarantine, 1890, 900, Some(17))];
        let pairs = condemned_pairs(&events, "gpu-a");
        assert_eq!(pairs.required_apply_passes(1890, 900, 17), 2);
        assert_eq!(pairs.required_apply_passes(1890, 900, 18), 1); // stronger contract re-proves
        assert_eq!(pairs.required_apply_passes(1890, 906, 17), 1); // different pair
                                                                   // A legacy entry without a contract version is conservative: always double proof.
        let legacy = [event(CondemnationSeverity::Quarantine, 1890, 900, None)];
        assert_eq!(
            condemned_pairs(&legacy, "gpu-a").required_apply_passes(1890, 900, 18),
            2
        );
    }
}
