//! Super 扩容前的 GPT 备份：打包成 zip、校验结构、读取信息并取出 GPT 镜像。
//!
//! 备份包结构（新格式为 zip，旧格式为同名目录，内容相同）：
//! ```text
//! <device>-super-expand-<stamp>.zip
//! ├─ gpt-sda-before.bin   sgdisk --backup 导出的 GPT
//! └─ plan.txt             key=value，记录磁盘、机型和扩容前后的分区范围
//! ```

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Local};
use serde::Serialize;
use zip::write::SimpleFileOptions;
use zip::ZipArchive;

use crate::config::ExpansionConfig;

pub const GPT_FILE: &str = "gpt-sda-before.bin";
pub const PLAN_FILE: &str = "plan.txt";
const MAX_MEMBER_BYTES: u64 = 16 << 20;
const GPT_SIGNATURE: &[u8] = b"EFI PART";

#[derive(Debug, Clone, Serialize)]
pub struct BackupInfo {
    pub path: String,
    pub file_name: String,
    /// "zip" 或 "dir"（旧版目录备份）
    pub kind: String,
    pub device: Option<String>,
    pub disk: Option<String>,
    pub created: Option<String>,
    pub super_old_bytes: Option<u64>,
    pub super_new_bytes: Option<u64>,
    /// 结构完整且与当前刷机包的扩容配置一致，可以用来恢复。
    pub supported: bool,
    pub problems: Vec<String>,
    pub warnings: Vec<String>,
}

/// 备份包里读出的原始内容。
pub struct BackupContents {
    pub gpt: Vec<u8>,
    pub plan: HashMap<String, String>,
}

pub fn parse_plan(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

/// 解析 plan.txt 中形如 `123-456` 的扇区范围。
pub fn parse_range(value: &str) -> Option<(u64, u64)> {
    let (a, b) = value.split_once('-')?;
    let (a, b) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    (b >= a).then_some((a, b))
}

pub fn is_gpt_backup(data: &[u8]) -> bool {
    !data.is_empty() && data.windows(GPT_SIGNATURE.len()).any(|w| w == GPT_SIGNATURE)
}

fn read_member<R: Read + std::io::Seek>(archive: &mut ZipArchive<R>, name: &str) -> Result<Option<Vec<u8>>> {
    // 允许备份被多包一层目录
    let found = archive
        .file_names()
        .map(|n| n.replace('\\', "/"))
        .filter(|n| n == name || (n.ends_with(&format!("/{}", name)) && n.matches('/').count() == 1))
        .min_by_key(|n| n.len());
    let Some(found) = found else { return Ok(None) };
    let mut entry = archive.by_name(&found)?;
    if entry.size() > MAX_MEMBER_BYTES {
        bail!("{} 过大（{} 字节），不是有效的 GPT 备份", name, entry.size());
    }
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut buf)?;
    Ok(Some(buf))
}

/// 读取 zip 或旧版目录备份。缺少文件时返回对应的问题描述。
pub fn read_backup(path: &Path) -> Result<BackupContents> {
    let (gpt, plan) = if path.is_dir() {
        let read_opt = |name: &str| -> Result<Option<Vec<u8>>> {
            let p = path.join(name);
            if !p.is_file() {
                return Ok(None);
            }
            if fs::metadata(&p)?.len() > MAX_MEMBER_BYTES {
                bail!("{} 过大，不是有效的 GPT 备份", name);
            }
            Ok(Some(fs::read(p)?))
        };
        (read_opt(GPT_FILE)?, read_opt(PLAN_FILE)?)
    } else {
        let file = File::open(path).with_context(|| format!("无法打开备份文件：{:?}", path))?;
        let mut archive = ZipArchive::new(file).map_err(|_| anyhow!("这不是有效的 zip 压缩包"))?;
        (read_member(&mut archive, GPT_FILE)?, read_member(&mut archive, PLAN_FILE)?)
    };
    let gpt = gpt.ok_or_else(|| anyhow!("备份中缺少 {}", GPT_FILE))?;
    let plan = plan.ok_or_else(|| anyhow!("备份中缺少 {}", PLAN_FILE))?;
    let plan = parse_plan(&String::from_utf8_lossy(&plan));
    Ok(BackupContents { gpt, plan })
}

