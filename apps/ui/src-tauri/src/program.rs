//! The program owns the Core's lifetime (user decisions 2026-10-03): opening it starts the
//! service, a heartbeat keeps it, Exit stops it at stock. The tray keeps the program running with
//! the window hidden and offers the forged profiles, Stock and Exit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use nidavellir_core::ipc::EXIT_REFUSED_FORGE;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, Window, WindowEvent, Wry};

use crate::ipc_client::call_service_with_params as call;

const HEARTBEAT: Duration = Duration::from_secs(5);
/// A stopped run lands within a dwell; this bounds the wait after the user confirmed Exit.
const FORGE_STOP_TIMEOUT: Duration = Duration::from_secs(120);
/// Set while exiting: no heartbeats, and a stopped Core is not started again.
static EXITING: AtomicBool = AtomicBool::new(false);
/// The window is on screen: not hidden in the tray, not minimized. The UI polls the Core only
/// then (2026-10-07: polling unseen, the WebView spent about 20x the Core's CPU).
static UI_VISIBLE: AtomicBool = AtomicBool::new(false);

fn set_ui_visible(app: &AppHandle, visible: bool) {
    if UI_VISIBLE.swap(visible, Ordering::SeqCst) != visible {
        let _ = app.emit("window-visibility", visible);
    }
}

#[tauri::command]
pub fn window_visible() -> bool {
    UI_VISIBLE.load(Ordering::SeqCst)
}

/// Tray id, menu label and apply request of each forged profile, in menu order.
const PROFILES: [(&str, &str, &str); 3] = [
    ("godforge", "Godforge", "ApplyPowerGodforge"),
    ("brokkrs", "Brokkr's Best", "ApplyPowerBrokkrs"),
    ("deep_calm", "Deep Calm", "ApplyPowerDeepCalm"),
];

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowSettings {
    close_to_tray: bool,
    minimize_to_tray: bool,
    start_with_windows: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        // Closing keeps the program in the tray, so an applied profile stays active.
        Self { close_to_tray: true, minimize_to_tray: false, start_with_windows: false }
    }
}

struct Program {
    settings: Mutex<WindowSettings>,
    /// The three profiles, then Stock.
    items: [CheckMenuItem<Wry>; 4],
    shown: Mutex<Option<TrayState>>,
}

pub fn started_minimized() -> bool {
    std::env::args().any(|arg| arg == "--minimized")
}

pub fn setup(app: &mut App) -> tauri::Result<()> {
    let handle = app.handle();
    let check = |id: &str, text: &str| CheckMenuItem::with_id(handle, id, text, false, false, None::<&str>);
    let items = [
        check(PROFILES[0].0, PROFILES[0].1)?,
        check(PROFILES[1].0, PROFILES[1].1)?,
        check(PROFILES[2].0, PROFILES[2].1)?,
        check("stock", "Stock")?,
    ];
    let menu = Menu::with_items(handle, &[
        &MenuItem::with_id(handle, "open", "Open Nidavellir", true, None::<&str>)?,
        &PredefinedMenuItem::separator(handle)?,
        &items[0],
        &items[1],
        &items[2],
        &items[3],
        &PredefinedMenuItem::separator(handle)?,
        &MenuItem::with_id(handle, "exit", "Exit Nidavellir", true, None::<&str>)?,
    ])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Nidavellir")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    let settings = load_settings(handle);
    app.manage(Program { settings: Mutex::new(settings), items, shown: Mutex::new(None) });
    if !started_minimized() {
        show_main(handle);
    }
    #[cfg(windows)]
    single_instance::listen(handle.clone());
    tauri::async_runtime::spawn(heartbeat_loop(handle.clone()));
    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        set_ui_visible(app, true);
    }
}

fn settings(app: &AppHandle) -> WindowSettings {
    app.try_state::<Program>()
        .and_then(|program| program.settings.lock().ok().map(|settings| *settings))
        .unwrap_or_default()
}

pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != "main" || EXITING.load(Ordering::SeqCst) {
        return;
    }
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            if settings(window.app_handle()).close_to_tray {
                let _ = window.hide();
                set_ui_visible(window.app_handle(), false);
            } else {
                request_exit(window.app_handle());
            }
        }
        WindowEvent::Resized(_) => {
            let minimized = window.is_minimized().unwrap_or(false);
            if minimized && settings(window.app_handle()).minimize_to_tray {
                let _ = window.hide();
            }
            // Restoring from the taskbar does not pass through show_main.
            set_ui_visible(window.app_handle(), !minimized && window.is_visible().unwrap_or(true));
        }
        _ => {}
    }
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "open" => show_main(app),
        "exit" => request_exit(app),
        "stock" => run_action(app, "ResetGpuTuning", "Could not return the GPU to stock"),
        _ => {
            if let Some((_, name, method)) = PROFILES.iter().find(|(key, ..)| *key == id) {
                run_action(app, method, &format!("Could not apply {name}"));
            }
        }
    }
}

fn run_action(app: &AppHandle, method: &'static str, failure: &str) {
    let (app, failure) = (app.clone(), failure.to_string());
    tauri::async_runtime::spawn(async move {
        if let Err(error) = call(method, None).await {
            notify(&app, format!("{failure}: {error}"));
        }
        // A check item flips on click; the Core's answer decides what stays checked.
        if let Some(program) = app.try_state::<Program>() {
            if let Ok(mut shown) = program.shown.lock() {
                *shown = None;
            }
        }
        refresh_tray(&app).await;
    });
}

fn notify(app: &AppHandle, message: String) {
    show_main(app);
    let _ = app.emit("tray-notice", message);
}

async fn heartbeat_loop(app: AppHandle) {
    ensure_core().await;
    loop {
        if !EXITING.load(Ordering::SeqCst) {
            if call("ProgramHeartbeat", None).await.is_err() && !EXITING.load(Ordering::SeqCst) {
                ensure_core().await;
            }
            refresh_tray(&app).await;
        }
        tokio::time::sleep(HEARTBEAT).await;
    }
}

/// Release builds only: a development build talks to the console Core the developer started and
/// must never start the installed service next to it (both would serve the same pipe).
async fn ensure_core() {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        // Failures show up as an offline Core in the window; the next heartbeat retries.
        let _ = tauri::async_runtime::spawn_blocking(core_service::ensure_running).await;
    }
}

async fn refresh_tray(app: &AppHandle) {
    let progress = call("GetPowerSweepProgress", None).await.ok();
    let applied = call("GetAppliedProfile", None).await.ok();
    let safe = call("GetSafeLoopStatus", None).await.ok();
    let state = tray_state(progress.as_ref(), applied.as_ref(), safe.as_ref());
    let Some(program) = app.try_state::<Program>() else { return };
    let Ok(mut shown) = program.shown.lock() else { return };
    if shown.as_ref() == Some(&state) {
        return;
    }
    for (index, item) in program.items.iter().enumerate() {
        if let Some(label) = state.labels.get(index) {
            let _ = item.set_text(label);
        }
        let _ = item.set_enabled(state.enabled[index]);
        let _ = item.set_checked(state.checked[index]);
    }
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(&state.tooltip));
    }
    *shown = Some(state);
}

#[derive(Clone, Debug, PartialEq)]
struct TrayState {
    labels: [String; 3],
    /// The three profiles, then Stock.
    enabled: [bool; 4],
    checked: [bool; 4],
    tooltip: String,
}

