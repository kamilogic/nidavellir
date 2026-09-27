//! The Safe Loop — Nidavellir's "parachute before the jump" (roadmap §4).
//!
//! A reboot-surviving state machine that makes aggressive tuning recoverable:
//! before every apply it arms an on-disk boot-flag; a clean validation clears
//! it. On the next boot the service reads the flag first — if it is still armed,
//! the last apply must have crashed the machine, so we blacklist the region
//! around that point, recede to the last known-good profile, and after three
//! consecutive crashes fall back to Safe Mode (stock profile, hands off).
//!
//! This module is deliberately split into pure logic (state machine, bugcheck
//! classification, blacklist/recovery decisions — all unit-tested) and a thin
//! filesystem-backed [`SafeLoopStore`] for persistence that survives reboots.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// Three consecutive crashes trips Safe Mode.
pub const SAFE_MODE_CRASH_THRESHOLD: u32 = 3;

/// Default blacklist radius (in per-axis steps) carved out around a crash point.
pub const DEFAULT_BLACKLIST_RADIUS: i64 = 1;

/// `0x101 CLOCK_WATCHDOG_TIMEOUT` — a core stopped responding; classic OC crash.
pub const BUGCHECK_CLOCK_WATCHDOG: u64 = 0x101;
/// `0x124 WHEA_UNCORRECTABLE_ERROR` — machine-check; classic undervolt/OC crash.
pub const BUGCHECK_WHEA_UNCORRECTABLE: u64 = 0x124;
/// `0x133 DPC_WATCHDOG_VIOLATION` — also commonly OC/driver instability.
pub const BUGCHECK_DPC_WATCHDOG: u64 = 0x133;
/// `0x116 VIDEO_TDR_FAILURE` — the GPU watchdog could not recover the display driver.
pub const BUGCHECK_VIDEO_TDR_FAILURE: u64 = 0x116;
/// `0x117 VIDEO_TDR_TIMEOUT_DETECTED` — the GPU watchdog detected a timeout.
pub const BUGCHECK_VIDEO_TDR_TIMEOUT: u64 = 0x117;

/// Exact boot-flag phase used by the supervised F2 Forge motor. Keep this explicit rather than
/// matching arbitrary "probe" strings so normal apply/use crashes retain the Safe Mode threshold.
pub const SUPERVISED_F2_FORGE_PHASE: &str = "f2_undervolt_probe";

/// Exact boot-flag phase used by Detector Lab. Its temporary points are diagnostic experiments,
/// not Forge learning evidence, so an interrupted session must recover to stock without teaching
/// the blacklist or consuming the normal-use Safe Mode crash budget.
pub const DETECTOR_LAB_PHASE: &str = "detector_lab";

/// Operator-owned elastic-curve session observed by Game Trace and the live silent-error canary.
/// Like Detector Lab, this is an experiment rather than profile/Forge learning.
pub const GAME_TRACE_DIAGNOSTIC_PHASE: &str = "game_trace_curve_diagnostic";

/// A point in tuning space: axis name → integer setting (mV offset, MHz, ratio…).
///
/// An empty map (or all-zero axes) is the *stock* point — the recovery target
/// that is always safe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TuningPoint {
    pub axes: BTreeMap<String, i64>,
}

impl TuningPoint {
    /// The always-safe stock point (no offsets applied).
    pub fn stock() -> Self {
        Self::default()
    }

    pub fn from_axes<I, S>(axes: I) -> Self
    where
        I: IntoIterator<Item = (S, i64)>,
        S: Into<String>,
    {
        Self {
            axes: axes.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        }
    }

    /// True when every axis is at its neutral (0) setting.
    pub fn is_stock(&self) -> bool {
        self.axes.values().all(|&v| v == 0)
    }

    /// Chebyshev (L∞) distance, treating any axis missing from either side as 0.
    /// Used to decide whether a candidate falls inside a blacklisted region.
    pub fn chebyshev(&self, other: &Self) -> i64 {
        let mut max = 0;
        for key in self.axes.keys().chain(other.axes.keys()) {
            let a = self.axes.get(key).copied().unwrap_or(0);
            let b = other.axes.get(key).copied().unwrap_or(0);
            max = max.max((a - b).abs());
        }
        max
    }
}

/// A carved-out region of tuning space known to be unstable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlacklistRegion {
    pub center: TuningPoint,
    pub radius: i64,
}

impl BlacklistRegion {
    pub fn around(center: TuningPoint, radius: i64) -> Self {
        Self { center, radius }
    }

    /// A point is blacklisted when it lies within `radius` of the crash center.
    pub fn contains(&self, point: &TuningPoint) -> bool {
        self.center.chebyshev(point) <= self.radius
    }
}

/// The live phase of the loop (roadmap §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeLoopState {
    Idle,
    Probing,
    Applying,
    Dwell,
    Validated,
    Unstable,
    SafeMode,
}

/// Events that drive the state machine forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeLoopEvent {
    /// Begin exploring a new candidate.
    StartProbe,
    /// Boot-flag armed, point pushed to hardware.
    Applied,
    /// Begin the dwell window with the stressor running.
    EnterDwell,
    /// Dwell completed cleanly — point is good.
    DwellPassed,
    /// WHEA correctable delta during dwell — revert without crashing.
    SoftFail,
    /// A hard crash was detected (post-reboot, boot-flag was armed).
    HardCrash,
    /// Crash threshold reached — drop to stock and stop.
    TripSafeMode,
    /// Operator/recovery reset back to idle.
    Reset,
}

/// Pure state-transition function. Unknown transitions leave the state intact,
/// so a stray event can never push the loop into an unsafe phase.
pub fn transition(state: SafeLoopState, event: SafeLoopEvent) -> SafeLoopState {
    use SafeLoopEvent as E;
    use SafeLoopState as S;
    match (state, event) {
        (_, E::TripSafeMode) => S::SafeMode,
        (_, E::Reset) => S::Idle,
        (_, E::HardCrash) => S::Unstable,
        (S::Idle, E::StartProbe) => S::Probing,
        (S::Probing, E::Applied) => S::Applying,
        (S::Applying, E::EnterDwell) => S::Dwell,
        (S::Dwell, E::DwellPassed) => S::Validated,
        (S::Dwell, E::SoftFail) => S::Unstable,
        // After a result, the next probe restarts the cycle.
        (S::Validated, E::StartProbe) | (S::Unstable, E::StartProbe) => S::Probing,
        (other, _) => other,
    }
}

/// How a bugcheck code maps to a stability verdict (roadmap §4.3, layer 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashClass {
    /// Bugcheck strongly associated with overclock/undervolt instability.
    OcInstability,
    /// A real BSOD, but not a typical OC signature (driver, disk, etc.).
    Unrelated,
    /// No bugcheck code available (e.g. a freeze with no dump).
    Unknown,
}

/// Durable source of a Forge incident that requires operator-visible accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForgeIncidentKind {
    /// A persisted candidate boot flag survived the restart, so the exact point is attributable.
    CandidateCrash,
    /// A running Forge checkpoint survived without a candidate boot flag. Do not invent a point.
    UnaccountedRestart,
    /// The operator reported that a forged profile failed under real use on this GPU.
    OperatorFieldFailure,
    /// The live worker failed internally without completing its terminal checkpoint.
    RuntimeFailure,
}

