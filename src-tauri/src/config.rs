use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageOp {
    pub action: String, // "flash" | "erase"
    pub partition: String,
    pub file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DynamicPartition {
    pub name: String,
    pub dat_bytes: u64,
    pub pad_to_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExpansionConfig {
    pub enabled: bool,
    pub codenames: Vec<String>,
    pub target_bytes: u64,
    pub disk: String,
    pub super_part: u32,
    pub userdata_part: u32,
    pub backup_dir: PathBuf,
    pub sgdisk_dir: PathBuf,
    pub remote_sgdisk_dir: String,
    pub confirm_word: String,
    pub restore_confirm_word: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashProfile {
    pub rom_root: PathBuf,
    pub profile_path: PathBuf,

    pub codenames: Vec<String>,
    pub display_name: String,
    pub notes: Vec<String>,
    pub author: String,
    pub qq_group: String,
    pub build_date: String,

    pub minimum_super_bytes: u64,
    pub anti_version_file: Option<String>,
    pub erase_if_present: Vec<String>,
    pub pre_erase: Vec<String>,

    pub images_dir: PathBuf,
    pub super_dir: PathBuf,
    pub work_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub expansion_dir: Option<PathBuf>,
    pub tools_dir: PathBuf,

    pub firmware: Vec<ImageOp>,
    pub boot_chain: Vec<ImageOp>,
    pub final_data: Vec<ImageOp>,

    pub wipe_super_image: Option<String>,
    pub wipe_super_dat: Option<String>,
    pub slot_suffix: String,
    pub dynamic_partitions: Vec<DynamicPartition>,

    pub set_active_slot: Option<String>,
    pub flash_crc_lists: bool,
    pub crc_list_file: String,
    pub sparse_crc_list_file: String,
    pub require_wipe_confirmation: bool,
    pub confirmation_word: String,
    pub update_confirmation_word: String,
    pub reboot_after_success: bool,
    pub enable_root_menu: bool,
    pub post_flash_id: bool,

    pub expansion: Option<ExpansionConfig>,
}

impl FlashProfile {
    pub fn package_summary(&self) -> String {
        format!(
            "作者：{}  |  QQ群：{}  |  构建日期：{}",
            self.author, self.qq_group, self.build_date
        )
    }

    pub fn resolve_tool(&self, parts: &[&str]) -> PathBuf {
        let mut p = self.tools_dir.clone();
        for part in parts {
            p.push(part);
        }
        p
    }

    pub fn fastboot(&self) -> PathBuf {
        self.resolve_tool(&["platform-tools", "fastboot.exe"])
    }

    pub fn adb(&self) -> PathBuf {
        self.resolve_tool(&["platform-tools", "adb.exe"])
    }

    pub fn img2simg(&self) -> PathBuf {
        self.resolve_tool(&["img2simg.exe"])
    }

    pub fn cygwin_dll(&self) -> PathBuf {
        self.resolve_tool(&["cygwin1.dll"])
    }
}

pub const DEFAULT_PROFILE_REL: &str = "META-INF/com/android/flash_profile.toml";

fn validate_name(value: &str, ctx: &str) -> Result<String> {
    let re = Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_.-]*$").unwrap();
    if !re.is_match(value) {
        bail!("{} 包含非法字符：{:?}", ctx, value);
    }
    Ok(value.to_string())
}

fn validate_relative_path(value: &str, ctx: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == "./" || trimmed == ".\\" {
        bail!("{} 不能为空或指向当前目录", ctx);
    }
    let p = Path::new(trimmed);
    if p.is_absolute() || p.components().any(|c| c.as_os_str() == "..") {
        bail!("{} 必须是刷机包内的相对路径：{:?}", ctx, trimmed);
    }
    Ok(trimmed.to_string())
}

fn resolve_package_path(rom_root: &Path, value: &str, ctx: &str) -> Result<PathBuf> {
    let rel = validate_relative_path(value, ctx)?;
    let root = rom_root.canonicalize().unwrap_or_else(|_| rom_root.to_path_buf());
    let candidate = root.join(&rel);
    // Ensure candidate starts with root
    if let Ok(c_canon) = candidate.canonicalize() {
        if !c_canon.starts_with(&root) {
            bail!("{} 超出刷机包根目录：{:?}", ctx, value);
        }
    }
    Ok(candidate)
}

fn validate_device_path(value: &str, ctx: &str) -> Result<String> {
    let re = Regex::new(r"^/[A-Za-z0-9._/-]+$").unwrap();
    if !re.is_match(value) || value.contains("..") {
        bail!("{} 必须是安全的设备绝对路径：{:?}", ctx, value);
    }
    Ok(value.to_string())
}

pub fn find_rom_root(start: Option<&Path>) -> Option<PathBuf> {
    let mut search_starts = Vec::new();
    if let Some(s) = start {
        search_starts.push(s.to_path_buf());
    } else {
        if let Ok(cwd) = std::env::current_dir() {
            search_starts.push(cwd);
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                search_starts.push(parent.to_path_buf());
            }
        }
    }

    for base in search_starts {
        let mut cur = base;
        loop {
            let candidate = cur.join(DEFAULT_PROFILE_REL.replace('/', "\\"));
            if candidate.is_file() {
                return Some(cur);
            }
            if !cur.pop() {
                break;
            }
        }
    }
    None
}

pub fn resolve_profile_path(
    rom_root: Option<&Path>,
    profile: Option<&Path>,
) -> Result<(PathBuf, PathBuf)> {
    if let Some(prof) = profile {
        if !prof.is_file() {
            bail!("指定的配置文件不存在：{:?}", prof);
        }
        let prof_canon = prof.canonicalize().unwrap_or_else(|_| prof.to_path_buf());
        if let Some(root) = rom_root {
            return Ok((root.canonicalize().unwrap_or_else(|_| root.to_path_buf()), prof_canon));
        }
        // Heuristic: search META-INF ancestor
        let mut cur = prof_canon.clone();
        while cur.pop() {
            if cur.file_name().map_or(false, |n| n == "META-INF") {
                if let Some(parent) = cur.parent() {
                    return Ok((parent.to_path_buf(), prof_canon));
                }
            }
        }
        let parent = prof_canon.parent().unwrap_or(&prof_canon).to_path_buf();
        return Ok((parent, prof_canon));
    }

    let root = match rom_root {
        Some(r) => r.canonicalize().unwrap_or_else(|_| r.to_path_buf()),
        None => find_rom_root(None).ok_or_else(|| {
            anyhow!(
                "未找到 ROM 包配置。\n需要：{{ROM根}}\\{}\n请在刷机包根目录运行，或指定 --rom-root",
                DEFAULT_PROFILE_REL.replace('/', "\\")
            )
        })?,
    };

    let profile_path = root.join(DEFAULT_PROFILE_REL.replace('/', "\\"));
    if !profile_path.is_file() {
        bail!(
            "未找到刷机配置：{:?}\n请确认 META-INF\\com\\android\\flash_profile.toml 存在于刷机包内。\n当前解析的 ROM 根目录：{:?}",
            profile_path,
            root
        );
    }

    Ok((root, profile_path))
}

pub fn load_profile(rom_root: &Path, profile_path: &Path) -> Result<FlashProfile> {
    let raw = std::fs::read_to_string(profile_path)
        .with_context(|| format!("无法读取配置文件：{:?}", profile_path))?;
    let data: toml::Value = toml::from_str(&raw)
        .with_context(|| format!("TOML 解析失败：{:?}", profile_path))?;

    let root_table = data.as_table().ok_or_else(|| anyhow!("配置根必须是表"))?;

    let package = root_table.get("package").and_then(|v| v.as_table())
        .ok_or_else(|| anyhow!("配置缺少 [package] 表"))?;
    let device = root_table.get("device").and_then(|v| v.as_table())
        .ok_or_else(|| anyhow!("配置缺少 [device] 表"))?;
    let requirements = root_table.get("requirements").and_then(|v| v.as_table())
        .ok_or_else(|| anyhow!("配置缺少 [requirements] 表"))?;
    let paths = root_table.get("paths").and_then(|v| v.as_table())
        .ok_or_else(|| anyhow!("配置缺少 [paths] 表"))?;
    let stages = root_table.get("stages").and_then(|v| v.as_table())
        .ok_or_else(|| anyhow!("配置缺少 [stages] 表"))?;
    let dynamic = root_table.get("dynamic").and_then(|v| v.as_table())
        .ok_or_else(|| anyhow!("配置缺少 [dynamic] 表"))?;
    let options = root_table.get("options").and_then(|v| v.as_table());
    let expansion = root_table.get("expansion").and_then(|v| v.as_table());

    let author = package.get("author").and_then(|v| v.as_str()).unwrap_or("").trim();
    if author.is_empty() {
        bail!("[package].author 不能为空");
    }
    let qq_group = package.get("qq_group").and_then(|v| v.as_str()).unwrap_or("").trim();
    if qq_group.is_empty() {
        bail!("[package].qq_group 不能为空");
    }
    let build_date = package.get("build_date").and_then(|v| v.as_str()).unwrap_or("").trim();
    if build_date.is_empty() {
        bail!("[package].build_date 不能为空");
    }
    // Validate YYYY-MM-DD
    let date_re = Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap();
    if !date_re.is_match(build_date) {
        bail!("[package].build_date 必须是 YYYY-MM-DD 日期格式");
    }

    let codenames_val = device.get("codenames").and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("[device].codenames 必须是非空字符串数组"))?;
    if codenames_val.is_empty() {
        bail!("[device].codenames 必须是非空字符串数组");
    }
    let mut codenames = Vec::new();
    for c in codenames_val {
        let s = c.as_str().ok_or_else(|| anyhow!("[device].codenames 必须是字符串数组"))?.trim();
        if s.is_empty() {
            bail!("[device].codenames 项不能为空");
        }
        codenames.push(validate_name(s, "[device].codenames")?);
    }

    let display_name = device.get("display_name").and_then(|v| v.as_str())
        .unwrap_or("HarusoraFlashTool").trim().to_string();

    let mut notes = Vec::new();
    if let Some(arr) = device.get("notes").and_then(|v| v.as_array()) {
        for n in arr {
            if let Some(ns) = n.as_str() {
                if !ns.trim().is_empty() {
                    notes.push(ns.trim().to_string());
                }
            }
        }
    }

    let min_super = requirements.get("minimum_super_bytes").and_then(|v| v.as_integer())
        .ok_or_else(|| anyhow!("[requirements].minimum_super_bytes 必须为正整数"))?;
    if min_super <= 0 {
        bail!("[requirements].minimum_super_bytes 必须为正整数");
    }
    let minimum_super_bytes = min_super as u64;

    let anti_version_file = requirements.get("anti_version_file").and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| validate_relative_path(s, "[requirements].anti_version_file"))
        .transpose()?;

    let mut erase_if_present = Vec::new();
    if let Some(arr) = requirements.get("erase_if_present").and_then(|v| v.as_array()) {
        for it in arr {
            let s = it.as_str().ok_or_else(|| anyhow!("[requirements].erase_if_present 必须是字符串数组"))?;
            erase_if_present.push(validate_name(s.trim(), "[requirements].erase_if_present")?);
        }
    }

    let mut pre_erase = Vec::new();
    if let Some(arr) = requirements.get("pre_erase").and_then(|v| v.as_array()) {
        for it in arr {
            let s = it.as_str().ok_or_else(|| anyhow!("[requirements].pre_erase 必须是字符串数组"))?;
            pre_erase.push(validate_name(s.trim(), "[requirements].pre_erase")?);
        }
    }

    let path_val = |key: &str, default: &str| -> Result<PathBuf> {
        let rel = paths.get(key).and_then(|v| v.as_str()).unwrap_or(default).trim();
        let rel = if rel.is_empty() { default } else { rel };
        resolve_package_path(rom_root, rel, &format!("[paths].{}", key))
    };

    let images_dir = path_val("images", "images")?;
    let super_dir = path_val("super", "super")?;
    let work_dir = path_val("work", "work")?;
    let logs_dir = path_val("logs", "logs")?;
    let tools_dir = path_val("tools", "tools")?;
    let expansion_dir = if let Some(exp) = paths.get("expansion_dir").and_then(|v| v.as_str()) {
        if !exp.trim().is_empty() {
            Some(resolve_package_path(rom_root, exp.trim(), "[paths].expansion_dir")?)
        } else {
            None
        }
    } else {
        None
    };

    let parse_ops = |stage_name: &str| -> Result<Vec<ImageOp>> {
        let mut ops = Vec::new();
        if let Some(arr) = stages.get(stage_name).and_then(|v| v.as_array()) {
            for (i, raw) in arr.iter().enumerate() {
                let tbl = raw.as_table().ok_or_else(|| anyhow!("stages.{}[{}] 必须是表", stage_name, i))?;
                let action = tbl.get("action").and_then(|v| v.as_str()).unwrap_or("").trim().to_lowercase();
                if action != "flash" && action != "erase" {
                    bail!("stages.{}[{}].action 必须是 flash 或 erase，收到：{:?}", stage_name, i, action);
                }
                let partition = tbl.get("partition").and_then(|v| v.as_str()).unwrap_or("").trim();
                if partition.is_empty() {
                    bail!("stages.{}[{}].partition 不能为空", stage_name, i);
                }
                let partition = validate_name(partition, &format!("stages.{}[{}].partition", stage_name, i))?;

                let file_name = if action == "flash" {
                    let f = tbl.get("file").and_then(|v| v.as_str()).unwrap_or("").trim();
                    if f.is_empty() {
                        bail!("stages.{}[{}] flash 必须指定 file", stage_name, i);
                    }
                    Some(validate_relative_path(f, &format!("stages.{}[{}].file", stage_name, i))?)
                } else {
                    tbl.get("file").and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
                };

                // Safety: block lock actions
                let blob = format!("{} {} {}", action, partition, file_name.as_deref().unwrap_or("")).to_lowercase();
                if blob.contains("lock") && !blob.contains("unlock") {
                    bail!("拒绝可能的锁 BL 操作：stages.{}[{}]", stage_name, i);
                }

                ops.push(ImageOp { action, partition, file: file_name });
            }
        }
        Ok(ops)
    };

    let firmware = parse_ops("firmware")?;
    let boot_chain = parse_ops("boot_chain")?;
    let final_data = parse_ops("final_data")?;

    let has_wipe_img = dynamic.contains_key("wipe_super_image");
    let has_wipe_dat = dynamic.contains_key("wipe_super_dat");
    if has_wipe_img && has_wipe_dat {
        bail!("[dynamic].wipe_super_image 与 [dynamic].wipe_super_dat 只能配置一个");
    }

    let (wipe_super_image, wipe_super_dat) = if has_wipe_dat {
        let name = dynamic.get("wipe_super_dat").and_then(|v| v.as_str()).unwrap_or("").trim();
        if name.is_empty() {
            bail!("[dynamic].wipe_super_dat 不能为空");
        }
        (None, Some(validate_name(name, "[dynamic].wipe_super_dat")?))
    } else {
        let name = dynamic.get("wipe_super_image").and_then(|v| v.as_str()).unwrap_or("super_empty_16g.img").trim();
        (Some(validate_relative_path(name, "[dynamic].wipe_super_image")?), None)
    };

    let slot_suffix = dynamic.get("slot_suffix").and_then(|v| v.as_str()).unwrap_or("_a").trim().to_string();
    if slot_suffix != "" && slot_suffix != "_a" && slot_suffix != "_b" {
        bail!("[dynamic].slot_suffix 只能是空字符串、_a 或 _b");
    }

    let mut dynamic_partitions = Vec::new();
    if let Some(arr) = dynamic.get("partitions").and_then(|v| v.as_array()) {
        for (i, p) in arr.iter().enumerate() {
            let tbl = p.as_table().ok_or_else(|| anyhow!("dynamic.partitions[{}] 必须是表", i))?;
            let name = tbl.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
            if name.is_empty() {
                bail!("dynamic.partitions[{}].name 不能为空", i);
            }
            let name = validate_name(name, &format!("dynamic.partitions[{}].name", i))?;
            let dat_bytes = tbl.get("dat_bytes").and_then(|v| v.as_integer())
                .ok_or_else(|| anyhow!("dynamic.partitions[{}].dat_bytes 必须为正整数", i))?;
            if dat_bytes <= 0 {
                bail!("dynamic.partitions[{}].dat_bytes 必须为正整数", i);
            }
            let pad_to_bytes = tbl.get("pad_to_bytes").and_then(|v| v.as_integer()).map(|b| b as u64);
            if let Some(pad) = pad_to_bytes {
                if pad < (dat_bytes as u64) {
                    bail!("dynamic.partitions[{}].pad_to_bytes 不能小于 dat_bytes", i);
                }
            }
            dynamic_partitions.push(DynamicPartition {
                name,
                dat_bytes: dat_bytes as u64,
                pad_to_bytes,
            });
        }
    }

    if firmware.is_empty() && boot_chain.is_empty() && dynamic_partitions.is_empty() {
        bail!("配置中没有任何刷写阶段（firmware / boot_chain / dynamic）");
    }

    let opt_bool = |key: &str, default: bool| -> Result<bool> {
        if let Some(opts) = options {
            if let Some(val) = opts.get(key) {
                return val.as_bool().ok_or_else(|| anyhow!("[options].{} 必须是布尔值", key));
            }
        }
        Ok(default)
    };

    let set_active_slot = options.and_then(|o| o.get("set_active_slot"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(ref s) = set_active_slot {
        if s != "a" && s != "b" {
            bail!("[options].set_active_slot 只能是 a、b 或空值");
        }
    }

    let crc_list_file = options.and_then(|o| o.get("crc_list_file")).and_then(|v| v.as_str()).unwrap_or("crclist.txt").trim().to_string();
    let sparse_crc_list_file = options.and_then(|o| o.get("sparse_crc_list_file")).and_then(|v| v.as_str()).unwrap_or("sparsecrclist.txt").trim().to_string();
    validate_relative_path(&crc_list_file, "[options].crc_list_file")?;
    validate_relative_path(&sparse_crc_list_file, "[options].sparse_crc_list_file")?;

    let flash_crc_lists = opt_bool("flash_crc_lists", true)?;
    let require_wipe_confirmation = opt_bool("require_wipe_confirmation", true)?;
    let confirmation_word = options.and_then(|o| o.get("confirmation_word")).and_then(|v| v.as_str()).unwrap_or("WIPE").trim().to_string();
    let update_confirmation_word = options.and_then(|o| o.get("update_confirmation_word")).and_then(|v| v.as_str()).unwrap_or("UPDATE").trim().to_string();
    let reboot_after_success = opt_bool("reboot_after_success", true)?;
    let enable_root_menu = opt_bool("enable_root_menu", true)?;
    let post_flash_id = opt_bool("post_flash_id", false)?;

    // Parse expansion
    let expansion_cfg = if let Some(exp_tbl) = expansion {
        let enabled = exp_tbl.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
        if enabled {
            let exp_codes = if let Some(c_arr) = exp_tbl.get("codenames").and_then(|v| v.as_array()) {
                let mut list = Vec::new();
                for c in c_arr {
                    let s = c.as_str().ok_or_else(|| anyhow!("[expansion].codenames 必须是字符串数组"))?.trim();
                    list.push(validate_name(s, "[expansion].codenames")?);
                }
                list
            } else {
                codenames.clone()
            };

            let target = exp_tbl.get("target_bytes").and_then(|v| v.as_integer()).unwrap_or(min_super);
            if target <= 0 {
                bail!("[expansion].target_bytes 必须为正整数");
            }

            let disk = validate_device_path(
                exp_tbl.get("disk").and_then(|v| v.as_str()).unwrap_or("/dev/block/sda").trim(),
                "[expansion].disk",
            )?;

            let super_part = exp_tbl.get("super_part").and_then(|v| v.as_integer()).unwrap_or(32) as u32;
            let userdata_part = exp_tbl.get("userdata_part").and_then(|v| v.as_integer()).unwrap_or(33) as u32;
            if super_part == 0 || userdata_part == 0 || super_part == userdata_part {
                bail!("[expansion] 分区号必须为不同的正整数");
            }

            let backup_rel = exp_tbl.get("backup_dir")
                .or_else(|| exp_tbl.get("backup_subdir"))
                .and_then(|v| v.as_str())
                .unwrap_or("backup").trim();
            let mut sgdisk_rel = exp_tbl.get("sgdisk_dir")
                .or_else(|| exp_tbl.get("sgdisk_subdir"))
                .and_then(|v| v.as_str())
                .unwrap_or("tools/sgdisk-arm64").trim();
            if sgdisk_rel == "sgdisk-arm64" {
                sgdisk_rel = "tools/sgdisk-arm64";
            }

            let backup_dir = resolve_package_path(rom_root, backup_rel, "[expansion].backup_dir")?;
            let mut sgdisk_dir = resolve_package_path(rom_root, sgdisk_rel, "[expansion].sgdisk_dir")?;
            if !sgdisk_dir.join("sgdisk").is_file() {
                if let Some(ref e_dir) = expansion_dir {
                    let legacy = e_dir.join("sgdisk-arm64");
                    if legacy.join("sgdisk").is_file() {
                        sgdisk_dir = legacy;
                    }
                }
            }

            let remote_sgdisk_dir = validate_device_path(
                exp_tbl.get("remote_sgdisk_dir").and_then(|v| v.as_str()).unwrap_or("/tmp/harusora-sgdisk").trim(),
                "[expansion].remote_sgdisk_dir",
            )?;

            let confirm_word = exp_tbl.get("confirm_word").and_then(|v| v.as_str()).unwrap_or("EXPAND").trim().to_string();
            let restore_confirm_word = exp_tbl.get("restore_confirm_word").and_then(|v| v.as_str()).unwrap_or("RESTORE").trim().to_string();

            Some(ExpansionConfig {
                enabled: true,
                codenames: exp_codes,
                target_bytes: target as u64,
                disk,
                super_part,
                userdata_part,
                backup_dir,
                sgdisk_dir,
                remote_sgdisk_dir,
                confirm_word,
                restore_confirm_word,
            })
        } else {
            None
        }
    } else {
        None
    };

    Ok(FlashProfile {
        rom_root: rom_root.to_path_buf(),
        profile_path: profile_path.to_path_buf(),
        codenames,
        display_name,
        notes,
        author: author.to_string(),
        qq_group: qq_group.to_string(),
        build_date: build_date.to_string(),
        minimum_super_bytes,
        anti_version_file,
        erase_if_present,
        pre_erase,
        images_dir,
        super_dir,
        work_dir,
        logs_dir,
        expansion_dir,
        tools_dir,
        firmware,
        boot_chain,
        final_data,
        wipe_super_image,
        wipe_super_dat,
        slot_suffix,
        dynamic_partitions,
        set_active_slot,
        flash_crc_lists,
        crc_list_file,
        sparse_crc_list_file,
        require_wipe_confirmation,
        confirmation_word,
        update_confirmation_word,
        reboot_after_success,
        enable_root_menu,
        post_flash_id,
        expansion: expansion_cfg,
    })
}
