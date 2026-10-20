use anyhow::{anyhow, bail, Result};
use chrono::Local;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::backup::{self, BackupInfo};
use crate::config::{ExpansionConfig, FlashProfile};
use crate::device::run_process;
use crate::logger::Logger;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartInfo {
    pub number: u32,
    pub code: String,
    pub guid: String,
    pub first: u64,
    pub last: u64,
    pub name: String,
    pub lines: Vec<String>,
}

/// 读取扩容方案的步骤。
pub const PLAN_STEPS: &[&str] = &["连接手机", "刷入包内 Recovery", "等待进入 Recovery", "准备分区工具", "读取分区表"];
/// 执行扩容的步骤（会重新进入 Recovery 并再次读取分区表）。
pub const APPLY_STEPS: &[&str] = &[
    "连接手机", "刷入包内 Recovery", "等待进入 Recovery", "准备分区工具", "读取分区表",
    "备份分区表", "清除数据", "重写分区表", "格式化 userdata",
];
/// 恢复官方分区表的步骤。
pub const RESTORE_STEPS: &[&str] = &[
    "检查备份", "连接手机", "刷入包内 Recovery", "等待进入 Recovery", "准备分区工具",
    "写入官方分区表", "核对分区范围", "清除数据",
];

/// 推送给前端的 `expand_progress` 事件。
#[derive(Debug, Clone, Serialize)]
pub struct ExpandProgress {
    /// "plan" | "apply" | "restore"
    pub phase: &'static str,
    pub steps: &'static [&'static str],
    /// 当前步骤序号（从 0 开始）；等于 steps.len() 表示全部完成。
    pub current: usize,
    /// 当前步骤的补充说明，例如等待时长。
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpandPlan {
    pub disk: String,
    pub sector_bytes: u64,
    pub disk_sectors: u64,
    pub last_usable_sector: u64,
    pub target_bytes: u64,
    pub target_sectors: u64,
    pub super_info: PartInfo,
    pub userdata_info: PartInfo,
    pub current_super_bytes: u64,
    pub new_super_last: u64,
    pub new_userdata_first: u64,
    pub new_userdata_bytes: u64,
    pub super_part: u32,
    pub userdata_part: u32,
}