/// Reboot-surviving Forge incident. It is stored with Safe Loop state so UI, recovery and export all
/// observe the same acknowledgement contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeIncident {
    pub id: String,
    pub kind: ForgeIncidentKind,
    pub detected_at: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub gpu_key: Option<String>,
    #[serde(default)]
    pub target_mhz: Option<u32>,
    #[serde(default)]
    pub anchor_mv: Option<u32>,
    pub message: String,
    #[serde(default)]
    pub acknowledged: bool,
}

impl ForgeIncident {
    pub fn new(
        kind: ForgeIncidentKind,
        run_id: Option<String>,
        gpu_key: Option<String>,
        target_mhz: Option<u32>,
        anchor_mv: Option<u32>,
        message: impl Into<String>,
    ) -> Self {
        let detected_at = chrono::Utc::now().to_rfc3339();
        let id = format!(
            "forge-incident-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        Self {
            id,
            kind,
            detected_at,
            run_id,
            gpu_key,
            target_mhz,
            anchor_mv,
            message: message.into(),
            acknowledged: false,
        }
    }
}

/// Classify a Windows bugcheck (stop) code.
pub fn classify_bugcheck(code: u64) -> CrashClass {
    match code {
        0 => CrashClass::Unknown,
        BUGCHECK_CLOCK_WATCHDOG
        | BUGCHECK_WHEA_UNCORRECTABLE
        | BUGCHECK_DPC_WATCHDOG
        | BUGCHECK_VIDEO_TDR_FAILURE
        | BUGCHECK_VIDEO_TDR_TIMEOUT => CrashClass::OcInstability,
        _ => CrashClass::Unrelated,
    }
}

/// Extract the bugcheck code from a Windows "BugCheck" event-log message.
///
/// The WER message reads e.g. `The computer has rebooted from a bugcheck.
/// The bugcheck was: 0x00000124 (0x..., ...)`. We take the first `0x` hex token,
/// which is always the stop code.
pub fn parse_bugcheck_code(text: &str) -> Option<u64> {
    let lower = text.to_ascii_lowercase();
    let idx = lower.find("0x")?;
    let rest = &lower[idx + 2..];
    let hex: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    if hex.is_empty() {
        return None;
    }
    u64::from_str_radix(&hex, 16).ok()
}

/// Layer-1 detection (roadmap §4.3): a rise in WHEA correctable errors during
/// the dwell window is a *soft fail* — instability before the hard crash.
pub fn whea_soft_fail(count_before: u32, count_after: u32) -> bool {
    count_after > count_before
}

/// Everything the loop must remember across reboots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeLoopRecord {
    pub state: SafeLoopState,
    pub consecutive_crashes: u32,
    pub last_validated: Option<TuningPoint>,
    pub blacklist: Vec<BlacklistRegion>,
    pub safe_mode: bool,
    /// Most recent crash classifications, newest last (capped).
    pub crash_log: Vec<CrashClass>,
    /// Incident currently blocking automatic Forge continuation / profile Apply.
    #[serde(default)]
    pub pending_forge_incident: Option<ForgeIncident>,
    /// Durable incident history, newest last (capped by the mutation helpers).
    #[serde(default)]
    pub forge_incidents: Vec<ForgeIncident>,
}

impl Default for SafeLoopRecord {
    fn default() -> Self {
        Self {
            state: SafeLoopState::Idle,
            consecutive_crashes: 0,
            last_validated: None,
            blacklist: Vec::new(),
            safe_mode: false,
            crash_log: Vec::new(),
            pending_forge_incident: None,
            forge_incidents: Vec::new(),
        }
    }
}

impl SafeLoopRecord {
    /// Is this candidate inside any known-unstable region?
    pub fn is_blacklisted(&self, point: &TuningPoint) -> bool {
        self.blacklist.iter().any(|r| r.contains(point))
    }

    /// Record a freshly validated point and reset the crash streak.
    pub fn mark_validated(&mut self, point: TuningPoint) {
        self.last_validated = Some(point);
        self.consecutive_crashes = 0;
        self.state = SafeLoopState::Validated;
    }

    /// The point recovery should fall back to: last good, else stock.
    pub fn recovery_target(&self) -> TuningPoint {
        self.last_validated
            .clone()
            .unwrap_or_else(TuningPoint::stock)
    }

    /// Clear the recovery *latch* after an operator reset: leave Safe Mode and zero the crash
    /// streak so tuning is allowed again, returning to [`SafeLoopState::Idle`]. Learning is
    /// PRESERVED — the unstable-region `blacklist`, `last_validated`, and `crash_log` history are
    /// kept. Full Reset also preserves this negative safety evidence; only positive qualification
    /// observations and the forge checkpoint are removed there.
    ///
    /// This is the missing piece that lets "Reset all" actually release a latched Safe Mode: the
    /// hardware/boot-flag reset never wrote this record, so `safe_mode` could only ever be set, not
    /// cleared.
    pub fn clear_recovery_latch(&mut self) {
        self.safe_mode = false;
        self.consecutive_crashes = 0;
        self.state = if self.pending_forge_incident.is_some() {
            SafeLoopState::Unstable
        } else {
            SafeLoopState::Idle
        };
    }

    /// Persist an incident once and latch Needs Attention until the operator acknowledges it.
    pub fn record_forge_incident(&mut self, incident: ForgeIncident) -> bool {
        let duplicate_id = self
            .forge_incidents
            .iter()
            .any(|known| known.id == incident.id);
        if duplicate_id {
            return false;
        }
        // Live worker accounting can latch a generic same-run interruption just before the
        // Sentinel/startup path recovers the exact armed candidate. Promote that one durable event
        // in place so the more specific CandidateCrash remains acknowledgeable/resumable without
        // duplicating the interruption in history.
        let promotion = self.pending_forge_incident.as_ref().and_then(|pending| {
            let same_run = pending.run_id.is_some() && pending.run_id == incident.run_id;
            let gpu_compatible = pending.gpu_key.is_none()
                || incident.gpu_key.is_none()
                || pending.gpu_key == incident.gpu_key;
            let incoming_exact_candidate = incident.kind == ForgeIncidentKind::CandidateCrash
                && incident.target_mhz.is_some()
                && incident.anchor_mv.is_some();
            let pending_is_less_specific = matches!(
                pending.kind,
                ForgeIncidentKind::RuntimeFailure | ForgeIncidentKind::UnaccountedRestart
            ) || (pending.kind == ForgeIncidentKind::CandidateCrash
                && (pending.target_mhz.is_none() || pending.anchor_mv.is_none()));
            (same_run && gpu_compatible && incoming_exact_candidate && pending_is_less_specific)
                .then(|| {
                    let mut promoted = incident.clone();
                    promoted.id = pending.id.clone();
                    promoted.detected_at = pending.detected_at.clone();
                    promoted.gpu_key = promoted.gpu_key.or_else(|| pending.gpu_key.clone());
                    promoted.acknowledged = false;
                    promoted
                })
        });
        if let Some(promoted) = promotion {
            if let Some(stored) = self
                .forge_incidents
                .iter_mut()
                .find(|stored| stored.id == promoted.id)
            {
                *stored = promoted.clone();
            } else {
                push_capped(&mut self.forge_incidents, promoted.clone(), 64);
            }
            self.pending_forge_incident = Some(promoted);
            if !self.safe_mode {
                self.state = SafeLoopState::Unstable;
            }
            return true;
        }
        // The active latch deduplicates the live Sentinel + startup reconciliation paths for one
        // interruption. Once the operator acknowledges it, a later TDR in the SAME resumed run is
        // a new safety event and must create a fresh pending incident.
        let duplicate_pending_run = self
            .pending_forge_incident
            .as_ref()
            .is_some_and(|pending| pending.run_id.is_some() && pending.run_id == incident.run_id);
        if duplicate_pending_run {
            return false;
        }
        self.pending_forge_incident = Some(incident.clone());
        push_capped(&mut self.forge_incidents, incident, 64);
        if !self.safe_mode {
            self.state = SafeLoopState::Unstable;
        }
        true
    }

