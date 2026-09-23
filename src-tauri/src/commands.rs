use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, command};

use crate::config::{load_profile, resolve_profile_path, FlashProfile};
use crate::device::{DeviceSession, EnvReport};
use crate::expand::{ExpandPlan, ExpandSession};
use crate::flash::{execute_flash_pipeline, gather_environment as flash_gather_env};
use crate::idflash::run_id_flash;
use crate::logger::Logger;
use crate::rootflash::flash_root_image;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSummary {
    pub serial: String,
    pub mode: String, // "fastboot" | "fastbootd" | "recovery" | "device"
    pub product: Option<String>,
}

fn get_active_profile(rom_root: Option<&str>, profile_path: Option<&str>) -> Result<FlashProfile, String> {
    let r_path = rom_root.map(Path::new);
    let p_path = profile_path.map(Path::new);
    let (root, resolved_p) = resolve_profile_path(r_path, p_path).map_err(|e| e.to_string())?;
    load_profile(&root, &resolved_p).map_err(|e| e.to_string())
}

#[command]
pub async fn get_profile(
    rom_root: Option<String>,
    profile_path: Option<String>,
) -> Result<FlashProfile, String> {
    get_active_profile(rom_root.as_deref(), profile_path.as_deref())
}

#[command]
pub async fn scan_devices(
    app: AppHandle,
    rom_root: Option<String>,
) -> Result<Vec<DeviceSummary>, String> {
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    let logger = Logger::new(Some(app));
    let session = DeviceSession::new(profile.fastboot(), profile.adb(), logger, false);

    let mut list = Vec::new();
    if let Ok(serials) = session.list_fastboot_serials() {
        for s in serials {
            let mut sub_session = DeviceSession::new(profile.fastboot(), profile.adb(), Logger::new(None), false);
            sub_session.serial = Some(s.clone());
            let is_us = sub_session.getvar("is-userspace", true).ok().flatten()
                .map_or(false, |v| v.to_lowercase() == "yes");
            let prod = sub_session.getvar("product", true).ok().flatten();
            list.push(DeviceSummary {
                serial: s,
                mode: if is_us { "fastbootd".to_string() } else { "fastboot".to_string() },
                product: prod,
            });
        }
    }

    if let Ok(adb_devs) = session.list_adb_devices() {
        for d in adb_devs {
            list.push(DeviceSummary {
                serial: d.serial,
                mode: d.state,
                product: None,
            });
        }
    }

    Ok(list)
}

