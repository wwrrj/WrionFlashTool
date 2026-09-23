use std::path::PathBuf;
use anyhow::{anyhow, bail, Result};
use crate::config::{FlashProfile, ImageOp};

pub fn is_preserved_data_partition(partition: &str) -> bool {
    let mut name = partition.trim().to_lowercase();
    for suffix in &["_ab", "_a", "_b"] {
        if name.ends_with(suffix) {
            name.truncate(name.len() - suffix.len());
            break;
        }
    }
    name == "userdata" || name == "metadata"
}

fn image_path(profile: &FlashProfile, op: &ImageOp) -> Option<PathBuf> {
    if op.action != "flash" {
        return None;
    }
    op.file.as_ref().map(|f| profile.images_dir.join(f))
}

pub fn collect_required_paths(profile: &FlashProfile, preserve_data: bool) -> Vec<PathBuf> {
    let mut required = vec![
        profile.fastboot(),
        profile.adb(),
    ];

    if !profile.dynamic_partitions.is_empty() {
        required.push(profile.img2simg());
        required.push(profile.cygwin_dll());

        if let Some(ref dat) = profile.wipe_super_dat {
            required.push(profile.super_dir.join(format!("{}.new.dat", dat)));
            required.push(profile.super_dir.join(format!("{}.transfer.list", dat)));
        } else if let Some(ref img) = profile.wipe_super_image {
            required.push(profile.images_dir.join(img));
        }
    }

    if profile.flash_crc_lists {
        required.push(profile.images_dir.join(&profile.crc_list_file));
        required.push(profile.images_dir.join(&profile.sparse_crc_list_file));
    }

    let all_ops = profile.firmware.iter()
        .chain(profile.boot_chain.iter())
        .chain(profile.final_data.iter());

    for op in all_ops {
        if preserve_data && is_preserved_data_partition(&op.partition) {
            continue;
        }
        if let Some(p) = image_path(profile, op) {
            required.push(p);
        }
    }

    for part in &profile.dynamic_partitions {
        required.push(profile.super_dir.join(format!("{}.new.dat", part.name)));
        required.push(profile.super_dir.join(format!("{}.transfer.list", part.name)));
    }

    required
}

pub fn validate_package(profile: &FlashProfile, preserve_data: bool) -> Result<()> {
    let required = collect_required_paths(profile, preserve_data);
    let mut missing = Vec::new();
    for p in required {
        if !p.is_file() {
            missing.push(p.display().to_string());
        }
    }
    if !missing.is_empty() {
        bail!("刷机包文件不完整：\n{}", missing.join("\n"));
    }

    for part in &profile.dynamic_partitions {
        let path = profile.super_dir.join(format!("{}.new.dat", part.name));
        let actual = std::fs::metadata(&path)?.len();
        if actual != part.dat_bytes {
            bail!(
                "{}.new.dat 大小异常：预期 {} 字节，实际 {} 字节",
                part.name, part.dat_bytes, actual
            );
        }
    }

    Ok(())
}

pub fn read_package_anti(profile: &FlashProfile) -> Result<Option<u32>> {
    let rel = match profile.anti_version_file {
        Some(ref f) => f,
        None => return Ok(None),
    };
    let path = profile.images_dir.join(rel);
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)?;
    let val: u32 = text.trim().parse()
        .map_err(|e| anyhow!("无法解析 anti 版本文件：{:?} -> {:?}: {}", path, text, e))?;
    Ok(Some(val))
}
