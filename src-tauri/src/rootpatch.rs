use anyhow::{anyhow, bail, Context, Result};
use chrono::Local;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::config::FlashProfile;
use crate::device::{run_process, run_process_in_dir_redacted, DeviceSession};
use crate::expand::ExpandProgress;
use crate::logger::Logger;
use crate::platform_tools;
use crate::rootflash::{prepare_root_flash, verify_boot_image, write_root_image};

#[derive(Debug, Clone, Default, Serialize)]
pub struct RootCandidates {
    pub boot_images: Vec<String>,
    pub init_boot_images: Vec<String>,
    pub kernel_images: Vec<String>,
    pub kpimg_images: Vec<String>,
    pub kernelsu_modules: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RootPatchResult {
    pub output_path: String,
    pub source_path: String,
    pub method: String,
    pub target: String,
    pub log_path: String,
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_lowercase()
}

fn is_ignored(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some(".git" | "target" | "logs" | "work" | "backup")
        )
    })
}

/// 在线刷包根目录中寻找启动镜像和可替换内核。优先级由调用方按目标决定。
pub fn discover(profile: &FlashProfile) -> RootCandidates {
    let mut out = RootCandidates {
        boot_images: Vec::new(),
        init_boot_images: Vec::new(),
        kernel_images: Vec::new(),
        kpimg_images: Vec::new(),
        kernelsu_modules: Vec::new(),
    };
    for entry in WalkDir::new(&profile.rom_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let relative = path.strip_prefix(&profile.rom_root).unwrap_or(path);
        if is_ignored(relative) {
            continue;
        }
        let name = file_name(path);
        let value = path.display().to_string();
        if name == "boot.img" || name == "boot_a.img" || name == "boot_b.img" {
            out.boot_images.push(value);
        } else if name == "init_boot.img" || name == "init_boot_a.img" || name == "init_boot_b.img"
        {
            out.init_boot_images.push(value);
        } else if name == "kpimg-android" || name == "kpimg_android" {
            out.kpimg_images.push(value);
        } else if name == "image"
            || name.starts_with("image.")
            || name == "kernel"
            || name == "kernel-b"
        {
            out.kernel_images.push(value);
        } else if name.ends_with(".ko") && name.contains("kernel") {
            out.kernelsu_modules.push(value);
        }
    }
    for values in [
        &mut out.boot_images,
        &mut out.init_boot_images,
        &mut out.kernel_images,
        &mut out.kpimg_images,
        &mut out.kernelsu_modules,
    ] {
        values.sort_by_key(|p| (p.to_lowercase().contains("images"), p.to_lowercase()));
        values.dedup();
    }
    out
}

fn choose_path(explicit: &str, candidates: &[String], label: &str) -> Result<PathBuf> {
    if !explicit.trim().is_empty() {
        let path = PathBuf::from(explicit.trim());
        if path.is_file() {
            return Ok(path);
        }
        bail!("{}不存在：{}", label, path.display());
    }
    candidates
        .first()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .ok_or_else(|| anyhow!("没有可用的{}，请选择文件", label))
}

/// 没有选择刷机包时使用的工作目录和日志目录：%LOCALAPPDATA%\WrionFlashTool（日志在 logs 子目录）。
pub fn standalone_dirs() -> (PathBuf, PathBuf) {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("WrionFlashTool");
    (base.clone(), base.join("logs"))
}

fn workspace(work_dir: &Path, method: &str) -> Result<PathBuf> {
    let dir = work_dir.join("root-patch").join(format!(
        "{}-{}",
        method,
        Local::now().format("%Y%m%d-%H%M%S")
    ));
    fs::create_dir_all(&dir)
        .with_context(|| format!("无法创建 Root 修补目录：{}", dir.display()))?;
    Ok(dir)
}

/// boot image 头里的 kernel_size（各版本头都在偏移 8）。init_boot 只有 ramdisk，为 0。
fn boot_kernel_size(path: &Path) -> Result<u32> {
    let mut head = [0u8; 12];
    let mut file = fs::File::open(path).with_context(|| format!("无法读取镜像：{}", path.display()))?;
    std::io::Read::read_exact(&mut file, &mut head)
        .with_context(|| format!("镜像过小：{}", path.display()))?;
    if &head[..8] != b"ANDROID!" {
        bail!("不是 Android boot image（缺少 ANDROID! 头）：{}", path.display());
    }
    Ok(u32::from_le_bytes([head[8], head[9], head[10], head[11]]))
}

