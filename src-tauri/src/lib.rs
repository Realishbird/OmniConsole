//! OmniConsole backend: sessions (SSH, Telnet, serial), saved devices and
//! the OS credential store. The UI lives in ../src (React + xterm.js).

mod commands;
mod redact;
mod session;
mod store;

use session::SessionManager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .manage(SessionManager::default())
        .invoke_handler(tauri::generate_handler![
            commands::session_open,
            commands::session_write,
            commands::session_resize,
            commands::session_break,
            commands::session_close,
            commands::session_answer,
            commands::serial_ports,
            commands::devices_load,
            commands::devices_save,
            commands::data_load,
            commands::data_save,
            commands::secret_set,
            commands::secret_delete,
            commands::secret_exists,
            commands::save_text,
            commands::devices_export,
            commands::devices_import,
            commands::log_folder,
            commands::open_log_folder,
            commands::reveal_log,
        ])
        .run(tauri::generate_context!())
        .expect("error while running OmniConsole");
}
