//! Apply a chosen GPU profile to the hardware and **persist** it so the Core
//! Service re-applies it on every boot (GPU offsets are volatile). Integrated
//! with the Safe Loop: the boot-flag is armed around an apply, so a profile
//! that crashes the machine is NOT re-applied on the next boot.
//!
//! Windows-only (NVAPI). Elsewhere these are inert.

use std::path::{Path, PathBuf};

use nidavellir_core::gpu_sweep::VfPoint;
use nidavellir_core::safe_loop::{default_data_dir, BootFlag, SafeLoopStore, TuningPoint};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// An F2 anchored-undervolt apply descriptor (persisted so apply-on-boot re-derives the same anchored
/// curve from the LIVE VF table). When present on an [`AppliedProfile`], the applied profile is an F2
/// undervolt and `core` is status metadata only — apply routes to [`apply_anchored_undervolt`], not
/// the F1 ceiling.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UndervoltApply {
    /// Target clock to hold (anchor raised to this; higher-voltage bins capped down to it).
    pub target_mhz: u32,
    /// The VF-table bin voltage to anchor at (the deterministic apply key, `vf_table_voltage_mv`).
    pub anchor_mv: u32,
    /// Exact physical GPU whose v29 matrix qualified this pair.
    #[serde(default)]
    pub gpu_key: Option<String>,
    /// Run that contains the complete exact-Apply matrix persisted in f2_observations.jsonl.
    #[serde(default)]
    pub qualification_run_id: Option<String>,
    /// Contract stamped when the descriptor was explicitly applied.
    #[serde(default)]
    pub qualification_contract_version: Option<u32>,
}

/// The profile currently applied (persisted to disk for apply-on-boot).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppliedProfile {
    pub label: String,
    pub core: Option<VfPoint>,
    pub mem_offset_mhz: Option<i32>,
    /// UTC timestamp of the most recent successful hardware apply. Startup recovery uses this to
    /// attribute a WER bugcheck only when it happened during this persisted profile's own session.
    /// Legacy payloads intentionally remain unattributable instead of importing old crashes.
    #[serde(default)]
    pub applied_at: Option<String>,
    /// F2 anchored-undervolt descriptor. `Some` ⇒ this profile is an F2 undervolt (apply routes to the
    /// anchored writer; `core` remains status metadata). `#[serde(default)]` ⇒ legacy F1 payloads load
    /// as `None`.
    #[serde(default)]
    pub undervolt: Option<UndervoltApply>,
}

fn qualified_undervolt_descriptor<'a>(
    descriptor: &'a UndervoltApply,
    current_gpu_key: &str,
) -> Result<(&'a str, &'a str, u32), String> {
    let gpu_key = descriptor
        .gpu_key
        .as_deref()
        .ok_or_else(|| "persisted F2 descriptor has no exact GPU identity".to_string())?;
    let run_id = descriptor
        .qualification_run_id
        .as_deref()
        .ok_or_else(|| "persisted F2 descriptor has no qualification run identity".to_string())?;
    let contract = descriptor.qualification_contract_version.ok_or_else(|| {
        "persisted F2 descriptor has no exact-Apply contract identity".to_string()
    })?;
    if gpu_key != current_gpu_key {
        return Err(format!(
            "persisted F2 descriptor belongs to another GPU ({gpu_key})"
        ));
    }
    if contract != nidavellir_core::f2_observation::F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION {
        return Err(format!(
            "persisted F2 descriptor contract v{contract} is not current exact-Apply v{}",
            nidavellir_core::f2_observation::F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION
        ));
    }
    Ok((gpu_key, run_id, contract))
}

fn applied_path() -> PathBuf {
    default_data_dir().join("gpu_applied.json")
}

fn load_applied_at(path: &Path) -> Result<Option<AppliedProfile>, String> {
    let data = match std::fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "read persisted GPU profile {}: {error}",
                path.display()
            ))
        }
    };
    serde_json::from_str(data.trim_start_matches('\u{feff}'))
        .map(Some)
        .map_err(|error| format!("invalid persisted GPU profile {}: {error}", path.display()))
}

pub fn load_applied_checked() -> Result<Option<AppliedProfile>, String> {
    load_applied_at(&applied_path())
}

pub fn load_applied() -> Option<AppliedProfile> {
    load_applied_checked().ok().flatten()
}

