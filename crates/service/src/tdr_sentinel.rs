//! v17 runtime TDR sentinel — Stage A (Event Log layer).
//!
//! Watches the Windows System event log for `nvlddmkm` Event ID 153 ("BusReset TDR") while a
//! Nidavellir profile is applied. The FIRST TDR is terminal for the current Windows boot: preserve
//! the raw event, persist the failed intent, return to stock, clear the persisted apply descriptor,
//! and require a reboot. A recovered driver context is never trusted for an automatic re-apply.
//! The independent silent-error canary is contained at stock without being mislabeled as a TDR;
//! any suggested higher-voltage point must return through exact-Apply qualification.
//!
//! Cost: one filtered `wevtutil` query every ~1 s (sub-millisecond CPU, ZERO GPU — never touches
//! the card during gameplay). The GPU canary (silent-error layer) is Stage B.
//!
//! Safety guards: acts ONLY when (a) an F2 undervolt profile is applied, (b) the Safe Loop boot
//! flag is NOT armed (during forge dwells the flag is armed — and a dwell DeviceLost RETAINS it —
//! so forge-owned TDRs are never double-handled), (c) the event is NEWER than service start /
//! the last handled event (historical log entries never trigger on boot).

#![cfg(windows)]

use std::sync::{Mutex, OnceLock};

use chrono::DateTime;
use nidavellir_core::safe_loop::{
    BlacklistRegion, BootFlag, SafeLoopStore, TuningPoint, DEFAULT_BLACKLIST_RADIUS,
};
use tracing::{info, warn};
use windows::Win32::System::SystemInformation::GetTickCount64;

const SENTINEL_POLL_MS: u64 = 1_000;
/// Post-action settle time (driver just recovered + we rewrote the curve).
const SENTINEL_COOLDOWN_MS: u64 = 60_000;
/// A canary-detected silent error is boundary-class → +2 bins at the same clock.
const SENTINEL_SILENT_BUMP_BINS: usize = 2;
/// Canary cadence: one owned TextureRop self-check every 20 s, and ONLY while the GPU is under
/// real load (silent errors at elastic idle voltages are meaningless and the canary must never
/// keep an idle card awake).
const SENTINEL_CANARY_POLL_MS: u64 = 20_000;
/// TextureRop self-check burst: long enough for ≥2 of the 250 ms checksum windows (reference +
/// compare). ~700 ms of shared GPU load every 20 s ≈ 3.5% duty while gaming — the price of a
/// canary that samples the BINDING failure path instead of being statistically blind.
const SENTINEL_CANARY_KERNEL_MS: u64 = 700;
const SENTINEL_CANARY_MIN_UTIL_PCT: f64 = 30.0;
/// Operator policy (2026-07-12, after the first successful field recovery): THREE strikes —
/// two automatic bumps, the third failure resets to stock and clears the profile. On the field
/// case (1815@843 → 862 exhausted at 2 strikes) the third bump would have landed 875 mV — the
/// operator's hand-validated golden voltage.
const SENTINEL_MAX_BUMPS_PER_SESSION: u32 = 2;
/// Event timestamps are second-granularity while the boot estimate comes from a millisecond uptime
/// counter. This only absorbs timestamp/initialization jitter; a real reboot is far outside it.
const BOOT_TIME_TOLERANCE_MS: u64 = 30_000;
const SENTINEL_STARTUP_HANDSHAKE_TIMEOUT_MS: u64 = 5_000;

static REBOOT_REQUIRED_EVENT: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub(crate) struct SentinelStartupSnapshot {
    latest_tdr: Option<String>,
    latest_bugcheck: Option<(String, u64)>,
}

