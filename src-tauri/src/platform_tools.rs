//! 内置的完整 Android platform-tools（fastboot / adb 及其依赖）。
//!
//! 所有刷机包（FlashProfile 刷机包和小米官方线刷包）都优先使用这份工具，不再依赖包内自带的版本。
//! `fastboot -w` 等命令需要同目录下的 make_f2fs / mke2fs 生成文件系统镜像，所以必须是完整的一套。
//!
//! 查找顺序：
//! 1. exe 旁边的 tools/platform-tools 或 platform-tools（方便手动替换版本）
//! 2. 编译进 exe 的内置版本，首次使用时释放到本机缓存目录

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

macro_rules! embed {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../resources/platform-tools/", $name)) as &[u8])),*]
    };
}

/// (文件名, 内容)，来自 platform-tools 36.0.1。
const EMBEDDED: &[(&str, &[u8])] = embed![
    "fastboot.exe",
    "adb.exe",
    "AdbWinApi.dll",
    "AdbWinUsbApi.dll",
    "libwinpthread-1.dll",
    "make_f2fs.exe",
    "make_f2fs_casefold.exe",
    "mke2fs.exe",
    "mke2fs.conf",
    "NOTICE.txt",
    "source.properties",
];

/// 外部目录必须包含这些文件才会被采用，否则仍使用内置的完整版本。
const REQUIRED: &[&str] = &["fastboot.exe", "adb.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll", "make_f2fs.exe", "mke2fs.exe"];

fn complete(dir: &Path) -> bool {
    REQUIRED.iter().all(|name| dir.join(name).is_file())
}

/// 内置工具的释放目录：%LOCALAPPDATA%\WrionFlashTool\platform-tools-<标识>。
fn cache_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    // 用全部文件的总大小区分版本：内置工具变化后释放到新目录，不会和旧文件混用
    let key: usize = EMBEDDED.iter().map(|(_, data)| data.len()).sum();
    base.join("WrionFlashTool").join(format!("platform-tools-{:x}", key))
}

/// 释放内置工具；已释放且大小一致的文件不会重写（adb 可能正在运行并占用它）。
fn extract_embedded() -> Result<PathBuf> {
    let dir = cache_dir();
    fs::create_dir_all(&dir).with_context(|| format!("无法创建工具目录：{:?}", dir))?;
    for (name, data) in EMBEDDED {
        let path = dir.join(name);
        if fs::metadata(&path).map_or(false, |m| m.len() == data.len() as u64) {
            continue;
        }
        // 临时文件按进程和线程区分：多个调用同时释放时互不覆盖
        let tmp = dir.join(format!("{}.{}-{:?}.part", name, std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
        fs::write(&tmp, data).with_context(|| format!("无法释放 {}", name))?;
        if let Err(e) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            // 另一个调用已经写好了同样的文件（或 adb 正在占用它）时不算失败
            if !fs::metadata(&path).map_or(false, |m| m.len() == data.len() as u64) {
                return Err(e).with_context(|| format!("无法释放 {}", name));
            }
        }
    }
    remove_stale_copies(&dir);
    Ok(dir)
}

/// 删除旧版本释放的工具目录；正在被占用（如 adb 仍在运行）的会删不掉，留到下次。
fn remove_stale_copies(current: &Path) {
    let Some(parent) = current.parent() else { return };
    let Ok(entries) = fs::read_dir(parent) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_copy = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("platform-tools-"));
        if is_copy && path != current && path.is_dir() {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

/// 返回可用的完整 platform-tools 目录。
pub fn tools_dir() -> Result<PathBuf> {
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        for candidate in [exe_dir.join("tools").join("platform-tools"), exe_dir.join("platform-tools")] {
            if complete(&candidate) {
                return Ok(candidate);
            }
        }
    }
    extract_embedded()
}