fn save_applied_at(path: &std::path::Path, profile: &AppliedProfile) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "persisted GPU profile path has no parent".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create GPU profile directory: {error}"))?;
    let json = serde_json::to_vec_pretty(profile)
        .map_err(|error| format!("serialize persisted GPU profile: {error}"))?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = parent.join(format!(".gpu_applied-{}-{unique}.tmp", std::process::id()));
    let write_result = (|| -> std::io::Result<()> {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(&json)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    write_result.map_err(|error| format!("persist GPU profile atomically: {error}"))
}

fn save_applied(profile: &AppliedProfile) -> Result<(), String> {
    save_applied_at(&applied_path(), profile)
}

fn clear_applied_path(path: &std::path::Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(crate) fn clear_applied_checked() -> Result<(), String> {
    clear_applied_path(&applied_path())
        .map_err(|error| format!("failed to clear persisted GPU profile: {error}"))
}

/// Remove positive active learning for Soft Reset: validated F2 observations are archived/filtered
/// while every negative or inconclusive line remains. Legacy single-clock knowledge is filtered too:
/// accumulated positive points are removed, but its silent-error/TDR/reboot boundaries remain.
/// Any parse or I/O failure aborts explicitly; missing files are success.
///
/// Deliberately does NOT touch `gpu_applied.json`, the boot-flag, `safe_loop.json`, or
/// `forge_state.json`; those are handled by [`reset`] / the caller so each concern stays explicit.
///
/// Soft Reset preserves the condemnation ledger. Only the separately confirmed Full Reset
/// forgets negative history; neither Clean Run nor stock-only recovery does so.
pub fn clear_all_learning() -> Result<(), String> {
    let base = default_data_dir();
    clear_all_learning_at(&base)
}

const FULL_RESET_PENDING: &str = "full_reset_pending";

pub(crate) fn full_reset_pending(base: &Path) -> bool {
    // Treat unreadable metadata as pending; a failed reset must never permit partial reuse.
    !matches!(std::fs::symlink_metadata(base.join(FULL_RESET_PENDING)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}

/// Caller holds the GPU lease and Sentinel gates, with workers stopped and stock confirmed.
/// The persistent marker blocks tuning across interruption; an explicit retry is idempotent.
pub(crate) fn forget_all_gpu_learning(store: &SafeLoopStore) -> Result<(), String> {
    if store.read_boot_flag_checked().map_err(|e| e.to_string())?.is_some() {
        return Err("Full Reset requires confirmed stock recovery and a disarmed Safe Loop".into());
    }
    let base = store.base_dir();
    std::fs::create_dir_all(base).map_err(|e| e.to_string())?;
    let marker = base.join(FULL_RESET_PENDING);
    std::fs::File::create(&marker).and_then(|file| file.sync_all())
        .map_err(|e| format!("Cannot begin Full Reset: {e}"))?;
    // Exact active stores plus generated learning archives only. Operational watchdog baselines
    // and development authorization/audit are not GPU learning and must not be reset here.
    let result = (|| -> std::io::Result<()> {
        for name in ["gpu_applied.json", "gpu_knowledge.json", "f2_observations.jsonl",
            "condemnation_ledger.jsonl", "forge_state.json", "safe_loop.json"] {
            clear_applied_path(&base.join(name))?;
        }
        clear_learning_archives(base)?;
        std::fs::remove_file(&marker)
    })();
    result.map_err(|e| format!("Full Reset incomplete; tuning remains blocked. Retry Full Reset: {e}"))
}

fn clear_learning_archives(base: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(base)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if (name.starts_with("f2_observations.pre-full-reset-") && name.ends_with(".jsonl"))
            || (name.starts_with("gpu_knowledge.pre-full-reset-") && name.ends_with(".json")) {
            std::fs::remove_file(entry.path())?;
        }
    }
    let archive = base.join("forge-archive");
    match std::fs::symlink_metadata(&archive) {
        Ok(metadata) => {
            // Never recursively traverse a redirected archive outside the data directory.
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(std::io::Error::other("Forge archive is a reparse point"));
                }
            }
            if metadata.file_type().is_symlink() {
                return Err(std::io::Error::other("Forge archive is a symbolic link"));
            }
            std::fs::remove_dir_all(archive)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn clear_all_learning_at(base: &Path) -> Result<(), String> {
    clear_all_learning_at_with(base, rewrite_legacy_gpu_knowledge_preserving_negatives)?;
    if base.exists() {
        clear_learning_archives(base).map_err(|e| format!("Soft Reset could not clear learning archives: {e}"))?;
    }
    Ok(())
}

fn clear_all_learning_at_with(
    base: &Path,
    rewrite_legacy: impl FnOnce(&Path) -> Result<Option<FullResetReplacement>, String>,
) -> Result<(), String> {
    let f2_path = base.join(nidavellir_core::f2_observation::F2_OBSERVATIONS_FILE);
    let legacy_path = base.join("gpu_knowledge.json");

    // Validate both active sources before replacing either one. Otherwise a corrupt legacy file
    // could be discovered only after the F2 JSONL had already lost its positive observations.
    validate_f2_learning(&f2_path)?;
    validate_legacy_gpu_knowledge(&legacy_path)?;

    let f2_replacement = rewrite_f2_learning_preserving_negatives(&f2_path)?;
    match rewrite_legacy(&legacy_path) {
        Ok(_) => Ok(()),
        Err(legacy_error) => {
            let Some(replacement) = f2_replacement else {
                return Err(legacy_error);
            };
            match rollback_full_reset_replacement(&replacement) {
                Ok(()) => Err(format!(
                    "legacy GPU knowledge reset failed ({legacy_error}); F2 learning at {} was restored byte-for-byte",
                    replacement.path.display()
                )),
                Err(rollback_error) => Err(format!(
                    "legacy GPU knowledge reset failed ({legacy_error}); F2 rollback also failed ({rollback_error}). Active path: {}; original archive: {}",
                    replacement.path.display(),
                    replacement.archive.display()
                )),
            }
        }
    }
}

#[derive(Debug)]
struct FullResetReplacement {
    path: PathBuf,
    archive: PathBuf,
    original: Vec<u8>,
}

/// Validation-only mirror of the private legacy knowledge schema in `gpu_power_sweep`. Full Reset
/// edits the original JSON value (so unknown forward-compatible fields survive), but first requires
/// every current field and every point payload to deserialize strictly under the known schema.
#[allow(dead_code)]
#[derive(Deserialize)]
struct LegacyGpuKnowledgeSchema {
    gpu_key: String,
    target_clock_mhz: u32,
    boundary: LegacyBoundaryKnowledgeSchema,
    points: std::collections::BTreeMap<i32, LegacyPointStatSchema>,
    schema_version: u32,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct LegacyBoundaryKnowledgeSchema {
    highest_clean: i32,
    lowest_silent_error: Option<i32>,
    lowest_tdr: Option<i32>,
    lowest_reboot: Option<i32>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct LegacyPointStatSchema {
    trials: u32,
    failures: u32,
    worst_severity: LegacyFailSeveritySchema,
    stable_trials: u32,
    clock_mhz_sum: u64,
    power_w_sum: f64,
    voltage_mv_sum: u64,
}

#[allow(dead_code)]
#[derive(Deserialize)]
enum LegacyFailSeveritySchema {
    None,
    SilentError,
    Tdr,
    Reboot,
}

fn validate_legacy_gpu_knowledge(path: &Path) -> Result<(), String> {
    let data = match std::fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "read legacy GPU knowledge {}: {error}",
                path.display()
            ))
        }
    };
    let value: serde_json::Value = serde_json::from_str(data.trim_start_matches('\u{feff}'))
        .map_err(|error| {
            format!(
                "invalid legacy GPU knowledge JSON {}: {error}",
                path.display()
            )
        })?;
    serde_json::from_value::<LegacyGpuKnowledgeSchema>(value)
        .map(|_| ())
        .map_err(|error| {
            format!(
                "invalid legacy GPU knowledge schema {}: {error}",
                path.display()
            )
        })
}

/// Clear only positive legacy knowledge while retaining all severity boundaries and identity fields.
/// The original is archived and a synced same-directory temporary is installed; parse/schema errors
/// happen before staging, and an install error attempts to restore the exact original.
fn rewrite_legacy_gpu_knowledge_preserving_negatives(
    path: &Path,
) -> Result<Option<FullResetReplacement>, String> {
    let data = match std::fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "read legacy GPU knowledge {}: {error}",
                path.display()
            ))
        }
    };
    let mut value: serde_json::Value = serde_json::from_str(data.trim_start_matches('\u{feff}'))
        .map_err(|error| {
            format!(
                "invalid legacy GPU knowledge JSON {}: {error}",
                path.display()
            )
        })?;
    let schema: LegacyGpuKnowledgeSchema =
        serde_json::from_value(value.clone()).map_err(|error| {
            format!(
                "invalid legacy GPU knowledge schema {}: {error}",
                path.display()
            )
        })?;
    if schema.boundary.highest_clean == 0 && schema.points.is_empty() {
        return Ok(None);
    }

    let root = value
        .as_object_mut()
        .expect("typed legacy knowledge validation guarantees an object");
    root.get_mut("boundary")
        .and_then(serde_json::Value::as_object_mut)
        .expect("typed legacy knowledge validation guarantees a boundary object")
        .insert("highest_clean".into(), serde_json::Value::from(0));
    root.insert(
        "points".into(),
        serde_json::Value::Object(Default::default()),
    );
    let filtered = serde_json::to_vec_pretty(&value)
        .map_err(|error| format!("serialize filtered legacy GPU knowledge: {error}"))?;

    let parent = path
        .parent()
        .ok_or_else(|| "legacy GPU knowledge path has no parent".to_string())?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = parent.join(format!(
        ".gpu_knowledge-full-reset-{}-{unique}.tmp",
        std::process::id()
    ));
    let archive = parent.join(format!("gpu_knowledge.pre-full-reset-{unique}.json"));
    let write_result = (|| -> std::io::Result<()> {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(&filtered)?;
        file.write_all(b"\n")?;
        file.sync_all()
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("stage filtered legacy GPU knowledge: {error}"));
    }

    if let Err(error) = install_full_reset_replacement(path, &temp, &archive) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!(
            "install filtered legacy GPU knowledge {}: {error}",
            path.display()
        ));
    }
    Ok(Some(FullResetReplacement {
        path: path.to_path_buf(),
        archive,
        original: data.into_bytes(),
    }))
}

/// Install a staged Full Reset rewrite while retaining the exact original as `archive`.
/// `ReplaceFileW` is the Windows primitive that atomically swaps the canonical file and creates its
/// backup in one operation. The fallback keeps the existing rollback transaction for non-Windows
/// unit-test/tooling builds.
#[cfg(windows)]
fn install_full_reset_replacement(path: &Path, temp: &Path, archive: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH};

    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let temp: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let archive: Vec<u16> = archive.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: all three NUL-terminated UTF-16 buffers live through the call and name closed,
    // same-directory files.
    unsafe {
        ReplaceFileW(
            PCWSTR(path.as_ptr()),
            PCWSTR(temp.as_ptr()),
            PCWSTR(archive.as_ptr()),
            REPLACEFILE_WRITE_THROUGH,
            None,
            None,
        )
    }
    .map_err(|error| format!("atomic ReplaceFileW failed: {error}"))
}

#[cfg(not(windows))]
fn install_full_reset_replacement(path: &Path, temp: &Path, archive: &Path) -> Result<(), String> {
    std::fs::rename(path, archive).map_err(|error| format!("archive original: {error}"))?;
    if let Err(error) = std::fs::rename(temp, path) {
        return Err(match std::fs::rename(archive, path) {
            Ok(()) => format!("install replacement failed and was rolled back: {error}"),
            Err(rollback_error) => format!(
                "install replacement failed ({error}); rollback also failed ({rollback_error}); archive retained at {}",
                archive.display()
            ),
        });
    }
    Ok(())
}

