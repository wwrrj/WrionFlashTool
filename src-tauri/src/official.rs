//! 小米官方线刷包（images/ + flash_all.bat）。
//!
//! 不直接运行 bat：逐行解析成受限的操作列表，只认识官方脚本里出现的几种写法
//! （机型校验、防回滚校验、按 getvar 结果条件执行、flash / erase / set_active / reboot）。
//! 任何无法识别的行、以及任何锁定 Bootloader 的命令都会让整个脚本被拒绝。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::config::FlashProfile;
use crate::device::DeviceSession;

pub const CLEAN_SCRIPT: &str = "flash_all.bat";
pub const KEEP_DATA_SCRIPT: &str = "flash_all_except_storage.bat";

/// 解析后的脚本操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptOp {
    /// `getvar product | findstr ... || exit`：手机代号必须是其中之一
    CheckProduct(Vec<String>),
    /// 防回滚校验：手机的 anti 不能高于 images/anti_version.txt
    CheckAnti,
    Flash { partition: String, file: PathBuf },
    Erase { partition: String },
    SetActive(String),
    Reboot,
    /// `getvar name | findstr "^name: value"` 后接 `if %errorlevel% equ 0 ( ... )`
    IfVar { name: String, expect: String, body: Vec<ScriptOp> },
}

/// 官方包在 FlashProfile 中附带的信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfficialInfo {
    pub device: String,
    pub build_number: String,
    /// 完整线刷脚本文件名（清除数据）
    pub clean_script: Option<String>,
    /// 保留数据的脚本文件名
    pub keep_data_script: Option<String>,
}

fn data_partition(partition: &str) -> bool {
    let base = partition.trim_end_matches("_ab").trim_end_matches("_a").trim_end_matches("_b");
    matches!(base, "userdata" | "metadata")
}

fn resolve_script_path(root: &Path, raw: &str, line_no: usize) -> Result<PathBuf> {
    let rel = raw.trim_matches('"').replace("%~dp0", "").replace('\\', "/");
    let rel = rel.trim_start_matches("./");
    let p = Path::new(rel);
    if rel.is_empty() || p.is_absolute() || rel.contains(':') || p.components().any(|c| c.as_os_str() == "..") {
        bail!("第 {} 行的文件路径不在刷机包内：{}", line_no, raw);
    }
    Ok(root.join(rel))
}

