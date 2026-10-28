//! 读取主板 ID：高通芯片序列号 /sys/devices/soc0/serial_number。
//!
//! 很多工具只要某一步报错就什么也读不出来，用户不知道卡在哪里。这里把读取拆成几步依次检查：
//! 准备 ADB → 启动 ADB 服务 → 查找手机 → USB 调试授权 → （Root 授权）→ 读取节点。
//! 哪一步没通过就停在那一步，返回原因、解决步骤，以及界面上可以一键执行的操作。
//! 整个过程只执行查看类命令，不会改动手机。

use std::path::{Path, PathBuf};
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::device::{run_process_timeout, ProcessResult, TIMEOUT_CODE};
use crate::logger::Logger;
use crate::platform_tools;
use crate::usb_driver;

/// 依次尝试的节点；后两个在部分内核上是同一个节点的另一条路径。
const NODES: &[&str] = &[
    "/sys/devices/soc0/serial_number",
    "/sys/devices/system/soc/soc0/serial_number",
    "/sys/bus/soc/devices/soc0/serial_number",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadMethod {
    /// 普通 ADB Shell 权限（Recovery 下通常已是 root）。
    Shell,
    /// 通过 su 获取 Root 权限读取。
    Root,
}

/// 界面上可以一键执行的解决操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixAction {
    /// 改用 Root 权限重新读取。
    RetryRoot,
    /// 安装内置 Android USB 驱动。
    InstallDriver,
    /// 结束并重新启动 ADB 服务。
    RestartAdb,
    /// 手机在 Fastboot 模式，重启进系统。
    RebootSystem,
}

/// 已执行的检查项，按顺序显示给用户。
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub label: String,
    pub ok: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Problem {
    /// 机器可读的错误类型，界面据此决定是否自动重试。
    pub code: &'static str,
    pub title: String,
    pub detail: String,
    pub steps: Vec<String>,
    pub actions: Vec<FixAction>,
    /// 出错命令的原始输出，方便发给别人排查。
    pub raw: Option<String>,
}

impl Problem {
    fn new(code: &'static str, title: &str, detail: impl Into<String>, steps: &[&str]) -> Self {
        Self {
            code,
            title: title.to_string(),
            detail: detail.into(),
            steps: steps.iter().map(|s| s.to_string()).collect(),
            actions: Vec::new(),
            raw: None,
        }
    }

    fn action(mut self, action: FixAction) -> Self {
        self.actions.push(action);
        self
    }

    /// 普通权限读不到、且还没用过 Root 时，提供“用 Root 权限读取”按钮。
    fn with_root(self, state: &str, via_root: bool) -> Self {
        if !via_root && state != "recovery" { self.action(FixAction::RetryRoot) } else { self }
    }