fn rollback_full_reset_replacement(replacement: &FullResetReplacement) -> Result<(), String> {
    let parent = replacement
        .path
        .parent()
        .ok_or_else(|| "Full Reset replacement path has no parent".to_string())?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let filtered_backup = parent.join(format!(
        ".full-reset-rollback-{}-{unique}.tmp",
        std::process::id()
    ));
    install_full_reset_replacement(&replacement.path, &replacement.archive, &filtered_backup)
        .map_err(|error| {
            format!(
                "could not restore {} from {}: {error}",
                replacement.path.display(),
                replacement.archive.display()
            )
        })?;

    let restored = std::fs::read(&replacement.path).map_err(|error| {
        format!(
            "restored {} but could not verify it: {error}; displaced filtered file is {}",
            replacement.path.display(),
            filtered_backup.display()
        )
    })?;
    if restored != replacement.original {
        return Err(format!(
            "restored {} does not match the original bytes; displaced filtered file is {}",
            replacement.path.display(),
            filtered_backup.display()
        ));
    }
    match std::fs::remove_file(&filtered_backup) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "original {} was restored and verified, but rollback backup {} could not be removed: {error}",
            replacement.path.display(),
            filtered_backup.display()
        )),
    }
}

fn validate_f2_learning(path: &Path) -> Result<(), String> {
    let data = match std::fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("read F2 learning {}: {error}", path.display())),
    };
    for (index, raw_line) in data.lines().enumerate() {
        let line = if index == 0 {
            raw_line.trim_start_matches('\u{feff}')
        } else {
            raw_line
        };
        if line.trim().is_empty() {
            continue;
        }
        serde_json::from_str::<nidavellir_core::f2_observation::F2Observation>(line).map_err(
            |error| {
                format!(
                    "invalid F2 learning {} line {}: {error}",
                    path.display(),
                    index + 1
                )
            },
        )?;
    }
    Ok(())
}

/// Full Reset forgets only positive F2 observations. Every failure, refusal, power-bound result and
/// inconclusive remains active safety evidence. The original JSONL is archived and the filtered file
/// is installed in one atomic replacement transaction; malformed input aborts without mutation.
fn rewrite_f2_learning_preserving_negatives(
    path: &Path,
) -> Result<Option<FullResetReplacement>, String> {
    let data = match std::fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("read F2 learning {}: {error}", path.display())),
    };
    let mut retained = Vec::new();
    let mut positive_count = 0usize;
    for (index, raw_line) in data.lines().enumerate() {
        let line = if index == 0 {
            raw_line.trim_start_matches('\u{feff}')
        } else {
            raw_line
        };
        if line.trim().is_empty() {
            continue;
        }
        let observation: nidavellir_core::f2_observation::F2Observation =
            serde_json::from_str(line).map_err(|error| {
                format!(
                    "invalid F2 learning {} line {}: {error}",
                    path.display(),
                    index + 1
                )
            })?;
        if observation.outcome.is_validated() {
            positive_count += 1;
        } else {
            retained.push(line.to_string());
        }
    }
    if positive_count == 0 {
        return Ok(None);
    }

    let parent = path
        .parent()
        .ok_or_else(|| "F2 learning path has no parent".to_string())?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("f2_observations");
    let temp = parent.join(format!(".{stem}-full-reset-{unique}.tmp"));
    let archive = parent.join(format!("{stem}.pre-full-reset-{unique}.jsonl"));
    let write_result = (|| -> std::io::Result<()> {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        for line in &retained {
            file.write_all(line.as_bytes())?;
            file.write_all(b"\n")?;
        }
        file.sync_all()
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("stage filtered F2 learning: {error}"));
    }

    if let Err(error) = install_full_reset_replacement(path, &temp, &archive) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!(
            "install filtered F2 learning {}: {error}",
            path.display()
        ));
    }
    Ok(Some(FullResetReplacement {
        path: path.to_path_buf(),
        archive,
        original: data.into_bytes(),
    }))
}

#[cfg(windows)]
fn curve_freq_at(voltage_mv: u32) -> Option<u32> {
    let c = nidavellir_gpu_nvapi::read_curve().ok()?;
    c.points
        .iter()
        .filter(|p| p.voltage_mv <= voltage_mv)
        .max_by_key(|p| p.voltage_mv)
        .map(|p| p.freq_mhz)
}

/// Realize a core V/F point by FLATTENING the curve. We do NOT hard-lock the
/// voltage: under a heavy (≈power-cap) game load a hard voltage lock removes the
/// card's power management and TDRs.
///
/// Preferred path (Pascal+ on a modern driver): the **elastic VF ceiling** — set
/// per-point frequency offsets so every curve point at or above `point.voltage_mv`
/// is flattened to `point.freq_mhz`, leaving lower-voltage points free. The GPU
/// keeps full power-management elasticity (it can still drop clocks/voltage on
/// light load) yet never boosts past the validated point — the true Afterburner
/// curve-flatten. Fallback (older driver / no modern curve API): a global clock
/// offset + an NVML max-clock cap (less elastic, but works everywhere).
/// Choose the VF-ceiling threshold for an apply. The profile's `voltage_mv` is a
/// MEASURED dwell value (a sparse sensor max), NOT a deterministic curve point, so
/// we snap it to a real VF-table bin (the lowest table voltage at/above it) — the
/// ceiling must land on an actual curve voltage (see `decisions.md`: voltage split).
/// Returns `(ceiling_mv, legacy_fallback)`; `legacy_fallback` is true only when no
/// bin could be resolved (empty/unknown curve) and the raw measured value is used as
/// a last resort. Pure + testable without hardware.
fn choose_ceiling_mv(curve: &[(usize, u32, u32)], measured_mv: u32) -> (u32, bool) {
    match nidavellir_gpu_nvapi::nearest_vf_bin_at_or_above(curve, measured_mv) {
        Some((_, table_mv)) => (table_mv, false),
        None => (measured_mv, true),
    }
}

#[cfg(windows)]
fn ensure_boot_transaction_is_clear(store: &SafeLoopStore) -> Result<(), String> {
    match store.read_boot_flag_checked() {
        Ok(None) => Ok(()),
        Ok(Some(flag)) => Err(format!(
            "Safe Loop transaction {} ({}) is still armed; GPU write refused",
            if flag.transaction_id.is_empty() {
                "legacy/unknown"
            } else {
                &flag.transaction_id
            },
            flag.phase
        )),
        Err(error) => Err(format!(
            "Safe Loop boot flag is unreadable; GPU write refused: {error}"
        )),
    }
}

/// Shared last-moment guard for non-forge hardware writers. With a physical V/F pair it consumes
/// the same field/ledger/TDR-cone policy as Forge; without one it still validates the current-boot
/// latch plus strict Safe Loop and durable-ledger readability.
#[cfg(windows)]
pub(crate) fn generic_hardware_write_preflight(
    store: &SafeLoopStore,
    vf_pair: Option<(u32, u32)>,
) -> Result<(), String> {
    if let Some(event) = crate::tdr_sentinel::reboot_required_event() {
        return Err(format!(
            "GPU reboot is required after driver reset {event}; no GPU write is allowed in this boot"
        ));
    }
    ensure_boot_transaction_is_clear(store)?;
    if let Some((target_mhz, anchor_mv)) = vf_pair {
        return crate::gpu_power_sweep::f2_hardware_apply_preflight(store, target_mhz, anchor_mv);
    }
    let record = store
        .load_record_checked()
        .map_err(|error| format!("Safe Loop record is unreadable; GPU write refused: {error}"))?;
    if record.pending_forge_incident.is_some() {
        return Err(
            "Forge recovery requires explicit operator acknowledgement before any GPU write".into(),
        );
    }
    if record.safe_mode {
        return Err("Safe Mode is active; GPU write refused".into());
    }
    nidavellir_core::condemnation::CondemnationLedger::new(store.base_dir())
        .load_all_checked()
        .map_err(|error| format!("durable condemnation ledger is unreadable: {error}"))?;
    Ok(())
}

