//! Commands the UI calls (see src/lib/backend.ts). File paths and secrets
//! stay on this side: the UI never names a file to read or write (the
//! dialogs are opened here) and can only touch OmniConsole's own keychain
//! entries.

use std::path::PathBuf;

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::oneshot;

use crate::redact;
use crate::session::{ConnectSpec, SessionEvent, SessionManager};
use crate::store::{self, Device};

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

fn dir(app: &AppHandle) -> CmdResult<PathBuf> {
    store::config_dir(app).map_err(err)
}

/// settings.json "logDir", or Documents/OmniConsole/Logs
fn log_dir(app: &AppHandle) -> PathBuf {
    dir(app)
        .ok()
        .and_then(|d| store::data_load(&d, "settings").ok())
        .and_then(|v| v.get("logDir").and_then(|p| p.as_str()).map(PathBuf::from))
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| store::default_log_dir(app))
}

/*
 *  sessions
 */
#[tauri::command]
pub fn session_open(
    app: AppHandle,
    sessions: State<'_, SessionManager>,
    spec: ConnectSpec,
    data: Channel<InvokeResponseBody>,
    events: Channel<SessionEvent>,
) -> CmdResult<String> {
    let config = dir(&app)?;
    Ok(sessions.open(config, &log_dir(&app), spec, data, events))
}

#[tauri::command]
pub fn session_write(sessions: State<'_, SessionManager>, id: String, data: String) {
    sessions.write(&id, data.into_bytes());
}

#[tauri::command]
pub fn session_resize(sessions: State<'_, SessionManager>, id: String, cols: u16, rows: u16) {
    sessions.resize(&id, cols, rows);
}

#[tauri::command]
pub fn session_break(sessions: State<'_, SessionManager>, id: String) {
    sessions.send_break(&id);
}

#[tauri::command]
pub fn session_close(sessions: State<'_, SessionManager>, id: String) {
    sessions.close(&id);
}

#[tauri::command]
pub fn session_answer(sessions: State<'_, SessionManager>, id: String, answer: Option<String>) {
    sessions.answer(&id, answer);
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortInfo {
    name: String,
    description: String,
}

#[tauri::command]
pub fn serial_ports() -> Vec<SerialPortInfo> {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let description = match p.port_type {
                serialport::SerialPortType::UsbPort(u) => {
                    u.product.or(u.manufacturer).unwrap_or_else(|| "USB serial".into())
                }
                serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
                _ => String::new(),
            };
            SerialPortInfo { name: p.port_name, description }
        })
        .collect()
}

/*
 *  devices, app data, secrets
 */
#[tauri::command]
pub fn devices_load(app: AppHandle) -> CmdResult<Vec<Device>> {
    store::load_devices(&dir(&app)?).map_err(err)
}

#[tauri::command]
pub fn devices_save(app: AppHandle, devices: Vec<Device>) -> CmdResult<()> {
    store::save_devices(&dir(&app)?, &devices).map_err(err)
}

#[tauri::command]
pub fn data_load(app: AppHandle, kind: String) -> CmdResult<serde_json::Value> {
    store::data_load(&dir(&app)?, &kind).map_err(err)
}

#[tauri::command]
pub fn data_save(app: AppHandle, kind: String, value: serde_json::Value) -> CmdResult<()> {
    store::data_save(&dir(&app)?, &kind, &value).map_err(err)
}

#[tauri::command]
pub fn secret_set(kind: String, device_id: String, value: String) -> CmdResult<()> {
    store::secret_set(&store::secret_name(&kind, &device_id).map_err(err)?, &value).map_err(err)
}

#[tauri::command]
pub fn secret_delete(kind: String, device_id: String) -> CmdResult<()> {
    store::secret_delete(&store::secret_name(&kind, &device_id).map_err(err)?).map_err(err)
}

#[tauri::command]
pub fn secret_exists(kind: String, device_id: String) -> bool {
    store::secret_name(&kind, &device_id).ok().and_then(|n| store::secret_get(&n)).is_some()
}

/*
 *  files (the dialogs open here, so the UI never chooses a path itself)
 */
pub(crate) async fn pick_save(app: &AppHandle, name: &str, filter: (&str, &[&str])) -> Option<PathBuf> {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(name)
        .add_filter(filter.0, filter.1)
        .save_file(move |p| {
            let _ = tx.send(p);
        });
    rx.await.ok().flatten().and_then(|p| p.into_path().ok())
}

/// Save text (terminal output, a selection) to a file the user picks.
/// Returns the path, or None when cancelled.
#[tauri::command]
pub async fn save_text(app: AppHandle, name: String, contents: String, mask_secrets: bool) -> CmdResult<Option<String>> {
    let Some(path) = pick_save(&app, &name, ("Text", &["txt", "log"])).await else {
        return Ok(None);
    };
    let contents = if mask_secrets { redact::text(&contents) } else { contents };
    std::fs::write(&path, contents).map_err(err)?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub async fn devices_export(app: AppHandle, devices: Vec<Device>) -> CmdResult<Option<String>> {
    let Some(path) = pick_save(&app, "omniconsole-devices.json", ("OmniConsole devices", &["json"])).await else {
        return Ok(None);
    };
    std::fs::write(&path, store::export_devices(&devices).map_err(err)?).map_err(err)?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub async fn devices_import(app: AppHandle) -> CmdResult<Option<Vec<Device>>> {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Devices (OmniConsole JSON or CSV)", &["json", "csv", "txt"])
        .pick_file(move |p| {
            let _ = tx.send(p);
        });
    let Some(path) = rx.await.ok().flatten().and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&path).map_err(err)?;
    store::parse_import(&text).map(Some).map_err(err)
}

#[tauri::command]
pub fn log_folder(app: AppHandle) -> String {
    log_dir(&app).display().to_string()
}

#[tauri::command]
pub fn open_log_folder(app: AppHandle) -> CmdResult<()> {
    let dir = log_dir(&app);
    std::fs::create_dir_all(&dir).map_err(err)?;
    app.opener().open_path(dir.display().to_string(), None::<&str>).map_err(err)
}

/// Show a log file the session reported, but only inside the log folder.
#[tauri::command]
pub fn reveal_log(app: AppHandle, path: String) -> CmdResult<()> {
    let base = log_dir(&app).canonicalize().map_err(err)?;
    let target = PathBuf::from(&path).canonicalize().map_err(err)?;
    if !target.starts_with(&base) {
        return Err("not a session log".into());
    }
    app.opener().reveal_item_in_dir(target).map_err(err)
}
