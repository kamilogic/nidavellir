//! Error reports and Forge summaries (user decisions 2026-10-16/17): one consent in the program,
//! then automatic. The Core only writes reports to `reports/pending/`. The program sends each one to
//! the relay (`tools/report-relay`) and acknowledges it, so this process never talks to the network.
//! Personal data stays out: user folders, the computer name and GPU identifiers are scrubbed, and a
//! random install ID stands in for the machine. Hardware models, driver versions and the run's
//! numbers stay, because the fixes need them.

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use nidavellir_core::ipc::{PowerSweepPoint, PowerSweepProgress};
use regex::Regex;

/// Reports waiting for the program; the oldest go first when more arrive.
const MAX_PENDING: usize = 20;
/// Sent reports kept for Settings.
const MAX_SENT: usize = 5;
const CORE_LOG_TAIL_BYTES: u64 = 24_000;
const FORGE_LOG_MAX_CHARS: usize = 24_000;
const EVENTS_TAIL: usize = 60;

static LAST_PANIC: Mutex<Option<String>> = Mutex::new(None);

/// Called by the panic hook, so it never waits for the lock.
pub(crate) fn note_panic(text: String) {
    if let Ok(mut last) = LAST_PANIC.try_lock() {
        *last = Some(text);
    }
}

fn data_dir() -> PathBuf {
    nidavellir_core::safe_loop::default_data_dir()
}

fn pending_dir() -> PathBuf {
    data_dir().join("reports").join("pending")
}

fn sent_dir() -> PathBuf {
    data_dir().join("reports").join("sent")
}

/// After a Forge worker exits: a report when the run failed or needs a person, a summary when it
/// finished. Expected stops (the user's pause, a staircase-edge crash) send nothing.
pub(crate) fn after_run(prog: &PowerSweepProgress, panicked: bool, expected_stop: bool) {
    let (kind, class) = match (panicked, prog.phase.as_str()) {
        (true, _) => ("error", "worker_panic"),
        (false, "finished") => ("summary", "forge_complete"),
        (false, "provisional") => ("summary", "forge_preview"),
        (false, "paused") if prog.stock_crash_paused => ("error", "stock_crash"),
        (false, "interrupted") if !expected_stop => ("error", "interrupted"),
        (false, "incomplete") if prog.stock_crashes > 1 => ("error", "stock_crash_repeat"),
        (false, "incomplete") => ("error", "incomplete"),
        (false, "needs_attention") => ("error", "needs_attention"),
        (false, "field_rejected") => ("error", "field_rejected"),
        _ => return,
    };
    let message = prog.note.clone().unwrap_or_else(|| format!("Forge ended in phase {}", prog.phase));
    queue(kind, class, message, Some(prog.clone()));
}

/// The Sentinel contained a driver crash or a failed GPU check while a forged profile was applied.
pub(crate) fn field_failure(kind: &str, target_mhz: u32, anchor_mv: u32) {
    let class = if kind == "tdr" { "field_tdr" } else { "field_check" };
    queue("error", class, format!("{kind} while a forged profile was applied at {target_mhz} MHz @ {anchor_mv} mV"), None);
}

/// Builds and writes a report on its own thread: the Forge worker and the Sentinel never wait for
/// log reads or event queries.
pub(crate) fn queue(kind: &'static str, class: &'static str, message: String, prog: Option<PowerSweepProgress>) {
    std::thread::spawn(move || {
        if let Err(error) = write_pending(kind, class, &message, prog.as_ref()) {
            tracing::warn!("report {class} not queued ({error})");
        }
    });
}