/// Profile Apply is the sole exception to the generic exact-Quarantine block: the same current
/// run/GPU/pair may re-prove that exact pair with the ledger-required number of complete v29
/// matrices. Rigid/TDR floors, deeper Quarantine bins and non-exact field overlaps never qualify.
#[cfg(windows)]
fn exact_undervolt_apply_preflight(
    store: &SafeLoopStore,
    target_mhz: u32,
    anchor_mv: u32,
    gpu_key: &str,
    qualification_run_id: &str,
) -> Result<(), String> {
    if let Some(event) = crate::tdr_sentinel::reboot_required_event() {
        return Err(format!(
            "GPU reboot is required after driver reset {event}; no GPU write is allowed in this boot"
        ));
    }
    ensure_boot_transaction_is_clear(store)?;
    let record = store
        .load_record_checked()
        .map_err(|error| format!("Safe Loop record is unreadable; GPU write refused: {error}"))?;
    if record.pending_forge_incident.is_some() {
        return Err(
            "Forge recovery requires explicit operator acknowledgement before profile Apply".into(),
        );
    }
    if record.safe_mode {
        return Err("Safe Mode is active; profile Apply refused".into());
    }
    let condemned = crate::gpu_power_sweep::current_f2_condemned_pairs(store, gpu_key)?;
    if condemned.refuses(target_mhz, anchor_mv) {
        return Err(format!(
            "F2 profile {target_mhz} MHz @ {anchor_mv} mV is below a durable Rigid/Quarantine/TDR boundary"
        ));
    }

    let quarantine_reproof =
        crate::gpu_undervolt::f2_exact_quarantine_reproof_passes(&condemned, target_mhz, anchor_mv);
    let field_refuses =
        crate::gpu_undervolt::field_pair_blacklisted(&record, target_mhz, anchor_mv);
    if field_refuses
        && !(crate::gpu_undervolt::f2_operational_blacklist_is_only_exact_pair(
            &record, target_mhz, anchor_mv,
        ) && quarantine_reproof.is_some())
    {
        return Err(format!(
            "F2 profile {target_mhz} MHz @ {anchor_mv} mV is inside a field blacklist that has no exact current-contract Quarantine re-proof path"
        ));
    }

    let required_matrices = quarantine_reproof.unwrap_or(1);
    let observations =
        nidavellir_core::f2_observation::F2ObservationStore::new(store.base_dir())
            .load_all_checked()
            .map_err(|error| {
                format!(
                    "F2 exact-Apply observations are unreadable; profile Apply refused: {error}"
                )
            })?;
    if !nidavellir_core::f2_observation::point_has_n_current_exact_apply_qualifications(
        &observations,
        qualification_run_id,
        target_mhz,
        anchor_mv,
        gpu_key,
        required_matrices,
    ) {
        return Err(format!(
            "F2 apply proof is missing/incomplete for run {qualification_run_id}, GPU {gpu_key}, exact pair {target_mhz}@{anchor_mv}: {required_matrices} complete v29 matrix/matrices required"
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn spawn_owned_boot_flag_clear(store: SafeLoopStore, flag: BootFlag) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(8));
        match store.clear_boot_flag_if_matches(&flag) {
            Ok(true) => info!(
                "GPU apply: survival window completed; cleared owned transaction {}",
                flag.transaction_id
            ),
            Ok(false) => info!(
                "GPU apply: survival timer for {} no longer owns the boot flag; leaving current transaction intact",
                flag.transaction_id
            ),
            Err(error) => warn!(
                "GPU apply: failed to clear owned boot transaction {}: {error}",
                flag.transaction_id
            ),
        }
    });
}

#[cfg(windows)]
pub fn apply_core(point: VfPoint) -> Result<(), String> {
    if nidavellir_gpu_nvapi::vf_curve_supported() {
        // Snap the MEASURED voltage to a deterministic VF-table bin and key the
        // ceiling on that — never on the raw measured value. The measured number is
        // kept only as descriptive telemetry on the point.
        let curve = nidavellir_gpu_nvapi::read_vf_curve_modern();
        let (ceiling_mv, legacy) = choose_ceiling_mv(&curve, point.voltage_mv);
        if legacy {
            warn!(
                "voltage_semantics: unable to map measured {} mV to a VF-table bin \
                 (empty/unknown curve); apply uses measured value as legacy ceiling",
                point.voltage_mv
            );
        } else {
            info!(
                "voltage_semantics: using vf_table_voltage_mv={ceiling_mv} \
                 measured_voltage_mv={} target={} MHz",
                point.voltage_mv, point.freq_mhz
            );
        }
        match nidavellir_gpu_nvapi::apply_vf_ceiling(ceiling_mv, point.freq_mhz) {
            Ok(n) => {
                info!(
                    "VF ceiling: {n} pts achatados para {} MHz acima de {} mV (elástico)",
                    point.freq_mhz, ceiling_mv
                );
                return Ok(());
            }
            Err(e) => warn!("VF ceiling falhou ({e}); usando fallback offset+cap"),
        }
    }
    // Fallback: offset the clock up so the GPU reaches freq at the lower voltage,
    // then hard-cap the max clock so it never boosts past the validated point.
    let base = curve_freq_at(point.voltage_mv).unwrap_or(point.freq_mhz);
    let offset = (point.freq_mhz as i64 - base as i64).clamp(-300, 400) as i32;
    nidavellir_gpu_nvapi::set_core_offset_mhz(offset)?;
    if let Err(e) = nidavellir_core::nvml_gpu::lock_core_clock_max_mhz(point.freq_mhz) {
        warn!(
            "core clock cap at {} MHz failed (continuing): {e}",
            point.freq_mhz
        );
    }
    Ok(())
}

/// Apply a profile (core point and/or memory offset) and persist it. Arms the
/// Safe Loop boot-flag around the apply; clears it after a short survival window
/// (a crash leaves it armed → not re-applied next boot).
#[cfg(windows)]
pub fn apply_and_persist(
    label: String,
    core: Option<VfPoint>,
    mem_offset_mhz: Option<i32>,
    store: &SafeLoopStore,
) -> Result<(), String> {
    generic_hardware_write_preflight(store, core.map(|point| (point.freq_mhz, point.voltage_mv)))?;
    // Remove any older descriptor before hardware mutation. If this cannot be confirmed, fail
    // closed: a later boot must never reapply stale settings after a new apply attempt.
    clear_applied_checked()?;
    let mut intent = TuningPoint::default();
    if let Some(c) = core {
        intent.axes.insert("gpu_freq_mhz".into(), c.freq_mhz as i64);
        intent
            .axes
            .insert("gpu_voltage_mv".into(), c.voltage_mv as i64);
    }
    if let Some(m) = mem_offset_mhz {
        intent.axes.insert("gpu_mem_offset_mhz".into(), m as i64);
    }
    let boot_flag = BootFlag::new(intent, "gpu_apply");
    if let Err(error) = store.arm_boot_flag(&boot_flag) {
        let reset = crate::gpu_power_sweep::reset_to_stock_checked();
        return Err(match reset {
            Ok(()) => {
                format!("GPU apply: failed to arm Safe Loop before write ({error}); GPU reset to stock")
            }
            Err(reset_error) => format!(
                "GPU apply: failed to arm Safe Loop before write ({error}); stock reset also failed ({reset_error})"
            ),
        });
    }

    let hardware_write = (|| -> Result<(), String> {
        if let Some(c) = core {
            apply_core(c)?;
        }
        if let Some(m) = mem_offset_mhz {
            nidavellir_gpu_nvapi::set_mem_offset_mhz(m)?;
        }
        Ok(())
    })();
    if let Err(error) = hardware_write {
        let reset = crate::gpu_power_sweep::reset_to_stock_checked();
        return Err(match reset {
            Ok(()) => format!("GPU apply failed ({error}); GPU reset to stock"),
            Err(reset_error) => {
                format!("GPU apply failed ({error}); stock reset also failed ({reset_error})")
            }
        });
    }

    if let Err(error) = save_applied(&AppliedProfile {
        label,
        core,
        mem_offset_mhz,
        applied_at: Some(nidavellir_core::f2_observation::now_rfc3339()),
        undervolt: None,
    }) {
        let reset = crate::gpu_power_sweep::reset_to_stock_checked();
        return Err(match reset {
            Ok(()) => format!("GPU apply persistence failed ({error}); GPU reset to stock"),
            Err(reset_error) => format!(
                "GPU apply persistence failed ({error}); stock reset also failed ({reset_error})"
            ),
        });
    }

    // The delayed clear carries the exact transaction owner. It can never delete a later probe,
    // Sentinel attribution or profile apply that replaced this flag during the survival window.
    spawn_owned_boot_flag_clear(store.clone(), boot_flag);
    Ok(())
}

