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
use crate::rootpatch::{self, PatchRequest, RootCandidates, RootToolOutcome};
use crate::backup::{self, BackupInfo};
use crate::boardid::{self, BoardIdOutcome, FixAction, ReadMethod};
use crate::config::find_rom_root;
use crate::official;
use crate::update;
use crate::usb_driver;
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
    if official::is_official_root(&picked) {
        return activate_dir(picked);
    }
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
        .or_else(|| official::find_official_root(&picked))
        .ok_or_else(|| format!(
            "这个文件夹里没有找到刷机配置。\n请选择 FlashProfile 刷机包解压后的根目录（含 {}），或小米官方线刷包的目录（含 flash_all.bat 和 images）。",
            crate::config::DEFAULT_PROFILE_REL.replace('/', "\\")
        ))?;
    activate_dir(root)
}

/// 先完整加载一次配置，有错误立即提示，不切换当前刷机包。
fn activate_dir(root: PathBuf) -> Result<String, String> {
    get_active_profile(Some(&root.display().to_string()), None)?;
    let root = root.canonicalize().unwrap_or(root);
    rompkg::set_active_root(Some(root.clone()));
    Ok(root.display().to_string())
}

/// Android USB 驱动是否已安装，以及是否有手机因缺少驱动无法识别。
#[command]
pub async fn usb_driver_status_cmd() -> Result<usb_driver::DriverStatus, String> {
    tokio::task::spawn_blocking(usb_driver::status).await.map_err(|e| e.to_string())
}

/// 安装内置的 Android USB 驱动（会弹出管理员授权）。
#[command]
pub async fn install_usb_driver_cmd() -> Result<usb_driver::InstallResult, String> {
    tokio::task::spawn_blocking(usb_driver::install)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
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

fn startup_official_root() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        candidates.push(dir);
    }
    candidates.into_iter().find(|d| official::is_official_root(d))
}

