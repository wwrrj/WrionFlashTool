pub mod config;
pub mod sdat2img;
pub mod sparse;
pub mod logger;
pub mod package;
pub mod device;
pub mod flash;
pub mod rootflash;
pub mod idflash;
pub mod expand;
pub mod rompkg;
pub mod backup;
pub mod official;
pub mod platform_tools;
pub mod commands;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::inspect_package_cmd,
            commands::extract_package_cmd,
            commands::cancel_extract_cmd,
            commands::dest_free_space_cmd,
            commands::use_rom_dir_cmd,
            commands::inspect_backup_cmd,
            commands::get_profile,
            commands::scan_devices,
            commands::run_diagnostics_cmd,
            commands::run_flash_cmd,
            commands::run_root_flash_cmd,
            commands::run_id_flash_cmd,
            commands::get_expand_plan_cmd,
            commands::apply_expand_cmd,
            commands::list_backups_cmd,
            commands::restore_backup_cmd,
            commands::clean_work_dir_cmd,
            commands::open_folder_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("运行 HarusoraFlashTool 发生错误");
}