/// Apply an F2 anchored-undervolt profile and persist it. The clock is a max-only ceiling (so it may
/// step down) while `anchor_mv` is a verified graphics-domain voltage lock. Mirrors
/// [`apply_and_persist`] but writes the anchored curve plus those two authoritative controls: arms
/// the Safe Loop boot-flag around the write (a crash leaves it armed → not re-applied next boot),
/// writes via the fail-closed [`crate::gpu_undervolt::apply_anchored_undervolt`] (which resets to
/// stock on any non-verified outcome), persists `gpu_applied.json` with the [`UndervoltApply`]
/// descriptor, then clears the boot-flag after a short survival window. Preserves any existing
/// memory offset.
#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
pub fn apply_and_persist_undervolt(
    label: String,
    target_mhz: u32,
    anchor_mv: u32,
    gpu_key: String,
    qualification_run_id: String,
    qualification_contract_version: u32,
    mem_offset_mhz: Option<i32>,
    store: &SafeLoopStore,
) -> Result<(), String> {
    if qualification_contract_version
        != nidavellir_core::f2_observation::F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION
    {
        return Err(format!(
            "F2 apply descriptor is not exact-Apply v{}",
            nidavellir_core::f2_observation::F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION
        ));
    }
    let current_gpu_key = crate::gpu_power_sweep::current_gpu_key();
    if gpu_key != current_gpu_key {
        return Err(format!(
            "F2 apply proof belongs to another GPU ({gpu_key}); current GPU is {current_gpu_key}"
        ));
    }
    // Last line of defence shared by explicit Apply and apply-on-boot. It re-loads strict Safe Loop
    // and ledger state, applies the current TDR cone, and admits exact Quarantine only after the
    // current run/GPU/pair has the required number of complete v29 matrices.
    exact_undervolt_apply_preflight(
        store,
        target_mhz,
        anchor_mv,
        &gpu_key,
        &qualification_run_id,
    )?;
    clear_applied_checked()?;
    let mut intent = TuningPoint::default();
    intent.axes.insert("gpu_freq_mhz".into(), target_mhz as i64);
    intent
        .axes
        .insert("gpu_voltage_mv".into(), anchor_mv as i64);
    if let Some(m) = mem_offset_mhz {
        intent.axes.insert("gpu_mem_offset_mhz".into(), m as i64);
    }
    let boot_flag = BootFlag::new(intent, "gpu_apply_undervolt");
    if let Err(error) = store.arm_boot_flag(&boot_flag) {
        let reset = crate::gpu_power_sweep::reset_to_stock_checked();
        return Err(match reset {
            Ok(()) => format!(
                "F2 apply: failed to arm Safe Loop before write ({error}); GPU reset to stock"
            ),
            Err(reset_error) => format!(
                "F2 apply: failed to arm Safe Loop before write ({error}); stock reset also failed ({reset_error})"
            ),
        });
    }

    // Fail-closed: a non-verified write has already reset to stock inside this call, so nothing is left
    // applied. The boot-flag stays armed on the error path (same as the F1 apply) — safe; reset clears it.
    crate::gpu_undervolt::apply_anchored_undervolt(target_mhz, anchor_mv)?;
    if let Some(m) = mem_offset_mhz {
        if let Err(e) = nidavellir_gpu_nvapi::set_mem_offset_mhz(m) {
            crate::gpu_power_sweep::reset_to_stock();
            let reset = nidavellir_gpu_nvapi::reset_all();
            return Err(match reset {
                Ok(()) => format!("F2 apply: memory offset failed ({e}); GPU reset to stock"),
                Err(reset_err) => format!(
                    "F2 apply: memory offset failed ({e}); stock reset also failed ({reset_err})"
                ),
            });
        }
    }

    if let Err(error) = save_applied(&AppliedProfile {
        label,
        // Existing IPC/UI status shape: expose the deterministic target + anchor while the
        // `undervolt` descriptor remains the authoritative apply-on-boot route.
        core: Some(VfPoint {
            freq_mhz: target_mhz,
            voltage_mv: anchor_mv,
        }),
        mem_offset_mhz,
        applied_at: Some(nidavellir_core::f2_observation::now_rfc3339()),
        undervolt: Some(UndervoltApply {
            target_mhz,
            anchor_mv,
            gpu_key: Some(gpu_key),
            qualification_run_id: Some(qualification_run_id),
            qualification_contract_version: Some(qualification_contract_version),
        }),
    }) {
        let reset = crate::gpu_power_sweep::reset_to_stock_checked();
        return Err(match reset {
            Ok(()) => format!("F2 apply persistence failed ({error}); GPU reset to stock"),
            Err(reset_error) => format!(
                "F2 apply persistence failed ({error}); stock reset also failed ({reset_error})"
            ),
        });
    }

    spawn_owned_boot_flag_clear(store.clone(), boot_flag);
    Ok(())
}

/// Re-apply the currently persisted core route while changing memory. An F2 profile must remain an
/// anchored v29 descriptor; routing it through the F1 writer would silently downgrade its proof and
/// make the next boot unsafe/unreplayable.
#[cfg(windows)]
pub(crate) fn apply_with_memory_offset(
    mut profile: AppliedProfile,
    mem_offset_mhz: i32,
    store: &SafeLoopStore,
) -> Result<(), String> {
    profile.mem_offset_mhz = Some(mem_offset_mhz);
    if let Some(descriptor) = profile.undervolt.clone() {
        let current_gpu_key = crate::gpu_power_sweep::current_gpu_key();
        let (gpu_key, run_id, contract) =
            qualified_undervolt_descriptor(&descriptor, &current_gpu_key)?;
        apply_and_persist_undervolt(
            profile.label,
            descriptor.target_mhz,
            descriptor.anchor_mv,
            gpu_key.to_string(),
            run_id.to_string(),
            contract,
            Some(mem_offset_mhz),
            store,
        )
    } else {
        apply_and_persist(profile.label, profile.core, Some(mem_offset_mhz), store)
    }
}

/// Reset the GPU to stock and forget the persisted profile.
#[cfg(windows)]
pub(crate) fn reset_hardware_and_descriptor_only() -> Result<(), String> {
    crate::gpu_power_sweep::reset_to_stock_checked()?;
    clear_applied_checked()
}

fn release_reset_latch_preserving_evidence(
    mut record: nidavellir_core::safe_loop::SafeLoopRecord,
) -> nidavellir_core::safe_loop::SafeLoopRecord {
    record.clear_recovery_latch();
    record
}

/// Reset the GPU to stock and forget the persisted profile.
#[cfg(windows)]
pub fn reset(store: &SafeLoopStore) -> Result<(), String> {
    // Read both safety files strictly before deciding what may be cleared. Hardware and the stale
    // applied descriptor are still returned to stock on a corrupt-state failure, but unreadable
    // Safe Loop data is never overwritten with defaults and an unreadable flag is never deleted.
    let record = store.load_record_checked();
    let boot_flag = store.read_boot_flag_checked();
    reset_hardware_and_descriptor_only()?;
    // Release the Safe Loop latch so tuning is allowed again: leave Safe Mode and zero the crash
    // streak, while PRESERVING learning (blacklist, last_validated, crash history). Without this the
    // operator's "Reset all" cannot clear a latched Safe Mode — the reset only ever touched the
    // boot-flag and hardware, never this record, so `safe_mode` was a one-way latch.
    let record = record.map_err(|error| {
        format!("GPU reset reached stock but Safe Loop record is unreadable: {error}")
    })?;
    let boot_flag = boot_flag.map_err(|error| {
        format!("GPU reset reached stock but Safe Loop boot flag is unreadable and remains armed: {error}")
    })?;
    let record = release_reset_latch_preserving_evidence(record);
    store
        .save_record(&record)
        .map_err(|e| format!("GPU reset completed but Safe Loop record could not be saved: {e}"))?;
    let Some(expected) = boot_flag.as_ref() else {
        return Ok(());
    };
    match store.clear_boot_flag_if_matches(expected) {
        Ok(true) => Ok(()),
        Ok(false) => Err(
            "GPU reset completed, but boot-flag ownership changed; the newer transaction remains armed"
                .into(),
        ),
        Err(error) => Err(format!(
            "GPU reset completed but its owned Safe Loop flag could not be cleared: {error}"
        )),
    }
}