    /// Release only the incident acknowledgement latch. Blacklist and incident history stay durable.
    pub fn acknowledge_forge_incident(&mut self) -> Option<ForgeIncident> {
        let mut incident = self.pending_forge_incident.take()?;
        incident.acknowledged = true;
        if let Some(stored) = self
            .forge_incidents
            .iter_mut()
            .find(|stored| stored.id == incident.id)
        {
            stored.acknowledged = true;
        }
        if !self.safe_mode {
            self.state = SafeLoopState::Idle;
        }
        Some(incident)
    }
}

/// The boot-flag: written to disk *before* an apply, deleted *after* a clean
/// validation. Its mere presence at boot means the last apply crashed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootFlag {
    pub intent: TuningPoint,
    pub phase: String,
    pub timestamp: String,
    /// Unique owner of this hardware transaction. Legacy flags deserialize with an empty owner and
    /// remain recoverable; every newly armed flag receives an identity so a delayed timer can only
    /// clear the transaction it created.
    #[serde(default)]
    pub transaction_id: String,
}

impl BootFlag {
    pub fn new(intent: TuningPoint, phase: impl Into<String>) -> Self {
        static NEXT_TRANSACTION: AtomicU64 = AtomicU64::new(1);
        let sequence = NEXT_TRANSACTION.fetch_add(1, Ordering::Relaxed);
        Self {
            intent,
            phase: phase.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            transaction_id: format!(
                "boot-{}-{}-{sequence}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ),
        }
    }

    fn same_transaction(&self, other: &Self) -> bool {
        if self.transaction_id.is_empty() || other.transaction_id.is_empty() {
            self == other
        } else {
            self.transaction_id == other.transaction_id
        }
    }
}

/// What the service should do on boot, decided purely from persisted state
/// (roadmap §4.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RecoveryAction {
    /// Boot-flag was clear and nothing is validated yet — nothing to restore.
    Idle,
    /// Boot-flag clear; reapply the last known-good profile.
    ApplyLastValidated { point: TuningPoint },
    /// A previous Forge execution ended without a terminal checkpoint. Stay at stock and wait for
    /// an explicit operator acknowledgement; never silently resume or reapply.
    AwaitOperatorAcknowledgement { incident: ForgeIncident },
    /// A Detector Lab session ended while its temporary point was armed. Return to stock and retain
    /// the diagnostic attribution, but do not convert an experiment interruption into learning.
    RecoverDiagnosticInterruption {
        interrupted: TuningPoint,
        class: CrashClass,
    },
    /// Boot-flag armed: blacklist the crash region and recede to known-good.
    BlacklistAndRecede {
        crashed: TuningPoint,
        recede_to: TuningPoint,
        class: CrashClass,
        /// Supervised Forge TDRs are expected boundary evidence and must not consume the normal-use
        /// Safe Mode crash budget. Unrelated crashes and non-Forge phases still count.
        count_toward_safe_mode: bool,
    },
    /// Crash threshold tripped — apply stock and stop touching anything.
    EnterSafeMode { stock: TuningPoint },
    /// A clean boot while already latched in Safe Mode: stay hands-off (stock), but do
    /// NOT count a new crash — no apply armed a boot-flag this cycle, so nothing actually
    /// crashed. Distinct from [`RecoveryAction::EnterSafeMode`], which is the crash that
    /// *trips* Safe Mode (and increments the streak).
    RemainSafeMode { stock: TuningPoint },
}

/// Pure recovery decision. `bugcheck` is the classification from the post-reboot
/// minidump/event analysis (layer 3); pass [`CrashClass::Unknown`] when none.
///
/// This does **not** mutate `record`; call [`apply_recovery`] to commit the
/// resulting state change once the decision is made.
pub fn decide_recovery(
    boot_flag: Option<&BootFlag>,
    bugcheck: CrashClass,
    record: &SafeLoopRecord,
) -> RecoveryAction {
    match boot_flag {
        Some(flag) => {
            // The apply that armed this flag never reached a clean validation.
            if matches!(
                flag.phase.as_str(),
                DETECTOR_LAB_PHASE | GAME_TRACE_DIAGNOSTIC_PHASE
            ) {
                return RecoveryAction::RecoverDiagnosticInterruption {
                    interrupted: flag.intent.clone(),
                    class: bugcheck,
                };
            }
            let supervised_forge_tdr =
                flag.phase == SUPERVISED_F2_FORGE_PHASE && bugcheck != CrashClass::Unrelated;
            let count_toward_safe_mode = !supervised_forge_tdr;
            let crashes = record
                .consecutive_crashes
                .saturating_add(u32::from(count_toward_safe_mode));
            if count_toward_safe_mode && crashes >= SAFE_MODE_CRASH_THRESHOLD {
                RecoveryAction::EnterSafeMode {
                    stock: TuningPoint::stock(),
                }
            } else {
                RecoveryAction::BlacklistAndRecede {
                    crashed: flag.intent.clone(),
                    recede_to: record.recovery_target(),
                    class: bugcheck,
                    count_toward_safe_mode,
                }
            }
        }
        None => {
            if record.safe_mode {
                // Already latched — re-enter Safe Mode without counting a crash. The boot-flag
                // is clear, so no apply was in flight; a clean reboot must not inflate the streak.
                RecoveryAction::RemainSafeMode {
                    stock: TuningPoint::stock(),
                }
            } else if let Some(incident) = record.pending_forge_incident.clone() {
                RecoveryAction::AwaitOperatorAcknowledgement { incident }
            } else if let Some(point) = record.last_validated.clone() {
                RecoveryAction::ApplyLastValidated { point }
            } else {
                RecoveryAction::Idle
            }
        }
    }
}

