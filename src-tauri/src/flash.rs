use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use anyhow::{anyhow, bail, Result};
use regex::Regex;
use tauri::{AppHandle, Emitter};

use crate::config::{FlashProfile, ImageOp};
use crate::device::{parse_size, run_process, DeviceSession, EnvReport};
use crate::package::{is_preserved_data_partition, read_package_anti, validate_package};
use crate::sdat2img::sdat2img;
use crate::sparse::{build_wipe_super_image, is_android_sparse};

pub const MIN_LOGICAL_FLASH_BYTES: u64 = 1024 * 1024;
pub const SMALL_LOGICAL_RECREATE_BYTES: u64 = 8 * 1024 * 1024;
pub const LOGICAL_BLOCK_BYTES: u64 = 4096;

pub fn logical_flash_size(out_size: u64, pad_to_bytes: Option<u64>) -> u64 {
    let mut size = out_size.max(pad_to_bytes.unwrap_or(0));
    if size < MIN_LOGICAL_FLASH_BYTES {
        size = MIN_LOGICAL_FLASH_BYTES;
    }
    if size % LOGICAL_BLOCK_BYTES != 0 {
        size += LOGICAL_BLOCK_BYTES - (size % LOGICAL_BLOCK_BYTES);
    }
    size
}

pub fn logical_size_for_sparse_payload(logical_size: u64, sparse_bytes: u64) -> u64 {
    if sparse_bytes <= logical_size {
        logical_size
    } else {
        logical_flash_size(sparse_bytes + LOGICAL_BLOCK_BYTES, None)
    }
}

pub fn is_overflow_flash_error(text: &str) -> bool {
    let lowered = text.to_lowercase();
    lowered.contains("value too large") || lowered.contains("eoverflow")
}

pub fn recreate_logical_partition(session: &DeviceSession, name: &str, size_bytes: u64) -> Result<()> {
    let _ = session.run_fastboot(
        &["delete-logical-partition", name],
        Some(&format!("delete-logical-partition {}", name)),
        true,
    );
    let size_str = size_bytes.to_string();
    let res = session.run_fastboot(
        &["create-logical-partition", name, &size_str],
        Some(&format!("create-logical-partition {} {}", name, size_bytes)),
        true,
    )?;
    if res.code != 0 {
        bail!(
            "创建逻辑分区 {} 失败（{}）：{}",
            name,
            res.code,
            res.text
        );
    }
    Ok(())
}

/// Return whether the device-side logical partition must grow before flashing.
/// `None` means fastbootd could not report the current size, so the caller must
/// use an explicit resize/recreate attempt instead of relying on auto-resize.
pub fn needs_logical_resize(current_bytes: Option<u64>, required_bytes: u64) -> bool {
    current_bytes.map_or(true, |current| current < required_bytes)
}

fn ensure_logical_partition(
    session: &DeviceSession,
    partition: &str,
    required_bytes: u64,
    force_recreate: bool,
) -> Result<()> {
    if !force_recreate {
        let reported = session.getvar(&format!("partition-size:{}", partition), true)?;
        let current = reported.as_deref().and_then(|value| parse_size(value).ok());
        if let Some(current_bytes) = current {
            if current_bytes >= required_bytes {
                return Ok(());
            }

            let size_str = required_bytes.to_string();
            let resized = session.run_fastboot(
                &["resize-logical-partition", partition, &size_str],
                Some(&format!("resize-logical-partition {} {}", partition, required_bytes)),
                true,
            )?;
            if resized.code == 0 {
                // Some fastbootd builds return success even when the logical
                // partition was not actually opened. Verify before flashing.
                let verified = session
                    .getvar(&format!("partition-size:{}", partition), true)?
                    .and_then(|value| parse_size(&value).ok());
                if verified.map_or(false, |size| size >= required_bytes) {
                    return Ok(());
                }
                session.logger.write(&format!(
                    "  │  {} resize 返回成功但分区仍不可读，改为删除并重建",
                    partition
                ));
            } else {
                session.logger.write(&format!(
                    "  │  {} 无法直接 resize，改为删除并重建：{}",
                    partition, resized.text
                ));
            }
        } else {
            session.logger.write(&format!(
                "  │  {} 尚不存在，跳过 resize，直接删除并重建",
                partition
            ));
        }
    }

    recreate_logical_partition(session, partition, required_bytes)
}