#[cfg(not(windows))]
pub fn apply_and_persist(
    _label: String,
    _core: Option<VfPoint>,
    _mem: Option<i32>,
    _store: &SafeLoopStore,
) -> Result<(), String> {
    Err("GPU apply is Windows-only".into())
}

#[cfg(not(windows))]
pub fn apply_and_persist_undervolt(
    _label: String,
    _target_mhz: u32,
    _anchor_mv: u32,
    _gpu_key: String,
    _qualification_run_id: String,
    _qualification_contract_version: u32,
    _mem: Option<i32>,
    _store: &SafeLoopStore,
) -> Result<(), String> {
    Err("GPU apply is Windows-only".into())
}

#[cfg(not(windows))]
pub(crate) fn apply_with_memory_offset(
    _profile: AppliedProfile,
    _mem_offset_mhz: i32,
    _store: &SafeLoopStore,
) -> Result<(), String> {
    Err("GPU apply is Windows-only".into())
}

#[cfg(not(windows))]
pub fn reset(_store: &SafeLoopStore) -> Result<(), String> {
    Err("GPU apply is Windows-only".into())
}

/// Re-apply the persisted profile at service startup. Skips if the Safe Loop
/// boot-flag is armed (last apply crashed), Safe Mode is active, or an interrupted Forge still
/// requires explicit operator acknowledgement.
#[cfg(windows)]
pub fn reapply_on_boot(store: &SafeLoopStore) {
    if full_reset_pending(store.base_dir()) {
        let _ = crate::gpu_power_sweep::reset_to_stock_checked();
        warn!("GPU apply-on-boot: incomplete Full Reset; staying at stock until reset is retried");
        return;
    }
    if let Some(event) = crate::tdr_sentinel::reboot_required_event() {
        warn!(
            "GPU apply-on-boot: driver reset {event} belongs to this Windows boot — reboot required, not re-applying"
        );
        return;
    }
    // Driver-resident clock, curve and voltage controls survive a service restart without a reboot.
    // Never clear the recovery flag or re-apply a profile until stock has been confirmed.
    if let Err(error) = crate::gpu_power_sweep::reset_to_stock_checked() {
        warn!(
            "GPU apply-on-boot: stock reset was not confirmed ({error}) — retaining recovery flag"
        );
        return;
    }
    let boot_flag = match store.read_boot_flag_checked() {
        Ok(flag) => flag,
        Err(error) => {
            warn!("GPU apply-on-boot: corrupt/unreadable boot flag ({error}) — stock confirmed, flag preserved, not re-applying");
            return;
        }
    };
    if let Some(flag) = boot_flag {
        warn!("GPU apply-on-boot: boot-flag armed (prior interrupted transaction) — stock confirmed, not re-applying");
        match store.clear_boot_flag_if_matches(&flag) {
            Ok(true) => {}
            Ok(false) => warn!(
                "GPU apply-on-boot: recovery flag ownership changed before clear; current transaction preserved"
            ),
            Err(e) => warn!("GPU apply-on-boot: failed to disarm accounted recovery flag: {e}"),
        }
        return;
    }
    let record = match store.load_record_checked() {
        Ok(record) => record,
        Err(error) => {
            warn!("GPU apply-on-boot: Safe Loop record is unreadable ({error}) — staying at stock");
            return;
        }
    };
    if let Err(error) =
        nidavellir_core::condemnation::CondemnationLedger::new(store.base_dir()).load_all_checked()
    {
        warn!("GPU apply-on-boot: durable condemnation ledger is unreadable ({error}) — staying at stock");
        return;
    }
    if record.safe_mode {
        warn!("GPU apply-on-boot: Safe Mode active — not re-applying");
        return;
    }
    if record.pending_forge_incident.is_some() {
        warn!("GPU apply-on-boot: Forge incident requires acknowledgement — staying at stock");
        return;
    }
    let ap = match load_applied_checked() {
        Ok(Some(profile)) => profile,
        Ok(None) => return,
        Err(error) => {
            warn!("GPU apply-on-boot: {error}; corrupt descriptor will not be applied");
            if let Err(clear_error) = clear_applied_checked() {
                warn!("GPU apply-on-boot: failed to clear corrupt descriptor: {clear_error}");
            }
            return;
        }
    };
    info!("GPU apply-on-boot: re-applying '{}'", ap.label);
    let current_gpu_key = crate::gpu_power_sweep::current_gpu_key();
    let res = match ap.undervolt {
        // F2 undervolt: re-derive + re-write the anchored curve from the LIVE VF table (fail-closed).
        Some(uv) => match qualified_undervolt_descriptor(&uv, &current_gpu_key) {
            Ok((gpu_key, run_id, contract)) => apply_and_persist_undervolt(
                ap.label,
                uv.target_mhz,
                uv.anchor_mv,
                gpu_key.to_string(),
                run_id.to_string(),
                contract,
                ap.mem_offset_mhz,
                store,
            ),
            Err(error) => Err(error),
        },
        // Legacy F1/pre-v29 V/F descriptors cannot prove they remain outside the current cone.
        None => {
            Err(format!(
                "legacy GPU V/F descriptor has no exact-Apply v{} proof; reapply refused",
                nidavellir_core::f2_observation::F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION
            ))
        }
    };
    if let Err(e) = res {
        warn!("GPU apply-on-boot failed: {e}");
        match clear_applied_checked() {
            Ok(()) => warn!(
                "GPU apply-on-boot: unsafe/unapplicable descriptor cleared; GPU remains stock"
            ),
            Err(clear_error) => warn!(
                "GPU apply-on-boot: descriptor could not be cleared ({clear_error}); every future boot will continue to refuse it"
            ),
        }
    }
}

#[cfg(not(windows))]
pub fn reapply_on_boot(_store: &SafeLoopStore) {}

#[cfg(test)]
mod tests {
    use super::{
        choose_ceiling_mv, clear_all_learning_at, clear_all_learning_at_with, clear_applied_path,
        load_applied_at, qualified_undervolt_descriptor, release_reset_latch_preserving_evidence,
        rewrite_f2_learning_preserving_negatives,
        rewrite_legacy_gpu_knowledge_preserving_negatives, save_applied_at, AppliedProfile,
        UndervoltApply,
    };
    use nidavellir_core::gpu_sweep::VfPoint;