/// Commit the effects of a recovery decision to the persisted record. Returns
/// the point that should be applied to hardware.
pub fn apply_recovery(record: &mut SafeLoopRecord, action: &RecoveryAction) -> TuningPoint {
    match action {
        RecoveryAction::Idle => {
            record.state = SafeLoopState::Idle;
            TuningPoint::stock()
        }
        RecoveryAction::ApplyLastValidated { point } => {
            record.state = SafeLoopState::Validated;
            point.clone()
        }
        RecoveryAction::AwaitOperatorAcknowledgement { .. } => {
            record.state = SafeLoopState::Unstable;
            TuningPoint::stock()
        }
        RecoveryAction::RecoverDiagnosticInterruption { .. } => {
            record.state = if record.safe_mode {
                SafeLoopState::SafeMode
            } else {
                SafeLoopState::Idle
            };
            TuningPoint::stock()
        }
        RecoveryAction::BlacklistAndRecede {
            crashed,
            recede_to,
            class,
            count_toward_safe_mode,
        } => {
            if *count_toward_safe_mode {
                record.consecutive_crashes = record.consecutive_crashes.saturating_add(1);
            }
            if !record.is_blacklisted(crashed) {
                record.blacklist.push(BlacklistRegion::around(
                    crashed.clone(),
                    DEFAULT_BLACKLIST_RADIUS,
                ));
            }
            push_capped(&mut record.crash_log, *class, 32);
            record.state = SafeLoopState::Unstable;
            recede_to.clone()
        }
        RecoveryAction::EnterSafeMode { stock } => {
            record.consecutive_crashes = record.consecutive_crashes.saturating_add(1);
            record.safe_mode = true;
            record.state = SafeLoopState::SafeMode;
            stock.clone()
        }
        RecoveryAction::RemainSafeMode { stock } => {
            // Re-assert the latched Safe Mode on a clean boot. Deliberately does NOT touch
            // `consecutive_crashes` — nothing crashed this cycle.
            record.safe_mode = true;
            record.state = SafeLoopState::SafeMode;
            stock.clone()
        }
    }
}

fn push_capped<T>(v: &mut Vec<T>, item: T, cap: usize) {
    v.push(item);
    if v.len() > cap {
        let overflow = v.len() - cap;
        v.drain(0..overflow);
    }
}

// ---------------------------------------------------------------------------
// Persistence — reboot-surviving on-disk state under %ProgramData%\Nidavellir.
// ---------------------------------------------------------------------------

const BOOT_FLAG_FILE: &str = "boot_flag.json";
const BOOT_FLAG_CLEAR_CLAIM_PREFIX: &str = ".boot_flag.json.clear-";
const RECORD_FILE: &str = "safe_loop.json";
const HEARTBEAT_FILE: &str = "heartbeat.txt";
const CLEAN_SHUTDOWN_FILE: &str = "clean_shutdown.txt";

// Boot-flag compare-and-clear must be indivisible relative to every arm/clear in this process. The
// process lock closes local timer races; the short-lived on-disk claim closes the duplicate-process
// race. An interrupted claim is deliberately detected as still armed on the next startup.
static BOOT_FLAG_IO_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn boot_flag_lock() -> std::io::Result<std::sync::MutexGuard<'static, ()>> {
    BOOT_FLAG_IO_LOCK
        .lock()
        .map_err(|_| std::io::Error::other("Safe Loop boot-flag I/O lock is poisoned"))
}