pub fn is_separate_id_partition(name: &str) -> bool {
    name.trim().to_lowercase() == "mi_ext"
}

fn flashed_partitions(text: &str) -> Vec<String> {
    let re = Regex::new(r"(?i)Partition\s+(\S+)\s+flashed successfully").unwrap();
    re.captures_iter(text).map(|c| c[1].to_string()).collect()
}

pub fn is_benign_flash_failure(partition: &str, text: &str) -> bool {
    let t = text;
    let has_1b = t.contains("0000001B");
    let has_error = t.to_lowercase().contains("error flashing partition");
    if !has_1b && !has_error {
        return false;
    }
    let ok = flashed_partitions(t);
    if ok.is_empty() {
        return false;
    }
    if partition.ends_with("_ab") {
        let base = &partition[..partition.len() - 3];
        let has_a = ok.iter().any(|p| p == &format!("{}_a", base) || p.ends_with("_a"));
        let has_b = ok.iter().any(|p| p == &format!("{}_b", base) || p.ends_with("_b"));
        if has_a && has_b {
            return true;
        }
        return has_1b && !ok.is_empty();
    }
    has_1b && !ok.is_empty()
}

pub fn flash_image(
    session: &DeviceSession,
    partition: &str,
    image_path: &Path,
    label: &str,
) -> Result<()> {
    let p_str = image_path.display().to_string();
    let res = session.run_fastboot(
        &["flash", partition, &p_str],
        Some(label),
        true,
    )?;
    if res.code == 0 {
        return Ok(());
    }

    if is_benign_flash_failure(partition, &res.text) {
        session.logger.write("  │  注：bootloader 已报告分区写入成功；远程错误 0000001B 在部分机型上可忽略。");
        return Ok(());
    }

    if partition.ends_with("_ab") {
        let base = &partition[..partition.len() - 3];
        session.logger.write(&format!("  │  {} 返回异常，尝试分别刷入 {}_a / {}_b …", partition, base, base));
        for slot in &["a", "b"] {
            let part = format!("{}_{}", base, slot);
            let sub_lbl = format!("刷入 {} ← {:?}", part, image_path.file_name().unwrap_or_default());
            let r = session.run_fastboot(
                &["flash", &part, &p_str],
                Some(&sub_lbl),
                true,
            )?;
            if r.code == 0 {
                continue;
            }
            if is_benign_flash_failure(&part, &r.text)
                || (r.text.contains(&format!("Partition {} flashed successfully", part)) && r.text.contains("0000001B"))
            {
                session.logger.write(&format!("  │  注：{} 已报告写入成功，忽略后续远程错误码。", part));
                continue;
            }
            bail!("刷入 {} 失败（{}）：flash {} {:?}\n{}", part, r.code, part, image_path, r.text);
        }
        return Ok(());
    }

    bail!("{} 失败（{}）：flash {} {:?}\n{}", label, res.code, partition, image_path, res.text);
}

pub fn gather_environment(
    profile: &FlashProfile,
    session: &mut DeviceSession,
    chosen_serial: Option<&str>,
) -> Result<EnvReport> {
    let serial = session.select_device(chosen_serial)?;
    let product = session.getvar("product", false)?
        .ok_or_else(|| anyhow!("无法读取 product 代号"))?;

    if !profile.codenames.iter().any(|c| c == &product) {
        bail!("机型不匹配：检测到 {}，本包仅支持：{}", product, profile.codenames.join(", "));
    }

    let size_text = session.getvar("partition-size:super", false)?
        .ok_or_else(|| anyhow!("无法读取 partition-size:super"))?;
    let super_bytes = parse_size(&size_text)?;
    let is_userspace = session.getvar("is-userspace", true)?
        .map_or(false, |v| v.to_lowercase() == "yes");
    let anti = session.getvar("anti", true)?;

    Ok(EnvReport {
        serial,
        product,
        super_bytes,
        super_ready: super_bytes >= profile.minimum_super_bytes,
        is_userspace,
        anti,
    })
}