fn range_bytes(plan: &HashMap<String, String>, key: &str, sector: Option<u64>) -> Option<u64> {
    let (a, b) = parse_range(plan.get(key)?)?;
    Some((b - a + 1) * sector?)
}

fn created_of(path: &Path, plan: &HashMap<String, String>) -> Option<String> {
    if let Some(c) = plan.get("created") {
        return Some(c.clone());
    }
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(DateTime::<Local>::from(modified).format("%Y-%m-%d %H:%M").to_string())
}

/// 检查备份结构，并与当前刷机包的扩容配置（磁盘、机型）比对。
pub fn inspect_backup(path: &Path, cfg: &ExpansionConfig) -> BackupInfo {
    let file_name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let mut info = BackupInfo {
        path: path.display().to_string(),
        file_name,
        kind: if path.is_dir() { "dir" } else { "zip" }.to_string(),
        device: None,
        disk: None,
        created: None,
        super_old_bytes: None,
        super_new_bytes: None,
        supported: false,
        problems: Vec::new(),
        warnings: Vec::new(),
    };

    let contents = match read_backup(path) {
        Ok(c) => c,
        Err(e) => {
            info.problems.push(e.to_string());
            return info;
        }
    };
    let plan = &contents.plan;

    if !is_gpt_backup(&contents.gpt) {
        info.problems.push(format!("{} 不是有效的 GPT 备份（缺少 GPT 头）", GPT_FILE));
    }

    info.disk = plan.get("disk").cloned();
    match info.disk.as_deref() {
        None => info.problems.push("plan.txt 缺少 disk，无法确认备份对应的磁盘".to_string()),
        Some(d) if d != cfg.disk => info.problems.push(format!("备份的磁盘是 {}，当前刷机包配置的是 {}", d, cfg.disk)),
        _ => {}
    }

    info.device = plan.get("device").cloned().filter(|d| !d.is_empty());
    match info.device.as_deref() {
        None | Some("unknown-device") => info.warnings.push("备份未记录机型，恢复时无法核对是否为同一台手机".to_string()),
        Some(d) if !cfg.codenames.iter().any(|c| c == d) => {
            info.problems.push(format!("备份来自 {}，当前刷机包支持 {}", d, cfg.codenames.join(" / ")))
        }
        _ => {}
    }

    let sector = plan.get("sector_bytes").and_then(|v| v.parse::<u64>().ok()).filter(|v| *v > 0);
    info.super_old_bytes = range_bytes(plan, "super_old", sector);
    info.super_new_bytes = range_bytes(plan, "super_new", sector);
    if parse_range(plan.get("super_old").map_or("", String::as_str)).is_none()
        || parse_range(plan.get("userdata_old").map_or("", String::as_str)).is_none()
    {
        info.warnings.push("plan.txt 缺少扩容前的分区范围，恢复后无法自动核对结果".to_string());
    }

    info.created = created_of(path, plan);
    info.supported = info.problems.is_empty();
    info
}

/// 列出备份目录中的 zip 备份和旧版目录备份，按时间倒序。
pub fn list_backups(cfg: &ExpansionConfig) -> Vec<BackupInfo> {
    let mut paths: Vec<PathBuf> = fs::read_dir(&cfg.backup_dir)
        .map(|it| {
            it.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    (p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")))
                        || (p.is_dir() && p.join(GPT_FILE).is_file())
                })
                .collect()
        })
        .unwrap_or_default();
    paths.sort_by_key(|p| std::cmp::Reverse(fs::metadata(p).and_then(|m| m.modified()).ok()));
    paths.iter().map(|p| inspect_backup(p, cfg)).collect()
}

/// 把 GPT 和 plan.txt 写成一个 zip 备份。先写临时文件，完成后再改名，避免留下半个备份。
pub fn write_backup_zip(dest: &Path, gpt: &[u8], plan_text: &str) -> Result<()> {
    if !is_gpt_backup(gpt) {
        bail!("导出的 GPT 无效（{} 字节，缺少 GPT 头），已停止扩容", gpt.len());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("zip.part");
    {
        let mut zip = zip::ZipWriter::new(File::create(&tmp)?);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(GPT_FILE, opts)?;
        zip.write_all(gpt)?;
        zip.start_file(PLAN_FILE, opts)?;
        zip.write_all(plan_text.as_bytes())?;
        zip.finish()?.sync_all()?;
    }
    fs::rename(&tmp, dest)?;
    Ok(())
}
