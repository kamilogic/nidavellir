//! Error reports and Forge summaries (user decisions 2026-10-16/17): one consent in the program,
//! then automatic. The Core writes each report to `reports/pending/` and sends it to the relay
//! (`tools/report-relay`) itself, so reports go out with the program closed, after an automatic
//! Windows restart and while a Forge is stuck (user decision 2026-10-17). The program mirrors the
//! consent into `forge_options.json`; nothing goes out without it, except a "Send now" click.
//! Personal data stays out: user folders, the computer name and GPU identifiers are scrubbed, and a
//! random install ID stands in for the machine. Hardware models, driver versions and the run's
//! numbers stay, because the fixes need them.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use nidavellir_core::ipc::{PowerSweepPoint, PowerSweepProgress};
use regex::Regex;

const RELAY: &str = "https://nidavellir-reports.kamilogic.workers.dev/report";
/// One waiting report a minute; "Send now" sends them all a few seconds apart, because the relay
/// groups same-signature reports and its label search lags a few seconds.
const SEND_EVERY: Duration = Duration::from_secs(60);
const SPACING: Duration = Duration::from_secs(3);
/// When nothing else needs the Core, it stays up this long after writing a report, and this long
/// trying to send one.
const WRITE_HOLD_MS: u64 = 60_000;
const SEND_HOLD_MS: u64 = 10 * 60_000;
/// Reports waiting to be sent; the oldest go first when more arrive.
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
/// finished or the user paused it. A staircase-edge crash sends nothing: the final summary has it.
pub(crate) fn after_run(prog: &PowerSweepProgress, panicked: bool, expected_stop: bool) {
    let Some((kind, class)) = run_report(prog, panicked, expected_stop) else { return };
    let message = prog.note.clone().unwrap_or_else(|| format!("Forge ended in phase {}", prog.phase));
    queue(kind, class, message, Some(prog.clone()));
}

fn run_report(prog: &PowerSweepProgress, panicked: bool, expected_stop: bool) -> Option<(&'static str, &'static str)> {
    Some(match (panicked, prog.phase.as_str()) {
        (true, _) => ("error", "worker_panic"),
        (false, "finished") => ("summary", "forge_complete"),
        (false, "provisional") => ("summary", "forge_preview"),
        (false, "paused") if prog.stock_crash_paused => ("error", "stock_crash"),
        // What a run the user paused learned, which otherwise never leaves the PC (2026-10-21: an
        // RTX 40 run stopped after 26 minutes sent nothing).
        (false, "paused") => ("summary", "forge_paused"),
        (false, "interrupted") if !expected_stop => ("error", "interrupted"),
        (false, "incomplete") if prog.stock_crashes > 1 => ("error", "stock_crash_repeat"),
        (false, "incomplete") => ("error", "incomplete"),
        (false, "needs_attention") => ("error", "needs_attention"),
        (false, "field_rejected") => ("error", "field_rejected"),
        _ => return None,
    })
}

/// The Sentinel contained a driver crash or a failed GPU check while a forged profile was applied.
pub(crate) fn field_failure(kind: &str, target_mhz: u32, anchor_mv: u32) {
    let class = if kind == "tdr" { "field_tdr" } else { "field_check" };
    queue("error", class, format!("{kind} while a forged profile was applied at {target_mhz} MHz @ {anchor_mv} mV"), None);
}

/// Present while a Forge worker runs. Found at the next start, it means the Core died during a run,
/// also one cut off before its first checkpoint: on 2026-10-20 a Core crash in the stock checks of
/// a Clean Run sent nothing.
fn forge_mark() -> PathBuf {
    data_dir().join("reports").join("forge_running")
}

pub(crate) fn forge_running(running: bool) {
    let mark = forge_mark();
    if running {
        let _ = mark.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(&mark, b"");
    } else {
        let _ = std::fs::remove_file(&mark);
    }
}