pub fn run_image_op(profile: &FlashProfile, session: &DeviceSession, op: &ImageOp) -> Result<()> {
    if op.action == "erase" {
        session.run_fastboot(&["erase", &op.partition], Some(&format!("擦除 {}", op.partition)), false)?;
        return Ok(());
    }
    let file_name = op.file.as_ref().ok_or_else(|| anyhow!("ImageOp 缺少 file"))?;
    let path = profile.images_dir.join(file_name);
    flash_image(
        session,
        &op.partition,
        &path,
        &format!("刷入 {} ← {}", op.partition, file_name),
    )
}

pub fn has_data_operation(profile: &FlashProfile, base_partition: &str) -> bool {
    profile.final_data.iter().any(|op| {
        let partition = op.partition.trim().to_lowercase();
        partition == base_partition
            || partition == format!("{}_a", base_partition)
            || partition == format!("{}_b", base_partition)
            || partition == format!("{}_ab", base_partition)
    })
}

fn ensure_clean_data_partitions(
    profile: &FlashProfile,
    session: &DeviceSession,
    preserve_data: bool,
) -> Result<()> {
    if preserve_data {
        return Ok(());
    }

    for partition in ["userdata", "metadata"] {
        if has_data_operation(profile, partition) {
            continue;
        }

        session.logger.write(&format!(
            "  ├─ 配置未声明 {} 操作，执行完整线刷默认擦除",
            partition
        ));
        session.run_fastboot(
            &["erase", partition],
            Some(&format!("格式化 {}", partition)),
            false,
        )?;
    }

    Ok(())
}

pub fn restore_dat_to_raw(
    profile: &FlashProfile,
    session: &DeviceSession,
    name: &str,
    dat: &Path,
    raw: &Path,
    app_handle: Option<&AppHandle>,
) -> Result<u64> {
    std::fs::create_dir_all(&profile.work_dir)?;
    let transfer = profile.super_dir.join(format!("{}.transfer.list", name));
    if !transfer.is_file() {
        bail!("缺少 {}.transfer.list，无法正确还原分区镜像", name);
    }
    if !dat.is_file() {
        bail!("缺少 {}.new.dat，无法正确还原分区镜像", name);
    }
    if session.dry_run {
        session.logger.write(&format!("  ├─ [dry-run] sdat2img {:?} -> {:?}", transfer.file_name(), raw.file_name()));
        File::create(raw)?;
        return Ok(0);
    }

    session.logger.write(&format!("sdat2img {:?} {:?} -> {:?}", transfer, dat, raw));

    let part_name = name.to_string();
    let handle_clone = app_handle.cloned();

    let out_size = sdat2img(&transfer, dat, raw, 4096, move |done, total| {
        let pct = if total > 0 { (done * 100) / total } else { 0 };
        if let Some(ref h) = handle_clone {
            let _ = h.emit("partition_progress", serde_json::json!({
                "name": part_name,
                "done": done,
                "total": total,
                "percent": pct
            }));
        }
    })?;

    session.logger.write(&format!("sdat2img {} raw 字节数: {}", name, out_size));
    Ok(out_size)
}