pub struct ExpandSession {
    pub profile: FlashProfile,
    pub cfg: ExpansionConfig,
    pub logger: Logger,
    pub device: String,
    pub sgdisk_cmd: String,
    pub adb_serial: Option<String>,
    pub fastboot_serial: Option<String>,
    phase: Option<(&'static str, &'static [&'static str])>,
    current: std::cell::Cell<usize>,
}

impl ExpandSession {
    pub fn new(profile: FlashProfile, logger: Logger) -> Result<Self> {
        let cfg = profile
            .expansion
            .clone()
            .ok_or_else(|| anyhow!("当前配置未启用物理扩容（[expansion].enabled）"))?;
        if !profile.adb().is_file() {
            bail!("缺少 adb：{:?}", profile.adb());
        }
        if !profile.fastboot().is_file() {
            bail!("缺少 fastboot：{:?}", profile.fastboot());
        }
        Ok(Self {
            profile,
            cfg,
            logger,
            device: String::new(),
            sgdisk_cmd: "sgdisk".to_string(),
            adb_serial: None,
            fastboot_serial: None,
            phase: None,
            current: std::cell::Cell::new(0),
        })
    }

    /// 开始一个带进度的阶段，之后用 [`mark`](Self::mark) 推进。
    pub fn begin(&mut self, phase: &'static str, steps: &'static [&'static str]) {
        self.phase = Some((phase, steps));
        self.current.set(0);
        self.emit_progress(None);
    }

    /// 进入名为 `step` 的步骤；不属于当前阶段的步骤会被忽略。
    fn mark(&self, step: &str) {
        let Some((_, steps)) = self.phase else { return };
        if let Some(index) = steps.iter().position(|s| *s == step) {
            self.current.set(index);
            self.emit_progress(None);
        }
    }

    /// 更新当前步骤的补充说明（不改变步骤）。
    fn detail(&self, text: String) {
        self.emit_progress(Some(text));
    }

    /// 标记当前阶段全部完成。
    pub fn finish(&self) {
        if let Some((_, steps)) = self.phase {
            self.current.set(steps.len());
            self.emit_progress(None);
        }
    }

    fn emit_progress(&self, detail: Option<String>) {
        if let Some((phase, steps)) = self.phase {
            self.logger.emit("expand_progress", ExpandProgress { phase, steps, current: self.current.get(), detail });
        }
    }

    pub fn adb_run(&self, args: &[&str], allow_fail: bool) -> Result<String> {
        let mut full_args = Vec::with_capacity(args.len() + 2);
        if args.first().copied() != Some("devices") {
            if let Some(serial) = self.adb_serial.as_deref() {
                full_args.extend(["-s", serial]);
            }
        }
        full_args.extend_from_slice(args);
        let res = run_process(&self.profile.adb(), &full_args, Some(&self.logger))?;
        if res.code != 0 && !allow_fail {
            bail!(
                "ADB 失败（{}）：adb {}\n{}",
                res.code,
                args.join(" "),
                res.text
            );
        }
        Ok(res.text)
    }

    pub fn adb_shell(&self, cmd: &str, allow_fail: bool) -> Result<String> {
        self.adb_run(&["shell", cmd], allow_fail)
    }

    pub fn fb_run(&self, args: &[&str], allow_fail: bool) -> Result<String> {
        let mut full_args = Vec::with_capacity(args.len() + 2);
        if args.first().copied() != Some("devices") {
            if let Some(serial) = self.fastboot_serial.as_deref() {
                full_args.extend(["-s", serial]);
            }
        }
        full_args.extend_from_slice(args);
        let res = run_process(&self.profile.fastboot(), &full_args, Some(&self.logger))?;
        if res.code != 0 && !allow_fail {
            bail!(
                "fastboot 失败（{}）：fastboot {}\n{}",
                res.code,
                args.join(" "),
                res.text
            );
        }
        Ok(res.text)
    }

    pub fn adb_transport_state(&mut self) -> String {
        if let Ok(text) = self.adb_run(&["devices"], true) {
            let re = Regex::new(r"(?m)^(\S+)\s+(recovery|device|sideload)\s*$").unwrap();
            let matches: Vec<_> = re.captures_iter(&text).collect();
            if matches.len() == 1 {
                self.adb_serial = Some(matches[0][1].to_string());
                return matches[0][2].to_string();
            }
        }
        String::new()
    }

    fn fastboot_devices(&mut self) -> Result<Vec<String>> {
        let res = run_process(&self.profile.fastboot(), &["devices"], Some(&self.logger))?;
        let re = Regex::new(r"^(\S+)\s+fastboot")?;
        let devices = res
            .text
            .lines()
            .filter_map(|line| re.captures(line.trim()).map(|caps| caps[1].to_string()))
            .collect::<Vec<_>>();
        if let Some(serial) = devices.first() {
            self.fastboot_serial = Some(serial.clone());
        }
        Ok(devices)
    }

    fn wait_for_fastboot(&mut self) -> Result<()> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(90) {
            if !self.fastboot_devices()?.is_empty() {
                return Ok(());
            }
            self.detail(format!("等待手机进入 Fastboot… 已等待 {} 秒", start.elapsed().as_secs()));
            std::thread::sleep(Duration::from_secs(1));
        }
        bail!("等待设备进入 fastboot 超时")
    }

    fn wait_for_recovery(&mut self) -> Result<()> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(90) {
            if self.adb_transport_state() == "recovery" {
                return Ok(());
            }
            self.detail(format!("等待手机进入 Recovery… 已等待 {} 秒", start.elapsed().as_secs()));
            std::thread::sleep(Duration::from_secs(1));
        }
        bail!("等待包内 Recovery ADB 超时")
    }

    fn flash_package_recovery(&mut self) -> Result<()> {
        let recovery_ops = self
            .profile
            .boot_chain
            .iter()
            .filter(|op| op.action == "flash" && op.partition.starts_with("recovery"))
            .collect::<Vec<_>>();
        if recovery_ops.is_empty() {
            bail!("刷机包未配置 recovery_a/recovery_b，无法进入扩容所需 Recovery")
        }

        for op in recovery_ops {
            let file = op
                .file
                .as_ref()
                .ok_or_else(|| anyhow!("{} 缺少 recovery 镜像文件", op.partition))?;
            let path = self.profile.images_dir.join(file);
            if !path.is_file() {
                bail!("包内 Recovery 镜像不存在：{:?}", path);
            }
            let path_arg = path.display().to_string();
            self.fb_run(&["flash", &op.partition, path_arg.as_str()], false)?;
        }
        self.logger
            .write("包内 Recovery 已刷入，正在重启到 Recovery");
        self.fb_run(&["reboot", "recovery"], false)?;
        self.mark("等待进入 Recovery");
        self.wait_for_recovery()
    }

    pub fn ensure_recovery(&mut self) -> Result<()> {
        self.mark("连接手机");
        let state = self.adb_transport_state();
        if state == "recovery" {
            self.logger
                .write("当前已在 Recovery，先返回 bootloader 刷入包内 Recovery");
            self.adb_run(&["reboot", "bootloader"], false)?;
            std::thread::sleep(Duration::from_secs(2));
        } else if !state.is_empty() {
            self.logger
                .write(&format!("检测到 ADB/{}，正在重启到 bootloader", state));
            self.adb_run(&["reboot", "bootloader"], false)?;
            std::thread::sleep(Duration::from_secs(2));
        }

        self.wait_for_fastboot()?;
        let userspace = self
            .fb_run(&["getvar", "is-userspace"], true)
            .unwrap_or_default()
            .to_lowercase()
            .contains("is-userspace: yes");
        if userspace {
            self.logger.write("检测到 fastbootd，正在返回 bootloader");
            self.fb_run(&["reboot", "bootloader"], false)?;
            std::thread::sleep(Duration::from_secs(2));
            self.wait_for_fastboot()?;
        }
        self.mark("刷入包内 Recovery");
        self.flash_package_recovery()?;

        let dev_out = self
            .adb_shell("getprop ro.product.device", true)
            .unwrap_or_default();
        let dev = dev_out.lines().next().unwrap_or("").trim().to_string();
        self.device = dev.clone();
        if !self.cfg.codenames.contains(&dev) {
            bail!(
                "Recovery 机型不匹配：检测到 {}，支持：{}",
                dev,
                self.cfg.codenames.join(", ")
            );
        }
        Ok(())
    }

    pub fn init_sgdisk(&mut self) -> Result<()> {
        self.mark("准备分区工具");
        let probe = self.adb_shell("command -v sgdisk >/dev/null 2>&1 && sgdisk --version >/dev/null 2>&1 && echo SGDISK_OK", true).unwrap_or_default();
        if probe.contains("SGDISK_OK") {
            self.sgdisk_cmd = "sgdisk".to_string();
            self.logger.write("设备自带可用 sgdisk");
            return Ok(());
        }

        let local_sgdisk = self.cfg.sgdisk_dir.join("sgdisk");
        if !local_sgdisk.is_file() {
            bail!(
                "Recovery 无内置 sgdisk，且未找到本地 bundled sgdisk: {:?}",
                local_sgdisk
            );
        }

        self.logger.write(&format!(
            "正在推送 sgdisk 到设备：{}",
            self.cfg.remote_sgdisk_dir
        ));
        let _ = self.adb_shell(&format!("mkdir -p {}", self.cfg.remote_sgdisk_dir), true);

        // Push files from sgdisk_dir
        if let Ok(entries) = std::fs::read_dir(&self.cfg.sgdisk_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    let remote_file = format!("{}/{}", self.cfg.remote_sgdisk_dir, name);
                    let _ = self.adb_run(&["push", &p.display().to_string(), &remote_file], true);
                }
            }
        }

