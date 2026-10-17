//! Error reports and Forge summaries in Settings. Since 0.8.1 the Core writes and sends them itself,
//! with the program closed too (user decision 2026-10-17); the program mirrors the user's choice to
//! it (program.rs), shows how many wait and asks for "Send now". Nothing here reads anything else.

use std::path::PathBuf;

use serde::Serialize;

use crate::ipc_client::call_service_with_params as call;

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

/// "Send now" in Settings, also for someone who did not turn on automatic sending: the Core sends
/// every waiting report in the background, a few seconds apart.
#[tauri::command]
pub async fn send_reports_now() -> Result<(), String> {
    call("SendReportsNow", None).await.map(|_| ())
}
