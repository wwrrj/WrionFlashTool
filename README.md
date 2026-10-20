# WrionFlashTool

面向 Windows 的 Android 刷机工具，基于 Rust、Tauri 2、React 和 TypeScript 构建。

WrionFlashTool 读取 ROM 包中的刷机配置，组织设备检查、分区写入与日志记录。支持 FlashProfile 刷机包（配置驱动）和小米官方线刷目录，并提供 ID 补丁、Root 镜像及动态分区处理入口。

## 功能

- **刷机向导**：选择 ROM、连接设备、核对计划、执行刷入并查看结果。
- **完整线刷与保留数据更新**：根据包内配置和脚本提供可用操作。
- **ROM 解压**：读取 ZIP 或已解压目录，支持复用已有结果与中断恢复。
- **独立工具**：刷入 ID 补丁、Root 镜像，以及配置启用的 Super 扩容与恢复。
- **设备支持**：内嵌 platform-tools，检测缺失的 Android USB 驱动并提供安装入口。
- **日志**：记录设备命令、处理阶段和分区转换进度。

完整线刷会清除数据。使用前应解锁 Bootloader、备份资料，并确认刷机包支持当前机型。工具拒绝执行锁定 Bootloader 的命令。

## 使用

1. 启动桌面程序，选择 ROM ZIP 或已解压目录。
2. 用 USB 连接设备，核对识别出的机型与连接模式。
3. 选择完整线刷或保留数据更新，检查高级选项和执行计划。
4. 按提示确认后开始刷入，等待完成并查看日志。

刷入完成后的自动重启以界面选项为准。ID 补丁和 Super 扩容也可从独立工具入口操作；未由当前配置启用的功能不会提供执行入口。

### FlashProfile 刷机包

程序查找 `META-INF/com/android/flash_profile.toml`，据此确定设备匹配、镜像映射、操作选项与所需工具。也可将程序放在已解压的 ROM 根目录运行。

```text
ROM/
├── META-INF/com/android/flash_profile.toml
├── images/
└── tools/
```

固件布局与可用操作以实际配置为准。完整的外部 platform-tools 可放在程序旁的 `tools/platform-tools/` 或 `platform-tools/`；否则使用内嵌版本。

### 小米官方线刷包

选择包含 `flash_all.bat` 和 `images/` 的已解压目录。程序解析受支持的脚本语句，执行机型与防回滚检查、条件分支和刷写操作，不直接运行 BAT。

完整线刷对应 `flash_all.bat`；保留数据更新对应 `flash_all_except_storage.bat`。若更新脚本仍改写用户数据分区，则不提供保留数据选项。未知语句、包外路径和锁定命令会导致处理失败。

### Super 扩容与恢复

仅在 ROM 配置显式启用时可用。扩容前备份 GPT，备份校验通过后才修改分区。恢复时核对备份结构、磁盘与机型，并在完成后检查分区范围。

## 从源码构建

需要 Windows 10 或更高版本、WebView2 Runtime，以及：

- Rust stable，使用 MSVC 工具链。
- Visual Studio 2022 Build Tools，包含 C++ 工具集和 Windows SDK。
- Node.js 与 pnpm。
- Tauri 2 CLI。

```powershell
git clone https://github.com/wwrrj/WrionFlashTool.git
cd WrionFlashTool
pnpm --dir frontend install --frozen-lockfile
cargo install tauri-cli --version '^2' --locked
./build.ps1
```

脚本先构建前端，再编译 Tauri Release 程序，将主程序复制到 `dist/` 和仓库根目录，并输出实际文件名。发布构建内嵌前端、platform-tools 和 USB 驱动资源。

内嵌工具是源码构建输入，位于 `src-tauri/resources/`，须保留原始字节及随附的许可声明。构建产物、依赖缓存和 ROM 数据不进入版本控制。

## 开发与验证

单独预览前端：

```powershell
pnpm --dir frontend dev
```

启动桌面开发模式：

```powershell
cd src-tauri
cargo tauri dev
```

从仓库根目录运行构建与测试：

```powershell
pnpm --dir frontend build
pnpm --dir frontend lint
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

浏览器预览使用演示数据。真实设备识别、USB 驱动安装和刷入行为需要在桌面程序中分别验证。

## 目录

| 路径 | 内容 |
| --- | --- |
| `frontend/src/` | 向导页面、交互组件、状态与事件订阅 |
| `src-tauri/src/` | 设备、配置、刷写、分区处理与 Tauri 命令 |
| `src-tauri/resources/` | 内嵌 platform-tools 与 USB 驱动 |
| `src-tauri/tests/` | Rust 测试与官方脚本夹具 |
| `build.ps1` | Windows Release 构建入口 |

界面开发见 [前端说明](frontend/README.md)。ROM 构建与方案维护分别由 [WrionTools](https://github.com/wwrrj/WrionTools) 和 [WrionSchemes](https://github.com/wwrrj/WrionSchemes) 负责。

报告问题时请包含程序版本、Windows 版本、设备代号、包格式、所选操作、复现步骤和相关日志。提交日志前移除设备序列号与私人数据。
