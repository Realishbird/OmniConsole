//! OmniConsole backend: sessions (SSH, Telnet, serial), saved devices and
//! the OS credential store. The UI lives in ../src (React + xterm.js).

mod commands;
mod session;
mod store;

use session::SessionManager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(SessionManager::default())
        .invoke_handler(tauri::generate_handler![
            commands::session_open,
            commands::session_write,
            commands::session_resize,
            commands::session_close,
            commands::session_answer,
            commands::serial_ports,
            commands::devices_load,
            commands::devices_save,
            commands::password_set,
            commands::password_delete,
            commands::password_exists,
            commands::save_text_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running OmniConsole");
}