fn restore_boot_flag_claim(claim: &Path, target: &Path) -> std::io::Result<bool> {
    match std::fs::hard_link(claim, target) {
        Ok(()) => {
            std::fs::remove_file(claim)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        // Keep `claim` as forensic evidence when restoration itself failed.
        Err(error) => Err(error),
    }
}

/// Filesystem-backed store for the Safe Loop. Default location is
/// `%ProgramData%\Nidavellir` (writable by the SYSTEM/admin service and
/// preserved across reboots); tests point `base` at a temp directory.
#[derive(Debug, Clone)]
pub struct SafeLoopStore {
    base: PathBuf,
}

impl SafeLoopStore {
    /// Store rooted at the machine-wide data directory.
    pub fn system() -> Self {
        Self::new(default_data_dir())
    }

    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    pub fn base_dir(&self) -> &Path {
        &self.base
    }

    fn ensure_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.base)
    }

    pub fn boot_flag_path(&self) -> PathBuf {
        self.base.join(BOOT_FLAG_FILE)
    }

    pub fn record_path(&self) -> PathBuf {
        self.base.join(RECORD_FILE)
    }

    pub fn heartbeat_path(&self) -> PathBuf {
        self.base.join(HEARTBEAT_FILE)
    }

    /// Arm the boot-flag before applying a point.
    pub fn arm_boot_flag(&self, flag: &BootFlag) -> std::io::Result<()> {
        let _guard = boot_flag_lock()?;
        self.ensure_dir()?;
        let orphaned_claims = self.boot_flag_clear_claims()?;
        if !orphaned_claims.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "Safe Loop boot flag has {} unfinished clear claim(s); refusing to arm a new transaction",
                    orphaned_claims.len()
                ),
            ));
        }
        let json = serde_json::to_string_pretty(flag)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let temp = self.base.join(format!(
            ".{BOOT_FLAG_FILE}.arm-{}-{}.tmp",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let write_result = (|| -> std::io::Result<()> {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;
            // Atomic create-if-absent: a second process can never overwrite an already armed owner.
            std::fs::hard_link(&temp, self.boot_flag_path())
        })();
        let cleanup = std::fs::remove_file(&temp);
        match (write_result, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Ok(()), Err(error)) => Err(std::io::Error::new(
                error.kind(),
                format!("boot flag armed but temporary file cleanup failed: {error}"),
            )),
            (Err(error), _) => Err(error),
        }
    }

    /// Read the boot-flag if armed.
    pub fn read_boot_flag(&self) -> Option<BootFlag> {
        self.read_boot_flag_checked().ok().flatten()
    }

    /// Strict safety-path boot-flag reader. Missing means disarmed; every other I/O or JSON error
    /// is returned so corrupt recovery state can never be mistaken for permission to write.
    pub fn read_boot_flag_checked(&self) -> std::io::Result<Option<BootFlag>> {
        let _guard = boot_flag_lock()?;
        self.read_boot_flag_checked_unlocked()
    }

    fn read_boot_flag_checked_unlocked(&self) -> std::io::Result<Option<BootFlag>> {
        let current = Self::read_boot_flag_at(&self.boot_flag_path())?;
        if current.is_some() {
            return Ok(current);
        }
        let claims = self.boot_flag_clear_claims()?;
        if claims.is_empty() {
            Ok(None)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Safe Loop boot flag has {} unfinished clear claim(s); recovery remains armed",
                    claims.len()
                ),
            ))
        }
    }

    fn boot_flag_clear_claims(&self) -> std::io::Result<Vec<PathBuf>> {
        let entries = match std::fs::read_dir(&self.base) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut claims = Vec::new();
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(BOOT_FLAG_CLEAR_CLAIM_PREFIX) && name.ends_with(".tmp") {
                claims.push(entry.path());
            }
        }
        Ok(claims)
    }

    fn read_boot_flag_at(path: &Path) -> std::io::Result<Option<BootFlag>> {
        let data = match std::fs::read_to_string(path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(std::io::Error::new(
                    error.kind(),
                    format!("read Safe Loop boot flag {}: {error}", path.display()),
                ))
            }
        };
        serde_json::from_str(strip_bom(&data))
            .map(Some)
            .map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid Safe Loop boot flag {}: {error}", path.display()),
                )
            })
    }

    pub fn is_boot_flag_armed(&self) -> bool {
        if self.boot_flag_path().exists() {
            return true;
        }
        match self.boot_flag_clear_claims() {
            Ok(claims) => !claims.is_empty(),
            // Status is read-only; an unreadable recovery directory must look armed, never clear.
            Err(_) => true,
        }
    }

    /// Clear the boot-flag after a clean validation.
    pub fn clear_boot_flag(&self) -> std::io::Result<()> {
        let _guard = boot_flag_lock()?;
        self.clear_boot_flag_unlocked()
    }

    fn clear_boot_flag_unlocked(&self) -> std::io::Result<()> {
        match std::fs::remove_file(self.boot_flag_path()) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Clear only when `expected` still owns the flag. Returns `false` when a newer transaction has
    /// replaced it. The canonical flag is first atomically claimed; a later process can then arm a
    /// new canonical path without that newer owner ever being deleted by this clear.
    pub fn clear_boot_flag_if_matches(&self, expected: &BootFlag) -> std::io::Result<bool> {
        let _guard = boot_flag_lock()?;
        let target = self.boot_flag_path();
        let claim = self.base.join(format!(
            "{BOOT_FLAG_CLEAR_CLAIM_PREFIX}{}-{}.tmp",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        match std::fs::rename(&target, &claim) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        }
        let current = match Self::read_boot_flag_at(&claim) {
            Ok(Some(current)) => current,
            Ok(None) => unreachable!("claimed boot flag disappeared while locally locked"),
            Err(error) => {
                let _ = restore_boot_flag_claim(&claim, &target);
                return Err(error);
            }
        };
        if expected.same_transaction(&current) {
            std::fs::remove_file(&claim)?;
            return Ok(true);
        }

        // A different transaction was claimed. Restore it only if no still-newer process has
        // already armed the canonical path. The hard link is create-if-absent and cannot replace
        // that later owner.
        if !restore_boot_flag_claim(&claim, &target)? {
            std::fs::remove_file(&claim)?;
        }
        Ok(false)
    }

    /// Load the persisted record, or the default if none/unreadable.
    pub fn load_record(&self) -> SafeLoopRecord {
        std::fs::read_to_string(self.record_path())
            .ok()
            .and_then(|d| serde_json::from_str(strip_bom(&d)).ok())
            .unwrap_or_default()
    }

    /// Strict safety-path loader. A missing record means pristine state; every other read or JSON
    /// error is returned so hardware preflights cannot silently replace unavailable safety state
    /// with [`SafeLoopRecord::default`].
    pub fn load_record_checked(&self) -> std::io::Result<SafeLoopRecord> {
        let path = self.record_path();
        let data = match std::fs::read_to_string(&path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SafeLoopRecord::default())
            }
            Err(error) => {
                return Err(std::io::Error::new(
                    error.kind(),
                    format!("read Safe Loop record {}: {error}", path.display()),
                ))
            }
        };
        serde_json::from_str(strip_bom(&data)).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid Safe Loop record {}: {error}", path.display()),
            )
        })
    }

    pub fn save_record(&self, record: &SafeLoopRecord) -> std::io::Result<()> {
        self.ensure_dir()?;
        let json = serde_json::to_string_pretty(record)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(self.record_path(), json)
    }

    /// Write a liveness heartbeat (roadmap §4.3, layer 2).
    pub fn write_heartbeat(&self) -> std::io::Result<()> {
        self.ensure_dir()?;
        std::fs::write(self.heartbeat_path(), chrono::Utc::now().to_rfc3339())
    }

    pub fn clean_shutdown_path(&self) -> PathBuf {
        self.base.join(CLEAN_SHUTDOWN_FILE)
    }

    /// Record that the service stopped *gracefully* (a clean OS shutdown/restart, or an explicit
    /// service stop). Startup recovery consumes this once so an armed boot-flag left behind by a
    /// user-initiated restart is NOT mistaken for a crash. Best-effort.
    pub fn write_clean_shutdown(&self) -> std::io::Result<()> {
        self.ensure_dir()?;
        std::fs::write(self.clean_shutdown_path(), chrono::Utc::now().to_rfc3339())
    }

    /// True when a graceful-shutdown marker is present. Consumed exactly once at startup so it can
    /// never mask a later, genuine crash.
    pub fn is_clean_shutdown_present(&self) -> bool {
        self.clean_shutdown_path().exists()
    }

    /// Remove the graceful-shutdown marker. Idempotent (absent ⇒ `Ok`).
    pub fn clear_clean_shutdown(&self) -> std::io::Result<()> {
        match std::fs::remove_file(self.clean_shutdown_path()) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// Strip a leading UTF-8 BOM so files touched by external editors still parse.
fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

/// `%ProgramData%\Nidavellir`, falling back to a temp dir if the env is unset.
pub fn default_data_dir() -> PathBuf {
    let base = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Nidavellir")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitions_follow_the_happy_path() {
        let mut s = SafeLoopState::Idle;
        s = transition(s, SafeLoopEvent::StartProbe);
        assert_eq!(s, SafeLoopState::Probing);
        s = transition(s, SafeLoopEvent::Applied);
        assert_eq!(s, SafeLoopState::Applying);
        s = transition(s, SafeLoopEvent::EnterDwell);
        assert_eq!(s, SafeLoopState::Dwell);
        s = transition(s, SafeLoopEvent::DwellPassed);
        assert_eq!(s, SafeLoopState::Validated);
    }

    #[test]
    fn soft_fail_and_crash_paths() {
        assert_eq!(
            transition(SafeLoopState::Dwell, SafeLoopEvent::SoftFail),
            SafeLoopState::Unstable
        );
        // A hard crash from any state lands in Unstable…
        assert_eq!(
            transition(SafeLoopState::Applying, SafeLoopEvent::HardCrash),
            SafeLoopState::Unstable
        );
        // …and the safe-mode trip overrides everything.
        assert_eq!(
            transition(SafeLoopState::Dwell, SafeLoopEvent::TripSafeMode),
            SafeLoopState::SafeMode
        );
    }

    #[test]
    fn unknown_transition_is_a_no_op() {
        assert_eq!(
            transition(SafeLoopState::Idle, SafeLoopEvent::DwellPassed),
            SafeLoopState::Idle
        );
    }

    #[test]
    fn bugcheck_classification() {
        assert_eq!(classify_bugcheck(0x124), CrashClass::OcInstability);
        assert_eq!(classify_bugcheck(0x101), CrashClass::OcInstability);
        assert_eq!(classify_bugcheck(0x133), CrashClass::OcInstability);
        assert_eq!(classify_bugcheck(0x116), CrashClass::OcInstability);
        assert_eq!(classify_bugcheck(0x117), CrashClass::OcInstability);
        assert_eq!(classify_bugcheck(0x50), CrashClass::Unrelated);
        assert_eq!(classify_bugcheck(0), CrashClass::Unknown);
    }

    #[test]
    fn bugcheck_text_parsing() {
        let msg = "The computer has rebooted from a bugcheck. The bugcheck was: \
                   0x00000124 (0x0000000000000000, 0xffff). A dump was saved.";
        assert_eq!(parse_bugcheck_code(msg), Some(0x124));
        assert_eq!(parse_bugcheck_code("0x101"), Some(0x101));
        assert_eq!(parse_bugcheck_code("no code here"), None);
    }

    #[test]
    fn whea_delta_is_soft_fail() {
        assert!(whea_soft_fail(2, 5));
        assert!(!whea_soft_fail(5, 5));
        assert!(!whea_soft_fail(5, 4));
    }

    #[test]
    fn chebyshev_and_blacklist_region() {
        let a = TuningPoint::from_axes([("vcore", -50), ("pl1", 120)]);
        let b = TuningPoint::from_axes([("vcore", -60), ("pl1", 120)]);
        assert_eq!(a.chebyshev(&b), 10);

        let region = BlacklistRegion::around(a.clone(), 15);
        assert!(region.contains(&b));
        let far = TuningPoint::from_axes([("vcore", -200), ("pl1", 120)]);
        assert!(!region.contains(&far));
    }

    #[test]
    fn missing_axis_treated_as_zero() {
        let a = TuningPoint::from_axes([("vcore", -30)]);
        let stock = TuningPoint::stock();
        assert_eq!(a.chebyshev(&stock), 30);
        assert!(stock.is_stock());
        assert!(!a.is_stock());
    }

    #[test]
    fn recovery_clean_boot_reapplies_last_validated() {
        let mut rec = SafeLoopRecord::default();
        let good = TuningPoint::from_axes([("vcore", -40)]);
        rec.mark_validated(good.clone());

        let action = decide_recovery(None, CrashClass::Unknown, &rec);
        assert_eq!(
            action,
            RecoveryAction::ApplyLastValidated {
                point: good.clone()
            }
        );
        assert_eq!(apply_recovery(&mut rec, &action), good);
    }

    #[test]
    fn recovery_clean_boot_nothing_validated_is_idle() {
        let rec = SafeLoopRecord::default();
        assert_eq!(
            decide_recovery(None, CrashClass::Unknown, &rec),
            RecoveryAction::Idle
        );
    }

    #[test]
    fn recovery_armed_flag_blacklists_and_recedes() {
        let mut rec = SafeLoopRecord::default();
        let good = TuningPoint::from_axes([("vcore", -40)]);
        rec.mark_validated(good.clone());
        let crashed = TuningPoint::from_axes([("vcore", -80)]);
        let flag = BootFlag::new(crashed.clone(), "probing");

        let action = decide_recovery(Some(&flag), CrashClass::OcInstability, &rec);
        assert_eq!(
            action,
            RecoveryAction::BlacklistAndRecede {
                crashed: crashed.clone(),
                recede_to: good.clone(),
                class: CrashClass::OcInstability,
                count_toward_safe_mode: true,
            }
        );
        let applied = apply_recovery(&mut rec, &action);
        assert_eq!(applied, good);
        assert_eq!(rec.consecutive_crashes, 1);
        assert!(rec.is_blacklisted(&crashed));
        assert_eq!(rec.crash_log, vec![CrashClass::OcInstability]);
    }

    #[test]
    fn three_consecutive_crashes_trip_safe_mode() {
        let mut rec = SafeLoopRecord::default();
        rec.consecutive_crashes = 2; // two prior crashes already on record
        let crashed = TuningPoint::from_axes([("vcore", -80)]);
        let flag = BootFlag::new(crashed, "probing");

        let action = decide_recovery(Some(&flag), CrashClass::OcInstability, &rec);
        assert_eq!(
            action,
            RecoveryAction::EnterSafeMode {
                stock: TuningPoint::stock()
            }
        );
        let applied = apply_recovery(&mut rec, &action);
        assert!(applied.is_stock());
        assert!(rec.safe_mode);
        assert_eq!(rec.state, SafeLoopState::SafeMode);
        assert_eq!(rec.consecutive_crashes, 3);
    }

    #[test]
    fn supervised_f2_tdrs_blacklist_without_entering_safe_mode() {
        let mut rec = SafeLoopRecord::default();
        for voltage_mv in [950, 943, 937, 931, 925] {
            let crashed = TuningPoint::from_axes([
                ("gpu_freq_mhz", 1935),
                ("gpu_vf_bin_mv", voltage_mv),
                ("gpu_offset_mhz", 15),
            ]);
            let flag = BootFlag::new(crashed.clone(), SUPERVISED_F2_FORGE_PHASE);
            let action = decide_recovery(Some(&flag), CrashClass::OcInstability, &rec);
            assert_eq!(
                action,
                RecoveryAction::BlacklistAndRecede {
                    crashed: crashed.clone(),
                    recede_to: rec.recovery_target(),
                    class: CrashClass::OcInstability,
                    count_toward_safe_mode: false,
                }
            );
            let _ = apply_recovery(&mut rec, &action);
        }
        assert_eq!(rec.consecutive_crashes, 0);
        assert!(!rec.safe_mode);
        assert_eq!(rec.blacklist.len(), 5);
    }

    #[test]
    fn unrelated_crash_during_f2_still_counts_toward_safe_mode() {
        let rec = SafeLoopRecord {
            consecutive_crashes: SAFE_MODE_CRASH_THRESHOLD - 1,
            ..SafeLoopRecord::default()
        };
        let flag = BootFlag::new(
            TuningPoint::from_axes([("gpu_freq_mhz", 1935)]),
            SUPERVISED_F2_FORGE_PHASE,
        );
        assert_eq!(
            decide_recovery(Some(&flag), CrashClass::Unrelated, &rec),
            RecoveryAction::EnterSafeMode {
                stock: TuningPoint::stock()
            }
        );
    }

    #[test]
    fn safe_mode_persists_on_clean_boot() {
        let mut rec = SafeLoopRecord::default();
        rec.safe_mode = true;
        assert_eq!(
            decide_recovery(None, CrashClass::Unknown, &rec),
            RecoveryAction::RemainSafeMode {
                stock: TuningPoint::stock()
            }
        );
    }

    #[test]
    fn clean_reboot_in_safe_mode_does_not_inflate_crash_streak() {
        // Reproduces the "every manual reboot adds a crash" bug: re-entering Safe Mode on a clean
        // boot (no boot-flag) must keep the streak fixed, not climb on each restart.
        let mut rec = SafeLoopRecord::default();
        rec.safe_mode = true;
        rec.consecutive_crashes = 3;
        for _ in 0..5 {
            let action = decide_recovery(None, CrashClass::Unknown, &rec);
            assert_eq!(
                action,
                RecoveryAction::RemainSafeMode {
                    stock: TuningPoint::stock()
                }
            );
            let applied = apply_recovery(&mut rec, &action);
            assert!(applied.is_stock());
        }
        assert_eq!(
            rec.consecutive_crashes, 3,
            "streak must not grow on clean reboots"
        );
        assert!(rec.safe_mode);
        assert_eq!(rec.state, SafeLoopState::SafeMode);
    }

    #[test]
    fn clear_recovery_latch_releases_safe_mode_but_keeps_learning() {
        let mut rec = SafeLoopRecord::default();
        let good = TuningPoint::from_axes([("vcore", -40)]);
        rec.mark_validated(good.clone());
        rec.blacklist.push(BlacklistRegion::around(
            TuningPoint::from_axes([("vcore", -80)]),
            1,
        ));
        rec.crash_log.push(CrashClass::OcInstability);
        rec.safe_mode = true;
        rec.consecutive_crashes = 4;
        rec.state = SafeLoopState::SafeMode;

        rec.clear_recovery_latch();

        // Latch released…
        assert!(!rec.safe_mode);
        assert_eq!(rec.consecutive_crashes, 0);
        assert_eq!(rec.state, SafeLoopState::Idle);
        // …learning preserved.
        assert_eq!(rec.last_validated, Some(good));
        assert_eq!(rec.blacklist.len(), 1);
        assert_eq!(rec.crash_log, vec![CrashClass::OcInstability]);
    }

    #[test]
    fn detector_lab_interruption_returns_to_stock_without_learning() {
        let good = TuningPoint::from_axes([("gpu_freq_mhz", 1800), ("gpu_vf_bin_mv", 875)]);
        let mut rec = SafeLoopRecord {
            last_validated: Some(good.clone()),
            consecutive_crashes: 2,
            ..SafeLoopRecord::default()
        };
        let interrupted = TuningPoint::from_axes([("gpu_freq_mhz", 1815), ("gpu_vf_bin_mv", 875)]);
        let flag = BootFlag::new(interrupted.clone(), DETECTOR_LAB_PHASE);

        let action = decide_recovery(Some(&flag), CrashClass::OcInstability, &rec);
        assert_eq!(
            action,
            RecoveryAction::RecoverDiagnosticInterruption {
                interrupted,
                class: CrashClass::OcInstability,
            }
        );
        assert!(apply_recovery(&mut rec, &action).is_stock());
        assert_eq!(rec.state, SafeLoopState::Idle);
        assert_eq!(rec.last_validated, Some(good));
        assert_eq!(rec.consecutive_crashes, 2);
        assert!(rec.blacklist.is_empty());
        assert!(rec.crash_log.is_empty());
        assert!(!rec.safe_mode);
    }

    #[test]
    fn pending_forge_incident_requires_acknowledgement_and_stays_at_stock() {
        let mut rec = SafeLoopRecord::default();
        rec.mark_validated(TuningPoint::from_axes([("gpu_freq_mhz", 1800)]));
        let incident = ForgeIncident::new(
            ForgeIncidentKind::UnaccountedRestart,
            Some("run-1".into()),
            Some("gpu-1".into()),
            None,
            None,
            "Forge restarted without an attributable candidate",
        );
        assert!(rec.record_forge_incident(incident.clone()));

        let action = decide_recovery(None, CrashClass::Unknown, &rec);
        assert_eq!(
            action,
            RecoveryAction::AwaitOperatorAcknowledgement {
                incident: incident.clone()
            }
        );
        assert!(apply_recovery(&mut rec, &action).is_stock());
        assert_eq!(rec.state, SafeLoopState::Unstable);

        rec.clear_recovery_latch();
        assert_eq!(rec.state, SafeLoopState::Unstable);
        assert_eq!(rec.pending_forge_incident, Some(incident));
    }

    #[test]
    fn acknowledging_incident_preserves_durable_history() {
        let mut rec = SafeLoopRecord::default();
        let incident = ForgeIncident::new(
            ForgeIncidentKind::OperatorFieldFailure,
            None,
            Some("gpu-1".into()),
            Some(1845),
            Some(862),
            "Confirmed under real use",
        );
        assert!(rec.record_forge_incident(incident.clone()));
        assert!(!rec.record_forge_incident(incident.clone()));

        let acknowledged = rec.acknowledge_forge_incident().unwrap();
        assert!(acknowledged.acknowledged);
        assert!(rec.pending_forge_incident.is_none());
        assert_eq!(rec.forge_incidents.len(), 1);
        assert!(rec.forge_incidents[0].acknowledged);
    }

    #[test]
    fn acknowledged_run_can_latch_a_later_candidate_crash_without_duplicating_pending_event() {
        let mut rec = SafeLoopRecord::default();
        let first = ForgeIncident::new(
            ForgeIncidentKind::CandidateCrash,
            Some("run-resumed".into()),
            Some("gpu-1".into()),
            Some(1920),
            Some(931),
            "first TDR",
        );
        assert!(rec.record_forge_incident(first.clone()));

        let duplicate_while_pending = ForgeIncident::new(
            ForgeIncidentKind::CandidateCrash,
            Some("run-resumed".into()),
            Some("gpu-1".into()),
            Some(1920),
            Some(931),
            "startup reconciliation of the same interruption",
        );
        assert!(!rec.record_forge_incident(duplicate_while_pending));
        assert_eq!(rec.forge_incidents.len(), 1);

        assert_eq!(rec.acknowledge_forge_incident().unwrap().id, first.id);
        let second = ForgeIncident::new(
            ForgeIncidentKind::CandidateCrash,
            Some("run-resumed".into()),
            Some("gpu-1".into()),
            Some(1860),
            Some(900),
            "second TDR after explicit resume",
        );
        assert!(rec.record_forge_incident(second.clone()));
        assert_eq!(rec.pending_forge_incident, Some(second));
        assert_eq!(rec.forge_incidents.len(), 2);
        assert!(rec.forge_incidents[0].acknowledged);
    }

    #[test]
    fn exact_candidate_crash_promotes_generic_same_run_incident_in_place() {
        for generic_kind in [
            ForgeIncidentKind::RuntimeFailure,
            ForgeIncidentKind::UnaccountedRestart,
        ] {
            let mut rec = SafeLoopRecord::default();
            let generic = ForgeIncident::new(
                generic_kind,
                Some("run-promote".into()),
                Some("gpu-1".into()),
                None,
                None,
                "generic interruption",
            );
            assert!(rec.record_forge_incident(generic.clone()));

            let exact = ForgeIncident::new(
                ForgeIncidentKind::CandidateCrash,
                Some("run-promote".into()),
                Some("gpu-1".into()),
                Some(1860),
                Some(868),
                "exact armed candidate",
            );
            assert!(rec.record_forge_incident(exact));

            let pending = rec.pending_forge_incident.as_ref().unwrap();
            assert_eq!(pending.kind, ForgeIncidentKind::CandidateCrash);
            assert_eq!(
                (pending.target_mhz, pending.anchor_mv),
                (Some(1860), Some(868))
            );
            assert_eq!(pending.id, generic.id);
            assert_eq!(pending.detected_at, generic.detected_at);
            assert_eq!(rec.forge_incidents, vec![pending.clone()]);
        }
    }

    #[test]
    fn exact_candidate_crash_promotes_incomplete_candidate_but_not_operator_or_other_gpu() {
        let mut incomplete = SafeLoopRecord::default();
        let partial = ForgeIncident::new(
            ForgeIncidentKind::CandidateCrash,
            Some("run-partial".into()),
            Some("gpu-1".into()),
            Some(1860),
            None,
            "candidate coordinates incomplete",
        );
        assert!(incomplete.record_forge_incident(partial.clone()));
        assert!(incomplete.record_forge_incident(ForgeIncident::new(
            ForgeIncidentKind::CandidateCrash,
            Some("run-partial".into()),
            Some("gpu-1".into()),
            Some(1860),
            Some(868),
            "candidate coordinates recovered",
        )));
        let promoted = incomplete.pending_forge_incident.as_ref().unwrap();
        assert_eq!(promoted.id, partial.id);
        assert_eq!(promoted.anchor_mv, Some(868));
        assert_eq!(incomplete.forge_incidents.len(), 1);

        let mut operator = SafeLoopRecord::default();
        let field_failure = ForgeIncident::new(
            ForgeIncidentKind::OperatorFieldFailure,
            Some("run-operator".into()),
            Some("gpu-1".into()),
            Some(1800),
            Some(875),
            "operator report",
        );
        assert!(operator.record_forge_incident(field_failure.clone()));
        assert!(!operator.record_forge_incident(ForgeIncident::new(
            ForgeIncidentKind::CandidateCrash,
            Some("run-operator".into()),
            Some("gpu-1".into()),
            Some(1815),
            Some(875),
            "must not replace operator evidence",
        )));
        assert_eq!(operator.pending_forge_incident, Some(field_failure));

        let mut other_gpu = SafeLoopRecord::default();
        let generic = ForgeIncident::new(
            ForgeIncidentKind::RuntimeFailure,
            Some("run-gpu".into()),
            Some("gpu-1".into()),
            None,
            None,
            "gpu-1 runtime failure",
        );
        assert!(other_gpu.record_forge_incident(generic.clone()));
        assert!(!other_gpu.record_forge_incident(ForgeIncident::new(
            ForgeIncidentKind::CandidateCrash,
            Some("run-gpu".into()),
            Some("gpu-2".into()),
            Some(1860),
            Some(868),
            "different GPU",
        )));
        assert_eq!(other_gpu.pending_forge_incident, Some(generic));
    }

    #[test]
    fn legacy_record_defaults_forge_incident_fields() {
        let rec: SafeLoopRecord = serde_json::from_str(
            r#"{"state":"idle","consecutive_crashes":0,"last_validated":null,"blacklist":[],"safe_mode":false,"crash_log":[]}"#,
        )
        .unwrap();
        assert!(rec.pending_forge_incident.is_none());
        assert!(rec.forge_incidents.is_empty());
    }

    #[test]
    fn clean_shutdown_marker_roundtrips() {
        let dir = std::env::temp_dir().join(format!("nidavellir-sl-clean-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = SafeLoopStore::new(&dir);

        assert!(!store.is_clean_shutdown_present());
        store.write_clean_shutdown().unwrap();
        assert!(store.is_clean_shutdown_present());
        store.clear_clean_shutdown().unwrap();
        assert!(!store.is_clean_shutdown_present());
        store.clear_clean_shutdown().unwrap(); // idempotent

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_roundtrips_flag_and_record() {
        let dir = std::env::temp_dir().join(format!("nidavellir-sl-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = SafeLoopStore::new(&dir);

        assert!(!store.is_boot_flag_armed());
        let flag = BootFlag::new(TuningPoint::from_axes([("vcore", -75)]), "probing");
        store.arm_boot_flag(&flag).unwrap();
        assert!(store.is_boot_flag_armed());
        assert_eq!(store.read_boot_flag().unwrap().intent, flag.intent);

        let mut rec = SafeLoopRecord::default();
        rec.mark_validated(TuningPoint::from_axes([("vcore", -40)]));
        store.save_record(&rec).unwrap();
        assert_eq!(store.load_record(), rec);

        store.clear_boot_flag().unwrap();
        assert!(!store.is_boot_flag_armed());
        store.clear_boot_flag().unwrap(); // idempotent

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delayed_owner_cannot_clear_a_later_boot_transaction() {
        let dir = std::env::temp_dir().join(format!(
            "nidavellir-sl-owned-flag-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SafeLoopStore::new(&dir);
        let first = BootFlag::new(TuningPoint::from_axes([("gpu_freq_mhz", 1800)]), "apply");
        let later = BootFlag::new(TuningPoint::from_axes([("gpu_freq_mhz", 1815)]), "sentinel");
        store.arm_boot_flag(&first).unwrap();
        assert!(
            store.arm_boot_flag(&later).is_err(),
            "arming cannot overwrite another transaction"
        );
        assert_eq!(store.read_boot_flag_checked().unwrap(), Some(first.clone()));
        store.clear_boot_flag().unwrap();
        store.arm_boot_flag(&later).unwrap();

        assert!(!store.clear_boot_flag_if_matches(&first).unwrap());
        assert_eq!(store.read_boot_flag_checked().unwrap(), Some(later.clone()));
        assert!(store.clear_boot_flag_if_matches(&later).unwrap());
        assert_eq!(store.read_boot_flag_checked().unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn interrupted_conditional_clear_remains_fail_closed() {
        let dir = std::env::temp_dir().join(format!(
            "nidavellir-sl-orphaned-clear-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = SafeLoopStore::new(&dir);
        let claimed = BootFlag::new(
            TuningPoint::from_axes([("gpu_freq_mhz", 1800)]),
            "interrupted-clear",
        );
        let claim_path = dir.join(format!("{BOOT_FLAG_CLEAR_CLAIM_PREFIX}orphaned-owner.tmp"));
        std::fs::write(&claim_path, serde_json::to_vec(&claimed).unwrap()).unwrap();

        let error = store.read_boot_flag_checked().unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            error.to_string().contains("unfinished clear claim"),
            "{error}"
        );
        assert!(store.is_boot_flag_armed());
        assert!(
            store
                .arm_boot_flag(&BootFlag::new(TuningPoint::stock(), "later"))
                .is_err(),
            "an orphaned clear claim must block every later hardware transaction"
        );
        assert!(claim_path.exists());
        assert!(!store.boot_flag_path().exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checked_boot_flag_reader_rejects_corrupt_json() {
        let dir = std::env::temp_dir().join(format!(
            "nidavellir-sl-corrupt-flag-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SafeLoopStore::new(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(store.boot_flag_path(), "{ truncated").unwrap();

        let error = store.read_boot_flag_checked().unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains(BOOT_FLAG_FILE), "{error}");
        assert!(
            store.is_boot_flag_armed(),
            "corrupt flag remains physically armed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_record_tolerates_utf8_bom() {
        let dir = std::env::temp_dir().join(format!("nidavellir-sl-bom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = SafeLoopStore::new(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let json = "\u{feff}{\"intent\":{\"axes\":{\"vcore\":-80}},\"phase\":\"probing\",\"timestamp\":\"2026-05-31T05:00:00Z\"}";
        std::fs::write(store.boot_flag_path(), json).unwrap();
        let flag = store
            .read_boot_flag()
            .expect("BOM-prefixed flag should still parse");
        assert_eq!(flag.intent, TuningPoint::from_axes([("vcore", -80)]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_record_defaults_when_missing() {
        let dir =
            std::env::temp_dir().join(format!("nidavellir-sl-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = SafeLoopStore::new(&dir);
        assert_eq!(store.load_record(), SafeLoopRecord::default());
    }

    #[test]
    fn checked_load_record_defaults_when_missing() {
        let dir = std::env::temp_dir().join(format!(
            "nidavellir-sl-checked-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let store = SafeLoopStore::new(&dir);

        assert_eq!(
            store.load_record_checked().unwrap(),
            SafeLoopRecord::default()
        );
    }

    #[test]
    fn checked_load_record_rejects_invalid_json_with_path_context() {
        let dir = std::env::temp_dir().join(format!(
            "nidavellir-sl-checked-invalid-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SafeLoopStore::new(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(store.record_path(), "{ truncated").unwrap();

        let error = store.load_record_checked().unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains(RECORD_FILE), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
