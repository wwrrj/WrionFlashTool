//! 内置的 Windows Android USB 驱动（WinUSB，按接口类型匹配，支持各品牌手机的 ADB 与 Fastboot）。
//!
//! 驱动文件释放到本机缓存目录后，用系统自带的 pnputil 安装；安装需要管理员权限，
//! 会弹出一次 UAC 授权。
//!
//! 检测“手机插上了但没有驱动”：Windows 会把这样的接口列为有错误码的设备，
//! 其兼容 ID 为 USB\Class_FF&SubClass_42（Android ADB / Fastboot 接口）。

use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::Serialize;

macro_rules! embed {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../resources/usb-driver/", $name)) as &[u8])),*]
    };
}

pub const INF_NAME: &str = "android_winusb.inf";

/// (相对路径, 内容)。.inf 必须保持原始字节，否则与 .cat 签名不符。
const FILES: &[(&str, &[u8])] = embed![
    "android_winusb.inf",
    "android_winusb.cat",
    "source.properties",
    "amd64/WdfCoInstaller01009.dll",
    "amd64/winusbcoinstaller2.dll",
    "amd64/WUDFUpdate_01009.dll",
    "amd64/NOTICE.txt",
    "i386/WdfCoInstaller01009.dll",
    "i386/winusbcoinstaller2.dll",
    "i386/WUDFUpdate_01009.dll",
    "i386/NOTICE.txt",
];

#[derive(Debug, Clone, Serialize)]
pub struct DriverStatus {
    /// 系统驱动库里已有这份驱动。
    pub installed: bool,
    /// 已连接、但缺少驱动的 Android 接口名称。
    pub missing_devices: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallResult {
    /// 需要重启电脑才能生效。
    pub reboot_required: bool,
    pub message: String,
}

fn catalog() -> &'static [u8] {
    FILES.iter().find(|(n, _)| *n == "android_winusb.cat").map(|(_, d)| *d).unwrap_or_default()
}

/// 系统驱动库（DriverStore）里是否已有这份驱动：按 .cat 内容比对，不依赖系统语言。
fn installed_in_driver_store() -> bool {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let store = root.join("System32").join("DriverStore").join("FileRepository");
    let Ok(entries) = fs::read_dir(store) else { return false };
    entries.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().to_lowercase();
        name.starts_with("android_winusb.inf_")
            && fs::read(e.path().join("android_winusb.cat")).is_ok_and(|c| c == catalog())
    })
}

#[cfg(windows)]
fn powershell(script: &str) -> Result<std::process::Output> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let full = format!("[Console]::OutputEncoding = [Text.Encoding]::UTF8; {}", script);
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &full])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .context("无法启动 PowerShell")
}

/// 缺少驱动的 Android 接口（ADB 或 Fastboot）。
#[cfg(windows)]
fn missing_devices() -> Vec<String> {
    let script = "Get-CimInstance Win32_PnPEntity -Filter 'ConfigManagerErrorCode<>0' | \
        Where-Object { (($_.CompatibleID + $_.HardwareID) -join ' ') -match 'Class_FF&SubClass_42' } | \
        ForEach-Object { $_.Name }";
    powershell(script)
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn missing_devices() -> Vec<String> {
    Vec::new()
}

pub fn status() -> DriverStatus {
    DriverStatus { installed: installed_in_driver_store(), missing_devices: missing_devices() }
}

/// 释放驱动文件，返回 .inf 路径。
pub fn extract() -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let key: usize = FILES.iter().map(|(_, d)| d.len()).sum();
    let dir = base.join("HarusoraFlashTool").join(format!("usb-driver-{:x}", key));
    for (name, data) in FILES {
        let path = dir.join(name);
        if fs::metadata(&path).is_ok_and(|m| m.len() == data.len() as u64) {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("无法创建驱动目录：{:?}", parent))?;
        }
        fs::write(&path, data).with_context(|| format!("无法释放驱动文件 {}", name))?;
    }
    Ok(dir.join(INF_NAME))
}

/// 以管理员身份运行 pnputil 安装驱动（会弹出 UAC 授权）。
#[cfg(windows)]
pub fn install() -> Result<InstallResult> {
    let inf = extract()?;
    let inf_ps = inf.display().to_string().replace('\'', "''");
    // 用户在 UAC 里点“否”时 Start-Process 会抛异常，转成 ERROR_CANCELLED(1223)
    let script = format!(
        "try {{ $p = Start-Process -FilePath pnputil.exe -ArgumentList '/add-driver', ('\"' + '{inf}' + '\"'), '/install' \
         -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode }} catch {{ exit 1223 }}",
        inf = inf_ps
    );
    let output = powershell(&script)?;
    match output.status.code() {
        Some(0) | Some(259) => Ok(InstallResult { reboot_required: false, message: "USB 驱动已安装".to_string() }),
        Some(3010) => Ok(InstallResult { reboot_required: true, message: "USB 驱动已安装，需要重启电脑后生效".to_string() }),
        Some(1223) => bail!("已取消安装：需要在弹出的“用户账户控制”窗口中点击“是”"),
        code => bail!(
            "USB 驱动安装失败（pnputil 退出码 {:?}）。\n{}",
            code,
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    }
}

#[cfg(not(windows))]
pub fn install() -> Result<InstallResult> {
    bail!("只能在 Windows 上安装 USB 驱动")
}
