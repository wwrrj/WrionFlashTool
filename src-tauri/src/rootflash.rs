use std::fs::File;
use std::io::Read;
use std::path::Path;
use anyhow::{anyhow, bail, Result};
use crate::config::FlashProfile;
use crate::device::{parse_size, DeviceSession};
use crate::flash::flash_image;

pub fn verify_boot_image(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("镜像文件不存在：{:?}", path);
    }
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    if ext != "img" {
        bail!("只接受 .img 格式的镜像文件");
    }
    let meta = path.metadata()?;
    if meta.len() < 4096 {
        bail!("镜像文件过小（< 4096 字节），拒绝刷入");
    }

    let mut fh = File::open(path)?;
    let mut magic = [0u8; 8];
    fh.read_exact(&mut magic)?;
    if &magic != b"ANDROID!" {
        bail!("镜像没有 Android boot image 头（ANDROID!），不能作为 boot/init_boot 刷入");
    }
    Ok(())
}

fn query_partition_size(session: &DeviceSession, target: &str) -> Result<u64> {
    let mut values = Vec::new();
    for slot in &["a", "b"] {
        if let Ok(Some(raw)) = session.getvar(&format!("partition-size:{}_{}", target, slot), true) {
            if let Ok(sz) = parse_size(&raw) {
                values.push(sz);
            }
        }
    }
    if values.is_empty() {
        if let Ok(Some(raw)) = session.getvar(&format!("partition-size:{}", target), true) {
            if let Ok(sz) = parse_size(&raw) {
                values.push(sz);
            }
        }
    }
    if values.is_empty() {
        bail!("无法读取 {} 分区物理大小，拒绝盲刷", target);
    }
    Ok(*values.iter().min().unwrap())
}

/// 刷入 Root 镜像前的准备：选中手机、校验机型（使用刷机包时）、确保处于 bootloader。
pub fn prepare_root_flash(profile: Option<&FlashProfile>, session: &mut DeviceSession) -> Result<()> {
    let chosen_serial = session.serial.clone();
    session.select_device(chosen_serial.as_deref())?;
    let product = session.getvar("product", false)?
        .ok_or_else(|| anyhow!("无法读取机型 product 代号"))?;
    match profile {
        Some(profile) if !profile.codenames.iter().any(|c| c == &product) => {
            bail!("机型不匹配：检测到 {}，支持：{}", product, profile.codenames.join(", "));
        }
        Some(_) => {}
        None => session.logger.write(&format!("  未使用刷机包，跳过机型校验（手机：{}）", product)),
    }

    let is_us = session.getvar("is-userspace", true)?.unwrap_or_default().to_lowercase() == "yes";
    if is_us {
        session.logger.write("当前为 fastbootd，正在切换到 bootloader fastboot…");
        session.run_fastboot(&["reboot", "bootloader"], Some("reboot bootloader"), true)?;
        session.wait_for_any_fastboot(90)?;
        session.wait_for_mode(false, 90)?;
    }
    Ok(())
}

/// 把 Root 镜像写入 `<target>_ab`；先核对分区物理大小，超出就拒绝。
pub fn write_root_image(session: &DeviceSession, image_path: &Path, target: &str) -> Result<()> {
    verify_boot_image(image_path)?;
    let limit = query_partition_size(session, target)?;
    let size = image_path.metadata()?.len();
    if size > limit {
        bail!("镜像大小（{} 字节）超过 {} 分区物理上限（{} 字节）", size, target, limit);
    }

    let ab_part = format!("{}_ab", target);
    flash_image(
        session,
        &ab_part,
        image_path,
        &format!("刷入 {} ← {:?}", ab_part, image_path.file_name().unwrap_or_default()),
    )?;

    session.logger.write("  Root 镜像刷入完成。");
    Ok(())
}

pub fn flash_root_image(
    profile: &FlashProfile,
    session: &mut DeviceSession,
    image_path: &Path,
    target: &str, // "boot" or "init_boot"
    reboot: bool,
) -> Result<bool> {
    verify_boot_image(image_path)?;
    prepare_root_flash(Some(profile), session)?;
    write_root_image(session, image_path, target)?;
    session.reboot_to_system(reboot)
}