pub fn convert_dat_to_sparse(
    profile: &FlashProfile,
    session: &DeviceSession,
    name: &str,
    dat: &Path,
    sparse: &Path,
    pad_to_bytes: Option<u64>,
    app_handle: Option<&AppHandle>,
) -> Result<u64> {
    let raw = profile.work_dir.join(format!("{}.raw.img", name));
    let mut out_size = restore_dat_to_raw(profile, session, name, dat, &raw, app_handle)?;
    if session.dry_run {
        session.logger.write(&format!("  ├─ [dry-run] img2simg {:?} -> {:?}", raw.file_name(), sparse.file_name()));
        File::create(sparse)?;
        return Ok(logical_flash_size(pad_to_bytes.unwrap_or(0), None));
    }

    let target_size = logical_flash_size(out_size, pad_to_bytes);
    if target_size > out_size {
        let f = std::fs::OpenOptions::new().write(true).open(&raw)?;
        f.set_len(target_size)?;
        session.logger.write(&format!("  │  {} 已补零到 {} 字节（避免 fastbootd 过小分区失败）", name, target_size));
        out_size = target_size;
    }

    for attempt in 1..=3 {
        session.logger.write(&format!("  │  img2simg：{} → sparse…（第 {} 次尝试）", name, attempt));
        let raw_str = raw.display().to_string();
        let sparse_str = sparse.display().to_string();
        let started = Instant::now();
        let res = run_process(&profile.img2simg(), &[&raw_str, &sparse_str], Some(&session.logger))?;
        if res.code != 0 {
            bail!("img2simg 转换 {} 失败: {}", name, res.text);
        }

        let sparse_bytes = sparse.metadata()?.len();
        session.logger.write(&format!(
            "  │  img2simg 已完成：{} sparse 字节，耗时 {:.1} 秒",
            sparse_bytes,
            started.elapsed().as_secs_f64()
        ));
        let safe_size = logical_size_for_sparse_payload(out_size, sparse_bytes);
        if safe_size == out_size {
            break;
        }

        let f = std::fs::OpenOptions::new().write(true).open(&raw)?;
        f.set_len(safe_size)?;
        let _ = std::fs::remove_file(sparse);
        session.logger.write(&format!("  │  {} sparse 封装比逻辑分区大，已补零到 {} 字节后重试", name, safe_size));
        out_size = safe_size;
        if attempt == 3 {
            bail!("{} sparse 封装尺寸连续超过逻辑分区，无法安全刷入", name);
        }
    }

    let _ = std::fs::remove_file(&raw);
    Ok(out_size)
}

pub fn prepare_wipe_super_source(
    profile: &FlashProfile,
    session: &DeviceSession,
    app_handle: Option<&AppHandle>,
) -> Result<PathBuf> {
    if let Some(ref name) = profile.wipe_super_dat {
        let dat = profile.super_dir.join(format!("{}.new.dat", name));
        let raw = profile.work_dir.join(format!("{}.raw.img", name));
        session.logger.write(&format!("  │  正在还原空 super：{}.new.dat", name));
        restore_dat_to_raw(profile, session, name, &dat, &raw, app_handle)?;
        return Ok(raw);
    }
    if let Some(ref img) = profile.wipe_super_image {
        return Ok(profile.images_dir.join(img));
    }
    bail!("线刷配置未指定空 super 的 IMG 或 DAT");
}

fn create_logical_partitions_from_profile(profile: &FlashProfile, session: &DeviceSession) -> bool {
    if profile.dynamic_partitions.is_empty() {
        return false;
    }
    let suffix = &profile.slot_suffix;
    let mut ok_any = false;
    for part in &profile.dynamic_partitions {
        let name = format!("{}{}", part.name, suffix);
        let size = logical_flash_size(part.pad_to_bytes.unwrap_or(part.dat_bytes), None);
        let _ = session.run_fastboot(&["delete-logical-partition", &name], None, true);
        let size_str = size.to_string();
        if let Ok(res) = session.run_fastboot(&["create-logical-partition", &name, &size_str], None, true) {
            if res.code == 0 {
                ok_any = true;
            }
        }
    }
    ok_any
}