impl SentinelStartupSnapshot {
    pub(crate) fn watcher_baseline(&self) -> Option<String> {
        self.latest_tdr.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct SentinelWatcherStartupState {
    schema_version: u32,
    baseline_seed: Option<String>,
    start_floor: String,
}

fn reboot_required_slot() -> &'static Mutex<Option<String>> {
    REBOOT_REQUIRED_EVENT.get_or_init(|| Mutex::new(None))
}

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn current_boot_epoch_ms() -> u64 {
    let uptime_ms = unsafe { GetTickCount64() };
    now_epoch_ms().saturating_sub(uptime_ms)
}

fn event_epoch_ms(timestamp: &str) -> Option<u64> {
    DateTime::parse_from_rfc3339(timestamp)
        .ok()?
        .timestamp_millis()
        .try_into()
        .ok()
}

fn event_is_from_current_boot(timestamp: &str, boot_epoch_ms: u64) -> bool {
    event_epoch_ms(timestamp)
        .is_some_and(|event_ms| event_ms >= boot_epoch_ms.saturating_sub(BOOT_TIME_TOLERANCE_MS))
}

/// Seed the process-local reboot latch from the durable Windows Event Log. A service restart on the
/// same boot therefore cannot make a post-TDR GPU look clean; only a newer Windows boot clears it.
pub(crate) fn initialize_reboot_guard() -> Result<SentinelStartupSnapshot, String> {
    let latest_tdr = match query_latest_tdr_event_checked() {
        Ok(event) => event,
        Err(error) => {
            mark_gpu_reboot_required("event-log-query-error:tdr");
            return Err(error);
        }
    };
    let latest_bugcheck = match query_latest_bugcheck_event_checked() {
        Ok(event) => event,
        Err(error) => {
            mark_gpu_reboot_required("event-log-query-error:bugcheck");
            return Err(error);
        }
    };
    let current_boot_tdr = latest_tdr
        .as_deref()
        .filter(|timestamp| event_is_from_current_boot(timestamp, current_boot_epoch_ms()))
        .map(str::to_owned);
    let mut slot = reboot_required_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *slot = current_boot_tdr;
    Ok(SentinelStartupSnapshot {
        latest_tdr,
        latest_bugcheck,
    })
}

pub(crate) fn mark_gpu_reboot_required(timestamp: &str) {
    let mut slot = reboot_required_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *slot = Some(timestamp.to_string());
}

pub(crate) fn reboot_required_event() -> Option<String> {
    reboot_required_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn legacy_gpu_write_guard_decision(
    reboot_event: Option<&str>,
    boot_flag: Result<(), String>,
    record: Result<(bool, bool), String>,
    ledger: Result<(), String>,
) -> Result<(), String> {
    if let Some(event) = reboot_event {
        return Err(format!(
            "GPU driver recovery is latched at {event}; reboot Windows before another GPU write"
        ));
    }
    boot_flag?;
    let (pending_incident, safe_mode) = record?;
    if pending_incident {
        return Err(
            "Safe Loop has a pending Forge incident; acknowledge recovery before another GPU write"
                .into(),
        );
    }
    if safe_mode {
        return Err("Safe Mode is active; refusing legacy GPU write".into());
    }
    ledger
}

fn legacy_boot_flag_guard(
    store: &SafeLoopStore,
    expected_owner: Option<&BootFlag>,
) -> Result<(), String> {
    let current = store.read_boot_flag_checked().map_err(|error| {
        format!("Safe Loop boot flag is unreadable; refusing GPU write: {error}")
    })?;
    match (expected_owner, current.as_ref()) {
        (None, None) => Ok(()),
        (None, Some(flag)) => Err(format!(
            "Safe Loop transaction {} ({}) is already armed; refusing legacy GPU write",
            if flag.transaction_id.is_empty() {
                "legacy/unknown"
            } else {
                &flag.transaction_id
            },
            flag.phase
        )),
        (Some(expected), Some(current)) if legacy_same_transaction(expected, current) => Ok(()),
        (Some(_), None) => Err("legacy GPU transaction lost its owned Safe Loop boot flag".into()),
        (Some(_), Some(_)) => Err(
            "legacy GPU transaction no longer owns the Safe Loop boot flag; refusing write".into(),
        ),
    }
}

fn legacy_same_transaction(expected: &BootFlag, current: &BootFlag) -> bool {
    if expected.transaction_id.is_empty() || current.transaction_id.is_empty() {
        expected == current
    } else {
        expected.transaction_id == current.transaction_id
    }
}

fn legacy_gpu_write_guard_with_owner(
    store: &SafeLoopStore,
    expected_owner: Option<&BootFlag>,
) -> Result<(), String> {
    if crate::gpu_apply::full_reset_pending(store.base_dir()) {
        return Err("Full Reset is incomplete; retry it before GPU writes".into());
    }
    let reboot_event = reboot_required_event();
    let boot_flag = legacy_boot_flag_guard(store, expected_owner);
    let record = store
        .load_record_checked()
        .map(|record| (record.pending_forge_incident.is_some(), record.safe_mode))
        .map_err(|error| format!("Safe Loop record is unreadable; refusing GPU write: {error}"));
    let ledger = nidavellir_core::condemnation::CondemnationLedger::new(store.base_dir())
        .load_all_checked()
        .map(|_| ())
        .map_err(|error| {
            format!("durable condemnation ledger is unreadable; refusing GPU write: {error}")
        });
    legacy_gpu_write_guard_decision(reboot_event.as_deref(), boot_flag, record, ledger)
}

/// Fail-closed guard for legacy workers that predate the Forge write preflight. It deliberately
/// performs a checked Safe Loop read on every mutation so a Sentinel incident raised after the
/// worker started still terminates that worker before its next hardware write.
pub(crate) fn legacy_gpu_write_guard(store: &SafeLoopStore) -> Result<(), String> {
    legacy_gpu_write_guard_with_owner(store, None)
}

/// Arm a legacy candidate only after the complete checked preflight, then prove that the same
/// transaction still owns the flag immediately before its first hardware write.
pub(crate) fn arm_legacy_gpu_transaction(
    store: &SafeLoopStore,
    flag: &BootFlag,
) -> Result<(), String> {
    legacy_gpu_write_guard(store)?;
    store
        .arm_boot_flag(flag)
        .map_err(|error| format!("failed to arm legacy Safe Loop transaction: {error}"))?;
    legacy_gpu_write_guard_with_owner(store, Some(flag))
}

/// Re-check every durable safety input while permitting only the caller's exact armed transaction.
pub(crate) fn legacy_owned_gpu_write_guard(
    store: &SafeLoopStore,
    flag: &BootFlag,
) -> Result<(), String> {
    legacy_gpu_write_guard_with_owner(store, Some(flag))
}

pub(crate) fn clear_legacy_gpu_transaction(
    store: &SafeLoopStore,
    flag: &BootFlag,
) -> Result<(), String> {
    match store.clear_boot_flag_if_matches(flag) {
        Ok(true) => Ok(()),
        Ok(false) => {
            Err("legacy Safe Loop transaction ownership changed; newer boot flag preserved".into())
        }
        Err(error) => Err(format!(
            "failed to clear owned legacy Safe Loop transaction: {error}"
        )),
    }
}

/// Recovery-only reset guard. A pending incident is precisely a reason to land at stock, but an
/// unreadable Safe Loop record or a driver-reset latch still makes even a reset write fail closed.
pub(crate) fn legacy_stock_reset_guard(store: &SafeLoopStore) -> Result<(), String> {
    if let Some(event) = reboot_required_event() {
        return Err(format!(
            "GPU driver recovery is latched at {event}; Sentinel already owns stock recovery"
        ));
    }
    store
        .load_record_checked()
        .map(|_| ())
        .map_err(|error| format!("Safe Loop record is unreadable; stock reset refused: {error}"))
}

/// A device-lost verdict in a legacy worker is terminal for the current Windows boot. Waiting for
/// the Event Log poll before latching would leave a window in which old recovery code could reapply.
pub(crate) fn mark_legacy_device_loss(worker: &str) {
    mark_gpu_reboot_required(&format!("legacy-device-loss:{worker}"));
}

/// Return a TDR written during the current diagnostic session, ignoring its captured baseline.
pub(crate) fn new_tdr_event_since(
    baseline: Option<&str>,
    session_started_epoch_ms: u64,
) -> Option<String> {
    let newest = query_latest_tdr_event()?;
    if baseline == Some(newest.as_str()) {
        return None;
    }
    let event_ms = event_epoch_ms(&newest)?;
    (event_ms.saturating_add(1_000) >= session_started_epoch_ms).then_some(newest)
}

/// What the sentinel decided for a detected TDR. Pure decision — testable without hardware.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SentinelAction {
    /// No undervolt applied (stock/F1) — record and move on.
    Ignore,
    /// Forge owns the GPU right now (boot flag armed) — never double-handle a dwell TDR.
    ForgeOwns,
    /// First failure: bump to this anchor (same clock) and re-apply.
    Bump { target_mhz: u32, new_anchor_mv: u32 },
    /// Ladder exhausted (or no higher bin exists): stay stock, clear the applied profile.
    Stock {
        target_mhz: u32,
        failed_anchor_mv: u32,
    },
}

/// Only a completed canary verdict can drive the silent-error fallback. A stalled GPU call stays
/// owned by the dedicated canary thread; the Event Log layer and boot reconciliation remain the
/// authoritative recovery paths for TDRs and hard wedges.
fn canary_returned_failure(verdict: &nidavellir_core::gpu_sweep::StabilityResult) -> bool {
    !matches!(verdict, nidavellir_core::gpu_sweep::StabilityResult::Stable)
}

/// Pure fallback-ladder decision. `bins_above` are the physical VF bins strictly above the failed
/// anchor, ascending (from the live sane curve).
pub(crate) fn sentinel_decide(
    applied: Option<(u32, u32)>,
    boot_flag_armed: bool,
    bumps_this_session: u32,
    bins_above: &[u32],
    bump_bins: usize,
) -> SentinelAction {
    let Some((target_mhz, anchor_mv)) = applied else {
        return SentinelAction::Ignore;
    };
    if boot_flag_armed {
        return SentinelAction::ForgeOwns;
    }
    if bumps_this_session >= SENTINEL_MAX_BUMPS_PER_SESSION {
        return SentinelAction::Stock {
            target_mhz,
            failed_anchor_mv: anchor_mv,
        };
    }
    // +N bins, or the highest available if the curve runs out before that (never less than +1).
    match bins_above
        .get(bump_bins.saturating_sub(1))
        .or(bins_above.last())
    {
        Some(&new_anchor_mv) => SentinelAction::Bump {
            target_mhz,
            new_anchor_mv,
        },
        None => SentinelAction::Stock {
            target_mhz,
            failed_anchor_mv: anchor_mv,
        },
    }
}

/// Extract `SystemTime='…'` from `wevtutil /f:xml` output (locale-proof, no regex).
pub(crate) fn parse_event_system_time(xml: &str) -> Option<String> {
    let start = xml.find("SystemTime='")? + "SystemTime='".len();
    let end = xml[start..].find('\'')? + start;
    Some(xml[start..end].to_string())
}

fn parse_wevtutil_query<T>(
    label: &str,
    success: bool,
    stdout: &[u8],
    stderr: &[u8],
    parse: impl FnOnce(&str) -> Option<T>,
) -> Result<Option<T>, String> {
    if !success {
        let detail = String::from_utf8_lossy(stderr).trim().to_string();
        return Err(if detail.is_empty() {
            format!("wevtutil {label} query failed without diagnostics")
        } else {
            format!("wevtutil {label} query failed: {detail}")
        });
    }
    let xml = String::from_utf8_lossy(stdout);
    if xml.trim().is_empty() {
        return Ok(None);
    }
    parse(&xml)
        .map(Some)
        .ok_or_else(|| format!("wevtutil {label} returned unreadable event XML"))
}

/// Newest nvlddmkm-153 event timestamp, distinguishing an empty log from an unavailable/corrupt
/// Event Log. Startup callers must use this checked form so a query failure cannot look like stock.
fn query_latest_tdr_event_checked() -> Result<Option<String>, String> {
    let out = std::process::Command::new("wevtutil")
        .args([
            "qe",
            "System",
            "/q:*[System[Provider[@Name='nvlddmkm'] and (EventID=153)]]",
            "/c:1",
            "/rd:true",
            "/f:xml",
        ])
        .output()
        .map_err(|error| format!("launch wevtutil TDR query: {error}"))?;
    parse_wevtutil_query(
        "nvlddmkm-153",
        out.status.success(),
        &out.stdout,
        &out.stderr,
        parse_event_system_time,
    )
}

/// Compatibility wrapper for diagnostic polling. A query failure latches all subsequent GPU
/// mutation in this boot instead of silently pretending that no TDR exists.
pub(crate) fn query_latest_tdr_event() -> Option<String> {
    match query_latest_tdr_event_checked() {
        Ok(event) => event,
        Err(error) => {
            warn!("sentinel: {error}; GPU writes are latched until reboot");
            mark_gpu_reboot_required("event-log-query-error:tdr");
            None
        }
    }
}

/// Newest WER bugcheck event as `(timestamp, stop_code)`. This complements nvlddmkm-153 for full
/// wedges that reboot with 0x133 but never let the NVIDIA provider finish logging its own event.
fn query_latest_bugcheck_event_checked() -> Result<Option<(String, u64)>, String> {
    let out = std::process::Command::new("wevtutil")
        .args([
            "qe",
            "System",
            "/q:*[System[Provider[@Name='Microsoft-Windows-WER-SystemErrorReporting'] and (EventID=1001)]]",
            "/c:1",
            "/rd:true",
            "/f:xml",
        ])
        .output()
        .map_err(|error| format!("launch wevtutil bugcheck query: {error}"))?;
    parse_wevtutil_query(
        "WER-1001",
        out.status.success(),
        &out.stdout,
        &out.stderr,
        |xml| {
            Some((
                parse_event_system_time(xml)?,
                nidavellir_core::safe_loop::parse_bugcheck_code(xml)?,
            ))
        },
    )
}

fn event_belongs_to_applied_session(event_ts: &str, applied_at: Option<&str>) -> bool {
    let Some(applied_at) = applied_at else {
        return false;
    };
    fn utc_order_key(timestamp: &str) -> Option<String> {
        let second: String = timestamp.chars().take(19).collect();
        if second.len() != 19 {
            return None;
        }
        let fraction = timestamp
            .get(19..)
            .and_then(|tail| tail.strip_prefix('.'))
            .map(|tail| {
                tail.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .take(9)
                    .collect::<String>()
            })
            .unwrap_or_default();
        Some(format!("{second}{fraction:0<9}"))
    }
    matches!(
        (utc_order_key(event_ts), utc_order_key(applied_at)),
        (Some(event), Some(applied)) if event >= applied
    )
}

/// Physical VF bins strictly above `anchor_mv` on the live sane base curve, ascending.
fn bins_above_anchor(anchor_mv: u32) -> Vec<u32> {
    use nidavellir_gpu_nvapi as gpu;
    let mut bins: Vec<u32> = gpu::read_vf_base_curve_modern()
        .into_iter()
        .filter(|&(_, mv, f)| crate::gpu_undervolt::is_f2_sane_point(mv, f))
        .map(|(_, mv, _)| mv)
        .filter(|&mv| mv > anchor_mv)
        .collect();
    bins.sort_unstable();
    bins.dedup();
    bins
}

fn baseline_path() -> std::path::PathBuf {
    nidavellir_core::safe_loop::default_data_dir().join("sentinel_baseline.txt")
}

fn watcher_startup_path() -> std::path::PathBuf {
    nidavellir_core::safe_loop::default_data_dir().join("sentinel_watcher_startup.json")
}

fn persist_watcher_startup_at(
    path: &std::path::Path,
    state: &SentinelWatcherStartupState,
) -> Result<(), String> {
    use std::io::Write;

    let parent = path.parent().ok_or_else(|| {
        format!(
            "sentinel watcher startup path has no parent: {}",
            path.display()
        )
    })?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create sentinel watcher startup directory: {error}"))?;
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| format!("serialize sentinel watcher startup state: {error}"))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("persist sentinel watcher startup state: {error}"))?;
    file.write_all(&bytes)
        .map_err(|error| format!("write sentinel watcher startup state: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("sync sentinel watcher startup state: {error}"))?;
    drop(file);
    let persisted = std::fs::read(path)
        .map_err(|error| format!("verify sentinel watcher startup state: {error}"))?;
    let verified: SentinelWatcherStartupState = serde_json::from_slice(&persisted)
        .map_err(|error| format!("parse persisted sentinel watcher startup state: {error}"))?;
    if &verified != state {
        return Err("verify sentinel watcher startup state: read-back mismatch".into());
    }
    Ok(())
}

fn persist_watcher_startup_checked(state: &SentinelWatcherStartupState) -> Result<(), String> {
    persist_watcher_startup_at(&watcher_startup_path(), state)
}

fn await_watcher_startup(
    ready: std::sync::mpsc::Receiver<Result<(), String>>,
) -> Result<(), String> {
    match ready.recv_timeout(std::time::Duration::from_millis(
        SENTINEL_STARTUP_HANDSHAKE_TIMEOUT_MS,
    )) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err("Sentinel watcher startup handshake timed out".into())
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err("Sentinel watcher exited before startup handshake".into())
        }
    }
}