        let binary_path = format!("{}/sgdisk", self.cfg.remote_sgdisk_dir);
        let _ = self.adb_shell(&format!("chmod 755 {}", binary_path), true);
        self.sgdisk_cmd = format!(
            "LD_LIBRARY_PATH={} {}",
            self.cfg.remote_sgdisk_dir, binary_path
        );
        Ok(())
    }

    fn parse_part_info_text(part_num: u32, text: &str) -> Result<PartInfo> {
        let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();

        let mut code = String::new();
        let mut guid = String::new();
        let mut first = 0u64;
        let mut last = 0u64;
        let mut name = String::new();

        let re_code = Regex::new(r"(?i)Partition GUID code:\s*(\S+)")?;
        let re_guid = Regex::new(r"(?i)Partition unique GUID:\s*(\S+)")?;
        // sgdisk normally prints First sector and Last sector on separate
        // lines.  Keep the two expressions independent so both the Android
        // and desktop builds of sgdisk are accepted.
        let re_first = Regex::new(r"(?i)First sector:\s*(\d+)")?;
        let re_last = Regex::new(r"(?i)Last sector:\s*(\d+)")?;
        let re_name = Regex::new(r"(?i)Partition name:\s*'([^']*)'")?;

        for l in &lines {
            if let Some(c) = re_code.captures(l) {
                code = c[1].to_string();
            }
            if let Some(c) = re_guid.captures(l) {
                guid = c[1].to_string();
            }
            if let Some(c) = re_first.captures(l) {
                first = c[1].parse().unwrap_or(0);
            }
            if let Some(c) = re_last.captures(l) {
                last = c[1].parse().unwrap_or(0);
            }
            if let Some(c) = re_name.captures(l) {
                name = c[1].to_string();
            }
        }

        if code.is_empty() || guid.is_empty() || last <= first {
            bail!("无法解析分区 {} 信息:\n{}", part_num, text);
        }

        Ok(PartInfo {
            number: part_num,
            code,
            guid,
            first,
            last,
            name,
            lines,
        })
    }

    fn parse_part_info(&self, part_num: u32, disk: &str) -> Result<PartInfo> {
        let text = self.adb_shell(
            &format!("{} --info={} {}", self.sgdisk_cmd, part_num, disk),
            false,
        )?;
        Self::parse_part_info_text(part_num, &text)
    }

    pub fn build_plan(&self) -> Result<ExpandPlan> {
        self.mark("读取分区表");
        let print_text = self.adb_shell(
            &format!("{} --print {}", self.sgdisk_cmd, self.cfg.disk),
            false,
        )?;
        let mut logical = 4096u64;
        let mut disk_sectors = 0u64;
        let mut last_usable = 0u64;

        let re_ss = Regex::new(r"Sector size \(logical/physical\):\s*(\d+)")?;
        let re_ds = Regex::new(r"Disk [^:]+:\s*(\d+)\s+sectors")?;
        let re_lu = Regex::new(r"last usable sector is\s*(\d+)")?;

        for line in print_text.lines() {
            if let Some(c) = re_ss.captures(line) {
                logical = c[1].parse().unwrap_or(4096);
            }
            if let Some(c) = re_ds.captures(line) {
                disk_sectors = c[1].parse().unwrap_or(0);
            }
            if let Some(c) = re_lu.captures(line) {
                last_usable = c[1].parse().unwrap_or(0);
            }
        }

        let super_info = self.parse_part_info(self.cfg.super_part, &self.cfg.disk)?;
        let userdata_info = self.parse_part_info(self.cfg.userdata_part, &self.cfg.disk)?;

        if super_info.name != "super" {
            bail!(
                "分区 {} 名称是 {:?}，不是 super",
                self.cfg.super_part,
                super_info.name
            );
        }
        if userdata_info.name != "userdata" {
            bail!(
                "分区 {} 名称是 {:?}，不是 userdata",
                self.cfg.userdata_part,
                userdata_info.name
            );
        }
        if (super_info.last + 1) != userdata_info.first {
            bail!(
                "super 与 userdata 不相邻：super_last={}, userdata_first={}",
                super_info.last,
                userdata_info.first
            );
        }
        if self.cfg.target_bytes % logical != 0 {
            bail!(
                "目标字节数 {} 无法被扇区大小 {} 整除",
                self.cfg.target_bytes,
                logical
            );
        }

        let target_sectors = self.cfg.target_bytes / logical;
        let new_super_last = super_info.first + target_sectors - 1;
        let new_userdata_first = new_super_last + 1;

        if new_super_last > last_usable {
            bail!(
                "目标 super 结束扇区 {} 超过最后可用扇区 {}",
                new_super_last,
                last_usable
            );
        }
        if new_userdata_first > userdata_info.last {
            bail!(
                "目标 super 超过 userdata 结束扇区：new_userdata_first={}",
                new_userdata_first
            );
        }

        let current_super = (super_info.last - super_info.first + 1) * logical;
        let new_ud_bytes = (userdata_info.last - new_userdata_first + 1) * logical;

        Ok(ExpandPlan {
            disk: self.cfg.disk.clone(),
            sector_bytes: logical,
            disk_sectors,
            last_usable_sector: last_usable,
            target_bytes: self.cfg.target_bytes,
            target_sectors,
            super_info,
            userdata_info,
            current_super_bytes: current_super,
            new_super_last,
            new_userdata_first,
            new_userdata_bytes: new_ud_bytes,
            super_part: self.cfg.super_part,
            userdata_part: self.cfg.userdata_part,
        })
    }

    /// 导出 GPT 并打包成 zip 备份，校验通过后才返回；任何一步失败都会中止扩容。
    pub fn create_backup(&self, plan: &ExpandPlan) -> Result<PathBuf> {
        let now = Local::now();
        let dev_name = if self.device.is_empty() {
            "unknown-device"
        } else {
            &self.device
        };
        let zip_path = self.cfg.backup_dir.join(format!(
            "{}-super-expand-{}.zip",
            dev_name,
            now.format("%Y%m%d-%H%M%S")
        ));

        let staging = self.profile.work_dir.join("gpt-backup");
        std::fs::create_dir_all(&staging)?;
        let local_gpt = staging.join(backup::GPT_FILE);
        let _ = std::fs::remove_file(&local_gpt);

        self.mark("备份分区表");
        self.logger.write("正在导出 GPT 分区表…");
        let remote_gpt = "/tmp/wrion-gpt-before.bin";
        self.adb_shell(
            &format!(
                "{cmd} --backup={remote} {disk}",
                cmd = self.sgdisk_cmd,
                remote = remote_gpt,
                disk = plan.disk
            ),
            false,
        )?;
        self.adb_run(&["pull", remote_gpt, &local_gpt.display().to_string()], false)?;
        let _ = self.adb_shell(&format!("rm -f {}", remote_gpt), true);
        let gpt = std::fs::read(&local_gpt)
            .map_err(|e| anyhow!("读取导出的 GPT 失败：{}，已停止扩容", e))?;

        let plan_content = format!(
            "disk={}\ndevice={}\ncreated={}\nsector_bytes={}\ntarget_bytes={}\ntarget_sectors={}\nsuper_old={}-{}\nsuper_new={}-{}\nuserdata_old={}-{}\nuserdata_new={}-{}\n",
            plan.disk, dev_name, now.format("%Y-%m-%d %H:%M"), plan.sector_bytes, plan.target_bytes, plan.target_sectors,
            plan.super_info.first, plan.super_info.last,
            plan.super_info.first, plan.new_super_last,
            plan.userdata_info.first, plan.userdata_info.last,
            plan.new_userdata_first, plan.userdata_info.last
        );
        backup::write_backup_zip(&zip_path, &gpt, &plan_content)?;
        let _ = std::fs::remove_dir_all(&staging);

        let check = backup::inspect_backup(&zip_path, &self.cfg);
        if !check.supported {
            bail!("GPT 备份校验失败，已停止扩容：{}", check.problems.join("；"));
        }
        self.logger
            .write(&format!("GPT 备份已保存并校验通过：{:?}", zip_path));
        Ok(zip_path)
    }

    pub fn apply_expand(&mut self, plan: &ExpandPlan) -> Result<PathBuf> {
        if plan.current_super_bytes == plan.target_bytes {
            self.logger.write("super 已是目标大小，无需扩容。");
            return Ok(self.cfg.backup_dir.clone());
        }
        if plan.current_super_bytes > plan.target_bytes {
            bail!("当前 super 超过目标大小，脚本不会缩小分区。");
        }

        let backup_dir = self.create_backup(plan)?;

        // Run every destructive operation as a separate command so the user
        // can identify the exact failing step in the log.
        self.mark("清除数据");
        self.logger.write("步骤 1/9：卸载 /data");
        let _ = self.adb_shell("umount -l /data", true);
        self.logger.write("步骤 2/9：卸载 /metadata");
        let _ = self.adb_shell("umount -l /metadata", true);
        self.logger.write("步骤 3/9：Recovery 擦除 data");
        let _ = self.adb_shell("twrp wipe data", true);
        self.logger.write("步骤 4/9：Recovery 擦除 metadata");
        let _ = self.adb_shell("twrp wipe metadata", true);
        let commands =
            [
                format!(
                    "{} --delete={} {}",
                    self.sgdisk_cmd, plan.super_part, plan.disk
                ),
                format!(
                    "{} --delete={} {}",
                    self.sgdisk_cmd, plan.userdata_part, plan.disk
                ),
                format!(
                "{} --new={}:{}:{} --change-name={}:{} --typecode={}:{} --partition-guid={}:{} {}",
                self.sgdisk_cmd,
                plan.super_part, plan.super_info.first, plan.new_super_last,
                plan.super_part, plan.super_info.name,
                plan.super_part, plan.super_info.code,
                plan.super_part, plan.super_info.guid,
                plan.disk,
            ),
                format!(
                "{} --new={}:{}:{} --change-name={}:{} --typecode={}:{} --partition-guid={}:{} {}",
                self.sgdisk_cmd,
                plan.userdata_part, plan.new_userdata_first, plan.userdata_info.last,
                plan.userdata_part, plan.userdata_info.name,
                plan.userdata_part, plan.userdata_info.code,
                plan.userdata_part, plan.userdata_info.guid,
                plan.disk,
            ),
            ];
        self.mark("重写分区表");
        for (index, command) in commands.iter().enumerate() {
            self.detail(format!("分区表操作 {}/{}", index + 1, commands.len()));
            self.logger
                .write(&format!("步骤 {}/9：执行 GPT 操作 {}", index + 5, command));
            let output = self.adb_shell(command, false)?;
            self.logger
                .write(&format!("GPT 操作 {} 输出：{}", index + 1, output));
        }

        self.mark("格式化 userdata");
        self.logger.write("步骤 9/9：格式化新 userdata");
        let _ = self.adb_shell("twrp wipe data", true);

        Ok(backup_dir)
    }

    pub fn list_backups(&self) -> Vec<BackupInfo> {
        backup::list_backups(&self.cfg)
    }

    /// 用备份（zip 或旧版目录）恢复扩容前的官方 GPT，并核对恢复后的分区范围。
    pub fn restore_official(&mut self, backup_path: &Path) -> Result<()> {
        self.mark("检查备份");
        let check = backup::inspect_backup(backup_path, &self.cfg);
        if !check.supported {
            bail!("这个备份不能用于当前刷机包：{}", check.problems.join("；"));
        }
        let contents = backup::read_backup(backup_path)?;

        self.ensure_recovery()?;
        if let Some(expected) = check.device.as_deref().filter(|d| *d != "unknown-device") {
            if expected != self.device {
                bail!(
                    "备份来自 {}，当前手机是 {}。为防止写错分区表，已停止恢复。",
                    expected,
                    self.device
                );
            }
        }
        self.init_sgdisk()?;

        let local_gpt = self.profile.work_dir.join("gpt-restore.bin");
        std::fs::create_dir_all(&self.profile.work_dir)?;
        std::fs::write(&local_gpt, &contents.gpt)?;

        let remote_gpt = "/tmp/wrion-gpt-restore.bin";
        self.adb_run(&["push", &local_gpt.display().to_string(), remote_gpt], false)?;
        let _ = std::fs::remove_file(&local_gpt);

        self.mark("写入官方分区表");
        self.logger.write("正在通过 sgdisk 恢复官方 GPT…");
        let cmd = format!(
            "{} --load-backup={} {}",
            self.sgdisk_cmd, remote_gpt, self.cfg.disk
        );
        let res = self.adb_shell(&cmd, false)?;
        self.logger.write(&format!("恢复输出: {}", res));
        let _ = self.adb_shell(&format!("rm -f {}", remote_gpt), true);

        self.mark("核对分区范围");
        self.verify_restored(&contents.plan)?;

        self.mark("清除数据");
        self.logger.write("官方分区表恢复成功，正在格式化数据…");
        let _ = self.adb_shell("twrp wipe data", true);
        let _ = self.adb_shell("twrp wipe metadata", true);
        Ok(())
    }

    /// 恢复后读取 super / userdata 的扇区范围，与备份记录的扩容前范围比对。
    fn verify_restored(&self, plan: &std::collections::HashMap<String, String>) -> Result<()> {
        let checks = [
            (self.cfg.super_part, "super_old"),
            (self.cfg.userdata_part, "userdata_old"),
        ];
        for (part, key) in checks {
            let Some((first, last)) = plan.get(key).and_then(|v| backup::parse_range(v)) else {
                self.logger
                    .write(&format!("备份未记录 {}，跳过恢复结果核对", key));
                continue;
            };
            let info = self.parse_part_info(part, &self.cfg.disk)?;
            if info.first != first || info.last != last {
                bail!(
                    "恢复后分区 {} 范围为 {}-{}，与备份记录的 {}-{} 不一致，请不要重启手机并联系作者",
                    part,
                    info.first,
                    info.last,
                    first,
                    last
                );
            }
        }
        self.logger.write("恢复结果核对通过：分区范围与扩容前一致");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ExpandSession;

    #[test]
    fn parses_sgdisk_sector_fields_on_separate_lines() {
        let output = r#"Partition GUID code: 8A12DE1-5E41-4C3B-8B4C-B1441E5D3A38 (Unknown)
Partition unique GUID: 821E3EBA-1B33-F194-B4B3-2FD3EABB567F
First sector: 228864 (at 894.0 MiB)
Last sector: 4423167 (at 16.9 GiB)
Partition size: 4194304 sectors (16.0 GiB)
Attribute flags: 0000000000000000
Partition name: 'super'
"#;

        let info = ExpandSession::parse_part_info_text(32, output).unwrap();
        assert_eq!(info.number, 32);
        assert_eq!(info.first, 228864);
        assert_eq!(info.last, 4423167);
        assert_eq!(info.name, "super");
    }
}