/// 从 findstr 的 `/c:"^name: *value"` 模式里取出期望值。
fn findstr_values(filter: &str, name: &str, line_no: usize) -> Result<Vec<String>> {
    let re = Regex::new(r#"/c:"([^"]*)""#).unwrap();
    let mut values = Vec::new();
    for caps in re.captures_iter(filter) {
        let pattern = caps[1].trim_start_matches('^');
        let rest = pattern
            .strip_prefix(&format!("{}:", name))
            .ok_or_else(|| anyhow!("第 {} 行的 findstr 条件无法识别：{}", line_no, pattern))?;
        let value = rest.trim_start_matches(' ').trim_start_matches('*').trim();
        if value.is_empty() || value.contains(|c: char| "*?[]\\.$".contains(c)) {
            bail!("第 {} 行的 findstr 条件无法识别：{}", line_no, pattern);
        }
        values.push(value.to_string());
    }
    if values.is_empty() {
        bail!("第 {} 行缺少 findstr 条件", line_no);
    }
    Ok(values)
}

/// 防回滚校验那一组 set / for / if 语句，统一当作一次 CheckAnti。
fn is_anti_line(lower: &str) -> bool {
    !lower.starts_with("fastboot ") && (lower.contains("anti") || lower.contains("%version%"))
}

/// 解析官方 flash_all*.bat。`root` 用于把 `%~dp0images/...` 解析成实际路径。
pub fn parse_bat(text: &str, root: &Path) -> Result<Vec<ScriptOp>> {
    let re_if = Regex::new(r"(?i)^if\s+%errorlevel%\s+equ\s+0\s*\($").unwrap();
    let mut top: Vec<ScriptOp> = Vec::new();
    // 条件块：(变量名, 期望值, 块内操作)
    let mut blocks: Vec<(String, String, Vec<ScriptOp>)> = Vec::new();
    let mut pending: Option<(String, String)> = None;
    let mut saw_anti = false;

    let push = |op: ScriptOp, top: &mut Vec<ScriptOp>, blocks: &mut Vec<(String, String, Vec<ScriptOp>)>| {
        let target = match blocks.last_mut() {
            Some((_, _, body)) => body,
            None => top,
        };
        // 官方脚本会把机型校验写两遍（一行提示、一行退出），合并成一次
        if matches!((&op, target.last()), (ScriptOp::CheckProduct(a), Some(ScriptOp::CheckProduct(b))) if a == b) {
            return;
        }
        target.push(op);
    };

    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = raw.trim();
        let lower = line.to_lowercase();
        let prev_condition = pending.take();

        if line.is_empty() || line.starts_with("::") || lower.starts_with("rem ") || lower == "rem" || lower == "@echo off" || lower == "echo off" {
            pending = prev_condition;
            continue;
        }
        if line == ")" {
            let (name, expect, body) = blocks.pop().ok_or_else(|| anyhow!("第 {} 行多了一个 )", line_no))?;
            push(ScriptOp::IfVar { name, expect, body }, &mut top, &mut blocks);
            continue;
        }
        if re_if.is_match(line) {
            let (name, expect) = prev_condition
                .ok_or_else(|| anyhow!("第 {} 行的条件判断前面没有 getvar 检查", line_no))?;
            blocks.push((name, expect, Vec::new()));
            continue;
        }
        if is_anti_line(&lower) {
            if !saw_anti {
                push(ScriptOp::CheckAnti, &mut top, &mut blocks);
                saw_anti = true;
            }
            continue;
        }
        if (lower.starts_with("echo ") || lower.starts_with("@echo ")) && !lower.contains("fastboot") {
            continue;
        }
        if !lower.starts_with("fastboot ") {
            bail!("第 {} 行无法识别，已拒绝执行：{}", line_no, line);
        }

        let (command, handler) = match line.split_once("||") {
            Some((c, h)) => (c, Some(h)),
            None => (line, None),
        };
        let (command, filter) = match command.split_once('|') {
            Some((c, f)) => (c, Some(f)),
            None => (command, None),
        };
        let command = command.replace("2>&1", "");
        let tokens: Vec<&str> = command.split_whitespace().collect();
        if tokens.len() < 3 || tokens[1] != "%*" {
            bail!("第 {} 行的 fastboot 命令格式无法识别：{}", line_no, line);
        }
        let args = &tokens[2..];
        let verb = args[0].to_lowercase();
        if (verb == "oem" || verb == "flashing") && args.iter().any(|a| a.to_lowercase().contains("lock")) {
            bail!("第 {} 行包含锁定 Bootloader 的命令（{}），本工具不会执行这个脚本", line_no, args.join(" "));
        }
        if filter.is_some() && verb != "getvar" {
            bail!("第 {} 行的命令格式无法识别：{}", line_no, line);
        }

        match (verb.as_str(), args.len()) {
            ("getvar", 2) => {
                let name = args[1].to_string();
                let filter = filter.ok_or_else(|| anyhow!("第 {} 行的 getvar 没有判断条件", line_no))?;
                let values = findstr_values(filter, &name, line_no)?;
                if name == "product" {
                    push(ScriptOp::CheckProduct(values), &mut top, &mut blocks);
                } else if handler.is_none() && values.len() == 1 {
                    pending = Some((name, values[0].clone()));
                } else {
                    bail!("第 {} 行的 getvar 检查无法识别：{}", line_no, line);
                }
            }
            ("flash", 3) => {
                let file = resolve_script_path(root, args[2], line_no)?;
                push(ScriptOp::Flash { partition: args[1].to_string(), file }, &mut top, &mut blocks);
            }
            ("erase", 2) => push(ScriptOp::Erase { partition: args[1].to_string() }, &mut top, &mut blocks),
            ("set_active", 2) if matches!(args[1], "a" | "b") => {
                push(ScriptOp::SetActive(args[1].to_string()), &mut top, &mut blocks)
            }
            ("reboot", 1) => push(ScriptOp::Reboot, &mut top, &mut blocks),
            _ => bail!("第 {} 行的 fastboot 命令不在允许范围内，已拒绝执行：{}", line_no, line),
        }
    }

    if !blocks.is_empty() {
        bail!("脚本的条件块没有结束（缺少 )）");
    }
    if !top.iter().any(|op| matches!(op, ScriptOp::CheckProduct(_))) {
        bail!("脚本没有校验手机机型，为安全起见不会执行");
    }
    Ok(top)
}