/// The same rules as the profile cards; the Core still enforces every guard on apply.
fn tray_state(progress: Option<&Value>, applied: Option<&Value>, safe: Option<&Value>) -> TrayState {
    let data = |response: Option<&Value>| response.map(|r| r["data"].clone()).unwrap_or(Value::Null);
    let (progress, applied, safe) = (data(progress), data(applied), data(safe));
    let flag = |value: &Value, key: &str| value[key].as_bool().unwrap_or(false);
    let number = |point: &Value, keys: [&str; 2]| keys.iter().find_map(|key| point[*key].as_f64());
    let online = progress.is_object();
    let running = flag(&progress, "running");
    let undervolt = flag(&progress, "is_undervolt");
    let points = PROFILES.map(|(key, ..)| progress[key].clone());
    let ready = points.iter().all(Value::is_object)
        && !running
        && (flag(&progress, "frontier_complete") || !undervolt);
    let safe_ok = safe.is_object()
        && !flag(&safe, "gpu_reboot_required")
        && !flag(&safe, "safe_mode")
        && !flag(&safe, "recovery_pending_ack")
        && safe["state"] != "unstable";
    let applicable = |point: &Value| {
        ready
            && safe_ok
            && (!undervolt
                || (flag(&progress, "profiles_qualified")
                    && flag(point, "apply_qualified")
                    && point["power_p99_w"].as_f64().is_some_and(|watts| watts > 0.0)))
    };
    let core = &applied["core"];
    let active = PROFILES.iter().zip(&points).position(|((_, name, _), point)| {
        ready
            && core.is_object()
            && applied["label"].as_str().is_some_and(|label| label.trim().eq_ignore_ascii_case(name))
            && core["freq_mhz"].as_f64() == number(point, ["target_clock_mhz", "clock_mhz"])
            && number(point, ["vf_table_voltage_mv", "voltage_mv"])
                .is_none_or(|voltage| core["voltage_mv"].as_f64() == Some(voltage))
    });
    let at_stock = applied.is_object() && core.is_null();
    let labels = [0, 1, 2].map(|index| {
        let name = PROFILES[index].1;
        let point = &points[index];
        match (number(point, ["target_clock_mhz", "clock_mhz"]), number(point, ["vf_table_voltage_mv", "voltage_mv"])) {
            (Some(mhz), Some(mv)) => format!("{name}  ·  {mhz:.0} MHz · {mv:.0} mV"),
            _ => name.to_string(),
        }
    });
    let tooltip = if !online {
        "Nidavellir · Core offline".to_string()
    } else if running {
        "Nidavellir · Forge running".to_string()
    } else if let Some(index) = active {
        format!("Nidavellir · {}", PROFILES[index].1)
    } else if at_stock {
        "Nidavellir · Stock".to_string()
    } else {
        "Nidavellir".to_string()
    };
    TrayState {
        labels,
        enabled: [applicable(&points[0]), applicable(&points[1]), applicable(&points[2]), online && !running],
        checked: [active == Some(0), active == Some(1), active == Some(2), at_stock],
        tooltip,
    }
}

/// Tray Exit or the close button without close-to-tray. A Forge run makes the window ask first.
pub fn request_exit(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match exit(&app).await {
            Err(error) if error.starts_with(EXIT_REFUSED_FORGE) => {
                show_main(&app);
                let _ = app.emit("exit-requested", ());
            }
            Err(error) => notify(&app, format!("Nidavellir could not stop the Core: {error}")),
            Ok(()) => {}
        }
    });
}

/// The user confirmed: stop the run and its automatic continuation (it stays resumable), then exit.
#[tauri::command]
pub async fn exit_program(app: AppHandle) -> Result<(), String> {
    let _ = call("StopPowerSweep", None).await;
    let _ = call("SetForgeAutoResume", Some(json!({ "enabled": false }))).await;
    let deadline = Instant::now() + FORGE_STOP_TIMEOUT;
    loop {
        match exit(&app).await {
            Err(error) if error.starts_with(EXIT_REFUSED_FORGE) && Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(error) if error.starts_with(EXIT_REFUSED_FORGE) => {
                return Err("The Forge run is still stopping. Try Exit again in a moment.".into());
            }
            result => return result,
        }
    }
}

/// A Core that accepted the exit stops by itself, so the program closes at once. A program opened
/// again meanwhile waits for that stop before starting the Core (`ensure_running`).
async fn exit(app: &AppHandle) -> Result<(), String> {
    EXITING.store(true, Ordering::SeqCst);
    match call("ExitProgram", None).await {
        Ok(_) => {}
        // Already stopping, no Core, or a Core older than this program, which keeps running.
        Err(error)
            if error.contains("shutting down")
                || error.starts_with("Core Service unavailable")
                || error.contains("unknown variant") => {}
        Err(error) => {
            EXITING.store(false, Ordering::SeqCst);
            return Err(error);
        }
    }
    app.exit(0);
    Ok(())
}

