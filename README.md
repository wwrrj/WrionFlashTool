# Harusora FlashTool

Harusora FlashTool 是一个面向 Windows 的 Android 刷机桌面软件，使用 Rust、Tauri 2、React 和 TypeScript 构建。它根据 ROM 包中的 `flash_profile.toml` 执行 fastboot、fastbootd 和 adb 操作，支持完整线刷、系统更新、ID 补丁、Root 镜像和动态分区处理。

> 刷机可能清空数据，甚至导致设备无法启动。开始前请解锁 Bootloader、备份重要资料，并确认 ROM 包明确支持你的设备。本工具不会执行锁定 Bootloader 的操作。

## 普通用户

### ROM 目录

发布者需要提供完整的 ROM 目录。可执行文件应放在 ROM 根目录：

```text
ROM 根目录/
├─ HarusoraFlashTool.exe
├─ META-INF/
│  └─ com/android/flash_profile.toml
├─ images/                         固件、启动镜像和辅助文件
├─ super/                          *.new.dat 与 *.transfer.list
└─ tools/
   ├─ platform-tools/adb.exe
   ├─ platform-tools/fastboot.exe
   ├─ img2simg.exe
   └─ cygwin1.dll                  如工具链需要
```

双击 `HarusoraFlashTool.exe` 启动软件。工具会从当前程序目录或父目录查找 `META-INF/com/android/flash_profile.toml`。

### 使用流程

1. 首次启动时阅读并同意使用说明。选择会保存在本机。
2. 在“选择设备与刷入方式”中确认设备、目标机型和刷入包。
3. 选择“完整线刷”或“系统更新”。完整线刷会清理用户数据，系统更新保留用户数据。
4. 按需启用 ID 补丁、Root 镜像和刷入完成后自动重启。
5. 工具进入准备阶段，检查设备代号、Super 容量、防回滚信息，并自动切换到合适的 fastboot 模式。
6. 在刷入页面查看阶段进度和最近输出。完整日志可以在“运行日志”页面查看。
7. 刷入完成后等待设备首次启动，不要在设备仍处于刷写状态时拔出 USB。

## 支持的操作

- **完整线刷**：写入固件、启动链、动态分区和数据相关操作，并要求输入确认词。
- **系统更新**：跳过 `userdata` 和 `metadata` 清理，保留用户数据。
- **ID 补丁**：刷入配置启用的 ColorOS `mi_ext` 补丁。
- **Root 镜像**：刷入已修补的 `boot` 或 `init_boot` 镜像，并校验镜像格式和分区容量。
- **物理扩容**：在 ROM 配置启用时，通过 Recovery 和 `sgdisk` 执行分区扩容及官方分区表恢复。
- **日志记录**：fastboot、adb、阶段事件和分区转换进度都会写入配置指定的日志目录。

## 开发环境

Windows 开发需要：

- Windows 10 或更高版本
- WebView2 Runtime
- Visual Studio 2022 Build Tools，包含 MSVC 和 Windows SDK
- Rust stable toolchain
- Node.js、pnpm

### 安装依赖

```powershell
pnpm --dir frontend install
```

### 启动开发版本

```powershell
cargo tauri dev --manifest-path src-tauri/Cargo.toml
```

也可以单独启动前端预览：

```powershell
pnpm --dir frontend dev
```

### 构建 Windows 可执行文件

在仓库根目录执行：

```powershell
.\build.ps1
```

构建脚本会先编译前端，再生成 Rust/Tauri Release 二进制。输出位置：

```text
dist/HarusoraFlashTool.exe
HarusoraFlashTool.exe
```

Release 桌面程序不会启动额外的控制台窗口。Rust 单元测试可以单独运行：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
```

## 项目结构

```text
frontend/              React/Tauri 前端和界面样式
src-tauri/src/         Rust 刷机、设备、配置和 Tauri 命令
src-tauri/tests/       Rust 核心单元测试
build.ps1              Windows Release 构建脚本
DESIGN-cal.md          视觉设计参考文件
```

项目不再包含旧的 Python 刷机实现、Python 依赖或 Python 测试套件。Rust/Tauri GUI 是唯一的桌面实现。
