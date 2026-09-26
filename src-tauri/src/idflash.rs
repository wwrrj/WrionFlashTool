use crate::config::FlashProfile;
use crate::device::DeviceSession;
use crate::flash::{
    is_overflow_flash_error, logical_flash_size, recreate_logical_partition,
    MIN_LOGICAL_FLASH_BYTES,
};
use crate::sdat2img::sdat2img;
use crate::sparse::is_android_sparse;
use anyhow::{anyhow, bail, Context, Result};
use std::fs::File;
use std::path::{Path, PathBuf};

pub fn ensure_fastbootd(session: &DeviceSession) -> Result<()> {
    if let Ok(Some(us)) = session.getvar("is-userspace", true) {
        if us.to_lowercase() == "yes" {
            session.logger.write("  已在 fastbootd。");
            return Ok(());
        }
    }

    session.logger.write("  进入 fastbootd…");
    let _ = session.run_fastboot(&["reboot", "fastboot"], Some("reboot fastboot"), true);
    std::thread::sleep(std::time::Duration::from_secs(2));
    session.wait_for_any_fastboot(90)?;
    session.wait_for_mode(true, 90)?;
    session.logger.write("已成功进入 fastbootd 模式");
    Ok(())
}

pub fn discover_id_candidates(rom_root: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let re_id = regex::Regex::new(r"(?i)\bid\b|mi_ext").unwrap();
    if let Ok(entries) = std::fs::read_dir(rom_root) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                let ext = p
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if (ext == "zip" || ext == "img") && re_id.is_match(name) {
                    candidates.push(p);
                }
            }
        }
    }
    candidates
}

pub fn resolve_id_input(input_path: &Path, work_dir: &Path) -> Result<PathBuf> {
    if !input_path.is_file() {
        bail!("指定的 ID 文件不存在：{:?}", input_path);
    }
    let ext = input_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext == "img" {
        return Ok(input_path.to_path_buf());
    }
    if ext != "zip" {
        bail!("ID 补丁只支持 .zip 或 .img 文件");
    }

    std::fs::create_dir_all(work_dir)?;
    let file = File::open(input_path).with_context(|| format!("无法打开 zip：{:?}", input_path))?;
    let mut archive =
        zip::ZipArchive::new(file).with_context(|| format!("无效 zip 压缩包：{:?}", input_path))?;

    let mut dat_file: Option<String> = None;
    let mut tf_file: Option<String> = None;

    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name.ends_with("mi_ext.new.dat") {
            dat_file = Some(name);
        } else if name.ends_with("mi_ext.transfer.list") {
            tf_file = Some(name);
        }
    }

    let dat_name = dat_file.ok_or_else(|| anyhow!("ID zip 中未找到 mi_ext.new.dat"))?;
    let tf_name = tf_file.ok_or_else(|| anyhow!("ID zip 中未找到 mi_ext.transfer.list"))?;

    let extracted_dat = work_dir.join("id_mi_ext.new.dat");
    let extracted_tf = work_dir.join("id_mi_ext.transfer.list");

    {
        let mut z_dat = archive.by_name(&dat_name)?;
        let mut out = File::create(&extracted_dat)?;
        std::io::copy(&mut z_dat, &mut out)?;
    }
    {
        let mut z_tf = archive.by_name(&tf_name)?;
        let mut out = File::create(&extracted_tf)?;
        std::io::copy(&mut z_tf, &mut out)?;
    }

    let raw_out = work_dir.join("id_mi_ext.raw.img");
    sdat2img(&extracted_tf, &extracted_dat, &raw_out, 4096, |_, _| {})?;

    let _ = std::fs::remove_file(&extracted_dat);
    let _ = std::fs::remove_file(&extracted_tf);

    Ok(raw_out)
}