/// 从 boot.img 里的内核版本字符串识别 KMI，例如 `Linux version 6.1.57-android14-11-…` → `android14-6.1`。
/// 规则与 ksud 的 parse_kmi 一致；内核被压缩时找不到，需要用户手动填写。
fn detect_kmi(boot: &Path) -> Option<String> {
    let data = fs::read(boot).ok()?;
    let re = regex::bytes::Regex::new(r"Linux version (\d+\.\d+)[^\s\x00]*?(android\d+)").ok()?;
    let caps = re.captures(&data)?;
    let version = std::str::from_utf8(&caps[1]).ok()?;
    let android = std::str::from_utf8(&caps[2]).ok()?;
    Some(format!("{}-{}", android, version))
}

fn checked_output(path: &Path) -> Result<PathBuf> {
    verify_boot_image(path)?;
    if fs::metadata(path)?.len() < 4096 {
        bail!("修补后的镜像过小：{}", path.display());
    }
    Ok(path.to_path_buf())
}

/// KernelSU（LKM 模式）只修补 init_boot：LKM 改的是 ramdisk，Android 13 起出厂的机型 ramdisk 在 init_boot。
/// 见 https://kernelsu.org/guide/installation.html ；替换内核的 GKI 模式才会用 boot，这里不支持。
fn patch_kernelsu(work_dir: &Path, source: &Path, kmi: &str, logger: &Logger) -> Result<PathBuf> {
    let tools = platform_tools::root_tools_dir()?;
    let ksud = tools.join("kernelsu").join("ksud.exe");
    let dir = workspace(work_dir, "kernelsu")?;
    let output_name = "wrion-kernelsu.img";
    let mut owned = vec![
        "boot-patch".to_string(),
        "--boot".to_string(),
        source.display().to_string(),
        "--out".to_string(),
        dir.display().to_string(),
        "--out-name".to_string(),
        output_name.to_string(),
        "--arch".to_string(),
        "aarch64".to_string(),
    ];
    // init_boot 里没有内核，ksud 在电脑上无法自行识别 KMI，必须显式传入
    owned.extend(["--kmi".to_string(), kmi.to_string()]);
    let args: Vec<&str> = owned.iter().map(String::as_str).collect();
    logger.log_step(&format!("KernelSU 修补 init_boot（KMI {}）", kmi));
    let result = run_process(&ksud, &args, Some(logger))?;
    if result.code != 0 {
        bail!(
            "KernelSU 修补失败（退出码 {}）\n{}",
            result.code,
            result.text
        );
    }
    let output = dir.join(output_name);
    checked_output(&output)
}

fn patch_apatch(
    work_dir: &Path,
    source: &Path,
    custom_kernel: Option<&Path>,
    super_key: &str,
    kpimg: Option<&Path>,
    logger: &Logger,
) -> Result<PathBuf> {
    if super_key.trim().len() < 8 {
        bail!("APatch SuperKey 至少需要 8 个字符");
    }
    let tools = platform_tools::root_tools_dir()?;
    let kptools = tools.join("apatch").join("kptools.exe");
    let kpimg = kpimg
        .map(PathBuf::from)
        .unwrap_or_else(|| tools.join("apatch").join("kpimg-android"));
    if !kpimg.is_file() {
        bail!("缺少 kpimg-android：{}", kpimg.display());
    }
    let dir = workspace(work_dir, "apatch")?;
    let boot = dir.join("boot.img");
    fs::copy(source, &boot).with_context(|| format!("无法复制 boot 镜像：{}", source.display()))?;

    let unpack_args = ["unpack", "boot.img"];
    let result =
        run_process_in_dir_redacted(&kptools, &unpack_args, &unpack_args, &dir, Some(logger))?;
    if result.code != 0 {
        bail!("APatch 解包失败（退出码 {}）\n{}", result.code, result.text);
    }
    let kernel = dir.join("kernel");
    if !kernel.is_file() {
        bail!("APatch 解包后没有找到 kernel 文件");
    }
    let kernel_b = dir.join("kernel-b");
    if let Some(custom) = custom_kernel {
        fs::copy(custom, &kernel_b)
            .with_context(|| format!("无法复制自定义内核：{}", custom.display()))?;
    } else {
        fs::copy(&kernel, &kernel_b)?;
    }
    let image = kernel_b.display().to_string();
    let kp = kpimg.display().to_string();
    let args = [
        "-p",
        "--image",
        image.as_str(),
        "--skey",
        super_key,
        "--kpimg",
        kp.as_str(),
        "--out",
        "kernel",
    ];
    let log_args = [
        "-p",
        "--image",
        image.as_str(),
        "--skey",
        "<已隐藏>",
        "--kpimg",
        kp.as_str(),
        "--out",
        "kernel",
    ];
    logger.log_step("APatch 修补内核");
    let result = run_process_in_dir_redacted(&kptools, &args, &log_args, &dir, Some(logger))?;
    if result.code != 0 {
        bail!(
            "APatch 内核修补失败（退出码 {}）\n{}",
            result.code,
            result.text
        );
    }
    let repack_args = ["repack", "boot.img"];
    let result =
        run_process_in_dir_redacted(&kptools, &repack_args, &repack_args, &dir, Some(logger))?;
    if result.code != 0 {
        bail!(
            "APatch 重打包失败（退出码 {}）\n{}",
            result.code,
            result.text
        );
    }
    checked_output(&dir.join("new-boot.img"))
}