fn clear_profile_logical_partitions(profile: &FlashProfile, session: &DeviceSession) {
    let selected_suffix = if profile.slot_suffix.is_empty() {
        ""
    } else {
        &profile.slot_suffix
    };
    let other_suffix = match selected_suffix {
        "_a" => "_b",
        "_b" => "_a",
        _ => "",
    };
    let mut names = Vec::new();
    for part in &profile.dynamic_partitions {
        let bases = [
            format!("{}{}", part.name, selected_suffix),
            format!("{}{}", part.name, other_suffix),
            part.name.clone(),
        ];
        for base in bases {
            for candidate in [base.clone(), format!("{}-cow", base), format!("{}_cow", base)] {
                if !names.iter().any(|name| name == &candidate) {
                    names.push(candidate);
                }
            }
        }
    }

    session.logger.write("  │  清理现有动态逻辑分区（含另一槽位与 COW）以释放 super 空间");
    for name in names {
        let _ = session.run_fastboot(
            &["delete-logical-partition", &name],
            Some(&format!("删除逻辑分区 {}", name)),
            true,
        );
    }
}

pub fn wipe_or_init_super(
    profile: &FlashProfile,
    session: &DeviceSession,
    wipe_img: &Path,
) -> Result<()> {
    if !wipe_img.is_file() {
        bail!("找不到空 super 刷写源：{:?}", wipe_img);
    }

    let us = session.getvar("is-userspace", true)?.unwrap_or_default();
    if us != "yes" {
        session.logger.write("  │  未检测到 fastbootd，尝试重新进入…");
        let _ = session.run_fastboot(&["reboot", "fastboot"], Some("reboot fastboot"), true);
        std::thread::sleep(Duration::from_secs(3));
        session.wait_for_mode(true, 90)?;
    }

    let _ = session.run_fastboot(
        &["snapshot-update", "cancel"],
        Some("取消 Virtual A/B 快照"),
        true,
    );

    std::fs::create_dir_all(&profile.work_dir)?;
    let mut candidates: Vec<(&str, PathBuf)> = Vec::new();

    let wipe_meta = profile.work_dir.join("super_empty_wipe.img");
    match build_wipe_super_image(wipe_img, &wipe_meta, 16 * 1024 * 1024) {
        Ok(_) => {
            session.logger.write(&format!("  │  已生成 wipe-super 元数据镜像（{:?} 字节）", wipe_meta.metadata()?.len()));
            candidates.push(("wipe-super", wipe_meta));
        }
        Err(e) => {
            session.logger.write(&format!("  │  生成 wipe-super 镜像失败：{}", e));
            if !is_android_sparse(wipe_img) {
                candidates.push(("wipe-super", wipe_img.to_path_buf()));
            }
        }
    }

    if is_android_sparse(wipe_img) {
        candidates.push(("flash-super", wipe_img.to_path_buf()));
    }

    let mut last_error = String::new();
    for (mode, path) in candidates {
        let attempts = if mode == "flash-super" { 3 } else { 2 };
        let p_str = path.display().to_string();
        for attempt in 1..=attempts {
            let label = format!("{} ← {:?}（第 {}/{} 次）", mode, path.file_name().unwrap_or_default(), attempt, attempts);
            let res = if mode == "wipe-super" {
                session.run_fastboot(&["wipe-super", &p_str], Some(&label), true)?
            } else {
                session.run_fastboot(&["flash", "super", &p_str], Some(&label), true)?
            };
            if res.code == 0 {
                session.logger.write(&format!("  │  空 super 初始化成功（{}）", mode));
                return Ok(());
            }
            last_error = res.text;
            if last_error.contains("No such file") || last_error.to_lowercase().contains("not found") {
                std::thread::sleep(Duration::from_secs(2 + attempt));
            } else {
                break;
            }
        }
        session.logger.write(&format!("  │  {} 未成功，尝试下一方案…", mode));
    }

    session.logger.write("  │  尝试按配置 create-logical-partition 创建逻辑分区…");
    if create_logical_partitions_from_profile(profile, session) {
        session.logger.write("  │  逻辑分区已创建，继续刷写动态分区");
        return Ok(());
    }

    bail!("在 fastbootd 初始化空 super 失败。\n最后错误：{}", last_error);
}