fn pad_image_file(src: &Path, work_dir: &Path, target: u64) -> Result<PathBuf> {
    let mut data = std::fs::read(src)?;
    if is_android_sparse(src) {
        bail!("sparse 格式无法直接补零");
    }
    if (data.len() as u64) < target {
        data.resize(target as usize, 0);
    }
    std::fs::create_dir_all(work_dir)?;
    let out = work_dir.join("mi_ext-padded.img");
    std::fs::write(&out, &data)?;
    Ok(out)
}

pub fn flash_mi_ext_image(session: &DeviceSession, image: &Path, work_dir: &Path) -> Result<()> {
    if !image.is_file() {
        bail!("ID 镜像不存在：{:?}", image);
    }

    let is_sparse = is_android_sparse(image);
    let raw_size = image.metadata()?.len();
    let flash_path = if !is_sparse && raw_size < MIN_LOGICAL_FLASH_BYTES {
        pad_image_file(image, work_dir, MIN_LOGICAL_FLASH_BYTES)?
    } else {
        image.to_path_buf()
    };

    let size = logical_flash_size(flash_path.metadata()?.len(), None);
    session.logger.write(&format!(
        "  ID 镜像：{:?}  ({} 字节，逻辑分区 {})",
        flash_path,
        flash_path.metadata()?.len(),
        size
    ));

    for part in &["mi_ext_a", "mi_ext_b"] {
        recreate_logical_partition(session, part, size)?;
        let p_str = flash_path.display().to_string();
        let res = session.run_fastboot(
            &["flash", part, &p_str],
            Some(&format!("刷入 {}", part)),
            true,
        )?;
        if res.code == 0 {
            session.logger.write(&format!("  {} 完成", part));
            continue;
        }

        if is_overflow_flash_error(&res.text) && !is_sparse {
            session
                .logger
                .write(&format!("  {} 仍过小，尝试 8MiB 补零…", part));
            let padded = pad_image_file(&flash_path, work_dir, 8 * 1024 * 1024)?;
            let retry_size = logical_flash_size(padded.metadata()?.len(), None);
            recreate_logical_partition(session, part, retry_size)?;
            let p_str2 = padded.display().to_string();
            let res2 = session.run_fastboot(
                &["flash", part, &p_str2],
                Some(&format!("刷入 {}（8MiB padded）", part)),
                true,
            )?;
            if res2.code == 0 {
                session.logger.write(&format!("  {} 完成（padded）", part));
                continue;
            }
        }

        if part.ends_with("_b") {
            session
                .logger
                .write(&format!("  警告：{} 跳过: {}", part, res.text));
            continue;
        }

        bail!("刷入 {} 失败：\n{}", part, res.text);
    }

    let _ = session.run_fastboot(&["set_active", "a"], Some("set_active a"), true);
    Ok(())
}

pub fn format_userdata_metadata(session: &DeviceSession) -> Result<()> {
    session.logger.write("  格式化 Data（fastboot -w）…");
    session.run_fastboot(
        &["-w"],
        Some("fastboot -w 格式化 userdata / metadata"),
        false,
    )?;
    Ok(())
}

pub fn run_id_flash(
    profile: &FlashProfile,
    session: &mut DeviceSession,
    id_file: &Path,
    format_data: bool,
    reboot: bool,
) -> Result<()> {
    let chosen_serial = session.serial.clone();
    session.select_device(chosen_serial.as_deref())?;
    let product = session
        .getvar("product", false)?
        .ok_or_else(|| anyhow!("无法读取机型 product 代号"))?;
    if !profile.codenames.iter().any(|c| c == &product) {
        bail!(
            "机型不匹配：检测到 {}，支持：{}",
            product,
            profile.codenames.join(", ")
        );
    }

    ensure_fastbootd(session)?;
    let resolved = resolve_id_input(id_file, &profile.work_dir)?;
    flash_mi_ext_image(session, &resolved, &profile.work_dir)?;
    session.logger.write("  ID 补丁刷入完成。");

    if format_data {
        format_userdata_metadata(session)?;
    }

    if reboot && !session.dry_run {
        session.run_fastboot(&["reboot"], Some("重启系统"), true)?;
    }

    Ok(())
}