fn settings_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join("window-settings.json"))
}

fn load_settings(app: &AppHandle) -> WindowSettings {
    let mut settings: WindowSettings = settings_path(app)
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    settings.start_with_windows = autostart::enabled();
    settings
}

#[tauri::command]
pub fn get_window_settings(app: AppHandle) -> WindowSettings {
    settings(&app)
}

#[tauri::command]
pub fn set_window_settings(app: AppHandle, settings: WindowSettings) -> Result<WindowSettings, String> {
    autostart::set(settings.start_with_windows)?;
    let path = settings_path(&app).ok_or("The settings folder is unavailable")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if let Some(program) = app.try_state::<Program>() {
        if let Ok(mut current) = program.settings.lock() {
            *current = settings;
        }
    }
    Ok(settings)
}

/// "Start with Windows": a per-user Run entry that opens the program in the tray.
#[cfg(windows)]
mod autostart {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "Nidavellir";

    pub fn enabled() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(RUN)
            .and_then(|key| key.get_value::<String, _>(VALUE))
            .is_ok()
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN).map_err(|e| e.to_string())?;
        if enabled {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            key.set_value(VALUE, &command_line(&exe)).map_err(|e| e.to_string())
        } else {
            match key.delete_value(VALUE) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.to_string()),
                _ => Ok(()),
            }
        }
    }

    pub(super) fn command_line(exe: &std::path::Path) -> String {
        format!("\"{}\" --minimized", exe.display())
    }
}

#[cfg(not(windows))]
mod autostart {
    pub fn enabled() -> bool {
        false
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        if enabled { Err("Start with Windows is available only on Windows".into()) } else { Ok(()) }
    }
}

/// The Core is the `NidavellirCore` service; the installer lets interactive users start it.
#[cfg(all(windows, not(debug_assertions)))]
mod core_service {
    use std::time::{Duration, Instant};

    use windows_service::service::{Service, ServiceAccess, ServiceState};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    const NAME: &str = "NidavellirCore";
    const ERROR_SERVICE_ALREADY_RUNNING: i32 = 1056;
    const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;

    /// None in a development build, which talks to a console Core.
    fn open(access: ServiceAccess) -> Result<Option<Service>, String> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .map_err(|e| format!("Service manager: {e}"))?;
        match manager.open_service(NAME, access) {
            Ok(service) => Ok(Some(service)),
            Err(windows_service::Error::Winapi(e)) if e.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST) => Ok(None),
            Err(e) => Err(format!("Core Service: {e}")),
        }
    }

    /// Start the Core if it is stopped, after any stop in progress.
    pub fn ensure_running() -> Result<(), String> {
        let Some(service) = open(ServiceAccess::QUERY_STATUS | ServiceAccess::START)? else { return Ok(()) };
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            let state = service.query_status().map_err(|e| e.to_string())?.current_state;
            match state {
                ServiceState::Running | ServiceState::StartPending => return Ok(()),
                ServiceState::Stopped => {
                    return match service.start::<&str>(&[]) {
                        Err(windows_service::Error::Winapi(e)) if e.raw_os_error() == Some(ERROR_SERVICE_ALREADY_RUNNING) => Ok(()),
                        other => other.map_err(|e| format!("Cannot start the Core Service: {e}")),
                    };
                }
                _ if Instant::now() >= deadline => return Err(format!("Core Service stuck in {state:?}")),
                _ => std::thread::sleep(Duration::from_millis(250)),
            }
        }
    }
}

/// One program per Windows session: a second launch shows the running one and exits.
pub fn claim_single_instance() -> bool {
    #[cfg(windows)]
    {
        single_instance::first(started_minimized())
    }
    #[cfg(not(windows))]
    {
        true
    }
}

