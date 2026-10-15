//! OmniConsole backend: sessions (SSH, Telnet, serial), saved devices and
//! the OS credential store. The UI lives in ../src (React + xterm.js).

mod ai;
mod cisco;
mod commands;
mod credentials;
mod deploy;
mod files;
mod ise_api;
mod launch;
mod nettools;
mod platforms;
mod redact;
mod session;
mod store;
mod tools;

use session::SessionManager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .manage(SessionManager::default())
        .manage(tools::ToolState::default())
        .manage(nettools::NetJobs::default())
        .manage(ai::AiJobs::default())
        .manage(files::FilesState::default())
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
            tools::run_command,
            tools::device_detect,
            tools::checks_run,
            tools::checks_cancel,
            tools::checks_catalog,
            tools::config_backup,
            tools::report_save,
            tools::configs_list,
            tools::config_read,
            tools::config_delete,
            tools::config_diff,
            tools::configs_search,
            tools::config_folder,
            tools::open_config_folder,
            tools::reveal_config,
            tools::save_bundle,
            tools::classify_commands,
            nettools::net_ping,
            nettools::net_traceroute,
            nettools::net_dns,
            nettools::net_ports,
            nettools::net_mtu,
            nettools::net_cancel,
            ai::ai_models,
            ai::ai_chat,
            ai::ai_cancel,
            deploy::templates_list,
            deploy::template_render,
            deploy::deploy_run,
            deploy::deploy_finish,
            deploy::deploy_rollback,
            deploy::firmware_inspect,
            deploy::firmware_plan,
            tools::complete,
            platforms::platform_profile,
            platforms::platforms_list,
            platforms::collect_run,
            ise_api::ise_api_checks,
            ise_api::ise_api_credential,
            credentials::credentials_load,
            credentials::credentials_save,
            credentials::credential_secret_set,
            credentials::credential_secret_get,
            credentials::credential_delete,
            credentials::credential_effective,
            credentials::credentials_export,
            credentials::credentials_import,
            credentials::credential_test,
            credentials::vault_status,
            credentials::vault_setup,
            credentials::vault_lock,
            credentials::vault_unlock,
            credentials::vault_touch,
            files::files_open,
            files::files_close,
            files::files_list,
            files::files_mkdir,
            files::files_delete,
            files::files_rename,
            files::files_download,
            files::files_upload,
            launch::launch_external,
            launch::open_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running OmniConsole");
}
