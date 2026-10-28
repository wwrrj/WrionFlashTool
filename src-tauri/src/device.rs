use crate::logger::Logger;
use anyhow::{anyhow, bail, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessResult {
    pub code: i32,
    pub text: String,
    pub stdout: String,
    pub stderr: String,
}

#[cfg(windows)]
fn normalize_external_path(value: &str) -> String {
    const EXTENDED_UNC_PREFIX: &str = "\\\\?\\UNC\\";
    const EXTENDED_PREFIX: &str = "\\\\?\\";

    if let Some(rest) = value.strip_prefix(EXTENDED_UNC_PREFIX) {
        format!("\\\\{}", rest)
    } else if let Some(rest) = value.strip_prefix(EXTENDED_PREFIX) {
        rest.to_string()
    } else {
        value.to_string()
    }
}

#[cfg(not(windows))]
fn normalize_external_path(value: &str) -> String {
    value.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdbDevice {
    pub serial: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvReport {
    pub serial: String,
    pub product: String,
    pub super_bytes: u64,
    /// 刷机包要求的最小 super 容量（[requirements].minimum_super_bytes）。
    pub super_required: u64,
    pub super_ready: bool,
    pub is_userspace: bool,
    pub anti: Option<String>,
    /// 刷机包的防回滚版本；包内没有 anti 文件时为空。
    pub anti_package: Option<u32>,
    /// 设备防回滚版本不高于刷机包（可以刷入）。
    pub anti_ok: bool,
}

pub fn parse_size(value: &str) -> Result<u64> {
    let trimmed = value.trim();
    if trimmed.to_lowercase().starts_with("0x") {
        let hex_str = &trimmed[2..];
        u64::from_str_radix(hex_str, 16)
            .map_err(|e| anyhow!("无效十六进制大小: {:?}: {}", trimmed, e))
    } else {
        trimmed
            .parse::<u64>()
            .map_err(|e| anyhow!("无效十进制大小: {:?}: {}", trimmed, e))
    }
}

/// 构造不弹出控制台窗口的命令，并按需把命令行写入日志。
fn build_command(file_path: &Path, args: &[&str], logger: Option<&Logger>) -> Command {
    let command_path = PathBuf::from(normalize_external_path(&file_path.to_string_lossy()));
    let normalized_args: Vec<String> = args
        .iter()
        .map(|arg| normalize_external_path(arg))
        .collect();

    if let Some(lg) = logger {
        let cmd_str = format!("{} {}", command_path.display(), normalized_args.join(" "));
        lg.write(&cmd_str);
    }

    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let mut c = Command::new(&command_path);
        c.creation_flags(CREATE_NO_WINDOW);
        c
    };

    #[cfg(not(windows))]
    let mut cmd = Command::new(&command_path);

    cmd.args(&normalized_args);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd
}

pub fn run_process(
    file_path: &Path,
    args: &[&str],
    logger: Option<&Logger>,
) -> Result<ProcessResult> {
    let output = build_command(file_path, args, logger)
        .output()
        .map_err(|e| anyhow!("执行程序失败 {:?}: {}", file_path, e))?;
    Ok(collect_result(
        output.status.code().unwrap_or(-1),
        &output.stdout,
        &output.stderr,
        logger,
    ))
}

/// 退出码：命令超时被结束。
pub const TIMEOUT_CODE: i32 = -2;

/// 与 run_process 相同，但超过 `timeout` 会结束进程并返回 TIMEOUT_CODE
/// （例如手机上的 Root 授权弹窗没人点，`adb shell su` 会一直等下去）。
pub fn run_process_timeout(
    file_path: &Path,
    args: &[&str],
    logger: Option<&Logger>,
    timeout: Duration,
) -> Result<ProcessResult> {
    use std::io::Read;
    use std::sync::mpsc;

    let mut child = build_command(file_path, args, logger)
        .spawn()
        .map_err(|e| anyhow!("执行程序失败 {:?}: {}", file_path, e))?;

    // 输出在后台线程读取，避免管道写满导致子进程卡住
    let read_pipe = |pipe: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    };
    let stdout_rx = read_pipe(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr_rx = read_pipe(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));

    let start = Instant::now();
    let code = loop {
        if let Some(status) = child.try_wait()? {
            break status.code().unwrap_or(-1);
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            break TIMEOUT_CODE;
        }
        thread::sleep(Duration::from_millis(100));
    };

    // 进程已退出；若有后台进程继承了管道，最多再等 2 秒
    let stdout = stdout_rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    let stderr = stderr_rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    let mut result = collect_result(code, &stdout, &stderr, logger);
    if code == TIMEOUT_CODE {
        let note = format!("（{} 秒内没有完成，已结束）", timeout.as_secs());
        if let Some(lg) = logger {
            lg.write(&note);
        }
        result.text = if result.text.is_empty() { note } else { format!("{}\n{}", result.text, note) };
    }
    Ok(result)
}

fn collect_result(code: i32, stdout: &[u8], stderr: &[u8], logger: Option<&Logger>) -> ProcessResult {
    let stdout = String::from_utf8_lossy(stdout).to_string();
    let stderr = String::from_utf8_lossy(stderr).to_string();

    let mut text_parts = Vec::new();
    if !stdout.trim().is_empty() {
        text_parts.push(stdout.trim());
    }
    if !stderr.trim().is_empty() {
        text_parts.push(stderr.trim());
    }
    let text = text_parts.join("\n");

    if let Some(lg) = logger {
        if !text.is_empty() {
            lg.write(&text);
        }
    }

    ProcessResult {
        code,
        text,
        stdout,
        stderr,
    }
}

pub struct DeviceSession {
    pub fastboot: PathBuf,
    pub adb: PathBuf,
    pub logger: Logger,
    pub serial: Option<String>,
    pub dry_run: bool,
}

impl DeviceSession {
    pub fn new(fastboot: PathBuf, adb: PathBuf, logger: Logger, dry_run: bool) -> Self {
        Self {
            fastboot,
            adb,
            logger,
            serial: None,
            dry_run,
        }
    }

    fn fb_args<'a>(&'a self, args: &'a [&'a str]) -> Vec<&'a str> {
        let mut out = Vec::new();
        if let Some(ref s) = self.serial {
            out.push("-s");
            out.push(s.as_str());
        }
        out.extend_from_slice(args);
        out
    }

    fn adb_args<'a>(&'a self, args: &'a [&'a str]) -> Vec<&'a str> {
        let mut out = Vec::new();
        if let Some(ref s) = self.serial {
            out.push("-s");
            out.push(s.as_str());
        }
        out.extend_from_slice(args);
        out
    }

    pub fn run_fastboot(
        &self,
        args: &[&str],
        label: Option<&str>,
        allow_failure: bool,
    ) -> Result<ProcessResult> {
        if self.dry_run && !args.is_empty() && args[0] != "devices" && args[0] != "getvar" {
            let mutating = matches!(
                args[0],
                "-w" | "flash"
                    | "erase"
                    | "reboot"
                    | "wipe-super"
                    | "set_active"
                    | "boot"
                    | "flashing"
                    | "oem"
                    | "delete-logical-partition"
                    | "create-logical-partition"
                    | "resize-logical-partition"
            );
            if mutating {
                let msg = format!("[dry-run] fastboot {}", self.fb_args(args).join(" "));
                self.logger.write(&msg);
                return Ok(ProcessResult {
                    code: 0,
                    text: msg.clone(),
                    stdout: msg,
                    stderr: String::new(),
                });
            }
        }

        if let Some(lbl) = label {
            self.logger.write(&format!("  ├─ {}", lbl));
        }

        let full_args = self.fb_args(args);
        let res = run_process(&self.fastboot, &full_args, Some(&self.logger))?;
        if res.code != 0 && !allow_failure {
            bail!(
                "{} 失败（{}）：fastboot {}\n{}",
                label.unwrap_or("fastboot"),
                res.code,
                args.join(" "),
                res.text
            );
        }
        Ok(res)
    }

    pub fn run_adb(
        &self,
        args: &[&str],
        label: Option<&str>,
        allow_failure: bool,
    ) -> Result<ProcessResult> {
        if let Some(lbl) = label {
            self.logger.write(&format!("  ├─ {}", lbl));
        }
        let full_args = self.adb_args(args);
        let res = run_process(&self.adb, &full_args, Some(&self.logger))?;
        if res.code != 0 && !allow_failure {
            bail!(
                "{} 失败（{}）：adb {}\n{}",
                label.unwrap_or("adb"),
                res.code,
                args.join(" "),
                res.text
            );
        }
        Ok(res)
    }

    pub fn getvar(&self, name: &str, allow_missing: bool) -> Result<Option<String>> {
        let res = self.run_fastboot(&["getvar", name], None, true)?;
        let re = Regex::new(&format!(r"{}:\s*(\S+)", regex::escape(name)))?;
        if let Some(caps) = re.captures(&res.text) {
            Ok(Some(caps[1].to_string()))
        } else {
            if allow_missing {
                Ok(None)
            } else {
                bail!("无法读取 fastboot 变量：{}\n{}", name, res.text);
            }
        }
    }

    pub fn list_fastboot_serials(&self) -> Result<Vec<String>> {
        let res = run_process(&self.fastboot, &["devices"], Some(&self.logger))?;
        let mut devices = Vec::new();
        let re = Regex::new(r"^(\S+)\s+fastboot")?;
        for line in res.text.lines() {
            if let Some(caps) = re.captures(line.trim()) {
                devices.push(caps[1].to_string());
            }
        }
        Ok(devices)
    }

    pub fn list_adb_devices(&self) -> Result<Vec<AdbDevice>> {
        if !self.adb.is_file() {
            return Ok(Vec::new());
        }
        let res = run_process(&self.adb, &["devices"], Some(&self.logger))?;
        let mut devices = Vec::new();
        let re = Regex::new(r"^(\S+)\s+(device|recovery|sideload)$")?;
        for line in res.text.lines() {
            if let Some(caps) = re.captures(line.trim()) {
                devices.push(AdbDevice {
                    serial: caps[1].to_string(),
                    state: caps[2].to_string(),
                });
            }
        }
        Ok(devices)
    }

    pub fn wait_for_any_fastboot(&self, timeout_secs: u64) -> Result<Vec<String>> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(timeout_secs) {
            let devs = self.list_fastboot_serials()?;
            if !devs.is_empty() {
                return Ok(devs);
            }
            thread::sleep(Duration::from_secs(1));
        }
        bail!("等待设备进入 fastboot 超时。");
    }

    pub fn wait_for_fastboot_serial(
        &self,
        chosen_serial: &str,
        timeout_secs: u64,
    ) -> Result<String> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(timeout_secs) {
            let devices = self.list_fastboot_serials()?;
            if devices.iter().any(|serial| serial == chosen_serial) {
                return Ok(chosen_serial.to_string());
            }
            thread::sleep(Duration::from_secs(1));
        }
        bail!("等待设备 {} 进入 fastboot 超时。", chosen_serial);
    }

    /// 刷入结束后重启进系统。`enabled` 为用户的“刷入完成后自动重启”选项。
    /// 返回手机是否已收到重启命令；重启失败不会让刷入失败，但会写入日志。
    pub fn reboot_to_system(&self, enabled: bool) -> Result<bool> {
        if !enabled {
            self.logger.write("已关闭“刷入完成后自动重启”，手机保持在当前模式，请手动重启。");
            return Ok(false);
        }
        if self.dry_run {
            return Ok(false);
        }
        let res = self.run_fastboot(&["reboot"], Some("重启手机进入系统"), true)?;
        if res.code != 0 {
            self.logger.write(&format!("[警告] 重启命令失败（{}），请手动长按电源键重启手机。", res.code));
            return Ok(false);
        }
        Ok(true)
    }

    pub fn wait_for_mode(&self, want_userspace: bool, timeout_secs: u64) -> Result<()> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(timeout_secs) {
            thread::sleep(Duration::from_secs(1));
            if let Ok(res) = self.run_fastboot(&["getvar", "is-userspace"], None, true) {
                if res.code == 0 {
                    let is_us = res.text.to_lowercase().contains("is-userspace: yes");
                    if is_us == want_userspace {
                        return Ok(());
                    }
                }
            }
        }
        if want_userspace {
            bail!("等待 fastbootd 超时。");
        } else {
            bail!("等待 bootloader fastboot 超时。");
        }
    }

    pub fn select_device(&mut self, chosen_serial: Option<&str>) -> Result<String> {
        if !self.fastboot.is_file() {
            bail!("缺少内置 fastboot: {:?}", self.fastboot);
        }

        if let Some(s) = chosen_serial {
            if self.dry_run {
                self.serial = Some(s.to_string());
                return Ok(s.to_string());
            }

            if self
                .list_fastboot_serials()?
                .iter()
                .any(|serial| serial == s)
            {
                self.serial = Some(s.to_string());
                return Ok(s.to_string());
            }

            let adb_device = self
                .list_adb_devices()?
                .into_iter()
                .find(|device| device.serial == s);
            if let Some(device) = adb_device {
                self.logger.write(&format!(
                    "[自动模式] 检测到 ADB/{}，正在切换到 bootloader fastboot...",
                    device.state
                ));
                let result = run_process(
                    &self.adb,
                    &["-s", &device.serial, "reboot", "bootloader"],
                    Some(&self.logger),
                )?;
                if result.code != 0 {
                    bail!("无法让设备 {} 重启到 bootloader：{}", s, result.text);
                }
                let serial = self.wait_for_fastboot_serial(s, 90)?;
                self.serial = Some(serial.clone());
                return Ok(serial);
            }

            bail!(
                "选中的设备 {} 未处于 fastboot、fastbootd 或可用的 ADB/Recovery 模式。",
                s
            );
        }

        self.serial = None;
        let mut devices = self.list_fastboot_serials()?;
        if devices.is_empty() {
            let adb_devs = self.list_adb_devices()?;
            if adb_devs.is_empty() {
                bail!("未发现设备。请连接处于 Android、Recovery、bootloader fastboot 或 fastbootd 的手机。");
            }
            let adb_dev = &adb_devs[0];
            if self.dry_run {
                bail!(
                    "Dry-run 需要设备已处于 fastboot/fastbootd。当前仅发现 ADB/{}（{}），不会自动重启设备。",
                    adb_dev.state, adb_dev.serial
                );
            }
            self.logger.write(&format!(
                "[自动模式] 检测到 ADB/{}，正在切换到 bootloader fastboot...",
                adb_dev.state
            ));
            let _ = run_process(
                &self.adb,
                &["-s", &adb_dev.serial, "reboot", "bootloader"],
                Some(&self.logger),
            );
            devices = self.wait_for_any_fastboot(90)?;
        }

        let serial = devices[0].clone();
        self.serial = Some(serial.clone());
        Ok(serial)
    }
}