#[cfg(windows)]
mod single_instance {
    use std::sync::OnceLock;

    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject, INFINITE};

    /// The first instance's event, kept open for the life of the process.
    static EVENT: OnceLock<isize> = OnceLock::new();

    /// False when another instance runs; it is asked to show its window unless this launch is quiet.
    /// A development build has its own name, so it starts next to the installed program.
    pub fn first(quiet: bool) -> bool {
        let name = if cfg!(debug_assertions) { w!("Local\\NidavellirUI-dev") } else { w!("Local\\NidavellirUI") };
        let Ok(event) = (unsafe { CreateEventW(None, false, false, name) }) else {
            return true;
        };
        if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
            let _ = EVENT.set(event.0 as isize);
            return true;
        }
        if !quiet {
            let _ = unsafe { SetEvent(event) };
        }
        false
    }

    pub fn listen(app: tauri::AppHandle) {
        let Some(&raw) = EVENT.get() else { return };
        std::thread::spawn(move || {
            while unsafe { WaitForSingleObject(HANDLE(raw as _), INFINITE) } == WAIT_OBJECT_0 {
                super::show_main(&app);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(data: Value) -> Value {
        json!({ "ok": true, "data": data })
    }

    fn point(mhz: u32, mv: u32) -> Value {
        json!({ "target_clock_mhz": mhz, "vf_table_voltage_mv": mv, "apply_qualified": true, "power_p99_w": 150.0 })
    }

    fn forged() -> Value {
        response(json!({
            "type": "PowerSweep", "running": false, "is_undervolt": true, "frontier_complete": true,
            "profiles_qualified": true, "godforge": point(1890, 937), "brokkrs": point(1800, 875),
            "deep_calm": point(1710, 825)
        }))
    }

    fn safe() -> Value {
        response(json!({ "type": "SafeLoop", "state": "idle" }))
    }

    #[test]
    fn tray_offers_forged_profiles_and_checks_the_applied_one() {
        let applied = response(json!({ "label": "Brokkr's Best", "core": { "freq_mhz": 1800, "voltage_mv": 875 } }));
        let state = tray_state(Some(&forged()), Some(&applied), Some(&safe()));
        assert_eq!(state.enabled, [true, true, true, true]);
        assert_eq!(state.checked, [false, true, false, false]);
        assert_eq!(state.labels[1], "Brokkr's Best  ·  1800 MHz · 875 mV");
        assert_eq!(state.tooltip, "Nidavellir · Brokkr's Best");
    }

    #[test]
    fn tray_locks_profiles_during_a_run_and_when_safety_needs_attention() {
        let stock = response(json!({ "label": null, "core": null }));
        let mut running = forged();
        running["data"]["running"] = json!(true);
        let state = tray_state(Some(&running), Some(&stock), Some(&safe()));
        assert_eq!(state.enabled, [false, false, false, false], "Stock would abort the run");
        let reboot = response(json!({ "type": "SafeLoop", "state": "idle", "gpu_reboot_required": true }));
        let state = tray_state(Some(&forged()), Some(&stock), Some(&reboot));
        assert_eq!(state.enabled, [false, false, false, true]);
        assert_eq!(state.checked, [false, false, false, true]);
        let offline = tray_state(None, None, None);
        assert_eq!(offline.enabled, [false; 4]);
        assert_eq!(offline.tooltip, "Nidavellir · Core offline");
    }

    #[test]
    fn closing_keeps_the_program_in_the_tray_by_default() {
        let settings: WindowSettings = serde_json::from_str(r#"{"minimizeToTray":true}"#).unwrap();
        assert!(settings.close_to_tray && settings.minimize_to_tray && !settings.start_with_windows);
    }

    #[cfg(windows)]
    #[test]
    fn start_with_windows_opens_the_installed_program_in_the_tray() {
        let line = autostart::command_line(std::path::Path::new(r"C:\Program Files\Nidavellir\nidavellir-ui.exe"));
        assert_eq!(line, r#""C:\Program Files\Nidavellir\nidavellir-ui.exe" --minimized"#);
    }
}
