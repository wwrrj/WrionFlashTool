use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use anyhow::{anyhow, bail, Result};
use chrono::Local;
use regex::Regex;
use serde::{Deserialize, Serialize};

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
}

impl ExpandSession {
    pub fn new(profile: FlashProfile, logger: Logger) -> Result<Self> {
        let cfg = profile.expansion.clone()
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
        })
    }

    pub fn adb_run(&self, args: &[&str], allow_fail: bool) -> Result<String> {
        let res = run_process(&self.profile.adb(), args, Some(&self.logger))?;
        if res.code != 0 && !allow_fail {
            bail!("ADB 失败（{}）：adb {}\n{}", res.code, args.join(" "), res.text);
        }
        Ok(res.text)
    }

    pub fn adb_shell(&self, cmd: &str, allow_fail: bool) -> Result<String> {
        self.adb_run(&["shell", cmd], allow_fail)
    }

    pub fn fb_run(&self, args: &[&str], allow_fail: bool) -> Result<String> {
        let res = run_process(&self.profile.fastboot(), args, Some(&self.logger))?;
        if res.code != 0 && !allow_fail {
            bail!("fastboot 失败（{}）：fastboot {}\n{}", res.code, args.join(" "), res.text);
        }
        Ok(res.text)
    }

    pub fn adb_transport_state(&self) -> String {
        if let Ok(text) = self.adb_run(&["devices"], true) {
            let re = Regex::new(r"(?m)^(\S+)\s+(recovery|device|sideload)\s*$").unwrap();
            let matches: Vec<_> = re.captures_iter(&text).collect();
            if matches.len() == 1 {
                return matches[0][2].to_string();
            }
        }
        String::new()
    }

    pub fn ensure_recovery(&mut self) -> Result<()> {
        let state = self.adb_transport_state();
        if state == "recovery" {
            self.logger.write("已在 Recovery (ADB 模式)");
        } else {
            self.logger.write("正在连接 Recovery… 请确保手机处于 Recovery 模式并开启 ADB");
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(60) {
                if self.adb_transport_state() == "recovery" {
                    break;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            if self.adb_transport_state() != "recovery" {
                bail!("等待 Recovery ADB 超时，请检查手机是否已进入 Recovery 且驱动正常");
            }
        }

        let dev_out = self.adb_shell("getprop ro.product.device", true).unwrap_or_default();
        let dev = dev_out.lines().next().unwrap_or("").trim().to_string();
        self.device = dev.clone();
        if !self.cfg.codenames.contains(&dev) {
            bail!("Recovery 机型不匹配：检测到 {}，支持：{}", dev, self.cfg.codenames.join(", "));
        }
        Ok(())
    }

    pub fn init_sgdisk(&mut self) -> Result<()> {
        let probe = self.adb_shell("command -v sgdisk >/dev/null 2>&1 && sgdisk --version >/dev/null 2>&1 && echo SGDISK_OK", true).unwrap_or_default();
        if probe.contains("SGDISK_OK") {
            self.sgdisk_cmd = "sgdisk".to_string();
            self.logger.write("设备自带可用 sgdisk");
            return Ok(());
        }

        let local_sgdisk = self.cfg.sgdisk_dir.join("sgdisk");
        if !local_sgdisk.is_file() {
            bail!("Recovery 无内置 sgdisk，且未找到本地 bundled sgdisk: {:?}", local_sgdisk);
        }

        self.logger.write(&format!("正在推送 sgdisk 到设备：{}", self.cfg.remote_sgdisk_dir));
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
        self.sgdisk_cmd = format!("LD_LIBRARY_PATH={} {}", self.cfg.remote_sgdisk_dir, binary_path);
        Ok(())
    }

    fn parse_part_info(&self, part_num: u32, disk: &str) -> Result<PartInfo> {
        let text = self.adb_shell(&format!("{} -i {} {}", self.sgdisk_cmd, part_num, disk), false)?;
        let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();

        let mut code = String::new();
        let mut guid = String::new();
        let mut first = 0u64;
        let mut last = 0u64;
        let mut name = String::new();

        let re_code = Regex::new(r"(?i)Partition GUID code:\s*(\S+)")?;
        let re_guid = Regex::new(r"(?i)Partition unique GUID:\s*(\S+)")?;
        let re_sectors = Regex::new(r"(?i)First sector:\s*(\d+).*?Last sector:\s*(\d+)")?;
        let re_name = Regex::new(r"(?i)Partition name:\s*'([^']*)'")?;

        for l in &lines {
            if let Some(c) = re_code.captures(l) {
                code = c[1].to_string();
            }
            if let Some(c) = re_guid.captures(l) {
                guid = c[1].to_string();
            }
            if let Some(c) = re_sectors.captures(l) {
                first = c[1].parse().unwrap_or(0);
                last = c[2].parse().unwrap_or(0);
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

    pub fn build_plan(&self) -> Result<ExpandPlan> {
        let print_text = self.adb_shell(&format!("{} -p {}", self.sgdisk_cmd, self.cfg.disk), false)?;
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
            bail!("分区 {} 名称是 {:?}，不是 super", self.cfg.super_part, super_info.name);
        }
        if userdata_info.name != "userdata" {
            bail!("分区 {} 名称是 {:?}，不是 userdata", self.cfg.userdata_part, userdata_info.name);
        }
        if (super_info.last + 1) != userdata_info.first {
            bail!(
                "super 与 userdata 不相邻：super_last={}, userdata_first={}",
                super_info.last, userdata_info.first
            );
        }
        if self.cfg.target_bytes % logical != 0 {
            bail!("目标字节数 {} 无法被扇区大小 {} 整除", self.cfg.target_bytes, logical);
        }

        let target_sectors = self.cfg.target_bytes / logical;
        let new_super_last = super_info.first + target_sectors - 1;
        let new_userdata_first = new_super_last + 1;

        if new_super_last > last_usable {
            bail!("目标 super 结束扇区 {} 超过最后可用扇区 {}", new_super_last, last_usable);
        }
        if new_userdata_first > userdata_info.last {
            bail!("目标 super 超过 userdata 结束扇区：new_userdata_first={}", new_userdata_first);
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

    pub fn create_backup(&self, plan: &ExpandPlan) -> Result<PathBuf> {
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        let dev_name = if self.device.is_empty() { "unknown-device" } else { &self.device };
        let out_dir = self.cfg.backup_dir.join(format!("{}-super-expand-{}", dev_name, stamp));
        std::fs::create_dir_all(&out_dir)?;

        self.logger.write(&format!("正在备份 GPT 到：{:?}", out_dir));

        let remote_gpt = "/tmp/harusora-gpt-before.bin";
        let _ = self.adb_shell(&format!("{cmd} --backup={remote} {disk}", cmd = self.sgdisk_cmd, remote = remote_gpt, disk = plan.disk), true);
        let _ = self.adb_run(&["pull", remote_gpt, &out_dir.join("gpt-sda-before.bin").display().to_string()], true);
        let _ = self.adb_shell(&format!("rm -f {}", remote_gpt), true);

        // Save plan.txt
        let plan_content = format!(
            "disk={}\ndevice={}\nsector_bytes={}\ntarget_bytes={}\ntarget_sectors={}\nsuper_old={}-{}\nsuper_new={}-{}\nuserdata_old={}-{}\nuserdata_new={}-{}\n",
            plan.disk, dev_name, plan.sector_bytes, plan.target_bytes, plan.target_sectors,
            plan.super_info.first, plan.super_info.last,
            plan.super_info.first, plan.new_super_last,
            plan.userdata_info.first, plan.userdata_info.last,
            plan.new_userdata_first, plan.userdata_info.last
        );
        std::fs::write(out_dir.join("plan.txt"), plan_content)?;

        Ok(out_dir)
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

        // Wipe userdata & metadata before rewriting GPT (safety requirement)
        self.logger.write("写入 GPT 前先格式化 userdata / metadata…");
        let _ = self.adb_shell("twrp wipe data; twrp wipe metadata; sync", true);
        let _ = self.adb_shell("umount -l /data; umount -l /metadata; sync", true);

        // Rewrite super and userdata using sgdisk
        self.logger.write("sgdisk 重写 super/userdata…");
        let sg_cmd = format!(
            "{cmd} --delete={s_num} --delete={u_num} \
             --new={s_num}:{s_start}:{s_end} --change-name={s_num}:{s_name} --typecode={s_num}:{s_code} --partition-guid={s_num}:{s_guid} \
             --new={u_num}:{u_start}:{u_end} --change-name={u_num}:{u_name} --typecode={u_num}:{u_code} --partition-guid={u_num}:{u_guid} \
             {disk}",
            cmd = self.sgdisk_cmd,
            s_num = plan.super_part,
            u_num = plan.userdata_part,
            s_start = plan.super_info.first,
            s_end = plan.new_super_last,
            s_name = plan.super_info.name,
            s_code = plan.super_info.code,
            s_guid = plan.super_info.guid,
            u_start = plan.new_userdata_first,
            u_end = plan.userdata_info.last,
            u_name = plan.userdata_info.name,
            u_code = plan.userdata_info.code,
            u_guid = plan.userdata_info.guid,
            disk = plan.disk,
        );

        let res = self.adb_shell(&sg_cmd, false)?;
        self.logger.write(&format!("sgdisk output:\n{}", res));

        self.logger.write("扩容分区表已重写完成。正在格式化新 userdata 分区…");
        let _ = self.adb_shell("twrp wipe data || true", true);

        Ok(backup_dir)
    }

    pub fn list_backups(&self) -> Vec<PathBuf> {
        let mut list = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.cfg.backup_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() && p.join("gpt-sda-before.bin").is_file() {
                    list.push(p);
                }
            }
        }
        list
    }

    pub fn restore_official(&mut self, backup_dir: &Path) -> Result<()> {
        let gpt_bin = backup_dir.join("gpt-sda-before.bin");
        if !gpt_bin.is_file() {
            bail!("备份目录中缺少 gpt-sda-before.bin: {:?}", backup_dir);
        }
        self.ensure_recovery()?;
        self.init_sgdisk()?;

        let remote_gpt = "/tmp/harusora-gpt-restore.bin";
        self.adb_run(&["push", &gpt_bin.display().to_string(), remote_gpt], false)?;

        self.logger.write("正在通过 sgdisk 恢复官方 GPT…");
        let cmd = format!("{} --load-backup={} {}", self.sgdisk_cmd, remote_gpt, self.cfg.disk);
        let res = self.adb_shell(&cmd, false)?;
        self.logger.write(&format!("恢复输出: {}", res));
        let _ = self.adb_shell(&format!("rm -f {}", remote_gpt), true);

        self.logger.write("官方分区表恢复成功，正在格式化数据…");
        let _ = self.adb_shell("twrp wipe data; twrp wipe metadata || true", true);
        Ok(())
    }
}