pub fn flash_dynamic_partition_image(
    session: &DeviceSession,
    partition: &str,
    image: &Path,
    logical_size: u64,
) -> Result<()> {
    let small = logical_size <= SMALL_LOGICAL_RECREATE_BYTES;
    ensure_logical_partition(session, partition, logical_size, small)?;
    let img_str = image.display().to_string();
    let res = session.run_fastboot(&["flash", partition, &img_str], Some(&format!("刷入 {}", partition)), true)?;
    if res.code == 0 {
        return Ok(());
    }

    if is_overflow_flash_error(&res.text) || small {
        let retry_size = logical_size.max(MIN_LOGICAL_FLASH_BYTES);
        session.logger.write("  │  逻辑分区写入失败，正在重建后重试…");
        ensure_logical_partition(session, partition, retry_size, true)?;
        let res2 = session.run_fastboot(&["flash", partition, &img_str], Some(&format!("刷入 {}（重试）", partition)), true)?;
        if res2.code == 0 {
            return Ok(());
        }
        bail!("刷入 {} 失败（{}）：flash {} {:?}\n{}", partition, res2.code, partition, image, res2.text);
    }

    bail!("刷入 {} 失败（{}）：flash {} {:?}\n{}", partition, res.code, partition, image, res.text);
}

pub fn flash_dynamic_partitions(
    profile: &FlashProfile,
    session: &DeviceSession,
    app_handle: Option<&AppHandle>,
) -> Result<()> {
    session.run_fastboot(&["reboot", "fastboot"], Some("重启到 fastbootd"), false)?;
    if !session.dry_run {
        session.wait_for_mode(true, 90)?;
    }

    let wipe_img = prepare_wipe_super_source(profile, session, app_handle)?;
    wipe_or_init_super(profile, session, &wipe_img)?;
    clear_profile_logical_partitions(profile, session);

    let _ = std::fs::remove_dir_all(&profile.work_dir);
    std::fs::create_dir_all(&profile.work_dir)?;

    let total = profile.dynamic_partitions.len();
    for (i, part) in profile.dynamic_partitions.iter().enumerate() {
        let index = i + 1;
        let target = format!("{}{}", part.name, profile.slot_suffix);
        if profile.post_flash_id && is_separate_id_partition(&part.name) {
            session.logger.write(&format!("  ├─ [{}/{}] 跳过 {}（由独立 ID 补丁刷入，避免覆盖）", index, total, part.name));
            continue;
        }

        let dat = profile.super_dir.join(format!("{}.new.dat", part.name));
        let sparse = profile.work_dir.join(format!("{}.img", part.name));
        session.logger.write(&format!("  ├─ [{}/{}] 转换与刷写 {} DAT → sparse", index, total, part.name));

        let flashed_size = convert_dat_to_sparse(
            profile,
            session,
            &part.name,
            &dat,
            &sparse,
            part.pad_to_bytes,
            app_handle,
        )?;

        flash_dynamic_partition_image(session, &target, &sparse, flashed_size)?;
        let _ = std::fs::remove_file(&sparse);
    }

    let _ = std::fs::remove_dir_all(&profile.work_dir);

    session.run_fastboot(&["reboot", "bootloader"], Some("返回 bootloader"), false)?;
    if !session.dry_run {
        session.wait_for_mode(false, 90)?;
    }

    Ok(())
}

pub fn initialize_crc(profile: &FlashProfile, session: &DeviceSession) -> Result<()> {
    if !profile.flash_crc_lists {
        return Ok(());
    }
    if let Ok(Some(crc)) = session.getvar("crc", true) {
        if crc == "1" {
            let crc_img = profile.images_dir.join(&profile.crc_list_file).display().to_string();
            let sparse_crc_img = profile.images_dir.join(&profile.sparse_crc_list_file).display().to_string();
            let _ = session.run_fastboot(&["flash", "crclist", &crc_img], Some("加载普通镜像 CRC 表"), true);
            let _ = session.run_fastboot(&["flash", "sparsecrclist", &sparse_crc_img], Some("加载 sparse 镜像 CRC 表"), true);
        }
    }
    Ok(())
}