/// Whether the previous Core stopped with a Forge worker running; clears the mark.
pub(crate) fn forge_was_running() -> bool {
    std::fs::remove_file(forge_mark()).is_ok()
}

/// Builds and writes a report on its own thread: the Forge worker and the Sentinel never wait for
/// log reads or event queries.
pub(crate) fn queue(kind: &'static str, class: &'static str, message: String, prog: Option<PowerSweepProgress>) {
    WRITING_UNTIL_MS.fetch_max(now_ms() + WRITE_HOLD_MS, Ordering::SeqCst);
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
        "system": system
            .details
            .iter()
            .map(|(key, value)| (key.clone(), serde_json::Value::String(clean(value.as_str().unwrap_or_default()))))
            .collect::<serde_json::Map<_, _>>(),
        "nvidia_smi": clean(&nvidia_smi()),
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

/// Set by "Send now"; the sender picks it up within a second.
static SEND_NOW: AtomicBool = AtomicBool::new(false);
/// A report being written keeps the Core up until then (epoch ms).
static WRITING_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
/// When sending started failing (epoch ms); 0 while it works.
static FAILING_SINCE_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis() as u64)
}

/// The choice the program mirrors from its Settings; no answer yet means no.
fn consented() -> bool {
    crate::gpu_power_sweep::load_forge_option("error_reports", false)
}

/// "Send now" in Settings: every waiting report, with or without automatic sending.
pub(crate) fn send_now() {
    SEND_NOW.store(true, Ordering::SeqCst);
}

/// The Core stays up for reports alone while one is being written, or while one waits with consent
/// and sending has not been failing for [`SEND_HOLD_MS`].
pub(crate) fn needs_core() -> bool {
    let now = now_ms();
    if now < WRITING_UNTIL_MS.load(Ordering::SeqCst) {
        return true;
    }
    let failing_since = FAILING_SINCE_MS.load(Ordering::SeqCst);
    (failing_since == 0 || now.saturating_sub(failing_since) < SEND_HOLD_MS)
        && consented()
        && !waiting(&pending_dir()).is_empty()
}

/// The sender: one waiting report a minute with consent, all of them on "Send now", and one report
/// when a running Forge stops publishing progress.
pub(crate) fn spawn_sender() {
    std::thread::spawn(|| {
        // Room for the network after a boot.
        let mut next = Instant::now() + Duration::from_secs(10);
        let mut stalled_run = None;
        while !crate::shutdown::is_requested() {
            std::thread::sleep(Duration::from_secs(1));
            let manual = SEND_NOW.swap(false, Ordering::SeqCst);
            if !manual && Instant::now() < next {
                continue;
            }
            if !manual {
                next = Instant::now() + SEND_EVERY;
                report_stalled_forge(&mut stalled_run);
                if !consented() {
                    continue;
                }
            }
            match send_pending(RELAY, &pending_dir(), &sent_dir(), if manual { MAX_PENDING } else { 1 }) {
                Ok(_) => FAILING_SINCE_MS.store(0, Ordering::SeqCst),
                Err(error) => {
                    if FAILING_SINCE_MS.compare_exchange(0, now_ms(), Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                        tracing::warn!("reports not sent ({error}); retrying every minute");
                    }
                }
            }
        }
    });
}

/// Once per run: a Forge that publishes nothing for 45 min is stuck where no other report sees it.
fn report_stalled_forge(reported_run: &mut Option<String>) {
    let Some(progress) = crate::gpu_power_sweep::stalled_forge_progress() else { return };
    let run = progress.run_id.clone().unwrap_or_default();
    if reported_run.as_deref() == Some(run.as_str()) {
        return;
    }
    *reported_run = Some(run);
    queue("error", "forge_stalled", "The Forge published no progress for 45 minutes while running".into(), Some(progress));
}

/// Report files in `dir`, oldest first: their names start with the creation time.
fn waiting(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    paths.retain(|path| path.extension().is_some_and(|ext| ext == "json"));
    paths.sort();
    paths
}