/// 按执行顺序展开（条件块内的操作也计入），用于统计步骤和检查文件。
pub fn flatten(ops: &[ScriptOp]) -> Vec<&ScriptOp> {
    let mut out = Vec::new();
    for op in ops {
        out.push(op);
        if let ScriptOp::IfVar { body, .. } = op {
            out.extend(flatten(body));
        }
    }
    out
}

pub fn touches_user_data(ops: &[ScriptOp]) -> bool {
    flatten(ops).iter().any(|op| match op {
        ScriptOp::Flash { partition, .. } | ScriptOp::Erase { partition } => data_partition(partition),
        _ => false,
    })
}

pub fn missing_files(ops: &[ScriptOp]) -> Vec<PathBuf> {
    flatten(ops)
        .into_iter()
        .filter_map(|op| match op {
            ScriptOp::Flash { file, .. } if !file.is_file() => Some(file.clone()),
            _ => None,
        })
        .collect()
}

fn parse_file(root: &Path, name: &str) -> Result<Option<Vec<ScriptOp>>> {
    let path = root.join(name);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).with_context(|| format!("无法读取 {}", name))?;
    let ops = parse_bat(&String::from_utf8_lossy(&bytes), root).with_context(|| format!("{} 解析失败", name))?;
    Ok(Some(ops))
}

/// 是否像官方线刷包的根目录。
pub fn is_official_root(dir: &Path) -> bool {
    dir.join("images").is_dir() && (dir.join(CLEAN_SCRIPT).is_file() || dir.join(KEEP_DATA_SCRIPT).is_file())
}

/// 在 `dir` 或它唯一的子目录中查找官方包根目录。
pub fn find_official_root(dir: &Path) -> Option<PathBuf> {
    if is_official_root(dir) {
        return Some(dir.to_path_buf());
    }
    let found: Vec<PathBuf> = std::fs::read_dir(dir).ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && is_official_root(p))
        .collect();
    (found.len() == 1).then(|| found[0].clone())
}

fn read_misc(root: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(root.join("misc.txt"))
        .map(|t| crate::backup::parse_plan(&t))
        .unwrap_or_default()
}