fn load_baseline_checked() -> Result<Option<String>, String> {
    match std::fs::read_to_string(baseline_path()) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("read sentinel baseline: {error}")),
    }
}

fn persist_baseline_at(path: &std::path::Path, ts: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("sentinel baseline path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create sentinel baseline directory: {error}"))?;
    std::fs::write(path, ts).map_err(|error| format!("persist sentinel baseline: {error}"))?;
    let persisted = std::fs::read_to_string(path)
        .map_err(|error| format!("verify sentinel baseline: {error}"))?;
    if persisted != ts {
        return Err("verify sentinel baseline: read-back mismatch".into());
    }
    Ok(())
}

fn persist_baseline_checked(ts: &str) -> Result<(), String> {
    persist_baseline_at(&baseline_path(), ts)
}

/// A Windows Event Log cursor is committed only after every safety-critical recovery write
/// completed. An error leaves the previous cursor untouched so the same raw event is retried.
fn commit_event_after_recovery_at(
    path: &std::path::Path,
    ts: &str,
    recovery: Result<(), String>,
) -> Result<(), String> {
    recovery?;
    persist_baseline_at(path, ts)
}

fn commit_event_after_recovery(ts: &str, recovery: Result<(), String>) -> Result<(), String> {
    commit_event_after_recovery_at(&baseline_path(), ts, recovery)
}

fn persist_field_failure_for_gpu(
    store: &SafeLoopStore,
    gpu_key: &str,
    target_mhz: u32,
    failed_mv: u32,
    note: String,
) -> Result<(), String> {
    let intent = TuningPoint::from_axes([
        ("gpu_freq_mhz", target_mhz as i64),
        ("gpu_vf_bin_mv", failed_mv as i64),
    ]);
    let region = BlacklistRegion::around(intent, DEFAULT_BLACKLIST_RADIUS);
    let mut rec = store
        .load_record_checked()
        .map_err(|error| format!("sentinel Safe Loop record is unreadable: {error}"))?;
    if !rec.blacklist.contains(&region) {
        rec.blacklist.push(region);
        store
            .save_record(&rec)
            .map_err(|error| format!("sentinel blacklist persist failed: {error}"))?;
    }

    let ledger = nidavellir_core::condemnation::CondemnationLedger::new(store.base_dir());
    if !ledger
        .condemned_pairs(gpu_key)
        .rigid
        .contains(&(target_mhz, failed_mv))
    {
        crate::gpu_undervolt::append_condemnation(
            store.base_dir(),
            nidavellir_core::condemnation::CondemnationSeverity::Rigid,
            nidavellir_core::condemnation::KIND_FIELD_TDR,
            Some(gpu_key.to_string()),
            target_mhz,
            failed_mv,
            None,
            note,
        );
        // `append_condemnation` is intentionally best-effort for ordinary recovery callers. This
        // sentinel transaction is stricter: reload the ledger and prove that the negative fact is
        // durable before allowing the Event Log cursor to move.
        if !nidavellir_core::condemnation::CondemnationLedger::new(store.base_dir())
            .condemned_pairs(gpu_key)
            .rigid
            .contains(&(target_mhz, failed_mv))
        {
            return Err(format!(
                "sentinel condemnation was not durable for {target_mhz} MHz @ {failed_mv} mV"
            ));
        }
    }
    Ok(())
}

fn persist_field_failure(
    store: &SafeLoopStore,
    target_mhz: u32,
    failed_mv: u32,
    note: String,
) -> Result<(), String> {
    persist_field_failure_for_gpu(
        store,
        &crate::gpu_power_sweep::current_gpu_key(),
        target_mhz,
        failed_mv,
        note,
    )
}

fn applied_failure_pair(profile: &crate::gpu_apply::AppliedProfile) -> Option<(u32, u32)> {
    profile
        .undervolt
        .as_ref()
        .map(|applied| (applied.target_mhz, applied.anchor_mv))
        .or_else(|| profile.core.map(|point| (point.freq_mhz, point.voltage_mv)))
}

fn demote_after_boot_failure(
    store: &SafeLoopStore,
    applied_pair: Option<(u32, u32)>,
    source: &str,
) -> Result<(), String> {
    if let Some((target_mhz, anchor_mv)) = applied_pair {
        persist_field_failure(
            store,
            target_mhz,
            anchor_mv,
            format!("{source} after this profile was applied; boot stays stock"),
        )?;
    }
    crate::gpu_apply::clear_applied_checked()?;
    let pair_json = applied_pair.map_or_else(String::new, |(target_mhz, anchor_mv)| {
        format!(",\"target_mhz\":{target_mhz},\"failed_mv\":{anchor_mv}")
    });
    append_sentinel_log(&format!(
        "\"event\":\"boot-reconcile\",\"source\":\"{source}\",\"action\":\"stock\"{pair_json}"
    ));
    Ok(())
}

/// BOOT RECONCILIATION — the hole a live sentinel cannot cover: a hard WEDGE freezes the whole
/// machine (sentinel included), the operator power-cycles, and on the next boot the crash events
/// are HISTORICAL (correctly inert for the live watcher) while `reapply_on_boot` would happily
/// re-apply the exact profile that just froze the PC. Called BEFORE `reapply_on_boot`: if a
/// nvlddmkm-153 newer than the last persisted baseline exists AND an undervolt profile is
/// persisted, the crash happened on OUR watch → blacklist the point + clear the applied profile
/// (boot comes up STOCK — a hard wedge is ladder-exhausted-grade, no auto-bump at cold boot).
/// First run (no baseline file) only initializes the baseline. The injected snapshot was queried
/// before any profile reapply, so this routine never creates an unobserved startup window.
pub fn startup_reconcile(
    store: &SafeLoopStore,
    snapshot: &SentinelStartupSnapshot,
) -> Result<bool, String> {
    let newest = snapshot.latest_tdr.as_deref();
    let baseline = load_baseline_checked()?;
    let applied_profile = crate::gpu_apply::load_applied_checked()
        .map_err(|error| format!("sentinel applied-profile attribution failed: {error}"))?;
    let applied_pair = applied_profile.as_ref().and_then(applied_failure_pair);
    let new_tdr = newest.is_some_and(|timestamp| {
        baseline
            .as_deref()
            .is_some_and(|old| timestamp > old.trim())
            || applied_profile.as_ref().is_some_and(|profile| {
                event_belongs_to_applied_session(timestamp, profile.applied_at.as_deref())
            })
    });
    let session_bugcheck = applied_profile.as_ref().and_then(|profile| {
        snapshot
            .latest_bugcheck
            .as_ref()
            .filter(|(timestamp, code)| {
                event_belongs_to_applied_session(timestamp, profile.applied_at.as_deref())
                    && nidavellir_core::safe_loop::classify_bugcheck(*code)
                        == nidavellir_core::safe_loop::CrashClass::OcInstability
            })
    });

    if applied_profile.is_none() {
        if let Some(newest) = newest {
            persist_baseline_checked(newest)?;
        }
        return Ok(false);
    }
    if !new_tdr && session_bugcheck.is_none() {
        if let Some(newest) = newest {
            persist_baseline_checked(newest)?;
        }
        return Ok(false);
    }
    let source = session_bugcheck
        .as_ref()
        .map(|(_, code)| format!("WER bugcheck 0x{code:X}"))
        .unwrap_or_else(|| "nvlddmkm-153 TDR/wedge".into());
    if let Some((target_mhz, anchor_mv)) = applied_pair {
        warn!(
            "sentinel: {source} at {target_mhz} MHz @ {anchor_mv} mV belongs to the persisted apply session — blacklisting and clearing the profile; boot stays STOCK"
        );
    } else {
        warn!(
            "sentinel: {source} belongs to a persisted profile without an attributable core pair — clearing the profile; boot stays STOCK"
        );
    }
    let recovery_event = session_bugcheck
        .as_ref()
        .map(|(timestamp, _)| timestamp.as_str())
        .or(newest)
        .unwrap_or("boot-reconcile");
    if let Err(error) = demote_after_boot_failure(store, applied_pair, &source) {
        // Keep re-apply closed in this process even when the original event belongs to the prior
        // boot. The baseline remains unchanged, so every service start retries the durable demotion.
        mark_gpu_reboot_required(recovery_event);
        warn!(
            "sentinel: boot demotion incomplete ({error}); event cursor retained and profile re-apply blocked"
        );
        return Err(error);
    }
    if let Some(newest) = newest {
        if let Err(error) = commit_event_after_recovery(newest, Ok(())) {
            warn!("sentinel: boot demotion completed but event cursor was not committed: {error}");
            mark_gpu_reboot_required(recovery_event);
            return Err(error);
        }
    }
    Ok(true)
}

