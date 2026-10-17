//! Delivers the Core's queued error reports and Forge summaries to the relay (`tools/report-relay`)
//! once the user agreed (user decision 2026-10-16: one consent, then automatic). The Core writes them
//! to ProgramData and scrubs them; the program reads them as the user, posts each one and asks the
//! Core to mark it sent. Nothing here reads anything else.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde_json::json;

use crate::ipc_client::call_service_with_params as call;

const RELAY: &str = "https://nidavellir-reports.kamilogic.workers.dev/report";
/// The relay groups same-signature reports and its label search lags a few seconds.
const SPACING: Duration = Duration::from_secs(3);
static SENDING: AtomicBool = AtomicBool::new(false);

fn reports_dir(kind: &str) -> PathBuf {
    std::env::var_os("ProgramData")
        .map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from)
        .join("Nidavellir")
        .join("reports")
        .join(kind)
}

/// Report files, oldest first: their names start with the creation time.
fn files(kind: &str) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(reports_dir(kind))
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    paths.retain(|path| path.extension().is_some_and(|ext| ext == "json"));
    paths.sort();
    paths
}

/// Sends up to `limit` waiting reports, oldest first, and returns how many the relay took.
pub async fn send_pending(limit: usize) -> Result<usize, String> {
    if SENDING.swap(true, Ordering::SeqCst) {
        return Ok(0);
    }
    let result = send(limit).await;
    SENDING.store(false, Ordering::SeqCst);
    result
}

/// Reports the relay already has whose `MarkReportSent` did not go through: never posted twice.
static DELIVERED: Mutex<Vec<String>> = Mutex::new(Vec::new());

async fn send(limit: usize) -> Result<usize, String> {
    let waiting = files("pending");
    if waiting.is_empty() {
        return Ok(0);
    }
    let client = client()?;
    let mut delivered = 0;
    let mut posted = false;
    for path in waiting.into_iter().take(limit) {
        let Some(id) = path.file_name().and_then(|name| name.to_str()).map(str::to_owned) else { continue };
        if !DELIVERED.lock().is_ok_and(|done| done.contains(&id)) {
            if posted {
                tokio::time::sleep(SPACING).await;
            }
            posted = true;
            let body = std::fs::read(&path).map_err(|e| format!("report {id}: {e}"))?;
            let response = client
                .post(RELAY)
                .header("Content-Type", "application/json")
                .body(body)
                .send()
                .await
                .map_err(|e| format!("relay: {e}"))?;
            let status = response.status();
            // Too many or a relay fault: retry later. Any other refusal never changes: drop it.
            if status.as_u16() == 429 || status.is_server_error() {
                return Err(format!("relay answered {status}"));
            }
            if status.is_success() {
                delivered += 1;
            }
            if let Ok(mut done) = DELIVERED.lock() {
                done.push(id.clone());
            }
        }
        call("MarkReportSent", Some(json!({ "id": id }))).await?;
        if let Ok(mut done) = DELIVERED.lock() {
            done.retain(|known| known != &id);
        }
    }
    Ok(delivered)
}

/// The updater's own HTTPS stack: rustls with ring as the process provider.
fn client() -> Result<reqwest::Client, String> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    reqwest::Client::builder()
        .user_agent(concat!("Nidavellir/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportStatus {
    pending: usize,
    /// Milliseconds since the epoch when the newest sent report was made.
    last_sent_ms: Option<u64>,
}

#[tauri::command]
pub fn report_status() -> ReportStatus {
    let created_ms = |path: &PathBuf| {
        path.file_name()?.to_str()?.split('-').next()?.parse::<u64>().ok()
    };
    ReportStatus { pending: files("pending").len(), last_sent_ms: files("sent").last().and_then(created_ms) }
}

/// "Send now" in Settings, for someone who did not turn on automatic sending.
#[tauri::command]
pub async fn send_reports_now() -> Result<usize, String> {
    send_pending(usize::MAX).await
}