/// 把官方包转换成 FlashProfile，以便复用设备检测、预检、Root 等流程。
pub fn load_official(root: &Path) -> Result<FlashProfile> {
    let clean = parse_file(root, CLEAN_SCRIPT)?;
    let keep = parse_file(root, KEEP_DATA_SCRIPT)?;
    let reference = clean.as_ref().or(keep.as_ref()).ok_or_else(|| anyhow!("没有找到 {}", CLEAN_SCRIPT))?;
    let codenames = reference
        .iter()
        .find_map(|op| match op {
            ScriptOp::CheckProduct(c) => Some(c.clone()),
            _ => None,
        })
        .unwrap_or_default();

    let missing: Vec<String> = clean.iter().chain(keep.iter())
        .flat_map(|ops| missing_files(ops))
        .map(|p| p.display().to_string())
        .collect();
    if !missing.is_empty() {
        bail!("官方线刷包文件不完整：\n{}", missing.join("\n"));
    }
    // 保留数据的脚本如果仍会改写 userdata / metadata，就不提供这个选项
    let keep_ok = keep.as_ref().is_some_and(|ops| !touches_user_data(ops));

    let misc = read_misc(root);
    let device = misc.get("device").cloned().unwrap_or_else(|| codenames.first().cloned().unwrap_or_default());
    let build_number = misc.get("build_number").or_else(|| misc.get("mi_os_build_version")).cloned().unwrap_or_default();
    let build_date = misc
        .get("userdata_version")
        .and_then(|v| v.get(..8))
        .filter(|d| d.chars().all(|c| c.is_ascii_digit()))
        .map(|d| format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..8]))
        .unwrap_or_default();

    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let images_dir = root.join("images");
    let tools = crate::platform_tools::tools_dir()?;

    Ok(FlashProfile {
        rom_root: root.clone(),
        profile_path: root.join(if clean.is_some() { CLEAN_SCRIPT } else { KEEP_DATA_SCRIPT }),
        codenames,
        package_name: if build_number.is_empty() { "小米官方线刷包".to_string() } else { format!("小米官方线刷包 {}", build_number) },
        display_name: device.clone(),
        notes: vec![
            "这是小米官方线刷包。工具会逐条解析并执行包内 flash_all 脚本中的命令，遇到无法识别的内容会直接拒绝。".to_string(),
            "工具不会执行 flash_all_lock 或任何锁定 Bootloader 的命令。".to_string(),
            "完整线刷对应 flash_all.bat，会清空手机数据；保留数据更新对应 flash_all_except_storage.bat。".to_string(),
        ],
        author: "Xiaomi 官方".to_string(),
        qq_group: String::new(),
        build_date,
        minimum_super_bytes: 0,
        anti_version_file: images_dir.join("anti_version.txt").is_file().then(|| "anti_version.txt".to_string()),
        erase_if_present: Vec::new(),
        pre_erase: Vec::new(),
        super_dir: images_dir.clone(),
        images_dir,
        work_dir: root.join("work"),
        logs_dir: root.join("logs"),
        expansion_dir: None,
        tools_dir: tools.clone(),
        firmware: Vec::new(),
        boot_chain: Vec::new(),
        final_data: Vec::new(),
        wipe_super_image: None,
        wipe_super_dat: None,
        slot_suffix: String::new(),
        dynamic_partitions: Vec::new(),
        set_active_slot: None,
        flash_crc_lists: false,
        crc_list_file: String::new(),
        sparse_crc_list_file: String::new(),
        require_wipe_confirmation: true,
        confirmation_word: "WIPE".to_string(),
        update_confirmation_word: "UPDATE".to_string(),
        reboot_after_success: true,
        enable_root_menu: true,
        post_flash_id: false,
        expansion: None,
        kind: "official".to_string(),
        official: Some(OfficialInfo {
            device,
            build_number,
            clean_script: clean.is_some().then(|| CLEAN_SCRIPT.to_string()),
            keep_data_script: keep_ok.then(|| KEEP_DATA_SCRIPT.to_string()),
        }),
        platform_tools_dir: Some(tools),
    })
}

/// 刷入进度里显示的 6 个阶段，与前端 FLASH_STAGES 对应。
fn stage_of(op: &ScriptOp) -> u32 {
    match op {
        ScriptOp::CheckProduct(_) | ScriptOp::CheckAnti | ScriptOp::IfVar { .. } => 1,
        ScriptOp::SetActive(_) | ScriptOp::Reboot => 6,
        ScriptOp::Flash { partition, .. } | ScriptOp::Erase { partition } => {
            let base = partition.trim_end_matches("_ab");
            if data_partition(partition) {
                5
            } else if base == "super" {
                4
            } else if matches!(base, "boot" | "init_boot" | "vendor_boot" | "recovery" | "dtbo" | "vbmeta" | "vbmeta_system" | "vm-bootsys") {
                3
            } else {
                2
            }
        }
    }
}

const STAGE_TITLES: [&str; 6] = ["安全预检", "写入固件", "写入启动链", "写入系统分区", "处理用户数据", "完成并重启"];

struct Runner<'a> {
    profile: &'a FlashProfile,
    session: &'a DeviceSession,
    app: Option<&'a AppHandle>,
    reboot: bool,
    index: usize,
    total: usize,
    stage: u32,
    rebooted: bool,
}

