use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, command};

use crate::config::{load_profile, resolve_profile_path, FlashProfile};
use crate::device::{DeviceSession, EnvReport};
use crate::expand::{ExpandPlan, ExpandSession, APPLY_STEPS, PLAN_STEPS, RESTORE_STEPS};
use crate::flash::{execute_flash_pipeline, gather_environment as flash_gather_env};
use crate::idflash::run_id_flash;
use crate::logger::Logger;
use crate::rootflash::flash_root_image;
use crate::backup::{self, BackupInfo};
use crate::config::find_rom_root;
use crate::rompkg::{self, PackageInfo};
use tauri::Emitter;

#[command]
pub async fn inspect_package_cmd(zip_path: String) -> Result<PackageInfo, String> {
    tokio::task::spawn_blocking(move || rompkg::inspect_package(Path::new(&zip_path)))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// 解压刷机包并把解压目录设为当前 ROM 根目录，返回该目录。
#[command]
pub async fn extract_package_cmd(
    app: AppHandle,
    zip_path: String,
    dest_dir: Option<String>,
) -> Result<String, String> {
    let zip = PathBuf::from(zip_path);
    let dest = dest_dir
        .filter(|d| !d.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| rompkg::default_dest(&zip));
    let root = tokio::task::spawn_blocking(move || {
        rompkg::extract_package(&zip, &dest, &mut |p| {
            let _ = app.emit("extract_progress", p);
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    rompkg::set_active_root(Some(root.clone()));
    Ok(root.display().to_string())
}

/// 直接使用已解压的 ROM 目录（选到其子目录或上一层目录也能识别）。
#[command]
pub async fn use_rom_dir_cmd(dir: String) -> Result<String, String> {
    let picked = PathBuf::from(&dir);
    let root = find_rom_root(Some(&picked))
        .or_else(|| {
            // 选到了 ROM 的上一层目录：只有一个子目录是 ROM 时自动进入
            let found: Vec<PathBuf> = std::fs::read_dir(&picked).ok()?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.join(crate::config::DEFAULT_PROFILE_REL).is_file())
                .collect();
            (found.len() == 1).then(|| found[0].clone())
        })
        .ok_or_else(|| format!(
            "这个文件夹里没有找到刷机配置（{}）。\n请选择 ROM 解压后的根目录，或直接选择 .zip 刷机包。",
            crate::config::DEFAULT_PROFILE_REL.replace('/', "\\")
        ))?;
    // 先完整加载一次配置，有错误立即提示，不切换当前刷机包
    get_active_profile(Some(&root.display().to_string()), None)?;
    let root = root.canonicalize().unwrap_or(root);
    rompkg::set_active_root(Some(root.clone()));
    Ok(root.display().to_string())
}

#[command]
pub async fn dest_free_space_cmd(dest: String) -> Result<Option<u64>, String> {
    Ok(rompkg::free_space(Path::new(&dest)))
}

#[command]
pub async fn cancel_extract_cmd() -> Result<(), String> {
    rompkg::request_cancel();
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSummary {
    pub serial: String,
    pub mode: String, // "fastboot" | "fastbootd" | "recovery" | "device"
    pub product: Option<String>,
}

fn get_active_profile(rom_root: Option<&str>, profile_path: Option<&str>) -> Result<FlashProfile, String> {
    let active = if rom_root.is_none() && profile_path.is_none() { rompkg::active_root() } else { None };
    let r_path = rom_root.map(Path::new).or(active.as_deref());
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
            let product = if matches!(d.state.as_str(), "device" | "recovery") {
                let mut adb_session = DeviceSession::new(
                    profile.fastboot(),
                    profile.adb(),
                    Logger::new(None),
                    false,
                );
                adb_session.serial = Some(d.serial.clone());
                adb_session
                    .run_adb(&["shell", "getprop", "ro.product.device"], None, true)
                    .ok()
                    .and_then(|result| result.stdout.lines().find_map(|line| {
                        let value = line.trim();
                        (!value.is_empty()).then(|| value.to_string())
                    }))
            } else {
                None
            };
            list.push(DeviceSummary {
                serial: d.serial,
                mode: d.state,
                product,
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

    // 读取机型、super 容量和防回滚版本；开机/Recovery 状态的手机会先重启到 bootloader
    tokio::task::spawn_blocking(move || -> anyhow::Result<EnvReport> {
        let chosen_serial = session.serial.clone();
        let report = flash_gather_env(&profile, &mut session, chosen_serial.as_deref())?;
        if report.is_userspace {
            session.run_fastboot(&["reboot", "bootloader"], Some("从 fastbootd 返回 bootloader"), false)?;
            if !session.dry_run {
                session.wait_for_mode(false, 90)?;
            }
        }
        Ok(report)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
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
        session.begin("plan", PLAN_STEPS);
        session.ensure_recovery()?;
        session.init_sgdisk()?;
        let plan = session.build_plan()?;
        session.finish();
        Ok::<_, anyhow::Error>(plan)
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
        session.begin("apply", APPLY_STEPS);
        session.ensure_recovery()?;
        session.init_sgdisk()?;
        let plan = session.build_plan()?;
        let backup = session.apply_expand(&plan)?;
        session.finish();
        Ok::<_, anyhow::Error>(backup)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(backup_dir.display().to_string())
}

fn expansion_of(profile: &FlashProfile) -> Result<&crate::config::ExpansionConfig, String> {
    profile.expansion.as_ref().ok_or_else(|| "当前配置未启用物理扩容".to_string())
}

#[command]
pub async fn list_backups_cmd(rom_root: Option<String>) -> Result<Vec<BackupInfo>, String> {
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    let cfg = expansion_of(&profile)?.clone();
    tokio::task::spawn_blocking(move || backup::list_backups(&cfg))
        .await
        .map_err(|e| e.to_string())
}

/// 检查用户选择的备份（zip 或目录）是否能用于当前刷机包。
#[command]
pub async fn inspect_backup_cmd(rom_root: Option<String>, backup_path: String) -> Result<BackupInfo, String> {
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    let cfg = expansion_of(&profile)?.clone();
    tokio::task::spawn_blocking(move || backup::inspect_backup(Path::new(&backup_path), &cfg))
        .await
        .map_err(|e| e.to_string())
}

#[command]
pub async fn restore_backup_cmd(
    app: AppHandle,
    rom_root: Option<String>,
    backup_path: String,
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
    let b_path = PathBuf::from(backup_path);

    tokio::task::spawn_blocking(move || {
        session.begin("restore", RESTORE_STEPS);
        session.restore_official(&b_path)?;
        session.finish();
        Ok::<_, anyhow::Error>(())
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