fn get_active_profile(rom_root: Option<&str>, profile_path: Option<&str>) -> Result<FlashProfile, String> {
    let active = if rom_root.is_none() && profile_path.is_none() { rompkg::active_root() } else { None };
    let r_path = rom_root.map(Path::new).or(active.as_deref());
    if profile_path.is_none() {
        // 小米官方线刷包：选中的目录，或程序放在官方包目录里运行
        let official_root = match r_path {
            Some(r) => official::is_official_root(r).then(|| r.to_path_buf()),
            None if find_rom_root(None).is_none() => startup_official_root(),
            None => None,
        };
        if let Some(root) = official_root {
            return official::load_official(&root).map_err(|e| format!("{:#}", e));
        }
    }
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
    let _ = logger.start(&profile.logs_dir, "wrion-diag");
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

/// 刷入类命令的结果。
#[derive(Debug, Clone, Serialize)]
pub struct FlashOutcome {
    pub log_path: String,
    /// 手机是否已收到重启命令（关闭自动重启或重启失败时为 false）。
    pub rebooted: bool,
}

#[command]
pub async fn run_flash_cmd(
    app: AppHandle,
    rom_root: Option<String>,
    chosen_serial: Option<String>,
    preserve_data: bool,
    confirm_word: String,
    reboot: bool,
) -> Result<FlashOutcome, String> {
    let mut profile = get_active_profile(rom_root.as_deref(), None)?;
    // 以用户的“刷入完成后自动重启”选项为准（界面默认勾选）；刷机包里的 reboot_after_success 只影响命令行模式
    profile.reboot_after_success = reboot;

    let required_word = if preserve_data {
        &profile.update_confirmation_word
    } else {
        &profile.confirmation_word
    };

    if confirm_word.trim() != required_word {
        return Err(format!("确认词不匹配，请输入 {}", required_word));
    }

    let mut logger = Logger::new(Some(app.clone()));
    let prefix = if preserve_data { "wrion-update" } else { "wrion-flash" };
    let log_path = logger.start(&profile.logs_dir, prefix).map_err(|e| e.to_string())?;

    let mut session = DeviceSession::new(profile.fastboot(), profile.adb(), logger, false);
    session.serial = chosen_serial;

    let reboot_after = profile.reboot_after_success;
    let rebooted = tokio::task::spawn_blocking(move || {
        if profile.is_official() {
            official::run_official(&profile, session, preserve_data, reboot_after, Some(&app))
        } else {
            execute_flash_pipeline(&profile, session, preserve_data, Some(&app))
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(FlashOutcome { log_path: log_path.display().to_string(), rebooted })
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
) -> Result<FlashOutcome, String> {
    if confirm_word.trim() != "ROOT" {
        return Err("请输入 ROOT 确认刷入".to_string());
    }
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    let mut logger = Logger::new(Some(app));
    let log_path = logger.start(&profile.logs_dir, "wrion-root").map_err(|e| e.to_string())?;

    let mut session = DeviceSession::new(profile.fastboot(), profile.adb(), logger, false);
    session.serial = chosen_serial;
    let img_path = PathBuf::from(image_path);

    let rebooted = tokio::task::spawn_blocking(move || {
        flash_root_image(&profile, &mut session, &img_path, &target, reboot)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(FlashOutcome { log_path: log_path.display().to_string(), rebooted })
}

/// 扫描当前线刷包中可用于 KernelSU/APatch 的启动镜像、内核和 kpimg。
#[command]
pub async fn root_candidates_cmd(rom_root: Option<String>) -> Result<RootCandidates, String> {
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    Ok(rootpatch::discover(&profile))
}

/// Root 工具：用 KernelSU（init_boot）或 APatch（boot）修补后直接刷入。
/// use_package 为 true 时使用当前刷机包里的镜像并校验机型；否则使用 image_path，不需要选择刷机包。
#[command]
pub async fn run_root_tool_cmd(
    app: AppHandle,
    chosen_serial: Option<String>,
    use_package: bool,
    method: String,
    image_path: String,
    kernel_path: String,
    kmi: String,
    super_key: String,
    reboot: bool,
) -> Result<RootToolOutcome, String> {
    let profile = if use_package { Some(get_active_profile(None, None)?) } else { None };
    let (work_dir, logs_dir, fastboot, adb) = match &profile {
        Some(p) => (p.work_dir.clone(), p.logs_dir.clone(), p.fastboot(), p.adb()),
        None => {
            let tools = crate::platform_tools::tools_dir().map_err(|e| format!("{:#}", e))?;
            let (work, logs) = rootpatch::standalone_dirs();
            (work, logs, tools.join("fastboot.exe"), tools.join("adb.exe"))
        }
    };
    let mut logger = Logger::new(Some(app));
    logger.start(&logs_dir, "wrion-root").map_err(|e| e.to_string())?;
    let mut session = DeviceSession::new(fastboot, adb, logger, false);
    session.serial = chosen_serial;
    let req = PatchRequest { method, image_path, kernel_path, kmi, super_key };

    tokio::task::spawn_blocking(move || {
        rootpatch::run_root_tool(profile.as_ref(), &mut session, &work_dir, &req, reboot)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{:#}", e))
}

/// 刷机包目录（及其上一层）里文件名像 ID 补丁的 .zip / .img，供界面快速选择。
#[command]
pub async fn id_candidates_cmd(rom_root: Option<String>) -> Result<Vec<String>, String> {
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    let mut dirs = vec![profile.rom_root.clone()];
    if let Some(parent) = profile.rom_root.parent() {
        dirs.push(parent.to_path_buf());
    }
    let mut found: Vec<String> = dirs
        .iter()
        .flat_map(|d| crate::idflash::discover_id_candidates(d))
        .map(|p| p.display().to_string())
        .collect();
    found.dedup();
    Ok(found)
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
) -> Result<FlashOutcome, String> {
    if confirm_word.trim() != "ID" {
        return Err("请输入 ID 确认刷入".to_string());
    }
    let profile = get_active_profile(rom_root.as_deref(), None)?;
    let mut logger = Logger::new(Some(app));
    let log_path = logger.start(&profile.logs_dir, "wrion-id").map_err(|e| e.to_string())?;

    let mut session = DeviceSession::new(profile.fastboot(), profile.adb(), logger, false);
    session.serial = chosen_serial;
    let id_p = PathBuf::from(id_file);

    let rebooted = tokio::task::spawn_blocking(move || {
        run_id_flash(&profile, &mut session, &id_p, format_data, reboot)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(FlashOutcome { log_path: log_path.display().to_string(), rebooted })
}

/// 读取主板 ID（soc0/serial_number）。不需要先选择刷机包；每一步的检查结果和失败时的解决办法都在返回值里。
#[command]
pub async fn read_board_id_cmd(
    app: AppHandle,
    chosen_serial: Option<String>,
    method: ReadMethod,
    quiet: Option<bool>,
) -> Result<BoardIdOutcome, String> {
    // quiet：界面在等待用户操作手机时自动重试，不写日志，免得刷屏
    let quiet = quiet.unwrap_or(false);
    let mut logger = Logger::new((!quiet).then_some(app));
    // 已选刷机包时同时写入日志文件，方便用户发给别人排查
    if let (false, Ok(profile)) = (quiet, get_active_profile(None, None)) {
        let _ = logger.start(&profile.logs_dir, "wrion-boardid");
    }
    tokio::task::spawn_blocking(move || boardid::read_board_id(&logger, chosen_serial.as_deref(), method))
        .await
        .map_err(|e| e.to_string())
}

/// 执行读取主板 ID 时给出的解决操作（重启 ADB 服务、从 Fastboot 重启进系统）。
#[command]
pub async fn board_id_fix_cmd(
    app: AppHandle,
    action: FixAction,
    serial: Option<String>,
) -> Result<String, String> {
    let logger = Logger::new(Some(app));
    tokio::task::spawn_blocking(move || boardid::run_fix(&logger, action, serial.as_deref()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{:#}", e))
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
    let _ = logger.start(&profile.logs_dir, "wrion-expand").map_err(|e| e.to_string())?;
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
    let _ = logger.start(&profile.logs_dir, "wrion-restore").map_err(|e| e.to_string())?;
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

#[command]
pub async fn check_update_cmd() -> Result<update::UpdateInfo, String> {
    tokio::task::spawn_blocking(update::check)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{:#}", e))
}

/// 在默认浏览器打开新版本下载页；只接受 https 链接。
#[command]
pub async fn open_download_cmd(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("下载链接必须是 https 地址".to_string());
    }
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| e.to_string())
}
