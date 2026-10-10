# Harusora FlashTool

Harusora FlashTool 是一个面向 Windows 的 Android 刷机桌面软件，使用 Rust、Tauri 2、React 和 TypeScript 构建。它根据 ROM 包中的 `flash_profile.toml` 执行 fastboot、fastbootd 和 adb 操作，支持完整线刷、系统更新、ID 补丁、Root 镜像和动态分区处理。

> 刷机可能清空数据，甚至导致设备无法启动。开始前请解锁 Bootloader、备份重要资料，并确认 ROM 包明确支持你的设备。本工具不会执行锁定 Bootloader 的操作。

## 普通用户

### 使用流程

双击 `HarusoraFlashTool.exe` 启动，按左侧的 5 个步骤操作即可：

1. **刷机包**：把 ROM 的 `.zip` 或已解压的 ROM 文件夹拖进窗口，或点击选择。文件夹会直接使用（选到上一层或子目录也能识别）；工具会读取包内的 `META-INF/com/android/flash_profile.toml`，确认后自动解压（默认解压到 zip 同目录下的同名文件夹，可更改）。同一个包已解压过会直接复用，中途中断可以从断点继续。
2. **连接手机**：用数据线连接手机，开机或 Fastboot 模式均可。工具会自动识别，并检查手机机型是否与刷机包匹配。
3. **选择方式**：“完整线刷”会清空数据；“保留数据更新”保留用户数据。ID 补丁、Root 镜像、Super 物理扩容放在“高级选项”里。
4. **开始刷入**：核对信息，完整线刷需要输入确认词。工具会先预检设备，再自动开始刷写，过程中不要拔线。
5. **完成**：等待手机首次开机。完整日志可随时在左下角“运行日志”查看。

首次启动需要逐项确认使用须知，选择会保存在本机。

### 直接放在 ROM 目录中使用

也可以像以前一样把 `HarusoraFlashTool.exe` 放在已解压的 ROM 根目录运行，工具会自动从当前目录或父目录找到刷机配置，跳过解压。ROM 目录结构：

```text
ROM 根目录/
├─ META-INF/com/android/flash_profile.toml
├─ images/                         固件、启动镜像、super/*.new.dat 等
└─ tools/
   ├─ platform-tools/adb.exe
   ├─ platform-tools/fastboot.exe
   ├─ img2simg.exe
   └─ cygwin1.dll                  如工具链需要
```

## 支持的操作

- **完整线刷**：写入固件、启动链、动态分区和数据相关操作，并要求输入确认词。
- **系统更新**：跳过 `userdata` 和 `metadata` 清理，保留用户数据。
- **ID 补丁**：刷入配置启用的 ColorOS `mi_ext` 补丁。
- **Root 镜像**：刷入已修补的 `boot` 或 `init_boot` 镜像，并校验镜像格式和分区容量。
- **物理扩容**：在 ROM 配置启用时，通过 Recovery 和 `sgdisk` 执行分区扩容及官方分区表恢复。扩容前的 GPT 会自动备份为 `backup/<机型>-super-expand-<时间>.zip`（内含 `gpt-sda-before.bin` 和 `plan.txt`），备份校验通过才会修改分区。恢复时可直接选择备份压缩包，工具会检查结构、磁盘和机型是否与当前刷机包一致；恢复后自动核对分区范围。旧版目录备份仍可使用。
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
frontend/src/
  App.tsx              应用外壳：步骤状态、刷入流程编排
  wizard.ts            步骤定义、刷入选项、阶段文案
  pages/               每个步骤一个页面（Welcome / Package / Device / Plan / Review / Run / Done）
  components/          标题栏、步骤栏、页面骨架、弹窗、日志抽屉等通用组件
  hooks/               设备轮询、刷机包解压、后端事件订阅
  lib/                 Tauri 调用封装（含浏览器预览数据）、格式化、本地存储
  styles/app.css       全局样式；设计变量在 index.css
src-tauri/src/         Rust 刷机、设备、配置和 Tauri 命令（rompkg.rs 负责刷机包 zip 解压）
src-tauri/tests/       Rust 核心单元测试
build.ps1              Windows Release 构建脚本
DESIGN-cal.md          视觉设计参考文件
```

浏览器预览（`pnpm --dir frontend dev`）会使用演示数据，可通过 `?step=device&preview=ready` 等参数直接打开某个页面调整界面。

项目不再包含旧的 Python 刷机实现、Python 依赖或 Python 测试套件。Rust/Tauri GUI 是唯一的桌面实现。