    #[test]
    fn full_reset_forgets_positive_negative_and_archived_learning_without_reuse() {
        use nidavellir_core::safe_loop::{SafeLoopRecord, SafeLoopStore, BlacklistRegion, TuningPoint};
        let base = std::env::temp_dir().join(format!("nidavellir-forget-all-{}-{}",
            std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = SafeLoopStore::new(&base);
        let mut record = SafeLoopRecord::default();
        record.blacklist.push(BlacklistRegion::around(TuningPoint::from_axes([("gpu_freq_mhz", 1920)]), 1));
        store.save_record(&record).unwrap();
        let names = ["gpu_applied.json", "gpu_knowledge.json", "f2_observations.jsonl",
            "condemnation_ledger.jsonl", "forge_state.json", "f2_observations.pre-full-reset-1.jsonl",
            "gpu_knowledge.pre-full-reset-1.json"];
        for name in names { std::fs::write(base.join(name), "old learning, including failed points").unwrap(); }
        std::fs::create_dir_all(base.join("forge-archive/run-old")).unwrap();
        std::fs::write(base.join("forge-archive/run-old/run-f2_observations.jsonl"), "old evidence").unwrap();
        std::fs::write(base.join("sentinel_baseline.txt"), "watcher cursor").unwrap();
        std::fs::write(base.join("operator-report.txt"), "export").unwrap();

        super::forget_all_gpu_learning(&store).unwrap();
        for name in names { assert!(!base.join(name).exists(), "{name}"); }
        assert!(!base.join("forge-archive").exists());
        assert!(!super::full_reset_pending(&base));
        let fresh = store.load_record_checked().unwrap();
        assert!(fresh.blacklist.is_empty());
        assert!(fresh.forge_incidents.is_empty());
        assert!(fresh.last_validated.is_none());
        assert!(nidavellir_core::condemnation::CondemnationLedger::new(&base).load_all_checked().unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(base.join("sentinel_baseline.txt")).unwrap(), "watcher cursor");
        assert!(base.join("operator-report.txt").exists());
        super::forget_all_gpu_learning(&store).unwrap(); // explicit retry stays pristine
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn full_reset_refuses_armed_recovery_and_keeps_partial_failure_blocked_until_retry() {
        use nidavellir_core::safe_loop::{SafeLoopStore, BootFlag, TuningPoint};
        let base = std::env::temp_dir().join(format!("nidavellir-forget-failure-{}-{}",
            std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = SafeLoopStore::new(&base);
        store.arm_boot_flag(&BootFlag::new(TuningPoint::from_axes([("gpu_freq_mhz", 1800)]), "test")).unwrap();
        std::fs::write(base.join("condemnation_ledger.jsonl"), "preserve before recovery").unwrap();
        assert!(super::forget_all_gpu_learning(&store).is_err());
        assert!(store.is_boot_flag_armed());
        assert!(!super::full_reset_pending(&base));
        assert_eq!(std::fs::read_to_string(base.join("condemnation_ledger.jsonl")).unwrap(), "preserve before recovery");
        store.clear_boot_flag().unwrap();
        // Simulate an undeletable active file without relying on platform ACLs.
        std::fs::create_dir(base.join("forge_state.json")).unwrap();
        assert!(super::forget_all_gpu_learning(&store).is_err());
        assert!(super::full_reset_pending(&base));
        assert!(crate::gpu_power_sweep::forge_start_block_reason(&store).unwrap().contains("interrupted"));
        std::fs::remove_dir(base.join("forge_state.json")).unwrap();
        super::forget_all_gpu_learning(&store).unwrap();
        assert!(!super::full_reset_pending(&base));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn soft_reset_removes_positive_archives_but_retains_active_failure_evidence() {
        let base = std::env::temp_dir().join(format!("nidavellir-soft-reset-{}-{}",
            std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(base.join("forge-archive/old-run")).unwrap();
        let positive = observation_line("validated", "stable");
        let negative = observation_line("silent_error", "silent_error");
        let path = base.join("f2_observations.jsonl");
        std::fs::write(&path, format!("{positive}\n{negative}\n")).unwrap();
        std::fs::write(base.join("condemnation_ledger.jsonl"), "negative history").unwrap();
        clear_all_learning_at(&base).unwrap();
        let active = std::fs::read_to_string(path).unwrap();
        assert!(!active.contains("\"outcome\":\"validated\""));
        assert!(active.contains("\"outcome\":\"silent_error\""));
        assert_eq!(std::fs::read_to_string(base.join("condemnation_ledger.jsonl")).unwrap(), "negative history");
        assert!(!base.join("forge-archive").exists());
        assert!(!std::fs::read_dir(&base).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains("pre-full-reset")));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn applied_profile_roundtrips_undervolt_descriptor() {
        let p = AppliedProfile {
            label: "Godforge".into(),
            core: Some(VfPoint {
                freq_mhz: 1800,
                voltage_mv: 875,
            }),
            mem_offset_mhz: None,
            applied_at: Some("2026-07-18T21:50:00+00:00".into()),
            undervolt: Some(UndervoltApply {
                target_mhz: 1800,
                anchor_mv: 875,
                gpu_key: Some("gpu-a".into()),
                qualification_run_id: Some("run-a".into()),
                qualification_contract_version: Some(
                    nidavellir_core::f2_observation::F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION,
                ),
            }),
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: AppliedProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.undervolt, p.undervolt);
        assert_eq!(
            back.core,
            Some(VfPoint {
                freq_mhz: 1800,
                voltage_mv: 875
            })
        );
        assert_eq!(
            back.applied_at.as_deref(),
            Some("2026-07-18T21:50:00+00:00")
        );
    }

    #[test]
    fn legacy_applied_profile_json_defaults_undervolt_none() {
        // A profile persisted before Phase 2 has no `undervolt` key → defaults None, so apply-on-boot
        // keeps the legacy F1 flatten path. Backward-compatible.
        let legacy = r#"{"label":"Brokkr's Best","core":{"freq_mhz":1800,"voltage_mv":906},"mem_offset_mhz":null}"#;
        let p: AppliedProfile = serde_json::from_str(legacy).unwrap();
        assert!(
            p.undervolt.is_none(),
            "missing key must default to legacy F1 behavior"
        );
        assert!(
            p.applied_at.is_none(),
            "legacy payload must not inherit crash attribution"
        );
        assert!(p.core.is_some());
    }

    #[test]
    fn persisted_f2_reapply_requires_current_gpu_run_and_exact29_identity() {
        let valid = UndervoltApply {
            target_mhz: 1800,
            anchor_mv: 893,
            gpu_key: Some("gpu-a".into()),
            qualification_run_id: Some("run-a".into()),
            qualification_contract_version: Some(
                nidavellir_core::f2_observation::F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION,
            ),
        };
        assert!(qualified_undervolt_descriptor(&valid, "gpu-a").is_ok());
        assert!(qualified_undervolt_descriptor(&valid, "gpu-b").is_err());

        let mut legacy = valid.clone();
        legacy.gpu_key = None;
        legacy.qualification_run_id = None;
        legacy.qualification_contract_version = None;
        assert!(qualified_undervolt_descriptor(&legacy, "gpu-a").is_err());

        let mut old_contract = valid;
        old_contract.qualification_contract_version = Some(
            nidavellir_core::f2_observation::F2_EXACT_APPLY_QUALIFICATION_CONTRACT_VERSION - 1,
        );
        assert!(qualified_undervolt_descriptor(&old_contract, "gpu-a").is_err());
    }

    #[test]
    fn confirmed_stock_reset_can_remove_persisted_apply_descriptor() {
        let path = std::env::temp_dir().join(format!(
            "nidavellir-applied-clear-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, "persisted profile").unwrap();
        clear_applied_path(&path).unwrap();
        assert!(!path.exists());
        clear_applied_path(&path).unwrap();
    }

    #[test]
    fn applied_profile_save_and_delete_failures_are_reported() {
        let base = std::env::temp_dir().join(format!(
            "nidavellir-applied-io-failure-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let parent_file = base.join("not-a-directory");
        std::fs::write(&parent_file, "occupied").unwrap();
        let profile = AppliedProfile {
            label: "test".into(),
            ..AppliedProfile::default()
        };
        assert!(save_applied_at(&parent_file.join("gpu_applied.json"), &profile).is_err());

        let directory_target = base.join("directory-target");
        std::fs::create_dir_all(&directory_target).unwrap();
        assert!(clear_applied_path(&directory_target).is_err());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn applied_profile_checked_load_rejects_corruption() {
        let path = std::env::temp_dir().join(format!(
            "nidavellir-applied-corrupt-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, "{ truncated").unwrap();
        let error = load_applied_at(&path).unwrap_err();
        assert!(error.contains("invalid persisted GPU profile"), "{error}");
        let _ = std::fs::remove_file(path);
    }

    fn observation_line(outcome: &str, dwell: &str) -> String {
        format!(
            r#"{{"run_id":"run-reset","timestamp":"2026-08-25T00:00:00Z","mode":"target-sweep","target_mhz":1800,"anchor_mv":875,"base_mhz":1700,"offset_mhz":100,"positive_offset_cap_mhz":200,"verifier_result":"raise_verified","dwell_result":"{dwell}","outcome":"{outcome}"}}"#
        )
    }

    #[test]
    fn full_reset_removes_only_validated_observations_and_archives_original() {
        let base = std::env::temp_dir().join(format!(
            "nidavellir-positive-reset-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join(nidavellir_core::f2_observation::F2_OBSERVATIONS_FILE);
        let positive = observation_line("validated", "stable");
        let negative = observation_line("silent_error", "silent_error");
        std::fs::write(&path, format!("{positive}\n{negative}\n")).unwrap();

        rewrite_f2_learning_preserving_negatives(&path).unwrap();

        let active = std::fs::read_to_string(&path).unwrap();
        assert!(!active.contains("\"outcome\":\"validated\""));
        assert!(active.contains("\"outcome\":\"silent_error\""));
        let archives = std::fs::read_dir(&base)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("pre-full-reset")
            })
            .count();
        assert_eq!(archives, 1);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn corrupt_observation_aborts_full_reset_without_mutation() {
        let base = std::env::temp_dir().join(format!(
            "nidavellir-corrupt-reset-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join(nidavellir_core::f2_observation::F2_OBSERVATIONS_FILE);
        let original = format!(
            "{}\n{{ truncated\n",
            observation_line("validated", "stable")
        );
        std::fs::write(&path, &original).unwrap();

        let error = rewrite_f2_learning_preserving_negatives(&path).unwrap_err();
        assert!(error.contains("line 2"), "{error}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(std::fs::read_dir(&base).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn corrupt_legacy_preflight_leaves_f2_learning_byte_identical() {
        let base = std::env::temp_dir().join(format!(
            "nidavellir-cross-corrupt-reset-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let f2_path = base.join(nidavellir_core::f2_observation::F2_OBSERVATIONS_FILE);
        let original = format!(
            "{}\r\n{}\r\n",
            observation_line("validated", "stable"),
            observation_line("silent_error", "silent_error")
        );
        std::fs::write(&f2_path, original.as_bytes()).unwrap();
        std::fs::write(base.join("gpu_knowledge.json"), "{ truncated").unwrap();

        let error = clear_all_learning_at(&base).unwrap_err();
        assert!(
            error.contains("invalid legacy GPU knowledge JSON"),
            "{error}"
        );
        assert_eq!(std::fs::read(&f2_path).unwrap(), original.as_bytes());
        assert_eq!(
            std::fs::read_dir(&base)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .contains("pre-full-reset"))
                .count(),
            0
        );
        let _ = std::fs::remove_dir_all(base);
    }

    fn legacy_knowledge_json() -> String {
        serde_json::json!({
            "gpu_key": "gpu-legacy",
            "target_clock_mhz": 1800,
            "boundary": {
                "highest_clean": 135,
                "lowest_silent_error": 150,
                "lowest_tdr": 165,
                "lowest_reboot": 180
            },
            "points": {
                "135": {
                    "trials": 3,
                    "failures": 0,
                    "worst_severity": "None",
                    "stable_trials": 3,
                    "clock_mhz_sum": 5400,
                    "power_w_sum": 450.0,
                    "voltage_mv_sum": 2700
                }
            },
            "schema_version": 7,
            "forward_compatible_note": "preserve me"
        })
        .to_string()
    }

    #[test]
    fn full_reset_clears_legacy_positives_but_preserves_negative_boundaries() {
        let base = std::env::temp_dir().join(format!(
            "nidavellir-legacy-reset-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("gpu_knowledge.json");
        std::fs::write(&path, legacy_knowledge_json()).unwrap();

        rewrite_legacy_gpu_knowledge_preserving_negatives(&path).unwrap();

        let active: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(active["gpu_key"], "gpu-legacy");
        assert_eq!(active["target_clock_mhz"], 1800);
        assert_eq!(active["schema_version"], 7);
        assert_eq!(active["boundary"]["highest_clean"], 0);
        assert_eq!(active["boundary"]["lowest_silent_error"], 150);
        assert_eq!(active["boundary"]["lowest_tdr"], 165);
        assert_eq!(active["boundary"]["lowest_reboot"], 180);
        assert_eq!(active["points"], serde_json::json!({}));
        assert_eq!(active["forward_compatible_note"], "preserve me");
        assert_eq!(
            std::fs::read_dir(&base)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .contains("gpu_knowledge.pre-full-reset"))
                .count(),
            1
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn second_learning_commit_failure_rolls_f2_back_byte_identical() {
        let base = std::env::temp_dir().join(format!(
            "nidavellir-cross-io-reset-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let f2_path = base.join(nidavellir_core::f2_observation::F2_OBSERVATIONS_FILE);
        let original = format!(
            "{}\n{}\n",
            observation_line("validated", "stable"),
            observation_line("silent_error", "silent_error")
        );
        std::fs::write(&f2_path, original.as_bytes()).unwrap();
        let legacy = legacy_knowledge_json();
        std::fs::write(base.join("gpu_knowledge.json"), legacy.as_bytes()).unwrap();

        let error = clear_all_learning_at_with(&base, |_| {
            Err("simulated second commit failure".to_string())
        })
        .unwrap_err();
        assert!(error.contains("simulated second commit failure"), "{error}");
        assert!(error.contains("restored byte-for-byte"), "{error}");
        assert_eq!(std::fs::read(&f2_path).unwrap(), original.as_bytes());
        assert_eq!(
            std::fs::read(base.join("gpu_knowledge.json")).unwrap(),
            legacy.as_bytes()
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn invalid_legacy_knowledge_schema_aborts_without_mutation() {
        let base = std::env::temp_dir().join(format!(
            "nidavellir-legacy-corrupt-reset-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("gpu_knowledge.json");
        let original = legacy_knowledge_json()
            .replace("\"lowest_tdr\":165", "\"lowest_tdr\":\"not-an-offset\"");
        std::fs::write(&path, &original).unwrap();

        let error = rewrite_legacy_gpu_knowledge_preserving_negatives(&path).unwrap_err();
        assert!(
            error.contains("invalid legacy GPU knowledge schema"),
            "{error}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(std::fs::read_dir(&base).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn full_reset_latch_release_preserves_pending_blacklist_and_negative_history() {
        use nidavellir_core::safe_loop::{
            BlacklistRegion, ForgeIncident, ForgeIncidentKind, SafeLoopRecord, TuningPoint,
        };
        let point = TuningPoint::from_axes([("gpu_freq_mhz", 1800), ("gpu_vf_bin_mv", 875)]);
        let mut record = SafeLoopRecord {
            safe_mode: true,
            ..SafeLoopRecord::default()
        };
        record.blacklist.push(BlacklistRegion::around(point, 1));
        assert!(record.record_forge_incident(ForgeIncident::new(
            ForgeIncidentKind::CandidateCrash,
            Some("run-reset".into()),
            Some("gpu-reset".into()),
            Some(1800),
            Some(875),
            "pending crash",
        )));
        record
            .crash_log
            .push(nidavellir_core::safe_loop::CrashClass::OcInstability);

        let reset = release_reset_latch_preserving_evidence(record.clone());
        assert!(!reset.safe_mode);
        assert_eq!(reset.pending_forge_incident, record.pending_forge_incident);
        assert_eq!(reset.blacklist, record.blacklist);
        assert_eq!(reset.crash_log, record.crash_log);
        assert_eq!(reset.forge_incidents, record.forge_incidents);
    }

    // (index, voltage_mv, freq_mhz) — shape of read_vf_curve_modern().
    fn curve() -> Vec<(usize, u32, u32)> {
        vec![
            (0, 800, 1700),
            (1, 837, 1750),
            (2, 850, 1770),
            (3, 1062, 1900),
        ]
    }

    #[test]
    fn ceiling_prefers_vf_table_bin_over_measured() {
        // Measured 843 (between bins) must snap UP to the real 850 table bin, not 843.
        let (mv, legacy) = choose_ceiling_mv(&curve(), 843);
        assert_eq!(mv, 850);
        assert!(!legacy);
        // An exact-bin measurement stays on its bin.
        assert_eq!(choose_ceiling_mv(&curve(), 837), (837, false));
    }

    #[test]
    fn ceiling_falls_back_to_measured_only_when_no_curve() {
        // No deterministic curve available (legacy/unknown) → use measured, flag legacy.
        let (mv, legacy) = choose_ceiling_mv(&[], 843);
        assert_eq!(mv, 843);
        assert!(legacy);
    }
}