    fn raw(mut self, text: &str) -> Self {
        let text = text.trim();
        if !text.is_empty() {
            self.raw = Some(text.to_string());
        }
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AdbDeviceState {
    pub serial: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoardInfo {
    /// 节点原始内容（十进制）。
    pub serial_number: String,
    /// 十六进制形式，例如 0x1a2b3c4d。
    pub hex: String,
    pub node: String,
    pub adb_serial: String,
    /// device（系统）或 recovery。
    pub mode: String,
    pub model: Option<String>,
    pub device: Option<String>,
    pub via_root: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoardIdOutcome {
    pub checks: Vec<Check>,
    /// ADB 能看到的全部手机，多于一台时界面让用户选择。
    pub devices: Vec<AdbDeviceState>,
    pub info: Option<BoardInfo>,
    pub problem: Option<Problem>,
}

struct Tools {
    adb: PathBuf,
    fastboot: PathBuf,
}

struct Reader<'a> {
    logger: &'a Logger,
    checks: Vec<Check>,
    devices: Vec<AdbDeviceState>,
}

impl Reader<'_> {
    fn pass(&mut self, label: &str, detail: Option<String>) {
        self.logger.write(&format!("  ✓ {}{}", label, detail.as_deref().map(|d| format!("：{}", d)).unwrap_or_default()));
        self.checks.push(Check { label: label.to_string(), ok: true, detail });
    }

    fn fail(&mut self, label: &str, problem: Problem) -> Problem {
        self.logger.write(&format!("  ✗ {}：{}", label, problem.title));
        self.checks.push(Check { label: label.to_string(), ok: false, detail: Some(problem.title.clone()) });
        problem
    }

    fn run(&self, exe: &Path, args: &[&str], secs: u64) -> ProcessResult {
        run_process_timeout(exe, args, Some(self.logger), Duration::from_secs(secs)).unwrap_or_else(|e| ProcessResult {
            code: -1,
            text: format!("{:#}", e),
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    fn shell(&self, tools: &Tools, serial: &str, command: &str, secs: u64) -> ProcessResult {
        self.run(&tools.adb, &["-s", serial, "shell", command], secs)
    }

    fn getprop(&self, tools: &Tools, serial: &str, name: &str) -> Option<String> {
        let res = self.shell(tools, serial, &format!("getprop {}", name), 10);
        let value = res.stdout.trim();
        (res.code == 0 && !value.is_empty()).then(|| value.to_string())
    }

    fn read(&mut self, chosen: Option<&str>, method: ReadMethod) -> Result<BoardInfo, Problem> {
        let tools = self.prepare_tools()?;
        self.start_server(&tools)?;
        let (serial, state) = self.find_device(&tools, chosen)?;
        self.check_authorized(&serial, &state)?;

        let via_root = method == ReadMethod::Root && state != "recovery";
        if via_root {
            self.check_root(&tools, &serial)?;
        }

        let model = self.getprop(&tools, &serial, "ro.product.model");
        let device = self.getprop(&tools, &serial, "ro.product.device");
        let (node, value) = self.read_node(&tools, &serial, &state, via_root)?;
        let hex = format!("0x{:x}", value);
        self.pass("读取主板 ID", Some(format!("{}（{}）", value, hex)));
        Ok(BoardInfo {
            serial_number: value.to_string(),
            hex,
            node,
            adb_serial: serial,
            mode: state,
            model,
            device,
            via_root,
        })
    }

    fn prepare_tools(&mut self) -> Result<Tools, Problem> {
        match platform_tools::tools_dir() {
            Ok(dir) => {
                self.pass("准备 ADB 工具", None);
                Ok(Tools { adb: dir.join("adb.exe"), fastboot: dir.join("fastboot.exe") })
            }
            Err(e) => Err(self.fail("准备 ADB 工具", Problem::new(
                "tools",
                "无法准备内置的 ADB 工具",
                format!("{:#}", e),
                &[
                    "杀毒软件可能拦截了 adb.exe：请把 Wrion FlashTool 加入信任列表后重试",
                    "确认 C 盘还有剩余空间",
                ],
            ))),
        }
    }

    fn start_server(&mut self, tools: &Tools) -> Result<(), Problem> {
        let res = self.run(&tools.adb, &["start-server"], 30);
        if res.code == 0 {
            self.pass("启动 ADB 服务", None);
            return Ok(());
        }
        let mut detail = "ADB 服务没能启动。最常见的原因是其他软件（手机助手、安卓模拟器等）自带的 ADB 占用了 5037 端口，或者与本工具的 ADB 版本冲突。".to_string();
        if let Some(owner) = port_owner(5037) {
            detail.push_str(&format!("\n当前占用 5037 端口的程序：{}", owner));
        }
        Err(self.fail("启动 ADB 服务", Problem::new(
            "adb_server",
            "ADB 服务启动失败",
            detail,
            &[
                "关闭手机助手类软件：360 手机助手、豌豆荚、应用宝、爱思助手、小米手机助手等",
                "关闭安卓模拟器：雷电、MuMu、夜神、逍遥、BlueStacks 等",
                "仍然不行：打开任务管理器，结束所有 adb.exe 进程",
                "然后点“重启 ADB 服务”，再重新读取",
            ],
        ).action(FixAction::RestartAdb).raw(&res.text)))
    }

    fn find_device(&mut self, tools: &Tools, chosen: Option<&str>) -> Result<(String, String), Problem> {
        let res = self.run(&tools.adb, &["devices"], 20);
        self.devices = parse_adb_devices(&res.stdout);

        if self.devices.is_empty() {
            let problem = self.no_device_problem(tools, &res.text);
            return Err(self.fail("查找手机", problem));
        }

        let usable = |d: &&AdbDeviceState| matches!(d.state.as_str(), "device" | "recovery");
        let picked = chosen
            .and_then(|s| self.devices.iter().find(|d| d.serial == s))
            .or_else(|| self.devices.iter().find(usable))
            .unwrap_or(&self.devices[0])
            .clone();
        let detail = if self.devices.len() > 1 {
            format!("发现 {} 台手机，使用 {}", self.devices.len(), picked.serial)
        } else {
            picked.serial.clone()
        };
        self.pass("查找手机", Some(detail));
        Ok((picked.serial, picked.state))
    }

    fn no_device_problem(&self, tools: &Tools, raw: &str) -> Problem {
        // 手机在 Fastboot 模式：ADB 看不到，但 fastboot 能看到
        let fb = self.run(&tools.fastboot, &["devices"], 10);
        if fb.stdout.lines().any(|l| l.contains("fastboot")) {
            return Problem::new(
                "in_fastboot",
                "手机处于 Fastboot 模式",
                "Fastboot 模式下无法读取主板 ID。需要让手机开机进入系统（或进入第三方 Recovery），再通过 ADB 读取。",
                &[
                    "点“重启进系统”，或长按电源键约 10 秒让手机重启",
                    "开机后保持亮屏解锁，工具会自动重新读取",
                ],
            ).action(FixAction::RebootSystem).raw(&fb.text);
        }

        let driver = usb_driver::status();
        if !driver.missing_devices.is_empty() {
            return Problem::new(
                "driver_missing",
                "电脑缺少手机的 USB 驱动",
                format!(
                    "Windows 已经检测到手机（{}），但没有可用的驱动，所以 ADB 看不到它。",
                    driver.missing_devices.join("、")
                ),
                &[
                    "点“安装 USB 驱动”，Windows 弹出授权窗口时点“是”",
                    "安装完成后重新插拔数据线",
                ],
            ).action(FixAction::InstallDriver);
        }

        let mut problem = Problem::new(
            "no_device",
            "没有找到打开了 USB 调试的手机",
            "ADB 没有看到任何手机。读取主板 ID 需要手机开机并打开 USB 调试（或处于第三方 Recovery）。",
            &[
                "打开开发者选项：设置 → 我的设备 → 全部参数与信息 → 连续点击“OS 版本”（或“MIUI 版本”）7 次",
                "打开 USB 调试：设置 → 更多设置 → 开发者选项 → 打开“USB 调试”",
                "插上数据线后下拉通知栏，把 USB 用途改为“传输文件”",
                "仍然没反应：换原装数据线，插电脑机箱后面的 USB 口，不要用扩展坞",
            ],
        ).raw(raw);
        if !driver.installed {
            problem.steps.push("这台电脑还没有安装 Android USB 驱动，可以先点“安装 USB 驱动”".to_string());
            problem = problem.action(FixAction::InstallDriver);
        }
        problem
    }

    fn check_authorized(&mut self, serial: &str, state: &str) -> Result<(), Problem> {
        let problem = match state {
            "device" => {
                self.pass("USB 调试授权", Some("系统已开机".to_string()));
                return Ok(());
            }
            "recovery" => {
                self.pass("USB 调试授权", Some("Recovery 模式".to_string()));
                return Ok(());
            }
            "unauthorized" | "authorizing" => Problem::new(
                "unauthorized",
                "手机还没有允许这台电脑调试",
                format!("手机 {} 已连接，但 USB 调试授权还没通过。", serial),
                &[
                    "解锁手机屏幕，弹出“允许 USB 调试吗？”时，勾选“一律允许使用这台计算机进行调试”，再点“允许”",
                    "没看到弹窗：拔掉数据线 → 开发者选项里点“撤销 USB 调试授权” → 重新插上",
                    "点完允许后工具会自动重新读取",
                ],
            ),
            "offline" => Problem::new(
                "offline",
                "手机连接状态为“离线”",
                "ADB 能看到手机，但无法和它通信。常见于数据线接触不良、多个 ADB 程序抢占，或授权过期。",
                &[
                    "重新插拔数据线，或换一根数据线、换一个 USB 口",
                    "关闭手机助手、安卓模拟器等也会使用 ADB 的软件",
                    "点“重启 ADB 服务”后重新读取",
                    "仍然离线：开发者选项里点“撤销 USB 调试授权”，再重新连接并点允许",
                ],
            ).action(FixAction::RestartAdb),
            "sideload" => Problem::new(
                "sideload",
                "手机处于 Sideload（线刷 / 卡刷）模式",
                "这个模式只能接收刷机包，不能执行命令，无法读取主板 ID。",
                &[
                    "在手机上退出 Sideload，回到系统或第三方 Recovery 的主界面",
                    "小米官方 Recovery 的“连接小米助手”模式也不支持，需要开机进系统",
                ],
            ),
            other => Problem::new(
                "bad_state",
                "手机处于无法读取的模式",
                format!("ADB 报告的状态是“{}”。", other),
                &["让手机正常开机进入系统并保持解锁，然后重新读取"],
            ),
        };
        Err(self.fail("USB 调试授权", problem))
    }

    fn check_root(&mut self, tools: &Tools, serial: &str) -> Result<(), Problem> {
        self.logger.write("  请求 Root 授权（请留意手机上的弹窗）…");
        let res = self.shell(tools, serial, "su -c id", 30);
        if res.text.contains("uid=0") {
            self.pass("Root 授权", None);
            return Ok(());
        }
        let lower = res.text.to_lowercase();
        let problem = if res.code == TIMEOUT_CODE {
            Problem::new(
                "root_timeout",
                "手机上的 Root 授权没有确认",
                "已向手机请求 Root 权限，但 30 秒内没有得到允许。",
                &[
                    "重新读取时留意手机屏幕，Magisk / KernelSU / APatch 弹出授权时点“允许”",
                    "没有弹窗：打开 Root 管理器 → 超级用户，找到“Shell”并打开授权",
                ],
            )
        } else if lower.contains("not found") || lower.contains("inaccessible") {
            Problem::new(
                "no_root",
                "手机没有 Root",
                "手机上找不到 su 命令，无法用 Root 权限读取。",
                &[
                    "可以改用第三方 Recovery：重启进 TWRP / OrangeFox，连上电脑后用“普通读取”，Recovery 下 ADB 默认就有读取权限",
                    "没有刷第三方 Recovery 的话，也可以用 fastboot boot 临时启动适配本机型的 TWRP 镜像（不会写入手机）",
                ],
            )
        } else {
            Problem::new(
                "root_denied",
                "Root 授权被拒绝",
                "Root 管理器拒绝了 Shell 的 Root 请求。",
                &[
                    "打开 Magisk / KernelSU / APatch → 超级用户，找到“Shell”并允许",
                    "然后重新读取，手机弹出授权时点“允许”",
                ],
            )
        };
        Err(self.fail("Root 授权", problem.raw(&res.text)))
    }

    fn read_node(&mut self, tools: &Tools, serial: &str, state: &str, via_root: bool) -> Result<(String, u64), Problem> {
        let mut denied: Option<(String, String)> = None;
        let mut missing = Vec::new();

        for node in NODES {
            let command = if via_root { format!("su -c 'cat {}'", node) } else { format!("cat {}", node) };
            let res = self.shell(tools, serial, &command, 20);
            let text = res.text.trim();
            let lower = text.to_lowercase();

            // 只解析 stdout：su 可能在 stderr 里打印额外的警告
            let out = res.stdout.trim();
            if res.code == 0 && !out.is_empty() && !lower.contains("no such file") && !lower.contains("permission denied") {
                return match parse_serial(out) {
                    Some(0) => Err(self.fail("读取主板 ID", Problem::new(
                        "zero_value",
                        "节点内容为 0",
                        format!("{} 读出来是 0，说明这个系统隐藏了真实的主板 ID。", node),
                        &read_elsewhere_steps(state, via_root),
                    ).raw(text).with_root(state, via_root))),
                    Some(value) => Ok((node.to_string(), value)),
                    None => Err(self.fail("读取主板 ID", Problem::new(
                        "bad_value",
                        "读到的内容不是主板 ID",
                        format!("{} 的内容无法识别，请把下面的原始输出发给刷机包作者或交流群。", node),
                        &["点“查看日志”可复制完整记录"],
                    ).raw(text))),
                };
            }

            if lower.contains("no such file") {
                missing.push(node.to_string());
                continue;
            }
            if lower.contains("permission denied") {
                denied.get_or_insert((node.to_string(), text.to_string()));
                continue;
            }
            if res.code == TIMEOUT_CODE {
                return Err(self.fail("读取主板 ID", Problem::new(
                    "shell_timeout",
                    "手机没有响应读取命令",
                    "命令发送到手机后 20 秒内没有返回。",
                    &["重新插拔数据线后重试", "手机卡顿时先重启手机再读取"],
                ).action(FixAction::RestartAdb).raw(text)));
            }
            if lower.contains("not found") || lower.contains("offline") || lower.contains("unauthorized") || lower.contains("closed") {
                return Err(self.fail("读取主板 ID", Problem::new(
                    "disconnected",
                    "读取时手机断开了",
                    "读取过程中手机掉线或切换了模式。",
                    &["检查数据线是否松动，保持手机亮屏解锁", "重新读取"],
                ).raw(text)));
            }
            // 其他报错先记下，继续尝试下一个节点
            missing.push(format!("{}（{}）", node, text.lines().next().unwrap_or("无输出")));
        }

        if let Some((node, raw)) = denied {
            return Err(self.fail("读取主板 ID", Problem::new(
                "permission_denied",
                "没有权限读取主板 ID 节点",
                format!(
                    "这台手机的系统（SELinux 安全策略）不允许普通 ADB 读取 {}。这是系统限制，和数据线、驱动无关，换别的工具用普通 ADB 读也是同样结果。",
                    node
                ),
                &read_elsewhere_steps(state, via_root),
            ).raw(&raw).with_root(state, via_root)));
        }

        let platform = self.getprop(tools, serial, "ro.soc.manufacturer")
            .into_iter()
            .chain(self.getprop(tools, serial, "ro.board.platform"))
            .collect::<Vec<_>>()
            .join(" / ");
        let platform_note = if platform.is_empty() { String::new() } else { format!("（本机芯片平台：{}）", platform) };
        Err(self.fail("读取主板 ID", Problem::new(
            "not_found",
            "手机上没有主板 ID 节点",
            format!(
                "在手机上找不到 soc0/serial_number{}。这个节点只存在于高通（骁龙）芯片的手机上，联发科（天玑）等芯片没有这个主板 ID。",
                platform_note
            ),
            &[
                "确认手机是骁龙芯片",
                "确认是骁龙但仍找不到：可能是系统内核屏蔽了节点，请在第三方 Recovery 里重新读取",
            ],
        ).raw(&missing.join("\n"))))
    }
}

fn read_elsewhere_steps(state: &str, via_root: bool) -> Vec<&'static str> {
    if state == "recovery" {
        return vec![
            "当前 Recovery 也读不到：换用 TWRP / OrangeFox 等第三方 Recovery 后重试",
            "或者开机进系统，用已 Root 的系统读取",
        ];
    }
    let mut steps = Vec::new();
    if !via_root {
        steps.push("手机已 Root（Magisk / KernelSU / APatch）：点“用 Root 权限读取”，手机弹出授权时点“允许”");
    }
    steps.push("没有 Root：重启进入第三方 Recovery（TWRP / OrangeFox），连上电脑后点“重新读取”，Recovery 下 ADB 默认就有读取权限");
    steps.push("没有刷第三方 Recovery：可以用 fastboot boot 临时启动适配本机型的 TWRP 镜像（不会写入手机），再读取");
    steps
}

/// 节点内容一般是十进制；个别内核输出 0x 开头的十六进制。
fn parse_serial(text: &str) -> Option<u64> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    match line.strip_prefix("0x").or_else(|| line.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => line.parse().ok(),
    }
}

fn parse_adb_devices(stdout: &str) -> Vec<AdbDeviceState> {
    let re = Regex::new(r"^(\S+)\s+(\S+)").unwrap();
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("List of devices") && !l.starts_with('*'))
        .filter_map(|l| re.captures(l))
        .map(|c| AdbDeviceState { serial: c[1].to_string(), state: c[2].to_string() })
        .collect()
}

/// 占用某个本机端口的程序（名称和 PID），用于提示是谁抢了 ADB 端口。
#[cfg(windows)]
fn port_owner(port: u16) -> Option<String> {
    let netstat = run_process_timeout(Path::new("netstat"), &["-ano", "-p", "TCP"], None, Duration::from_secs(10)).ok()?;
    let suffix = format!(":{}", port);
    let pid = netstat.stdout.lines().find_map(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        (cols.len() >= 5 && cols[1].ends_with(&suffix) && cols[3] == "LISTENING").then(|| cols[4].to_string())
    })?;
    let filter = format!("PID eq {}", pid);
    let tasklist = run_process_timeout(Path::new("tasklist"), &["/FI", &filter, "/FO", "CSV", "/NH"], None, Duration::from_secs(10)).ok()?;
    let name = tasklist.stdout.lines().next()?.split(',').next()?.trim_matches('"').to_string();
    Some(if name.is_empty() { format!("PID {}", pid) } else { format!("{}（PID {}）", name, pid) })
}

#[cfg(not(windows))]
fn port_owner(_port: u16) -> Option<String> {
    None
}

/// 按步骤读取主板 ID；任何一步失败都会在结果里给出原因和解决办法，不会返回错误。
pub fn read_board_id(logger: &Logger, chosen: Option<&str>, method: ReadMethod) -> BoardIdOutcome {
    logger.log_step(if method == ReadMethod::Root { "读取主板 ID（Root）" } else { "读取主板 ID" });
    let mut reader = Reader { logger, checks: Vec::new(), devices: Vec::new() };
    let result = reader.read(chosen, method);
    let (info, problem) = match result {
        Ok(info) => (Some(info), None),
        Err(problem) => (None, Some(problem)),
    };
    BoardIdOutcome { checks: reader.checks, devices: reader.devices, info, problem }
}

/// 执行解决操作（安装驱动由 usb_driver 处理），返回给用户看的结果说明。
pub fn run_fix(logger: &Logger, action: FixAction, serial: Option<&str>) -> anyhow::Result<String> {
    let dir = platform_tools::tools_dir()?;
    match action {
        FixAction::RestartAdb => {
            let adb = dir.join("adb.exe");
            let _ = run_process_timeout(&adb, &["kill-server"], Some(logger), Duration::from_secs(15));
            let res = run_process_timeout(&adb, &["start-server"], Some(logger), Duration::from_secs(30))?;
            if res.code != 0 {
                let owner = port_owner(5037).map(|o| format!("\n5037 端口被 {} 占用，请先关闭它。", o)).unwrap_or_default();
                anyhow::bail!("ADB 服务仍然无法启动。{}\n{}", owner, res.text);
            }
            Ok("ADB 服务已重启".to_string())
        }
        FixAction::RebootSystem => {
            let fastboot = dir.join("fastboot.exe");
            let mut args = Vec::new();
            if let Some(s) = serial {
                args.extend(["-s", s]);
            }
            args.push("reboot");
            let res = run_process_timeout(&fastboot, &args, Some(logger), Duration::from_secs(20))?;
            if res.code != 0 {
                anyhow::bail!("重启命令失败，请长按电源键手动重启。\n{}", res.text);
            }
            Ok("已发送重启命令，请等待手机开机并解锁屏幕".to_string())
        }
        other => anyhow::bail!("这个操作不由读取工具执行：{:?}", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_serial_formats() {
        assert_eq!(parse_serial("3482937158\n"), Some(3482937158));
        assert_eq!(parse_serial("0x1A2B\r\n"), Some(0x1a2b));
        assert_eq!(parse_serial("\n  42  \n"), Some(42));
        assert_eq!(parse_serial("cat: x: Permission denied"), None);
    }

    #[test]
    fn parses_adb_states() {
        let out = "* daemon not running; starting now at tcp:5037\n* daemon started successfully\nList of devices attached\nabc123\tunauthorized\ndef456\tdevice\n\n";
        let list = parse_adb_devices(out);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].state, "unauthorized");
        assert_eq!(list[1].serial, "def456");
    }
}