pub fn execute_flash_pipeline(
    profile: &FlashProfile,
    mut session: DeviceSession,
    preserve_data: bool,
    app_handle: Option<&AppHandle>,
) -> Result<()> {
    let emit_stage = |stage: u32, title: &str| {
        if let Some(h) = app_handle {
            let _ = h.emit("flash_stage", serde_json::json!({
                "stage": stage,
                "total": 6,
                "title": title
            }));
        }
    };

    // Stage 1: Preflight
    emit_stage(1, "安全预检与环境确认");
    validate_package(profile, preserve_data)?;

    let chosen_serial = session.serial.clone();
    let report = gather_environment(profile, &mut session, chosen_serial.as_deref())?;
    if !report.super_ready {
        bail!(
            "super 只有 {} 字节，未达到要求的 {} 字节。请先运行独立扩容工具。",
            report.super_bytes, profile.minimum_super_bytes
        );
    }

    if report.is_userspace {
        session.run_fastboot(&["reboot", "bootloader"], Some("从 fastbootd 返回 bootloader"), false)?;
        if !session.dry_run {
            session.wait_for_mode(false, 90)?;
        }
    }

    if let Some(pkg_anti) = read_package_anti(profile)? {
        let mut dev_anti = 0u32;
        if let Some(ref a_str) = report.anti {
            if let Ok(num) = a_str.trim().parse::<u32>() {
                dev_anti = num;
            }
        }
        if dev_anti > pkg_anti {
            bail!("Anti rollback 检查失败：设备={}，刷机包={}。", dev_anti, pkg_anti);
        }
        session.logger.write(&format!("Anti check OK: device={}, package={}", dev_anti, pkg_anti));
    }

    initialize_crc(profile, &session)?;

    // Stage 2: Firmware
    emit_stage(2, "固件与擦除操作");
    for name in &profile.erase_if_present {
        if let Ok(Some(ptype)) = session.getvar(&format!("partition-type:{}", name), true) {
            if ptype == "raw" {
                session.run_fastboot(&["erase", name], Some(&format!("擦除 {}", name)), true)?;
            }
        }
    }
    for name in &profile.pre_erase {
        session.run_fastboot(&["erase", name], Some(&format!("擦除 {}", name)), true)?;
    }
    for op in &profile.firmware {
        run_image_op(profile, &session, op)?;
    }

    // Stage 3: Boot Chain
    emit_stage(3, "补齐 fastbootd 启动链");
    for op in &profile.boot_chain {
        run_image_op(profile, &session, op)?;
    }
    if let Some(ref slot) = profile.set_active_slot {
        session.run_fastboot(&["set_active", slot], Some(&format!("预设活动槽位 {}", slot)), true)?;
    }

    // Stage 4: Super & Dynamic
    emit_stage(4, "super 与动态分区刷写");
    if !profile.dynamic_partitions.is_empty() {
        flash_dynamic_partitions(profile, &session, app_handle)?;
    }

    // Stage 5: Final Data
    emit_stage(5, if preserve_data { "保留数据分区处理" } else { "数据分区与最终槽位" });
    for op in &profile.final_data {
        if preserve_data && is_preserved_data_partition(&op.partition) {
            session.logger.write(&format!("  ├─ [更新模式] 跳过 {} {}", op.action, op.partition));
            continue;
        }
        run_image_op(profile, &session, op)?;
    }
    ensure_clean_data_partitions(profile, &session, preserve_data)?;
    if let Some(ref slot) = profile.set_active_slot {
        session.run_fastboot(&["set_active", slot], Some(&format!("设置最终活动槽位 {}", slot)), true)?;
    }

    // Stage 6: Complete
    emit_stage(6, "刷写圆满完成");
    session.logger.write("=== 刷机流程全部完成 ===");

    if profile.reboot_after_success && !session.dry_run {
        session.run_fastboot(&["reboot"], Some("重启设备进入系统"), true)?;
    }

    Ok(())
}