/// 修补请求。image_path 为空时使用刷机包里的镜像。
pub struct PatchRequest {
    pub method: String,
    pub image_path: String,
    /// 仅 APatch：替换后再修补的自定义内核。
    pub kernel_path: String,
    /// 仅 KernelSU：为空时自动识别。
    pub kmi: String,
    /// 仅 APatch。
    pub super_key: String,
}

/// 修补启动镜像。`profile` 为空表示没有使用刷机包，此时必须指定 image_path。
/// `device_kmi` 从已开机的手机读取 KMI，只在需要时调用。
pub fn patch(
    profile: Option<&FlashProfile>,
    work_dir: &Path,
    req: &PatchRequest,
    device_kmi: &dyn Fn() -> Option<String>,
    logger: &Logger,
) -> Result<RootPatchResult> {
    let candidates = profile.map(discover).unwrap_or_default();
    let method = req.method.trim().to_lowercase();
    // 分区由修补方式决定，不让用户选：KernelSU → init_boot，APatch（修补内核）→ boot
    let (source_candidates, label, expected) = match method.as_str() {
        "kernelsu" => (&candidates.init_boot_images, "init_boot.img", "init_boot"),
        "apatch" => (&candidates.boot_images, "boot.img", "boot"),
        _ => bail!("未知 Root 修补方式：{}", method),
    };
    let external = !req.image_path.trim().is_empty();
    let source = choose_path(&req.image_path, source_candidates, label)?;
    logger.write(&format!("  原始镜像：{}{}", source.display(), if external { "" } else { "（刷机包内）" }));
    let has_kernel = boot_kernel_size(&source)? > 0;
    if method == "kernelsu" && has_kernel {
        bail!(
            "选择的镜像带有内核，是 boot.img 而不是 init_boot.img：{}\nKernelSU 需要修补 init_boot.img。",
            source.display()
        );
    }
    if method == "apatch" && !has_kernel {
        bail!(
            "选择的镜像没有内核，是 init_boot.img 而不是 boot.img：{}\nAPatch 修补的是内核，需要 boot.img。",
            source.display()
        );
    }
    let custom = if req.kernel_path.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(req.kernel_path.trim()))
    };
    if let Some(path) = custom.as_ref() {
        if !path.is_file() {
            bail!("自定义内核不存在：{}", path.display());
        }
    }
    let output = if method == "kernelsu" {
        if custom.is_some() {
            bail!("KernelSU 修补 init_boot 不替换内核，请清空自定义内核");
        }
        let kmi = resolve_kmi(&req.kmi, external, &candidates, device_kmi, logger)?;
        if kmi.starts_with("android12-") {
            bail!("KMI 是 {}：这类内核 KernelSU 修补的是 boot 而不是 init_boot，本工具暂不支持", kmi);
        }
        patch_kernelsu(work_dir, &source, &kmi, logger)?
    } else {
        let kp = candidates.kpimg_images.first().map(PathBuf::from);
        patch_apatch(work_dir, &source, custom.as_deref(), &req.super_key, kp.as_deref(), logger)?
    };
    Ok(RootPatchResult {
        output_path: output.display().to_string(),
        source_path: source.display().to_string(),
        method,
        target: expected.to_string(),
        log_path: logger
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
    })
}

/// KMI 来源：用户填写 → 刷机包 boot.img（使用包内镜像时）→ 已开机手机的 uname。
/// 外部镜像不用刷机包的 boot.img，因为两者不一定是同一个版本。
fn resolve_kmi(
    manual: &str,
    external: bool,
    candidates: &RootCandidates,
    device_kmi: &dyn Fn() -> Option<String>,
    logger: &Logger,
) -> Result<String> {
    if !manual.trim().is_empty() {
        logger.write(&format!("  KMI：{}（手动填写）", manual.trim()));
        return Ok(manual.trim().to_string());
    }
    if !external {
        if let Some(kmi) = candidates.boot_images.first().and_then(|b| detect_kmi(Path::new(b))) {
            logger.write(&format!("  KMI：{}（来自刷机包 boot.img）", kmi));
            return Ok(kmi);
        }
    }
    if let Some(kmi) = device_kmi() {
        logger.write(&format!("  KMI：{}（来自手机当前内核）", kmi));
        return Ok(kmi);
    }
    bail!("无法识别 KMI。请让手机开机并打开 USB 调试后重试，或手动填写 KMI，例如 android14-6.1")
}

