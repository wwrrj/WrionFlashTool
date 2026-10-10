//! 刷机包 zip 的探测与解压。
//!
//! 选择 zip 后先读取包内 `META-INF/com/android/flash_profile.toml` 做预览，
//! 再整体解压到目标目录，之后所有命令都以该目录作为 ROM 根目录。

use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::config::DEFAULT_PROFILE_REL;

const MARKER_FILE: &str = ".harusora_extract.json";
const COPY_BUF: usize = 1 << 20;

static ACTIVE_ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);

/// 当前通过“选择刷机包”激活的 ROM 根目录。
pub fn active_root() -> Option<PathBuf> {
    ACTIVE_ROOT.lock().ok().and_then(|g| g.clone())
}

pub fn set_active_root(root: Option<PathBuf>) {
    if let Ok(mut g) = ACTIVE_ROOT.lock() {
        *g = root;
    }
}

pub fn request_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageInfo {
    pub zip_path: String,
    pub package_name: String,
    pub display_name: String,
    pub codenames: Vec<String>,
    pub total_bytes: u64,
    pub file_count: usize,
    pub default_dest: String,
    /// 默认目录里已有同一个 zip 的完整解压结果，可直接使用。
    pub already_extracted: bool,
    /// 默认解压目录所在磁盘的可用空间（无法获取时为空）。
    pub free_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtractProgress {
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub file_index: usize,
    pub file_count: usize,
    pub current: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Marker {
    zip_name: String,
    zip_size: u64,
    zip_mtime: u64,
    complete: bool,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
}

/// 目标路径所在磁盘的可用字节数；路径尚不存在时取最近的已存在上级目录。
#[cfg(windows)]
pub fn free_space(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let mut p = path;
    while !p.exists() {
        p = p.parent()?;
    }
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut avail = 0u64;
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(avail)
}

#[cfg(not(windows))]
pub fn free_space(_path: &Path) -> Option<u64> {
    None
}

fn normalize(name: &str) -> String {
    name.replace('\\', "/")
}

/// 返回 profile 在 zip 内的前缀（空串或形如 `Rom/`），用于支持 ROM 被多包一层目录的情况。
fn find_prefix<R: Read + std::io::Seek>(archive: &ZipArchive<R>) -> Result<String> {
    let mut best: Option<String> = None;
    for name in archive.file_names() {
        let n = normalize(name);
        if let Some(prefix) = n.strip_suffix(DEFAULT_PROFILE_REL) {
            if prefix.is_empty() || (prefix.ends_with('/') && prefix.matches('/').count() == 1) {
                if best.as_ref().map_or(true, |b| prefix.len() < b.len()) {
                    best = Some(prefix.to_string());
                }
            }
        }
    }
    best.ok_or_else(|| {
        anyhow!("这个 zip 不是有效的刷机包：缺少 {}", DEFAULT_PROFILE_REL)
    })
}

fn zip_identity(zip_path: &Path) -> Result<Marker> {
    let meta = fs::metadata(zip_path).with_context(|| format!("无法读取刷机包：{:?}", zip_path))?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    Ok(Marker {
        zip_name: zip_path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        zip_size: meta.len(),
        zip_mtime: mtime,
        complete: false,
    })
}

fn read_marker(dest: &Path) -> Option<Marker> {
    let text = fs::read_to_string(dest.join(MARKER_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_marker(dest: &Path, marker: &Marker) -> Result<()> {
    fs::write(dest.join(MARKER_FILE), serde_json::to_vec_pretty(marker)?)?;
    Ok(())
}

fn same_zip(a: &Marker, b: &Marker) -> bool {
    a.zip_name == b.zip_name && a.zip_size == b.zip_size && a.zip_mtime == b.zip_mtime
}

pub fn default_dest(zip_path: &Path) -> PathBuf {
    let stem = zip_path.file_stem().map_or_else(|| "rom".to_string(), |s| s.to_string_lossy().into_owned());
    zip_path.parent().unwrap_or_else(|| Path::new(".")).join(stem)
}

fn open_archive(zip_path: &Path) -> Result<ZipArchive<BufReader<File>>> {
    let file = File::open(zip_path).with_context(|| format!("无法打开刷机包：{:?}", zip_path))?;
    ZipArchive::new(BufReader::with_capacity(COPY_BUF, file))
        .map_err(|e| anyhow!("刷机包不是有效的 zip 文件或已损坏：{}", e))
}

pub fn inspect_package(zip_path: &Path) -> Result<PackageInfo> {
    let mut archive = open_archive(zip_path)?;
    let prefix = find_prefix(&archive)?;

    let mut profile_text = String::new();
    archive
        .by_name(&format!("{}{}", prefix, DEFAULT_PROFILE_REL))
        .map_err(|e| anyhow!("无法读取包内刷机配置：{}", e))?
        .read_to_string(&mut profile_text)
        .context("包内刷机配置不是有效的 UTF-8 文本")?;
    let value: toml::Value = toml::from_str(&profile_text).context("包内刷机配置解析失败")?;

    let str_at = |table: &str, key: &str| {
        value.get(table).and_then(|t| t.get(key)).and_then(|v| v.as_str()).unwrap_or("").to_string()
    };
    let codenames = value
        .get("device")
        .and_then(|d| d.get("codenames"))
        .and_then(|c| c.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let mut total_bytes = 0u64;
    let mut file_count = 0usize;
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i)?;
        if !entry.is_dir() && normalize(entry.name()).starts_with(&prefix) {
            total_bytes += entry.size();
            file_count += 1;
        }
    }

    let dest = default_dest(zip_path);
    let identity = zip_identity(zip_path)?;
    let already_extracted = read_marker(&dest).map_or(false, |m| m.complete && same_zip(&m, &identity));

    Ok(PackageInfo {
        zip_path: zip_path.display().to_string(),
        package_name: str_at("package", "name"),
        display_name: str_at("device", "display_name"),
        codenames,
        total_bytes,
        file_count,
        default_dest: dest.display().to_string(),
        already_extracted,
        free_bytes: free_space(&dest),
    })
}

/// 解压刷机包并返回 ROM 根目录。已完整解压过同一个 zip 时直接复用；
/// 中途中断的解压会按文件大小跳过已完成的文件继续。
pub fn extract_package(
    zip_path: &Path,
    dest: &Path,
    on_progress: &mut dyn FnMut(&ExtractProgress),
) -> Result<PathBuf> {
    CANCEL.store(false, Ordering::SeqCst);
    let mut archive = open_archive(zip_path)?;
    let prefix = find_prefix(&archive)?;
    let identity = zip_identity(zip_path)?;

    let existing = read_marker(dest);
    let dest_nonempty = dest.is_dir() && fs::read_dir(dest)?.next().is_some();
    if dest_nonempty && existing.is_none() {
        bail!("目标目录不是空目录，也不是本工具创建的解压目录：{:?}\n请选择一个空目录或不存在的目录。", dest);
    }
    let resumable = existing.as_ref().map_or(false, |m| same_zip(m, &identity));
    if resumable && existing.as_ref().map_or(false, |m| m.complete) {
        return finish(dest);
    }

    let mut total_bytes = 0u64;
    let mut file_count = 0usize;
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i)?;
        if !entry.is_dir() && normalize(entry.name()).starts_with(&prefix) {
            total_bytes += entry.size();
            file_count += 1;
        }
    }

    let needed = total_bytes + (512 << 20);
    if !resumable {
        if let Some(free) = free_space(dest) {
            if free < needed {
                bail!(
                    "磁盘空间不足：解压需要约 {:.1} GiB，目标磁盘只剩 {:.1} GiB。\n请清理空间，或把解压目录换到其他磁盘。",
                    needed as f64 / 1073741824.0,
                    free as f64 / 1073741824.0
                );
            }
        }
    }

    fs::create_dir_all(dest).with_context(|| format!("无法创建目标目录：{:?}", dest))?;
    write_marker(dest, &identity)?;

    let mut done_bytes = 0u64;
    let mut file_index = 0usize;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut buf = vec![0u8; COPY_BUF];

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(enclosed) = entry.enclosed_name() else {
            bail!("刷机包包含不安全的路径：{}", entry.name());
        };
        let rel_str = normalize(&enclosed.to_string_lossy());
        let Some(rel) = rel_str.strip_prefix(&prefix) else { continue };
        if rel.is_empty() {
            continue;
        }
        let target = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        file_index += 1;
        let size = entry.size();
        let up_to_date = resumable && fs::metadata(&target).map_or(false, |m| m.is_file() && m.len() == size);
        if up_to_date {
            done_bytes += size;
            continue;
        }

        let part = target.with_extension(format!(
            "{}.part",
            target.extension().map_or_else(String::new, |e| e.to_string_lossy().into_owned())
        ));
        let mut out = File::create(&part).with_context(|| format!("无法写入文件：{:?}", part))?;
        loop {
            if CANCEL.load(Ordering::SeqCst) {
                drop(out);
                let _ = fs::remove_file(&part);
                bail!("已取消解压");
            }
            let n = entry.read(&mut buf).with_context(|| format!("读取 {} 失败，刷机包可能已损坏", rel))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).with_context(|| format!("写入 {} 失败，请检查磁盘剩余空间", rel))?;
            done_bytes += n as u64;
            if last_emit.elapsed() >= Duration::from_millis(120) {
                last_emit = Instant::now();
                on_progress(&ExtractProgress {
                    done_bytes,
                    total_bytes,
                    file_index,
                    file_count,
                    current: rel.to_string(),
                });
            }
        }
        out.flush()?;
        drop(out);
        if target.exists() {
            fs::remove_file(&target)?;
        }
        fs::rename(&part, &target)?;
    }

    on_progress(&ExtractProgress {
        done_bytes: total_bytes,
        total_bytes,
        file_index: file_count,
        file_count,
        current: String::new(),
    });
    write_marker(dest, &Marker { complete: true, ..identity })?;
    finish(dest)
}

fn finish(dest: &Path) -> Result<PathBuf> {
    let root = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());
    if !root.join(DEFAULT_PROFILE_REL).is_file() {
        bail!("解压完成但缺少刷机配置：{:?}", root.join(DEFAULT_PROFILE_REL));
    }
    Ok(root)
}