#[command]
pub async fn run_diagnostics_cmd(
    app: AppHandle,
    rom_root: Option<String>,
    chosen_serial: Option<String>,
) -> Result<EnvReport, String> {
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    let mut logger = Logger::new(Some(app));
    let _ = logger.start(&profile.logs_dir, "harusora-diag");
    let mut session = DeviceSession::new(profile.fastboot(), profile.adb(), logger, false);
    session.serial = chosen_serial;

    let chosen_serial = session.serial.clone();
    let report = flash_gather_env(&profile, &mut session, chosen_serial.as_deref())
        .map_err(|e| e.to_string())?;

    if report.is_userspace {
        session
            .run_fastboot(
                &["reboot", "bootloader"],
                Some("从 fastbootd 返回 bootloader"),
                false,
            )
            .map_err(|e| e.to_string())?;
        if !session.dry_run {
            session
                .wait_for_mode(false, 90)
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(report)
}

#[command]
pub async fn run_flash_cmd(
    app: AppHandle,
    rom_root: Option<String>,
    chosen_serial: Option<String>,
    preserve_data: bool,
    confirm_word: String,
    reboot: bool,
) -> Result<String, String> {
    let mut profile = get_active_profile(rom_root.as_deref(), None)?;
    profile.reboot_after_success = profile.reboot_after_success && reboot;

    let required_word = if preserve_data {
        &profile.update_confirmation_word
    } else {
        &profile.confirmation_word
    };

    if confirm_word.trim() != required_word {
        return Err(format!("确认词不匹配，请输入 {}", required_word));
    }

    let mut logger = Logger::new(Some(app.clone()));
    let prefix = if preserve_data { "harusora-update" } else { "harusora-flash" };
    let log_path = logger.start(&profile.logs_dir, prefix).map_err(|e| e.to_string())?;

    let mut session = DeviceSession::new(profile.fastboot(), profile.adb(), logger, false);
    session.serial = chosen_serial;

    tokio::task::spawn_blocking(move || {
        execute_flash_pipeline(&profile, session, preserve_data, Some(&app))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(log_path.display().to_string())
}

#[command]
pub async fn run_root_flash_cmd(
    app: AppHandle,
    rom_root: Option<String>,
    chosen_serial: Option<String>,
    image_path: String,
    target: String,
    confirm_word: String,
    reboot: bool,
) -> Result<String, String> {
    if confirm_word.trim() != "ROOT" {
        return Err("请输入 ROOT 确认刷入".to_string());
    }
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    let mut logger = Logger::new(Some(app));
    let log_path = logger.start(&profile.logs_dir, "harusora-root").map_err(|e| e.to_string())?;

    let mut session = DeviceSession::new(profile.fastboot(), profile.adb(), logger, false);
    session.serial = chosen_serial;
    let img_path = PathBuf::from(image_path);

    tokio::task::spawn_blocking(move || {
        flash_root_image(&profile, &mut session, &img_path, &target, reboot)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(log_path.display().to_string())
}

#[command]
pub async fn run_id_flash_cmd(
    app: AppHandle,
    rom_root: Option<String>,
    chosen_serial: Option<String>,
    id_file: String,
    format_data: bool,
    confirm_word: String,
    reboot: bool,
) -> Result<String, String> {
    if confirm_word.trim() != "ID" {
        return Err("请输入 ID 确认刷入".to_string());
    }
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    let mut logger = Logger::new(Some(app));
    let log_path = logger.start(&profile.logs_dir, "harusora-id").map_err(|e| e.to_string())?;

    let mut session = DeviceSession::new(profile.fastboot(), profile.adb(), logger, false);
    session.serial = chosen_serial;
    let id_p = PathBuf::from(id_file);

    tokio::task::spawn_blocking(move || {
        run_id_flash(&profile, &mut session, &id_p, format_data, reboot)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(log_path.display().to_string())
}

#[command]
pub async fn get_expand_plan_cmd(
    app: AppHandle,
    rom_root: Option<String>,
) -> Result<ExpandPlan, String> {
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    let logger = Logger::new(Some(app));
    let mut session = ExpandSession::new(profile, logger).map_err(|e| e.to_string())?;

    tokio::task::spawn_blocking(move || {
        session.ensure_recovery()?;
        session.init_sgdisk()?;
        session.build_plan()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[command]
pub async fn apply_expand_cmd(
    app: AppHandle,
    rom_root: Option<String>,
    confirm_word: String,
) -> Result<String, String> {
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    let exp_cfg = profile.expansion.as_ref()
        .ok_or_else(|| "当前配置未启用物理扩容".to_string())?;
    if confirm_word.trim() != exp_cfg.confirm_word {
        return Err(format!("请输入 {} 确认扩容", exp_cfg.confirm_word));
    }

    let mut logger = Logger::new(Some(app));
    let _ = logger.start(&profile.logs_dir, "harusora-expand").map_err(|e| e.to_string())?;
    let mut session = ExpandSession::new(profile, logger).map_err(|e| e.to_string())?;

    let backup_dir = tokio::task::spawn_blocking(move || {
        session.ensure_recovery()?;
        session.init_sgdisk()?;
        let plan = session.build_plan()?;
        session.apply_expand(&plan)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(backup_dir.display().to_string())
}

#[command]
pub async fn list_backups_cmd(
    app: AppHandle,
    rom_root: Option<String>,
) -> Result<Vec<String>, String> {
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    let logger = Logger::new(Some(app));
    let session = ExpandSession::new(profile, logger).map_err(|e| e.to_string())?;
    let backups = session.list_backups();
    Ok(backups.into_iter().map(|p| p.display().to_string()).collect())
}

#[command]
pub async fn restore_backup_cmd(
    app: AppHandle,
    rom_root: Option<String>,
    backup_dir: String,
    confirm_word: String,
) -> Result<String, String> {
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    let exp_cfg = profile.expansion.as_ref()
        .ok_or_else(|| "当前配置未启用物理扩容".to_string())?;
    if confirm_word.trim() != exp_cfg.restore_confirm_word {
        return Err(format!("请输入 {} 确认恢复", exp_cfg.restore_confirm_word));
    }

    let mut logger = Logger::new(Some(app));
    let _ = logger.start(&profile.logs_dir, "harusora-restore").map_err(|e| e.to_string())?;
    let mut session = ExpandSession::new(profile, logger).map_err(|e| e.to_string())?;
    let b_path = PathBuf::from(backup_dir);

    tokio::task::spawn_blocking(move || {
        session.restore_official(&b_path)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok("官方分区表恢复成功".to_string())
}

#[command]
pub async fn clean_work_dir_cmd(
    rom_root: Option<String>,
) -> Result<String, String> {
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    if profile.work_dir.is_dir() {
        std::fs::remove_dir_all(&profile.work_dir).map_err(|e| e.to_string())?;
        Ok(format!("已成功清理工作目录：{:?}", profile.work_dir))
    } else {
        Ok(format!("工作目录不存在，无需清理：{:?}", profile.work_dir))
    }
}

#[command]
pub async fn open_folder_cmd(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        let _ = std::fs::create_dir_all(p);
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