fn write_pending(kind: &str, class: &str, message: &str, prog: Option<&PowerSweepProgress>) -> Result<(), String> {
    let system = system_facts();
    let nvml = nidavellir_core::nvml_gpu::read_nvidia_gpus_nvml().into_iter().next();
    let gpu = nvml.as_ref().map(|reading| reading.name.clone()).unwrap_or_else(|| "unknown".into());
    let computer = std::env::var("COMPUTERNAME").unwrap_or_default();
    let users = profile_names();
    let mut gpu_ids: Vec<String> = nvml.as_ref().and_then(|reading| reading.uuid.clone()).into_iter().collect();
    gpu_ids.extend(prog.and_then(|p| p.resume_compatibility.as_ref()).map(|identity| identity.gpu_key.clone()));
    let clean = |text: &str| scrub(text, &computer, &users, &gpu_ids);
    let panic = LAST_PANIC.lock().ok().and_then(|last| last.clone());
    let report = serde_json::json!({
        "schema": 1,
        "kind": kind,
        "class": class,
        "install_id": install_id(),
        "app_version": env!("CARGO_PKG_VERSION"),
        "build": env!("NIDAVELLIR_BUILD_REVISION"),
        "windows": system.windows,
        "gpu": gpu,
        "gpu_family": gpu_family(&gpu),
        "vram_mb": nvml.as_ref().and_then(|reading| reading.vram_total_mb),
        "driver": system.driver,
        "forge": prog.map(forge_facts),
        "message": clean(message),
        "panic": panic.as_deref().map(clean),
        "events": prog.map(|p| events(p).iter().map(|line| clean(line)).collect::<Vec<_>>()),
        "result": prog.filter(|_| kind == "summary").map(result),
        "forge_log": prog.and_then(|p| crate::gpu_power_sweep::forge_log_text(p).ok()).map(|log| clean(&head_and_tail(&log.text, FORGE_LOG_MAX_CHARS))),
        "driver_events": clean(&driver_events()),
        "core_log": clean(&core_log_tail()),
        "created_at": nidavellir_core::f2_observation::now_rfc3339(),
    });
    let dir = pending_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let body = serde_json::to_vec(&report).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{stamp}-{kind}-{class}.json")), body).map_err(|e| e.to_string())?;
    keep_newest(&dir, MAX_PENDING);
    Ok(())
}

/// The program sent this report: move it to `sent/`, keeping the last few for Settings.
pub(crate) fn mark_sent(id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err("invalid report id".into());
    }
    let sent = sent_dir();
    std::fs::create_dir_all(&sent).map_err(|e| e.to_string())?;
    std::fs::rename(pending_dir().join(id), sent.join(id)).map_err(|e| format!("report {id}: {e}"))?;
    keep_newest(&sent, MAX_SENT);
    Ok(())
}

/// Only names this module writes: `<millis>-<kind>-<class>.json`, never a path.
fn valid_id(id: &str) -> bool {
    id.len() <= 120
        && id.ends_with(".json")
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !id.contains("..")
}

/// Names start with the creation time in milliseconds, so name order is age order.
fn keep_newest(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut names: Vec<_> = entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|ext| ext == "json")).collect();
    names.sort();
    let excess = names.len().saturating_sub(keep);
    for old in names.into_iter().take(excess) {
        let _ = std::fs::remove_file(old);
    }
}

/// A random ID made once per installation; it stands in for any hardware identifier.
fn install_id() -> String {
    let path = data_dir().join("install_id");
    if let Ok(id) = std::fs::read_to_string(&path) {
        let id = id.trim();
        if id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit()) {
            return id.to_owned();
        }
    }
    let id = random_hex();
    let _ = std::fs::write(&path, &id);
    id
}

fn random_hex() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    (0..2u64)
        .map(|salt| {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u64(salt);
            hasher.write_u128(nanos);
            format!("{:016x}", hasher.finish())
        })
        .collect()
}

/// "NVIDIA GeForce RTX 5070 Ti" is family "RTX 50": fixes target GPU families, not single cards.
fn gpu_family(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    words
        .windows(2)
        .find_map(|pair| {
            let generation = pair[1].get(..2).filter(|digits| pair[1].len() >= 3 && digits.chars().all(|c| c.is_ascii_digit()))?;
            matches!(pair[0], "RTX" | "GTX" | "MX").then(|| format!("{} {generation}", pair[0]))
        })
        .unwrap_or_else(|| name.trim().chars().take(40).collect())
}

static USER_FOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)([\\/])users([\\/])[^\\/\s"']+"#).expect("static pattern"));
static GPU_UUID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"GPU-[0-9A-Fa-f]{8}(-[0-9A-Fa-f]{4}){3}-[0-9A-Fa-f]{12}").expect("static pattern"));
// Defense in depth: nothing logs these today.
static IPV4: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").expect("static pattern"));
static SID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bS-1-5-[0-9-]+").expect("static pattern"));
static MAC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b").expect("static pattern"));

