//! Commands the UI calls (see src/lib/backend.ts).

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, State};

use crate::session::{ConnectSpec, SessionEvent, SessionManager};
use crate::store::{self, Device};

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn session_open(
    app: AppHandle,
    sessions: State<'_, SessionManager>,
    spec: ConnectSpec,
    data: Channel<InvokeResponseBody>,
    events: Channel<SessionEvent>,
) -> CmdResult<String> {
    let dir = store::config_dir(&app).map_err(err)?;
    Ok(sessions.open(dir, spec, data, events))
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

#[tauri::command]
pub fn devices_load(app: AppHandle) -> CmdResult<Vec<Device>> {
    store::load_devices(&app).map_err(err)
}

#[tauri::command]
pub fn devices_save(app: AppHandle, devices: Vec<Device>) -> CmdResult<()> {
    store::save_devices(&app, &devices).map_err(err)
}

#[tauri::command]
pub fn password_set(device_id: String, password: String) -> CmdResult<()> {
    store::password_set(&device_id, &password).map_err(err)
}

#[tauri::command]
pub fn password_delete(device_id: String) -> CmdResult<()> {
    store::password_delete(&device_id).map_err(err)
}

#[tauri::command]
pub fn password_exists(device_id: String) -> bool {
    store::password_get(&device_id).is_some()
}

#[tauri::command]
pub fn save_text_file(path: String, contents: String) -> CmdResult<()> {
    std::fs::write(&path, contents).map_err(err)
}