/// Posts up to `limit` waiting reports, oldest first. Each one the relay took, or refused for good,
/// moves to `sent`. Returns how many it took.
fn send_pending(relay: &str, pending: &Path, sent: &Path, limit: usize) -> Result<usize, String> {
    let reports = waiting(pending);
    if reports.is_empty() {
        return Ok(0);
    }
    let client = client()?;
    let mut delivered = 0;
    for (index, path) in reports.into_iter().take(limit).enumerate() {
        if index > 0 {
            std::thread::sleep(SPACING);
        }
        let body = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let status = client
            .post(relay)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .map_err(|e| format!("relay: {e}"))?
            .status();
        // Too many or a relay fault: retry later. Any other refusal never changes: drop it.
        if status.as_u16() == 429 || status.is_server_error() {
            return Err(format!("relay answered {status}"));
        }
        if status.is_success() {
            delivered += 1;
        } else {
            tracing::warn!("report {} refused by the relay ({status}); dropped", path.display());
        }
        move_to(&path, sent)?;
    }
    Ok(delivered)
}

/// rustls with ring as the process provider: the stack the program used before.
fn client() -> Result<reqwest::blocking::Client, String> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    reqwest::blocking::Client::builder()
        .user_agent(concat!("Nidavellir/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

/// Moves a handled report to `sent`, keeping the last few for Settings.
fn move_to(report: &Path, sent: &Path) -> Result<(), String> {
    std::fs::create_dir_all(sent).map_err(|e| e.to_string())?;
    let name = report.file_name().ok_or("report without a file name")?;
    std::fs::rename(report, sent.join(name)).map_err(|e| format!("{}: {e}", report.display()))?;
    keep_newest(sent, MAX_SENT);
    Ok(())
}

/// A program from before 0.8.1 sent this report itself.
pub(crate) fn mark_sent(id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err("invalid report id".into());
    }
    move_to(&pending_dir().join(id), &sent_dir())
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
// Defense in depth: nothing logs these today. Longer dotted runs are versions: a VBIOS
// "98.05.45.00.83" lost its first four parts to this (field report 2026-10-20).
static IPV4: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{1,3}(?:\.\d{1,3})+\b").expect("static pattern"));
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
    // "0.8.4.0" is Nidavellir's own file version in Windows' crash events, not an address.
    IPV4.replace_all(&out, |found: &regex::Captures| {
        if found[0].split('.').count() == 4 && !found[0].starts_with("0.") { "<ip>".to_owned() } else { found[0].to_owned() }
    })
    .into_owned()
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
    let (sender, output) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stdout) = stdout {
            let _ = stdout.read_to_end(&mut bytes);
        }
        let _ = sender.send(bytes);
    });
    let deadline = std::time::Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(100)),
            _ => {
                // No wait after the kill: a command stuck inside the driver ends only when the
                // driver lets it, and the report must not wait for that.
                let _ = child.kill();
                break;
            }
        }
    }
    let output = output.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or_default();
    String::from_utf8_lossy(&output).into_owned()
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

/// The latest GPU driver and crash events from Windows with their data, since an nvlddmkm 153 has
/// no message text: driver resets (nvlddmkm, Display), blue screens (BugCheck), unexpected restarts
/// (Kernel-Power 41), the kernel reports Windows wrote for them (LiveKernelEvent 141/117) and
/// crashes of Nidavellir itself (Application Error 1000), which leave no line in core.log.
fn driver_events() -> String {
    // Separate queries, so a run of unexpected restarts never crowds the driver resets out.
    let query = |log: &str, filter: &str, count: u32| {
        event_lines(&output_within(
            std::process::Command::new(system32("wevtutil.exe"))
                .args(["qe", log, &format!("/q:{filter}"), &format!("/c:{count}"), "/rd:true", "/f:xml"]),
            std::time::Duration::from_secs(15),
        ))
    };
    let mut lines = query("System", "*[System[Provider[@Name='nvlddmkm' or @Name='Display']]]", 12);
    lines.extend(query(
        "System",
        "*[System[Provider[@Name='Microsoft-Windows-WER-SystemErrorReporting'] or (Provider[@Name='Microsoft-Windows-Kernel-Power'] and EventID=41)]]",
        4,
    ));
    lines.extend(
        query("Application", "*[System[Provider[@Name='Windows Error Reporting'] and EventID=1001]]", 15)
            .into_iter()
            .filter(|line| line.contains("LiveKernelEvent") || line.contains("BlueScreen"))
            .take(6),
    );
    lines.extend(
        query("Application", "*[System[Provider[@Name='Application Error'] and EventID=1000]]", 10)
            .into_iter()
            .filter(|line| line.to_ascii_lowercase().contains("nidavellir"))
            .take(3),
    );
    lines.join("\n")
}

static EVENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<Event[ >].*?</Event>").expect("static pattern"));
static EVENT_PROVIDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"<Provider Name=['"]([^'"]*)"#).expect("static pattern"));
static EVENT_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<EventID[^>]*>(\d+)</EventID>").expect("static pattern"));
static EVENT_TIME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"SystemTime=['"]([^'"]*)"#).expect("static pattern"));
static EVENT_DATA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<Data(?: Name=['][^']*[']| Name=[^>]*)?>(.*?)</Data>").expect("static pattern"));

/// `time provider id: data | data` per event of a wevtutil XML answer, newest first.
fn event_lines(xml: &str) -> Vec<String> {
    EVENT
        .find_iter(xml)
        .map(|event| {
            let event = event.as_str();
            let field = |pattern: &Regex| pattern.captures(event).map(|found| found[1].to_owned()).unwrap_or_default();
            let data: Vec<String> = EVENT_DATA
                .captures_iter(event)
                .map(|found| unescape_xml(found[1].trim()))
                .filter(|value| !value.is_empty())
                .collect();
            let line = format!("{} {} {}: {}", field(&EVENT_TIME), field(&EVENT_PROVIDER), field(&EVENT_ID), data.join(" | "));
            line.chars().take(500).collect()
        })
        .collect()
}

fn unescape_xml(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The NVIDIA tool's view of the card: exact chip and board IDs, VBIOS, PCIe link, power limits and
/// memory, which differ across the 10- to 50-series. Header line plus values; empty without it.
fn nvidia_smi() -> String {
    const FIELDS: &str = "name,pci.device_id,pci.sub_device_id,vbios_version,pcie.link.gen.current,pcie.link.gen.max,pcie.link.width.current,pcie.link.width.max,power.default_limit,power.min_limit,power.max_limit,clocks.max.graphics,clocks.max.memory,memory.total,memory.used,temperature.gpu";
    let legacy = Path::new(r"C:\Program Files\NVIDIA Corporation\NVSMI\nvidia-smi.exe").to_path_buf();
    [system32("nvidia-smi.exe"), legacy]
        .into_iter()
        .filter(|path| path.exists())
        .map(|path| {
            output_within(
                std::process::Command::new(path).args([format!("--query-gpu={FIELDS}"), "--format=csv".into()]),
                std::time::Duration::from_secs(10),
            )
        })
        .map(|output| output.trim().to_owned())
        .find(|output| !output.is_empty())
        .unwrap_or_default()
}

struct SystemFacts {
    windows: String,
    driver: String,
    /// CPU, memory, GPU scheduling and timeout settings, displays and the GPU tools that are open.
    details: serde_json::Map<String, serde_json::Value>,
}

/// Windows build, the NVIDIA driver version ("576.80") and the machine around the GPU, read without
/// touching it. GPU tools are reported only by name from a fixed list (overlays and other tuners
/// that can fight the Forge's curve), never the rest of what runs.
fn system_facts() -> SystemFacts {
    let script = [
        r#"$v = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'"#,
        r#""windows=$($v.CurrentBuild).$($v.UBR) $($v.DisplayVersion)""#,
        r#""driver=$((Get-CimInstance Win32_VideoController | Where-Object { $_.Name -match 'NVIDIA' } | Select-Object -First 1).DriverVersion)""#,
        r#""cpu=$((Get-CimInstance Win32_Processor | Select-Object -First 1).Name)""#,
        r#""ram_gb=$([math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB))""#,
        r#"$g = Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\GraphicsDrivers' -ErrorAction SilentlyContinue"#,
        r#""gpu_scheduling=$($g.HwSchMode)""#,
        r#""tdr_settings=delay $($g.TdrDelay) ddi $($g.TdrDdiDelay) level $($g.TdrLevel)""#,
        r#""displays=$((Get-CimInstance Win32_VideoController | ForEach-Object { '{0} {1}x{2}@{3}' -f $_.Name, $_.CurrentHorizontalResolution, $_.CurrentVerticalResolution, $_.CurrentRefreshRate }) -join ' / ')""#,
        r#""gpu_tools=$((Get-Process -Name MSIAfterburner,RTSS,PrecisionX_x64,GPUTweakIII,'NVIDIA app','NVIDIA Overlay',Discord,steam,obs64,HWiNFO64,GPU-Z,OCCT,FurMark -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name -Unique) -join ', ')""#,
    ]
    .join("; ");
    let output = output_within(
        std::process::Command::new(system32("WindowsPowerShell\\v1.0\\powershell.exe"))
            .args(["-NoProfile", "-NonInteractive", "-Command", &script]),
        std::time::Duration::from_secs(25),
    );
    let mut details = serde_json::Map::new();
    let (mut windows, mut driver) = ("unknown".to_owned(), "unknown".to_owned());
    for (key, value) in output.lines().filter_map(|line| line.trim().split_once('=')) {
        let value = value.trim();
        match key {
            "windows" if !value.is_empty() => windows = value.to_owned(),
            "driver" if !value.is_empty() => driver = nvidia_driver_version(value),
            "windows" | "driver" => {}
            _ => {
                details.insert(key.to_owned(), serde_json::Value::String(value.to_owned()));
            }
        }
    }
    SystemFacts { windows, driver, details }
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
        let text = r"C:\Users\Leona\AppData\x C:/users/fulano/y C:\Users\John Doe\z running as LEONA HKEY_USERS\S-1-5-21-1004336348-1177238915-682003330-1001 Computer: DESKTOP-ABC12 desktop-abc12 GPU-0a1b2c3d-1111-2222-3333-444455556666 GPU-ffffffff-1111-2222-3333-444455556666 at 192.168.0.12 mac 3C:7C:3F:1A:2B:4D device ready RTX 5070 576.80 Windows 26200.6584 VBIOS 98.05.45.00.83 nidavellir-service.exe | 0.8.4.0";
        let clean = scrub(text, "DESKTOP-ABC12", &users, &ids);
        for leak in ["leona", "fulano", "john", "doe", "desktop-abc12", "s-1-5-21", "192.168", "3c:7c", "gpu-0a1b2c3d", "gpu-ffffffff"] {
            assert!(!clean.to_lowercase().contains(leak), "{leak} leaked: {clean}");
        }
        assert!(clean.contains(r"C:\Users\<user>\AppData") && clean.contains("device ready"), "whole words only: {clean}");
        assert!(clean.contains("RTX 5070 576.80") && clean.contains("26200.6584") && clean.contains("VBIOS 98.05.45.00.83") && clean.contains("| 0.8.4.0"), "versions stay: {clean}");

        assert!(valid_id("1791962702636-error-stock_crash.json"));
        assert!(!valid_id(r"..\..\Windows\win.ini") && !valid_id("a/b.json") && !valid_id("x.txt"));
        assert_eq!(head_and_tail("abcdef", 4), "ab\n…\nef");
    }

    #[test]
    fn a_paused_run_sends_a_summary_and_a_stock_crash_pause_stays_an_error() {
        let paused = PowerSweepProgress { phase: "paused".into(), ..Default::default() };
        assert_eq!(run_report(&paused, false, true), Some(("summary", "forge_paused")));
        assert_eq!(run_report(&paused, false, false), Some(("summary", "forge_paused")));
        let crashed = PowerSweepProgress { stock_crash_paused: true, ..paused.clone() };
        assert_eq!(run_report(&crashed, false, false), Some(("error", "stock_crash")));
        let edge = PowerSweepProgress { phase: "interrupted".into(), ..Default::default() };
        assert_eq!(run_report(&edge, false, true), None, "a staircase-edge crash sends nothing");
        assert_eq!(run_report(&edge, true, true), Some(("error", "worker_panic")));
    }

    #[test]
    fn windows_events_keep_their_data_because_a_driver_reset_has_no_message_text() {
        let xml = concat!(
            "<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='nvlddmkm'/>",
            "<EventID Qualifiers='49322'>153</EventID><TimeCreated SystemTime='2026-10-19T10:19:22.3270000Z'/>",
            "<Computer>DESKTOP-ABC12</Computer></System><EventData><Data>\\Device\\Video3</Data>",
            "<Data>Error occurred on GPUID: 100</Data><Data></Data></EventData></Event>",
            "<Event xmlns='x'><System><Provider Name='Windows Error Reporting'/><EventID>1001</EventID>",
            "<TimeCreated SystemTime='2026-10-19T10:19:40.0000000Z'/></System><EventData>",
            "<Data>0</Data><Data>LiveKernelEvent</Data><Data>141</Data><Data>a &amp; b</Data></EventData></Event>",
        );
        let lines = event_lines(xml);
        assert_eq!(lines, [
            r"2026-10-19T10:19:22.3270000Z nvlddmkm 153: \Device\Video3 | Error occurred on GPUID: 100",
            "2026-10-19T10:19:40.0000000Z Windows Error Reporting 1001: 0 | LiveKernelEvent | 141 | a & b",
        ]);
        assert!(lines.iter().all(|line| !line.contains("DESKTOP")), "the computer name stays out");
    }

    #[test]
    fn the_core_sends_the_oldest_report_first_and_keeps_only_what_the_relay_asks_to_retry() {
        use std::io::{Read, Write};
        let dir = std::env::temp_dir().join(format!("nidavellir-send-{}", std::process::id()));
        let (pending, sent) = (dir.join("pending"), dir.join("sent"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&pending).unwrap();
        for (name, body) in [("2-error-b.json", "b"), ("1-error-a.json", "a"), ("3-error-c.json", "c")] {
            std::fs::write(pending.join(name), body).unwrap();
        }
        // A stand-in relay: it takes the first report, refuses the second and fails on the third.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let relay = format!("http://{}/report", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            ["200 OK", "400 Bad Request", "503 Service Unavailable"].map(|status| {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                let body = loop {
                    let read = stream.read(&mut buffer).unwrap();
                    request.extend_from_slice(&buffer[..read]);
                    let text = String::from_utf8_lossy(&request).into_owned();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let length = head.lines().find_map(|line| {
                            let line = line.to_ascii_lowercase();
                            line.strip_prefix("content-length:").and_then(|value| value.trim().parse::<usize>().ok())
                        });
                        if body.len() >= length.unwrap_or(0) {
                            break body.to_owned();
                        }
                    }
                    assert!(read > 0, "request cut short");
                };
                write!(stream, "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                body
            })
        });
        assert_eq!(send_pending(&relay, &pending, &sent, 1), Ok(1), "taken");
        assert_eq!(send_pending(&relay, &pending, &sent, 1), Ok(0), "refused for good: dropped");
        assert!(send_pending(&relay, &pending, &sent, 1).is_err(), "a relay fault is retried later");
        assert_eq!(server.join().unwrap(), ["a", "b", "c"], "oldest first");
        assert_eq!(waiting(&sent).len(), 2);
        assert_eq!(waiting(&pending), [pending.join("3-error-c.json")], "kept for the next minute");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