/// Removes what could identify the person or the machine. `users` are the PC's profile names: the
/// Core runs as SYSTEM and cannot tell which one is signed in, so every one is removed, as a whole
/// word (privacy audit 2026-10-17).
fn scrub(text: &str, computer: &str, users: &[String], gpu_ids: &[String]) -> String {
    // Whole profile names first: the folder pattern stops at a space and would leave "Doe" of
    // "John Doe" behind.
    let mut out = text.to_owned();
    for user in users.iter().filter(|user| user.chars().count() >= 2) {
        if let Ok(name) = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(user))) {
            out = name.replace_all(&out, "<user>").into_owned();
        }
    }
    out = USER_FOLDER.replace_all(&out, "${1}Users${2}<user>").into_owned();
    if computer.len() >= 2 {
        if let Ok(name) = Regex::new(&format!("(?i){}", regex::escape(computer))) {
            out = name.replace_all(&out, "<pc>").into_owned();
        }
    }
    for id in gpu_ids.iter().filter(|id| id.len() >= 6) {
        out = out.replace(id.as_str(), "<gpu-id>");
    }
    out = GPU_UUID.replace_all(&out, "<gpu-id>").into_owned();
    out = SID.replace_all(&out, "<sid>").into_owned();
    out = MAC.replace_all(&out, "<mac>").into_owned();
    IPV4.replace_all(&out, "<ip>").into_owned()
}

/// Every local profile folder name under `C:\Users`, the built-in ones aside.
fn profile_names() -> Vec<String> {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    std::fs::read_dir(format!(r"{drive}\Users"))
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| !matches!(name.to_ascii_lowercase().as_str(), "public" | "default" | "default user" | "all users" | "defaultuser0"))
                .collect()
        })
        .unwrap_or_default()
}

/// A report never waits on a wedged driver or event log: past `limit` the command is stopped and
/// whatever it printed is kept. The output is read while the command runs, so a full pipe cannot
/// stall it, and decoded leniently (event text may not be UTF-8).
fn output_within(command: &mut std::process::Command, limit: std::time::Duration) -> String {
    use std::io::Read;
    let Ok(mut child) = command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn() else {
        return String::new();
    };
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stdout) = stdout {
            let _ = stdout.read_to_end(&mut bytes);
        }
        bytes
    });
    let deadline = std::time::Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned()
}

fn forge_facts(prog: &PowerSweepProgress) -> serde_json::Value {
    serde_json::json!({
        "run": prog.run_id,
        "mode": prog.mode,
        "learning": prog.learning,
        "phase": prog.phase,
        "task": prog.current_task,
        "stock_crashes": prog.stock_crashes,
        "elapsed_ms": prog.elapsed_ms,
        "stock_mhz": prog.stock_clock_mhz,
        "attempts": prog.discovery_search.as_ref().map(|search| search.attempts_used),
    })
}

fn events(prog: &PowerSweepProgress) -> Vec<String> {
    let start = prog.log_entries.len().saturating_sub(EVENTS_TAIL);
    prog.log_entries[start..]
        .iter()
        .map(|entry| {
            let detail = entry.detail.as_deref().map(|detail| format!(" ({detail})")).unwrap_or_default();
            format!("{:?} {}: {}{detail}", entry.tone, entry.phase, entry.text)
        })
        .collect()
}

/// What a finished run produced, for tuning across GPU families.
fn result(prog: &PowerSweepProgress) -> serde_json::Value {
    let point = |p: &Option<PowerSweepPoint>| {
        p.as_ref().map(|pt| {
            serde_json::json!({
                "mhz": pt.target_clock_mhz.unwrap_or(pt.clock_mhz),
                "mv": pt.vf_table_voltage_mv.or(pt.boundary_voltage_mv).unwrap_or(pt.voltage_mv),
                "watts_p99": pt.power_p99_w.unwrap_or(pt.power_w),
            })
        })
    };
    serde_json::json!({
        "qualified": prog.profiles_qualified,
        "godforge": point(&prog.godforge),
        "brokkrs": point(&prog.brokkrs),
        "deep_calm": point(&prog.deep_calm),
        "bands": prog.discovery_search.as_ref().map(|search| &search.bands),
    })
}

fn head_and_tail(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_owned();
    }
    let half = max_chars / 2;
    let head: String = text.chars().take(half).collect();
    let tail: String = text.chars().skip(count - half).collect();
    format!("{head}\n…\n{tail}")
}

fn core_log_tail() -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(data_dir().join("core.log")) else { return String::new() };
    let length = file.metadata().map_or(0, |meta| meta.len());
    let _ = file.seek(SeekFrom::Start(length.saturating_sub(CORE_LOG_TAIL_BYTES)));
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    // Start at a whole line.
    text.split_once('\n').map_or_else(|| text.to_string(), |(_, rest)| rest.to_owned())
}