/// 已开机手机的 KMI：`uname -r` 形如 `6.1.57-android14-11-g…`。
fn device_kmi(session: &DeviceSession) -> Option<String> {
    let res = session.run_adb(&["shell", "uname", "-r"], None, true).ok()?;
    if res.code != 0 {
        return None;
    }
    parse_uname_kmi(res.stdout.trim())
}

fn parse_uname_kmi(release: &str) -> Option<String> {
    let re = regex::Regex::new(r"^(\d+\.\d+)\S*?(android\d+)").ok()?;
    let caps = re.captures(release)?;
    Some(format!("{}-{}", &caps[2], &caps[1]))
}

#[derive(Debug, Clone, Serialize)]
pub struct RootToolOutcome {
    #[serde(flatten)]
    pub patch: RootPatchResult,
    /// 手机是否已收到重启命令。
    pub rebooted: bool,
}

/// Root 工具的步骤，按修补方式和是否重启组合。
const ROOT_STEPS: [&[&str]; 4] = [
    &["修补 init_boot", "连接手机", "刷入 init_boot"],
    &["修补 init_boot", "连接手机", "刷入 init_boot", "重启手机"],
    &["修补 boot", "连接手机", "刷入 boot"],
    &["修补 boot", "连接手机", "刷入 boot", "重启手机"],
];

/// 修补并刷入：先在电脑上生成修补后的镜像，再写入手机的 init_boot / boot。
pub fn run_root_tool(
    profile: Option<&FlashProfile>,
    session: &mut DeviceSession,
    work_dir: &Path,
    req: &PatchRequest,
    reboot: bool,
) -> Result<RootToolOutcome> {
    let apatch = req.method.trim().eq_ignore_ascii_case("apatch");
    let steps = ROOT_STEPS[(apatch as usize) * 2 + reboot as usize];
    let mark = |session: &DeviceSession, step: &str| {
        let current = steps.iter().position(|s| *s == step).unwrap_or(steps.len());
        session.logger.emit("expand_progress", ExpandProgress { phase: "root", steps, current, detail: None });
    };

    mark(session, steps[0]);
    let patched = {
        let s: &DeviceSession = session;
        patch(profile, work_dir, req, &|| device_kmi(s), &s.logger)?
    };
    session.logger.write(&format!("  修补完成：{}", patched.output_path));

    mark(session, "连接手机");
    prepare_root_flash(profile, session)?;
    mark(session, steps[2]);
    write_root_image(session, Path::new(&patched.output_path), &patched.target)?;
    if reboot {
        mark(session, "重启手机");
    }
    let rebooted = session.reboot_to_system(reboot)?;
    mark(session, "");
    Ok(RootToolOutcome { patch: patched, rebooted })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(dir: &Path, name: &str, kernel_size: u32, body: &[u8]) -> PathBuf {
        let mut data = b"ANDROID!".to_vec();
        data.extend_from_slice(&kernel_size.to_le_bytes());
        data.extend_from_slice(body);
        let path = dir.join(name);
        fs::write(&path, data).unwrap();
        path
    }

    #[test]
    fn tells_boot_from_init_boot() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(boot_kernel_size(&image(dir.path(), "init_boot.img", 0, b"")).unwrap(), 0);
        assert!(boot_kernel_size(&image(dir.path(), "boot.img", 4096, b"")).unwrap() > 0);
        fs::write(dir.path().join("x.img"), b"NOTABOOT0000").unwrap();
        assert!(boot_kernel_size(&dir.path().join("x.img")).is_err());
    }

    #[test]
    fn parses_device_kmi() {
        assert_eq!(parse_uname_kmi("6.1.57-android14-11-g1234abcd").as_deref(), Some("android14-6.1"));
        assert_eq!(parse_uname_kmi("5.10.198-android12-9-00085"), Some("android12-5.10".to_string()));
        assert_eq!(parse_uname_kmi("4.19.157-perf+"), None);
    }

    #[test]
    fn detects_kmi_like_ksud() {
        let dir = tempfile::tempdir().unwrap();
        let boot = image(dir.path(), "boot.img", 4096, b"\0\0Linux version 6.1.57-android14-11-g1234abcd (build@host) #1 SMP\0");
        assert_eq!(detect_kmi(&boot).as_deref(), Some("android14-6.1"));
        let boot = image(dir.path(), "boot2.img", 4096, b"\0Linux version 5.15.123-android13-8-00001-gabc\0");
        assert_eq!(detect_kmi(&boot).as_deref(), Some("android13-5.15"));
        let packed = image(dir.path(), "boot3.img", 4096, b"\x04\x22\x4d\x18 compressed");
        assert_eq!(detect_kmi(&packed), None);
    }
}