impl Runner<'_> {
    fn emit_stage(&mut self, stage: u32) {
        // 阶段只前进不后退（官方脚本里启动链分区分散在 super 前后）
        if stage <= self.stage {
            return;
        }
        self.stage = stage;
        if let Some(h) = self.app {
            let _ = h.emit("flash_stage", serde_json::json!({ "stage": stage, "total": 6, "title": STAGE_TITLES[stage as usize - 1] }));
        }
    }

    fn step(&mut self, op: &ScriptOp, label: String) {
        self.index += 1;
        self.emit_stage(stage_of(op));
        self.session.logger.write(&format!("[{}/{}] {}", self.index, self.total, label));
        if let Some(h) = self.app {
            let _ = h.emit("flash_step", serde_json::json!({ "index": self.index, "total": self.total, "label": label }));
        }
    }

    fn run(&mut self, ops: &[ScriptOp]) -> Result<()> {
        for op in ops {
            match op {
                ScriptOp::CheckProduct(codenames) => {
                    self.step(op, "校验手机机型".to_string());
                    let product = self.session.getvar("product", false)?.unwrap_or_default();
                    if !codenames.iter().any(|c| c == &product) {
                        bail!("机型不匹配：手机是 {}，这个官方包适用于 {}", product, codenames.join(" / "));
                    }
                }
                ScriptOp::CheckAnti => {
                    self.step(op, "校验防回滚版本".to_string());
                    let package = crate::package::read_package_anti(self.profile)?.unwrap_or(0);
                    let device = self.session.getvar("anti", true)?
                        .and_then(|v| v.trim().parse::<u32>().ok())
                        .unwrap_or(0);
                    if device > package {
                        bail!("防回滚检查未通过：手机为 {}，刷机包为 {}，刷入会导致无法开机", device, package);
                    }
                }
                ScriptOp::IfVar { name, expect, body } => {
                    self.step(op, format!("检查 {}", name));
                    let value = self.session.getvar(name, true)?.unwrap_or_default();
                    if value == *expect {
                        self.run(body)?;
                    } else {
                        self.session.logger.write(&format!("{} = {:?}，跳过 {} 个操作", name, value, flatten(body).len()));
                        self.index += flatten(body).len();
                    }
                }
                ScriptOp::Flash { partition, file } => {
                    let name = file.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    self.step(op, format!("写入 {}（{}）", partition, name));
                    let file_arg = file.display().to_string();
                    self.session.run_fastboot(&["flash", partition, &file_arg], None, false)
                        .with_context(|| format!("写入 {} 失败", partition))?;
                }
                ScriptOp::Erase { partition } => {
                    self.step(op, format!("擦除 {}", partition));
                    self.session.run_fastboot(&["erase", partition], None, false)
                        .with_context(|| format!("擦除 {} 失败", partition))?;
                }
                ScriptOp::SetActive(slot) => {
                    self.step(op, format!("切换到 {} 槽位", slot));
                    self.session.run_fastboot(&["set_active", slot], None, false)?;
                }
                ScriptOp::Reboot => {
                    self.step(op, if self.reboot { "重启手机".to_string() } else { "跳过重启（已关闭自动重启）".to_string() });
                    self.rebooted = self.session.reboot_to_system(self.reboot)?;
                }
            }
        }
        Ok(())
    }
}

/// 按官方脚本刷入。`preserve_data` 选择 flash_all_except_storage.bat。返回手机是否已重启进系统。
pub fn run_official(
    profile: &FlashProfile,
    mut session: DeviceSession,
    preserve_data: bool,
    reboot: bool,
    app: Option<&AppHandle>,
) -> Result<bool> {
    let info = profile.official.as_ref().ok_or_else(|| anyhow!("当前不是官方线刷包"))?;
    let script = if preserve_data { info.keep_data_script.as_deref() } else { info.clean_script.as_deref() }
        .ok_or_else(|| anyhow!(if preserve_data { "这个官方包不支持保留数据更新" } else { "这个官方包没有完整线刷脚本" }))?;
    let ops = parse_file(&profile.rom_root, script)?.ok_or_else(|| anyhow!("没有找到 {}", script))?;
    if preserve_data && touches_user_data(&ops) {
        bail!("{} 会改写用户数据，已停止“保留数据更新”", script);
    }
    let missing = missing_files(&ops);
    if !missing.is_empty() {
        bail!("官方线刷包文件不完整：\n{}", missing.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n"));
    }

    let chosen = session.serial.clone();
    session.select_device(chosen.as_deref())?;
    let userspace = session.getvar("is-userspace", true)?.is_some_and(|v| v.eq_ignore_ascii_case("yes"));
    if userspace {
        session.run_fastboot(&["reboot", "bootloader"], Some("从 fastbootd 返回 bootloader"), false)?;
        session.wait_for_mode(false, 90)?;
    }
    session.logger.write(&format!("按官方脚本 {} 刷入，共 {} 个操作", script, flatten(&ops).len()));

    let mut runner = Runner { profile, session: &session, app, reboot, index: 0, total: flatten(&ops).len(), stage: 0, rebooted: false };
    runner.run(&ops)?;
    runner.emit_stage(6);
    let rebooted = runner.rebooted;
    session.logger.write("官方脚本执行完成");
    Ok(rebooted)
}