fn system32(program: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    Path::new(&root).join("System32").join(program)
}

/// The latest NVIDIA driver and display events from the Windows System log.
fn driver_events() -> String {
    output_within(
        std::process::Command::new(system32("wevtutil.exe"))
            .args(["qe", "System", "/q:*[System[Provider[@Name='nvlddmkm' or @Name='Display']]]", "/c:10", "/rd:true", "/f:text"]),
        std::time::Duration::from_secs(15),
    )
}

struct SystemFacts {
    windows: String,
    driver: String,
}

/// Windows build and the NVIDIA driver version ("576.80"), read without touching the GPU.
fn system_facts() -> SystemFacts {
    let script = "$v = Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion'; \"$($v.CurrentBuild).$($v.UBR) $($v.DisplayVersion)\"; (Get-CimInstance Win32_VideoController | Where-Object { $_.Name -match 'NVIDIA' } | Select-Object -First 1).DriverVersion";
    let output = output_within(
        std::process::Command::new(system32("WindowsPowerShell\\v1.0\\powershell.exe"))
            .args(["-NoProfile", "-NonInteractive", "-Command", script]),
        std::time::Duration::from_secs(20),
    );
    let mut lines = output.lines().map(str::trim);
    let windows = lines.next().filter(|line| !line.is_empty()).unwrap_or("unknown").to_owned();
    let driver = lines.next().map(nvidia_driver_version).unwrap_or_else(|| "unknown".into());
    SystemFacts { windows, driver }
}

/// Windows lists the NVIDIA driver as "32.0.15.7680"; its last five digits are "576.80".
fn nvidia_driver_version(windows_version: &str) -> String {
    let digits: String = windows_version.chars().filter(char::is_ascii_digit).collect();
    if digits.len() < 5 {
        return if windows_version.is_empty() { "unknown".into() } else { windows_version.to_owned() };
    }
    let last = &digits[digits.len() - 5..];
    format!("{}.{}", &last[..3], &last[3..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_name_gpu_families_and_scrub_what_identifies_the_person_or_the_machine() {
        assert_eq!(gpu_family("NVIDIA GeForce RTX 5070"), "RTX 50");
        assert_eq!(gpu_family("NVIDIA GeForce RTX 3060 Ti"), "RTX 30");
        assert_eq!(gpu_family("NVIDIA GeForce GTX 1660 SUPER"), "GTX 16");
        assert_eq!(gpu_family("NVIDIA RTX A4000"), "NVIDIA RTX A4000");
        assert_eq!(nvidia_driver_version("32.0.15.7680"), "576.80");
        assert_eq!(nvidia_driver_version(""), "unknown");

        let ids = vec!["GPU-0a1b2c3d-1111-2222-3333-444455556666".to_string()];
        let users = vec!["leona".to_string(), "John Doe".to_string(), "dev".to_string()];
        let text = r"C:\Users\Leona\AppData\x C:/users/fulano/y C:\Users\John Doe\z running as LEONA HKEY_USERS\S-1-5-21-1004336348-1177238915-682003330-1001 Computer: DESKTOP-ABC12 desktop-abc12 GPU-0a1b2c3d-1111-2222-3333-444455556666 GPU-ffffffff-1111-2222-3333-444455556666 at 192.168.0.12 mac 3C:7C:3F:1A:2B:4D device ready RTX 5070 576.80 Windows 26200.6584";
        let clean = scrub(text, "DESKTOP-ABC12", &users, &ids);
        for leak in ["leona", "fulano", "john", "doe", "desktop-abc12", "s-1-5-21", "192.168", "3c:7c", "gpu-0a1b2c3d", "gpu-ffffffff"] {
            assert!(!clean.to_lowercase().contains(leak), "{leak} leaked: {clean}");
        }
        assert!(clean.contains(r"C:\Users\<user>\AppData") && clean.contains("device ready"), "whole words only: {clean}");
        assert!(clean.contains("RTX 5070 576.80") && clean.contains("26200.6584"), "versions stay: {clean}");

        assert!(valid_id("1791962702636-error-stock_crash.json"));
        assert!(!valid_id(r"..\..\Windows\win.ini") && !valid_id("a/b.json") && !valid_id("x.txt"));
        assert_eq!(head_and_tail("abcdef", 4), "ab\n…\nef");
    }
}