/// UI-facing status (overwritten each action): last sentinel event + operator recommendation.
fn write_sentinel_status(json: &str) {
    let _ = std::fs::create_dir_all(nidavellir_core::safe_loop::default_data_dir());
    let _ = std::fs::write(
        nidavellir_core::safe_loop::default_data_dir().join("sentinel_status.json"),
        json,
    );
}

fn append_sentinel_log(entry: &str) {
    let path = nidavellir_core::safe_loop::default_data_dir().join("sentinel_log.jsonl");
    let line = format!(
        "{{\"ts\":\"{}\",{entry}}}\n",
        nidavellir_core::f2_observation::now_rfc3339()
    );
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

/// Cross-LAYER episode dedup: the canary (stall) and the event-log watcher (the driver's 153 from
/// the same episode, seconds later) must never both act — observed in the field as a doubled
/// "ladder exhausted" 4 s apart. Any layer that ACTS stamps this; the other skips within the window.
static LAST_ACTION_EPOCH_S: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const CROSS_LAYER_DEDUP_S: u64 = 90;

static SENTINEL_CANARY_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static SENTINEL_CANARY_SEQUENCE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Correlation marker sampled by Game Trace. Sequence increments for every live canary attempt;
/// `active` brackets context creation through verdict so a future field trace can prove overlap.
pub(crate) fn canary_trace_marker() -> (bool, u64) {
    (
        SENTINEL_CANARY_ACTIVE.load(std::sync::atomic::Ordering::SeqCst),
        SENTINEL_CANARY_SEQUENCE.load(std::sync::atomic::Ordering::SeqCst),
    )
}

struct CanaryTraceGuard;

impl CanaryTraceGuard {
    fn begin() -> Self {
        SENTINEL_CANARY_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        SENTINEL_CANARY_ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}

impl Drop for CanaryTraceGuard {
    fn drop(&mut self) {
        SENTINEL_CANARY_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

fn epoch_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn active_game_trace_diagnostic(store: &SafeLoopStore) -> Result<Option<(u32, u32)>, String> {
    let Some(flag) = store
        .read_boot_flag_checked()
        .map_err(|error| format!("Safe Loop diagnostic attribution is unreadable: {error}"))?
    else {
        return Ok(None);
    };
    if flag.phase != nidavellir_core::safe_loop::GAME_TRACE_DIAGNOSTIC_PHASE {
        return Ok(None);
    }
    let target_mhz = flag
        .intent
        .axes
        .get("gpu_freq_mhz")
        .and_then(|value| u32::try_from(*value).ok());
    let anchor_mv = flag
        .intent
        .axes
        .get("gpu_vf_bin_mv")
        .and_then(|value| u32::try_from(*value).ok());
    target_mhz
        .zip(anchor_mv)
        .map(Some)
        .ok_or_else(|| "Safe Loop diagnostic flag has incomplete GPU coordinates".into())
}

fn handle_game_trace_diagnostic_failure(
    store: &SafeLoopStore,
    point: (u32, u32),
    kind: &str,
) -> Result<(), String> {
    let now = epoch_s();
    if !claim_action_epoch(&LAST_ACTION_EPOCH_S, now) {
        append_sentinel_log(&format!(
            "\"event\":\"{kind}\",\"action\":\"same-episode-skip\",\"scope\":\"game-trace-diagnostic\""
        ));
        return Err("diagnostic recovery is already owned by this failure episode".into());
    }
    let (target_mhz, anchor_mv) = point;
    warn!(
        "sentinel: {kind} during manual curve diagnostic at {target_mhz} MHz @ {anchor_mv} mV — returning to stock without learning"
    );
    let mut errors = Vec::new();
    if let Err(error) = crate::manual_point::reset_and_disarm(store) {
        errors.push(format!("diagnostic stock reset failed: {error}"));
    }
    // `reset_and_disarm` predates checked descriptor deletion. Verify it explicitly before the
    // Event Log cursor can advance; a stale profile must never resurrect after the required reboot.
    if let Err(error) = crate::gpu_apply::clear_applied_checked() {
        errors.push(format!("diagnostic descriptor cleanup failed: {error}"));
    }
    let reset_ok = errors.is_empty();
    append_sentinel_log(&format!(
        "\"event\":\"{kind}\",\"action\":\"stock\",\"scope\":\"game-trace-diagnostic\",\"target_mhz\":{target_mhz},\"anchor_mv\":{anchor_mv},\"reset_ok\":{reset_ok}"
    ));
    write_sentinel_status(&format!(
        "{{\"ts\":\"{}\",\"event\":\"{kind}\",\"action\":\"stock\",\"scope\":\"game-trace-diagnostic\",\"target_mhz\":{target_mhz},\"failed_mv\":{anchor_mv},\"reset_ok\":{reset_ok},\"recommendation\":\"Diagnostic detector reported {kind}; GPU returned to stock without blacklist learning.\"}}",
        nidavellir_core::f2_observation::now_rfc3339()
    ));
    if errors.is_empty() {
        Ok(())
    } else {
        let error = errors.join("; ");
        warn!("sentinel: {error}");
        Err(error)
    }
}

fn armed_failure_pair_checked(store: &SafeLoopStore) -> Result<Option<(u32, u32)>, String> {
    let Some(flag) = store
        .read_boot_flag_checked()
        .map_err(|error| format!("Safe Loop failure attribution is unreadable: {error}"))?
    else {
        return Ok(None);
    };
    let target_mhz = flag
        .intent
        .axes
        .get("gpu_freq_mhz")
        .and_then(|value| u32::try_from(*value).ok());
    let anchor_mv = flag
        .intent
        .axes
        .get("gpu_vf_bin_mv")
        .or_else(|| flag.intent.axes.get("gpu_voltage_mv"))
        .and_then(|value| u32::try_from(*value).ok());
    target_mhz
        .zip(anchor_mv)
        .map(Some)
        .ok_or_else(|| "armed Safe Loop flag has incomplete GPU failure coordinates".into())
}

fn resolve_live_tdr_failure_pair(
    applied_pair: Result<Option<(u32, u32)>, String>,
    armed_pair: Result<Option<(u32, u32)>, String>,
) -> Result<Option<(u32, u32)>, String> {
    let applied_pair = applied_pair?;
    let armed_pair = armed_pair?;
    Ok(applied_pair.or(armed_pair))
}

fn contain_unreadable_attribution_at_stock(scope: &str, error: &str) {
    if reboot_required_event().is_none() {
        mark_gpu_reboot_required(&format!("state-attribution-error:{scope}"));
    }
    let reset = crate::gpu_power_sweep::reset_to_stock_checked();
    warn!(
        "sentinel: unreadable {scope} attribution ({error}); GPU writes are latched, stock reset={}",
        reset
            .as_ref()
            .err()
            .map(String::as_str)
            .unwrap_or("ok")
    );
    append_sentinel_log(&format!(
        "\"event\":\"state-read-error\",\"scope\":\"{scope}\",\"action\":\"stock-latch-retry\",\"reset_ok\":{}",
        reset.is_ok()
    ));
}

/// Terminal recovery for a live TDR outside Forge/Detector Lab ownership. The negative evidence is
/// written before descriptor removal; every operation is checked. Any failure retains the Event Log
/// cursor and the process-local reboot latch, so the unsafe profile cannot be re-applied.
fn handle_live_tdr_reboot_required(store: &SafeLoopStore, event: &str) -> Result<(), String> {
    let applied_pair = crate::gpu_apply::load_applied_checked()
        .map(|profile| profile.as_ref().and_then(applied_failure_pair))
        .map_err(|error| format!("persisted applied-profile attribution is unreadable: {error}"));
    let failed_pair = match resolve_live_tdr_failure_pair(
        applied_pair,
        armed_failure_pair_checked(store),
    ) {
        Ok(pair) => pair,
        Err(attribution_error) => {
            // The raw Event Log cursor must remain uncommitted: deleting corrupt attribution state
            // would turn an exact failure into an unattributed one on retry. Hardware still lands at
            // stock, while the current-boot latch already raised by the watcher blocks every write.
            return match crate::gpu_power_sweep::reset_to_stock_checked() {
                Ok(()) => Err(format!(
                    "{attribution_error}; GPU returned to stock and attribution was retained for retry"
                )),
                Err(reset_error) => Err(format!(
                    "{attribution_error}; stock reset was not confirmed: {reset_error}"
                )),
            };
        }
    };
    append_sentinel_log(&format!(
        "\"event\":\"tdr\",\"event_ts\":\"{event}\",\"action\":\"reboot-required\",\"phase\":\"recovery-start\""
    ));

    let persistence = failed_pair.map_or(Ok(()), |(target_mhz, failed_mv)| {
        persist_field_failure(
            store,
            target_mhz,
            failed_mv,
            format!("live nvlddmkm-153 TDR at {event}; terminal stock/reboot recovery"),
        )
    });
    // Returning to stock is always attempted, even when evidence persistence failed. In that case
    // the descriptor/boot flag stay intact solely as retry attribution; the reboot latch prevents use.
    let stock = crate::gpu_power_sweep::reset_to_stock_checked();
    let descriptor = if persistence.is_ok() {
        crate::gpu_apply::clear_applied_checked()
    } else {
        Err("descriptor retained until the failed TDR condemnation can be persisted".into())
    };

    let mut errors = Vec::new();
    if let Err(error) = persistence {
        errors.push(error);
    }
    if let Err(error) = stock {
        errors.push(format!("stock reset was not confirmed: {error}"));
    }
    if let Err(error) = descriptor {
        errors.push(error);
    }
    if errors.is_empty() {
        store.clear_boot_flag().map_err(|error| {
            format!("Safe Loop flag cleanup failed after TDR recovery: {error}")
        })?;
        let pair_json = failed_pair.map_or_else(String::new, |(target_mhz, failed_mv)| {
            format!(",\"target_mhz\":{target_mhz},\"failed_mv\":{failed_mv}")
        });
        append_sentinel_log(&format!(
            "\"event\":\"tdr\",\"event_ts\":\"{event}\",\"action\":\"stock-reboot-required\",\"phase\":\"recovery-complete\"{pair_json}"
        ));
        write_sentinel_status(&format!(
            "{{\"ts\":\"{}\",\"event\":\"tdr\",\"event_ts\":\"{event}\",\"action\":\"stock-reboot-required\",\"reboot_required\":true,\"recommendation\":\"GPU returned to stock after a driver reset. Reboot Windows before any further GPU mutation.\"}}",
            nidavellir_core::f2_observation::now_rfc3339()
        ));
        Ok(())
    } else {
        let error = errors.join("; ");
        append_sentinel_log(&format!(
            "\"event\":\"tdr\",\"event_ts\":\"{event}\",\"action\":\"reboot-required\",\"phase\":\"recovery-incomplete\",\"error\":\"{error}\""
        ));
        Err(error)
    }
}

fn record_active_forge_tdr_durably(
    store: &SafeLoopStore,
    event_timestamp: &str,
) -> Result<(), String> {
    let _ = crate::gpu_power_sweep::record_active_forge_tdr(store, event_timestamp)?;
    let record = store
        .load_record_checked()
        .map_err(|error| format!("active Forge TDR record is unreadable: {error}"))?;
    let incident = record
        .pending_forge_incident
        .as_ref()
        .filter(|incident| incident.message.contains(event_timestamp))
        .ok_or_else(|| {
            "active Forge TDR incident was not durably stored; event cursor retained".to_string()
        })?;
    if incident.kind == nidavellir_core::safe_loop::ForgeIncidentKind::CandidateCrash {
        let run_id = incident
            .run_id
            .as_deref()
            .ok_or_else(|| "CandidateCrash has no exact run id".to_string())?;
        let gpu_key = incident
            .gpu_key
            .as_deref()
            .ok_or_else(|| "CandidateCrash has no exact GPU key".to_string())?;
        let target_mhz = incident
            .target_mhz
            .ok_or_else(|| "CandidateCrash has no exact target clock".to_string())?;
        let anchor_mv = incident
            .anchor_mv
            .ok_or_else(|| "CandidateCrash has no exact VF bin".to_string())?;
        crate::gpu_power_sweep::ensure_reconciled_candidate_crash_condemnation(
            store,
            gpu_key,
            Some(run_id),
            target_mhz,
            anchor_mv,
            &incident.message,
        )
        .map_err(|error| format!("failed to persist CandidateCrash condemnation: {error}"))?;
    }
    Ok(())
}

/// Atomically claim one cross-layer recovery episode. The watcher and canary are independent
/// threads; a load followed by an unconditional store lets both mutate the GPU when they observe
/// the same failure concurrently. Compare-exchange makes exactly one layer the recovery owner.
fn claim_action_epoch(last_action: &std::sync::atomic::AtomicU64, now: u64) -> bool {
    loop {
        let previous = last_action.load(std::sync::atomic::Ordering::SeqCst);
        if previous != 0 && now.saturating_sub(previous) < CROSS_LAYER_DEDUP_S {
            return false;
        }
        match last_action.compare_exchange(
            previous,
            now,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        ) {
            Ok(_) => return true,
            Err(_) => continue,
        }
    }
}

fn contain_canary_failure_at_stock(
    store: &SafeLoopStore,
    target_mhz: u32,
    failed_mv: u32,
    note: String,
) -> Result<(), String> {
    let persistence = persist_field_failure(store, target_mhz, failed_mv, note);
    let stock = crate::gpu_power_sweep::reset_to_stock_checked();
    let descriptor = crate::gpu_apply::clear_applied_checked();
    let mut errors = Vec::new();
    if let Err(error) = persistence {
        errors.push(error);
    }
    if let Err(error) = stock {
        errors.push(format!("stock reset was not confirmed: {error}"));
    }
    if let Err(error) = descriptor {
        errors.push(error);
    }
    if errors.is_empty() {
        store
            .clear_boot_flag()
            .map_err(|error| format!("Safe Loop flag cleanup failed: {error}"))?;
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Handle one returned canary failure. A higher-voltage fallback remains useful discovery advice,
/// but it is not exact-Apply-v29-qualified; runtime containment therefore returns to stock instead
/// of manufacturing a proof token or re-applying an unqualified point.
fn handle_failure(
    store: &SafeLoopStore,
    bumps_this_session: u32,
    bump_bins: usize,
    kind: &str,
) -> bool {
    let last = LAST_ACTION_EPOCH_S.load(std::sync::atomic::Ordering::SeqCst);
    let now = epoch_s();
    if last != 0 && now.saturating_sub(last) < CROSS_LAYER_DEDUP_S {
        info!("sentinel: {kind} within {CROSS_LAYER_DEDUP_S}s of the last action — same episode, skipping");
        append_sentinel_log(&format!(
            "\"event\":\"{kind}\",\"action\":\"same-episode-skip\""
        ));
        return false;
    }
    let applied = match crate::gpu_apply::load_applied_checked() {
        Ok(profile) => profile.and_then(|p| p.undervolt.map(|u| (u.target_mhz, u.anchor_mv))),
        Err(error) => {
            contain_unreadable_attribution_at_stock("canary-applied-profile", &error);
            return false;
        }
    };
    let boot_flag_armed = match store.read_boot_flag_checked() {
        Ok(flag) => flag.is_some(),
        Err(error) => {
            contain_unreadable_attribution_at_stock("canary-boot-flag", &error.to_string());
            return false;
        }
    };
    let action = sentinel_decide(
        applied,
        boot_flag_armed,
        bumps_this_session,
        &applied
            .map(|(_, mv)| bins_above_anchor(mv))
            .unwrap_or_default(),
        bump_bins,
    );
    match action {
        SentinelAction::Ignore => {
            info!("sentinel: nvlddmkm-153 TDR with no undervolt applied — recorded, no action");
            append_sentinel_log(&format!("\"event\":\"{kind}\",\"action\":\"ignore\""));
            false
        }
        SentinelAction::ForgeOwns => {
            info!("sentinel: TDR while the Safe Loop boot flag is armed — forge owns recovery");
            append_sentinel_log(&format!("\"event\":\"{kind}\",\"action\":\"forge-owns\""));
            false
        }
        SentinelAction::Bump {
            target_mhz,
            new_anchor_mv,
        } => {
            if !claim_action_epoch(&LAST_ACTION_EPOCH_S, now) {
                info!("sentinel: {kind} recovery was claimed concurrently by the other layer");
                append_sentinel_log(&format!(
                    "\"event\":\"{kind}\",\"action\":\"same-episode-skip\""
                ));
                return false;
            }
            let (_, failed_mv) = applied.expect("Bump implies applied");
            warn!(
                "sentinel: {kind} at {target_mhz} MHz @ {failed_mv} mV — candidate fallback \
                 {new_anchor_mv} mV requires fresh exact-Apply qualification; returning to stock"
            );
            let recovery = contain_canary_failure_at_stock(
                store,
                target_mhz,
                failed_mv,
                format!(
                    "in-game {kind}; {new_anchor_mv} mV fallback censored pending exact-Apply qualification"
                ),
            );
            if let Err(error) = &recovery {
                warn!("sentinel: {kind} containment incomplete ({error})");
            }
            append_sentinel_log(&format!(
                "\"event\":\"{kind}\",\"action\":\"stock\",\"target_mhz\":{target_mhz},\"failed_mv\":{failed_mv},\"suggested_mv\":{new_anchor_mv},\"recovery_ok\":{}",
                recovery.is_ok()
            ));
            write_sentinel_status(&format!(
                "{{\"ts\":\"{}\",\"event\":\"{kind}\",\"action\":\"stock\",\"target_mhz\":{target_mhz},\"failed_mv\":{failed_mv},\"suggested_mv\":{new_anchor_mv},\"recommendation\":\"Silent instability was contained at stock. Re-forge to qualify a safer voltage before applying again.\"}}",
                nidavellir_core::f2_observation::now_rfc3339()
            ));
            false
        }
        SentinelAction::Stock {
            target_mhz,
            failed_anchor_mv,
        } => {
            if !claim_action_epoch(&LAST_ACTION_EPOCH_S, now) {
                info!("sentinel: {kind} recovery was claimed concurrently by the other layer");
                append_sentinel_log(&format!(
                    "\"event\":\"{kind}\",\"action\":\"same-episode-skip\""
                ));
                return false;
            }
            warn!(
                "sentinel: {kind} at {target_mhz} MHz @ {failed_anchor_mv} mV after a previous bump \
                 this session — staying at stock (ladder exhausted); profile cleared"
            );
            let recovery = contain_canary_failure_at_stock(
                store,
                target_mhz,
                failed_anchor_mv,
                format!("{kind} after a prior bump this session; ladder exhausted, stock"),
            );
            if let Err(error) = &recovery {
                warn!("sentinel: {kind} containment incomplete ({error})");
            }
            append_sentinel_log(&format!(
                "\"event\":\"{kind}\",\"action\":\"stock\",\"reason\":\"ladder-exhausted\",\"recovery_ok\":{}",
                recovery.is_ok()
            ));
            write_sentinel_status(&format!(
                "{{\"ts\":\"{}\",\"event\":\"{kind}\",\"action\":\"stock\",\"strike\":3,\"target_mhz\":{target_mhz},\"failed_mv\":{failed_anchor_mv},\"recommendation\":\"3 falhas na mesma sessao: GPU em STOCK e perfil removido por seguranca. Recomendado: aplicar Deep Calm (validado) e re-forjar — os pontos ruins ja estao na blacklist e o novo Forge parte acima deles.\"}}",
                nidavellir_core::f2_observation::now_rfc3339()
            ));
            false
        }
    }
}

// Independent gates keep the Event Log watchdog responsive even if the GPU canary stalls.
// Shutdown closes admission first, then waits for both in-flight bodies before its stock reset.
static EVENT_ACTIVITY: std::sync::Mutex<()> = std::sync::Mutex::new(());
static CANARY_ACTIVITY: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn lock_reset_activity() -> Result<(
    std::sync::MutexGuard<'static, ()>, std::sync::MutexGuard<'static, ()>
), String> {
    let started = std::time::Instant::now();
    loop {
        match EVENT_ACTIVITY.try_lock() {
            Ok(event) => match CANARY_ACTIVITY.try_lock() {
                Ok(canary) => return Ok((event, canary)),
                Err(std::sync::TryLockError::Poisoned(_)) => return Err("Sentinel canary activity is poisoned".into()),
                Err(std::sync::TryLockError::WouldBlock) => {}
            },
            Err(std::sync::TryLockError::Poisoned(_)) => return Err("Sentinel Event Log activity is poisoned".into()),
            Err(std::sync::TryLockError::WouldBlock) => {}
        }
        if started.elapsed() >= std::time::Duration::from_secs(10) {
            return Err("Sentinel is still busy; learning was preserved. Retry the reset after it stops.".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

pub(crate) fn quiesce_for_shutdown() -> Result<(), String> {
    let _event = EVENT_ACTIVITY.lock().map_err(|_| "Sentinel Event Log activity is poisoned")?;
    let _canary = CANARY_ACTIVITY.lock().map_err(|_| "Sentinel canary activity is poisoned")?;
    Ok(())
}

fn watcher_event_is_new(last_handled: Option<&str>, start_floor: &str, newest: &str) -> bool {
    if last_handled == Some(newest) {
        return false;
    }
    newest.len() < 19 || newest[..19] >= start_floor[..]
}

/// Spawn the sentinel before any persisted profile can be re-applied. `initial_baseline` comes from
/// the checked startup query; the absolute floor is captured synchronously here (not inside the new
/// thread), so a TDR raised while the watcher thread is being scheduled is still a new event. The
/// function returns only after the watcher thread durably persists that seed/floor and acknowledges
/// it is active; callers must treat every error as a hard block on reapply and hardware work.
pub fn spawn(store: SafeLoopStore, initial_baseline: Option<String>) -> Result<(), String> {
    // Shared bump budget across BOTH layers: one automatic bump per service session, total.
    let bumps = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let watcher_started = nidavellir_core::f2_observation::now_rfc3339();
    let start_floor: String = watcher_started.chars().take(19).collect();
    let startup_state = SentinelWatcherStartupState {
        schema_version: 1,
        baseline_seed: initial_baseline.clone(),
        start_floor: start_floor.clone(),
    };
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);

    // Layer 1 — Event Log (TDR/BusReset; authoritative, zero GPU).
    {
        let store = store.clone();
        std::thread::Builder::new()
            .name("nidavellir-tdr-watcher".into())
            .spawn(move || {
            let startup = persist_watcher_startup_checked(&startup_state).and_then(|_| {
                match startup_state.baseline_seed.as_deref() {
                    Some(seed) => persist_baseline_checked(seed),
                    None => Ok(()),
                }
            });
            let startup_ok = startup.is_ok();
            if ready_tx.send(startup).is_err() || !startup_ok {
                return;
            }
            let mut last_handled = initial_baseline;
            // Audit #3: absolute time floor — even if the baseline query failed (Event Log not
            // ready at boot), a HISTORICAL event can never trigger an action. Both timestamp
            // formats share the "YYYY-MM-DDTHH:MM:SS" prefix, so a 19-char lexicographic compare
            // is a valid ordering at second granularity.
            info!(
                "sentinel: watching nvlddmkm-153 (baseline {last_handled:?}, floor {start_floor})"
            );
            loop {
                std::thread::sleep(std::time::Duration::from_millis(SENTINEL_POLL_MS));
                let _shutdown_guard = match EVENT_ACTIVITY.lock() { Ok(guard) => guard, Err(_) => return };
                if crate::shutdown::is_requested() { return; }
                let newest = match query_latest_tdr_event_checked() {
                    Ok(Some(event)) => event,
                    Ok(None) => continue,
                    Err(error) => {
                        warn!("sentinel: {error}; runtime GPU writes are latched until reboot");
                        mark_gpu_reboot_required("event-log-query-error:runtime");
                        continue;
                    }
                };
                if last_handled.as_deref() == Some(newest.as_str()) {
                    continue;
                }
                if !watcher_event_is_new(last_handled.as_deref(), &start_floor, &newest) {
                    match persist_baseline_checked(&newest) {
                        Ok(()) => last_handled = Some(newest),
                        Err(error) => {
                            warn!("sentinel: historical event cursor remains uncommitted ({error})")
                        }
                    }
                    continue;
                }
                // A driver reset can leave context/device state unreliable even after tuning was
                // returned to stock. Keep every GPU-mutating path closed until Windows reboots.
                mark_gpu_reboot_required(&newest);
                // Never mutate hardware while Forge owns it. Hand the event to the run owner: it
                // persists attribution when the boot flag is armed, otherwise records an explicitly
                // unattributed incident, then requests a cooperative stop.
                if crate::gpu_power_sweep::FORGE_ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
                    let recovery = record_active_forge_tdr_durably(&store, &newest);
                    append_sentinel_log("\"event\":\"tdr\",\"action\":\"forge-stop-requested\"");
                    match commit_event_after_recovery(&newest, recovery) {
                        Ok(()) => {
                            info!(
                                "sentinel: TDR event during active Forge — durable incident recorded; cooperative stop requested"
                            );
                            last_handled = Some(newest);
                        }
                        Err(error) => warn!(
                            "sentinel: active Forge TDR remains uncommitted ({error}); raw event will be retried"
                        ),
                    }
                    continue;
                }
                if crate::detector_lab::request_active_tdr_stop(&newest) {
                    info!(
                        "sentinel: TDR event during active Detector Lab — cooperative stop requested without concurrent reset or blacklist learning"
                    );
                    append_sentinel_log(
                        "\"event\":\"tdr\",\"action\":\"detector-lab-stop-requested\",\"learning\":\"none\"",
                    );
                    // Detector Lab owns stock recovery. Do not consume the raw event while its
                    // worker may still be unwinding; the next poll commits it only after ownership
                    // is released and checked stock/descriptor cleanup can run.
                    continue;
                }
                let diagnostic = match active_game_trace_diagnostic(&store) {
                    Ok(diagnostic) => diagnostic,
                    Err(error) => {
                        contain_unreadable_attribution_at_stock("tdr-diagnostic-boot-flag", &error);
                        warn!(
                            "sentinel: TDR attribution remains unreadable; raw event will be retried"
                        );
                        continue;
                    }
                };
                if let Some(point) = diagnostic {
                    let recovery = handle_game_trace_diagnostic_failure(&store, point, "tdr");
                    match commit_event_after_recovery(&newest, recovery) {
                        Ok(()) => last_handled = Some(newest),
                        Err(error) => warn!(
                            "sentinel: diagnostic TDR remains uncommitted ({error}); raw event will be retried"
                        ),
                    }
                    drop(_shutdown_guard);
                    std::thread::sleep(std::time::Duration::from_millis(SENTINEL_COOLDOWN_MS));
                    continue;
                }
                // A driver-reset TDR is terminal for this Windows boot. Never route it through the
                // adaptive bump path: recovered D3D/NVAPI state is not trusted for another write.
                let recovery = handle_live_tdr_reboot_required(&store, &newest);
                match commit_event_after_recovery(&newest, recovery) {
                    Ok(()) => last_handled = Some(newest.clone()),
                    Err(error) => {
                        warn!(
                            "sentinel: live TDR recovery remains uncommitted ({error}); raw event will be retried"
                        );
                        continue;
                    }
                }
                drop(_shutdown_guard);
                std::thread::sleep(std::time::Duration::from_millis(SENTINEL_COOLDOWN_MS));
                let _shutdown_guard = match EVENT_ACTIVITY.lock() { Ok(guard) => guard, Err(_) => return };
                if crate::shutdown::is_requested() { return; }
                // Audit #5: absorb the SAME episode's cascade residue (multiple 153s logged while
                // we were handling + cooling down) so it can never burn the 2nd strike — only a
                // genuinely NEW failure after this point counts against the session budget.
                match query_latest_tdr_event_checked() {
                    Ok(Some(residual)) => match persist_baseline_checked(&residual) {
                        Ok(()) => last_handled = Some(residual),
                        Err(error) => {
                            warn!("sentinel: cooldown event cursor remains uncommitted ({error})")
                        }
                    },
                    Ok(None) => {}
                    Err(error) => {
                        warn!("sentinel: cooldown query failed ({error}); GPU writes stay latched");
                        mark_gpu_reboot_required("event-log-query-error:cooldown");
                    }
                }
            }
            })
            .map_err(|error| format!("failed to spawn Sentinel Event Log watcher: {error}"))?;
    }
    await_watcher_startup(ready_rx)?;

    // Layer 2 — GPU canary (v17.3, ACTIVE): a ~700 ms TextureRop SELF-CHECK every 20 s, ONLY while
    // an undervolt is applied AND the GPU is under real load (>30% util — an idle card is never
    // woken). It detects returned non-stable verdicts on the binding TextureRop path.
    // The check runs synchronously on this dedicated thread so its GPU context is never detached:
    // if the driver stalls, no replacement worker is spawned and no reset races a still-running
    // canary. The independent Event Log layer handles a recovered TDR; boot reconciliation remains
    // the final net for a full machine wedge.
    std::thread::Builder::new()
        .name("nidavellir-silent-canary".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(SENTINEL_CANARY_POLL_MS));
            let _shutdown_guard = match CANARY_ACTIVITY.lock() { Ok(guard) => guard, Err(_) => return };
            if crate::shutdown::is_requested() { return; }
            let applied = match crate::gpu_apply::load_applied_checked() {
                Ok(profile) => profile.and_then(|p| p.undervolt),
                Err(error) => {
                    contain_unreadable_attribution_at_stock("canary-poll-applied-profile", &error);
                    continue;
                }
            };
            let diagnostic = match active_game_trace_diagnostic(&store) {
                Ok(diagnostic) => diagnostic,
                Err(error) => {
                    contain_unreadable_attribution_at_stock(
                        "canary-poll-diagnostic-boot-flag",
                        &error,
                    );
                    continue;
                }
            };
            if (applied.is_none() && diagnostic.is_none())
                || (store.is_boot_flag_armed() && diagnostic.is_none())
                // Audit #2: never spin a second GPU context or act while a forge run owns the card.
                || crate::gpu_power_sweep::FORGE_ACTIVE.load(std::sync::atomic::Ordering::SeqCst)
            {
                continue;
            }
            let under_load = nidavellir_core::nvml_gpu::read_nvidia_gpus_nvml()
                .first()
                .and_then(|g| g.utilization_pct)
                .is_some_and(|u| u >= SENTINEL_CANARY_MIN_UTIL_PCT);
            if !under_load {
                continue;
            }
            // Keep the context and call owned by this thread for their entire lifetime. Rust threads
            // cannot be cancelled safely; a recv_timeout around an inner worker would only abandon the
            // worker and let recovery mutate the GPU while that worker was still running.
            let verdict = {
                let _trace_guard = CanaryTraceGuard::begin();
                match nidavellir_gpu_stress::GpuCtx::new() {
                    Ok(ctx) => {
                        ctx.run_canary_texture_selfcheck(SENTINEL_CANARY_KERNEL_MS)
                            .result
                    }
                    // Context creation failed (driver busy/hiccup) — inconclusive, never a fallback.
                    Err(_) => continue,
                }
            };
            if canary_returned_failure(&verdict) {
                warn!("sentinel: texture canary detected {verdict:?} at the applied point");
                if let Some(point) = diagnostic {
                    let _ = handle_game_trace_diagnostic_failure(&store, point, "silent-canary");
                } else {
                    let n = bumps.load(std::sync::atomic::Ordering::SeqCst);
                    if handle_failure(&store, n, SENTINEL_SILENT_BUMP_BINS, "silent-canary") {
                        bumps.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                }
                drop(_shutdown_guard);
                std::thread::sleep(std::time::Duration::from_millis(SENTINEL_COOLDOWN_MS));
            }
        })
        .map_err(|error| format!("failed to spawn Sentinel silent canary: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_canary_planner_preserves_clock_and_stops_at_the_finite_budget() {
        let bins = [850, 856, 862, 868];
        // The canary may suggest a physical-bin lift, but runtime containment does not apply it;
        // exact-Apply qualification remains mandatory.
        assert_eq!(
            sentinel_decide(
                Some((1815, 843)),
                false,
                0,
                &bins,
                SENTINEL_SILENT_BUMP_BINS,
            ),
            SentinelAction::Bump {
                target_mhz: 1815,
                new_anchor_mv: 856
            }
        );
        assert_eq!(
            sentinel_decide(
                Some((1815, 862)),
                false,
                1,
                &[868, 875],
                SENTINEL_SILENT_BUMP_BINS,
            ),
            SentinelAction::Bump {
                target_mhz: 1815,
                new_anchor_mv: 875
            }
        );
        assert_eq!(
            sentinel_decide(
                Some((1815, 875)),
                false,
                2,
                &[881],
                SENTINEL_SILENT_BUMP_BINS,
            ),
            SentinelAction::Stock {
                target_mhz: 1815,
                failed_anchor_mv: 875
            }
        );
        assert_eq!(
            sentinel_decide(Some((1815, 843)), false, 0, &[], SENTINEL_SILENT_BUMP_BINS,),
            SentinelAction::Stock {
                target_mhz: 1815,
                failed_anchor_mv: 843
            }
        );
        assert_eq!(
            sentinel_decide(Some((1815, 843)), true, 0, &bins, SENTINEL_SILENT_BUMP_BINS,),
            SentinelAction::ForgeOwns
        );
        assert_eq!(
            sentinel_decide(None, false, 0, &bins, SENTINEL_SILENT_BUMP_BINS),
            SentinelAction::Ignore
        );
    }

    #[test]
    fn failed_recovery_never_advances_the_event_cursor() {
        let base = std::env::temp_dir().join(format!(
            "nid-sentinel-cursor-{}-{}",
            std::process::id(),
            now_epoch_ms()
        ));
        let path = base.join("baseline.txt");
        let error = commit_event_after_recovery_at(
            &path,
            "2026-08-14T21:00:00.000000000Z",
            Err("descriptor cleanup failed".into()),
        )
        .unwrap_err();
        assert!(error.contains("descriptor cleanup failed"));
        assert!(!path.exists());

        commit_event_after_recovery_at(&path, "2026-08-14T21:00:00.000000000Z", Ok(())).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "2026-08-14T21:00:00.000000000Z"
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn baseline_persist_failure_never_commits_the_in_memory_cursor() {
        let parent_file = std::env::temp_dir().join(format!(
            "nid-sentinel-baseline-parent-{}-{}",
            std::process::id(),
            now_epoch_ms()
        ));
        std::fs::write(&parent_file, b"not-a-directory").unwrap();
        let path = parent_file.join("baseline.txt");
        let error = commit_event_after_recovery_at(&path, "2026-08-14T21:00:01.000000000Z", Ok(()))
            .unwrap_err();
        assert!(error.contains("baseline"), "{error}");
        assert!(!path.exists());
        let _ = std::fs::remove_file(parent_file);
    }

    #[test]
    fn transient_wevtutil_failure_is_not_misread_as_an_empty_event_log() {
        let error = parse_wevtutil_query(
            "nvlddmkm-153",
            false,
            b"",
            b"The RPC server is unavailable",
            parse_event_system_time,
        )
        .unwrap_err();
        assert!(error.contains("RPC server is unavailable"), "{error}");

        let empty = parse_wevtutil_query(
            "nvlddmkm-153",
            true,
            b"  \r\n",
            b"",
            parse_event_system_time,
        )
        .unwrap();
        assert_eq!(empty, None);

        let malformed = parse_wevtutil_query(
            "nvlddmkm-153",
            true,
            b"<Event><System></System></Event>",
            b"",
            parse_event_system_time,
        )
        .unwrap_err();
        assert!(malformed.contains("unreadable event XML"), "{malformed}");
    }

    #[test]
    fn watcher_seed_catches_a_tdr_raised_while_reapply_is_starting() {
        let seed = "2026-08-14T21:00:00.000000000Z";
        let watcher_floor = "2026-08-14T21:05:00";
        let during_reapply = "2026-08-14T21:05:00.500000000Z";
        assert!(watcher_event_is_new(
            Some(seed),
            watcher_floor,
            during_reapply
        ));
        assert!(!watcher_event_is_new(Some(seed), watcher_floor, seed));
        assert!(!watcher_event_is_new(
            None,
            watcher_floor,
            "2026-08-14T20:59:59.999999900Z"
        ));
    }

    #[test]
    fn watcher_handshake_requires_durable_seed_and_floor() {
        let base = std::env::temp_dir().join(format!(
            "nid-sentinel-handshake-{}-{}",
            std::process::id(),
            now_epoch_ms()
        ));
        let path = base.join("watcher.json");
        let state = SentinelWatcherStartupState {
            schema_version: 1,
            baseline_seed: Some("2026-08-14T21:00:00.000000000Z".into()),
            start_floor: "2026-08-14T21:05:00".into(),
        };
        let expected = state.clone();
        let path_for_thread = path.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let result = persist_watcher_startup_at(&path_for_thread, &state);
            ready_tx.send(result).unwrap();
        });
        await_watcher_startup(ready_rx).unwrap();
        thread.join().unwrap();
        let persisted: SentinelWatcherStartupState =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(persisted, expected);

        let parent_file = base.join("not-a-directory");
        std::fs::write(&parent_file, "blocks child persistence").unwrap();
        let (failed_tx, failed_rx) = std::sync::mpsc::sync_channel(1);
        let failed_path = parent_file.join("watcher.json");
        std::thread::spawn(move || {
            failed_tx
                .send(persist_watcher_startup_at(&failed_path, &expected))
                .unwrap();
        })
        .join()
        .unwrap();
        assert!(await_watcher_startup(failed_rx)
            .unwrap_err()
            .contains("watcher startup"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn legacy_worker_write_guard_is_fail_closed() {
        let clear = || legacy_gpu_write_guard_decision(None, Ok(()), Ok((false, false)), Ok(()));
        assert_eq!(clear(), Ok(()));
        assert!(
            legacy_gpu_write_guard_decision(Some("tdr"), Ok(()), Ok((false, false)), Ok(()))
                .unwrap_err()
                .contains("reboot Windows")
        );
        assert!(legacy_gpu_write_guard_decision(
            None,
            Err("boot flag occupied".into()),
            Ok((false, false)),
            Ok(())
        )
        .unwrap_err()
        .contains("occupied"));
        assert!(
            legacy_gpu_write_guard_decision(None, Ok(()), Ok((true, false)), Ok(()))
                .unwrap_err()
                .contains("pending Forge incident")
        );
        assert!(
            legacy_gpu_write_guard_decision(None, Ok(()), Ok((false, true)), Ok(()))
                .unwrap_err()
                .contains("Safe Mode")
        );
        assert!(legacy_gpu_write_guard_decision(
            None,
            Ok(()),
            Err("record unreadable".into()),
            Ok(())
        )
        .unwrap_err()
        .contains("record unreadable"));
        assert!(legacy_gpu_write_guard_decision(
            None,
            Ok(()),
            Ok((false, false)),
            Err("ledger unreadable".into())
        )
        .unwrap_err()
        .contains("ledger unreadable"));
    }

    #[test]
    fn legacy_boot_flag_guard_requires_exact_transaction_owner() {
        let base = std::env::temp_dir().join(format!(
            "nid-sentinel-owner-{}-{}",
            std::process::id(),
            now_epoch_ms()
        ));
        let store = SafeLoopStore::new(&base);
        assert_eq!(legacy_boot_flag_guard(&store, None), Ok(()));

        let owned = BootFlag::new(TuningPoint::stock(), "owned");
        store.arm_boot_flag(&owned).unwrap();
        assert!(legacy_boot_flag_guard(&store, None)
            .unwrap_err()
            .contains("already armed"));
        assert_eq!(legacy_boot_flag_guard(&store, Some(&owned)), Ok(()));
        let other = BootFlag::new(TuningPoint::stock(), "other");
        assert!(legacy_boot_flag_guard(&store, Some(&other))
            .unwrap_err()
            .contains("no longer owns"));
        assert!(store.clear_boot_flag_if_matches(&owned).unwrap());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn legacy_guard_checks_live_boot_record_and_condemnation_files() {
        let base = std::env::temp_dir().join(format!(
            "nid-sentinel-guard-files-{}-{}",
            std::process::id(),
            now_epoch_ms()
        ));
        let store = SafeLoopStore::new(&base);
        assert_eq!(legacy_gpu_write_guard(&store), Ok(()));

        let mut record = nidavellir_core::safe_loop::SafeLoopRecord {
            safe_mode: true,
            ..Default::default()
        };
        store.save_record(&record).unwrap();
        assert!(legacy_gpu_write_guard(&store)
            .unwrap_err()
            .contains("Safe Mode"));
        record.safe_mode = false;
        store.save_record(&record).unwrap();

        let occupied = BootFlag::new(TuningPoint::stock(), "occupied");
        store.arm_boot_flag(&occupied).unwrap();
        assert!(legacy_gpu_write_guard(&store)
            .unwrap_err()
            .contains("already armed"));
        assert!(store.clear_boot_flag_if_matches(&occupied).unwrap());

        std::fs::write(
            base.join(nidavellir_core::condemnation::CONDEMNATION_LEDGER_FILE),
            "{ malformed jsonl\n",
        )
        .unwrap();
        assert!(legacy_gpu_write_guard(&store)
            .unwrap_err()
            .contains("condemnation ledger"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn corrupt_tdr_attribution_is_not_downgraded_to_missing() {
        let base = std::env::temp_dir().join(format!(
            "nid-sentinel-attribution-{}-{}",
            std::process::id(),
            now_epoch_ms()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let store = SafeLoopStore::new(&base);
        std::fs::write(store.boot_flag_path(), "{ truncated").unwrap();

        assert!(active_game_trace_diagnostic(&store)
            .unwrap_err()
            .contains("unreadable"));
        assert!(armed_failure_pair_checked(&store)
            .unwrap_err()
            .contains("unreadable"));
        assert!(resolve_live_tdr_failure_pair(
            Ok(Some((1860, 900))),
            Err("corrupt boot flag".into())
        )
        .unwrap_err()
        .contains("corrupt boot flag"));
        assert!(
            resolve_live_tdr_failure_pair(Err("corrupt applied profile".into()), Ok(None))
                .unwrap_err()
                .contains("corrupt applied profile")
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn failed_ledger_append_keeps_the_field_tdr_uncommitted() {
        let base = std::env::temp_dir().join(format!(
            "nid-sentinel-ledger-failure-{}-{}",
            std::process::id(),
            now_epoch_ms()
        ));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(base.join(nidavellir_core::condemnation::CONDEMNATION_LEDGER_FILE))
            .unwrap();
        let store = SafeLoopStore::new(&base);
        let error = persist_field_failure_for_gpu(
            &store,
            "gpu-test",
            1860,
            900,
            "forced ledger failure".into(),
        )
        .unwrap_err();
        assert!(error.contains("was not durable"), "{error}");
        assert!(
            !nidavellir_core::condemnation::CondemnationLedger::new(&base)
                .condemned_pairs("gpu-test")
                .rigid
                .contains(&(1860, 900))
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn canary_only_acts_on_a_returned_failure_verdict() {
        use nidavellir_core::gpu_sweep::StabilityResult;

        assert!(!canary_returned_failure(&StabilityResult::Stable));
        assert!(canary_returned_failure(&StabilityResult::SilentError));
        assert!(canary_returned_failure(&StabilityResult::Unstable));
        assert!(canary_returned_failure(&StabilityResult::Crash));
    }

    #[test]
    fn cross_layer_recovery_claim_is_atomic_and_respects_the_dedup_window() {
        let last = std::sync::atomic::AtomicU64::new(0);
        assert!(claim_action_epoch(&last, 1_000));
        assert!(!claim_action_epoch(&last, 1_000));
        assert!(!claim_action_epoch(&last, 1_000 + CROSS_LAYER_DEDUP_S - 1));
        assert!(claim_action_epoch(&last, 1_000 + CROSS_LAYER_DEDUP_S));
    }

    #[test]
    fn parses_wevtutil_xml_system_time() {
        let xml = "<Event><System><TimeCreated SystemTime='2026-07-09T07:43:40.123456700Z'/></System></Event>";
        assert_eq!(
            parse_event_system_time(xml).as_deref(),
            Some("2026-07-09T07:43:40.123456700Z")
        );
        assert_eq!(parse_event_system_time("no events"), None);
    }

    #[test]
    fn reboot_guard_only_accepts_tdrs_from_the_current_boot() {
        let boot_ms = event_epoch_ms("2026-07-22T22:00:00.000000000Z").unwrap();
        assert!(event_is_from_current_boot(
            "2026-07-22T23:07:07.090086400Z",
            boot_ms
        ));
        assert!(!event_is_from_current_boot(
            "2026-07-21T23:07:07.090086400Z",
            boot_ms
        ));
        assert!(!event_is_from_current_boot("invalid", boot_ms));
    }

    #[test]
    fn bugcheck_attribution_requires_this_applied_session() {
        let applied = "2026-07-18T21:50:00.250000000+00:00";
        assert!(!event_belongs_to_applied_session(
            "2026-07-18T21:49:59.999999900Z",
            Some(applied)
        ));
        assert!(!event_belongs_to_applied_session(
            "2026-07-18T21:50:00.100000000Z",
            Some(applied)
        ));
        assert!(event_belongs_to_applied_session(
            "2026-07-18T21:50:00.900000000Z",
            Some(applied)
        ));
        assert!(event_belongs_to_applied_session(
            "2026-07-18T21:53:37.000000000Z",
            Some(applied)
        ));
        assert!(!event_belongs_to_applied_session(
            "2026-07-18T21:53:37.000000000Z",
            None
        ));
    }
}
