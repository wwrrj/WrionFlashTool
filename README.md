# HarusoraFlashTool

Harusora 团队维护的配置驱动 Android fastboot 刷机工具。一个通用 EXE 可服务不同机型和 ROM：支持机型、镜像到分区的映射、动态分区、作者信息及可选扩容都由刷机包内的 TOML 配置决定。

> 刷机和扩容存在数据丢失风险。请先解锁 Bootloader、备份数据，并确认刷机包明确支持你的设备。本工具不会锁定 Bootloader。

## 普通用户快速开始

刷机包发布者应提供完整目录。用户不需要安装 Python，也不需要 BAT：

```text
ROM 根目录/
├─ HarusoraFlashTool.exe
├─ META-INF/com/android/flash_profile.toml
├─ images/                         固件与启动镜像
├─ super/                          *.new.dat 与 *.transfer.list
└─ tools/
   ├─ platform-tools/adb.exe
   ├─ platform-tools/fastboot.exe
   ├─ img2simg.exe
   └─ cygwin1.dll
```

1. 将手机连接电脑，双击根目录的 `HarusoraFlashTool.exe`。
2. 先选择“只读检测”，确认机型、包体和 `super` 大小通过。
3. 首次安装选择“完整线刷”，会清空 `userdata` 和 `metadata`。
4. 同一 ROM 后续版本可选择“系统更新”，配置中的数据分区操作会被跳过。

程序会自动从 EXE 所在目录向上查找配置。日志写入 ROM 根目录的 `logs/`。

## 菜单能力

- 完整线刷：按配置刷写固件、启动链和动态分区，必须输入确认词。
- 系统更新：保留 `userdata`/`metadata`，仍建议提前备份。
- Root 镜像：由 `enable_root_menu` 控制，只接受带 `ANDROID!` 头的 `boot.img` 或 `init_boot.img`。
- 物理扩容：只有 `[expansion].enabled = true` 时显示；扩容和恢复都会清空数据，并要求独立确认。
- ColorOS ID 后处理：默认关闭；仅特定包需要时设置 `post_flash_id = true`。

## 发包前生成配置

需要 Python 3.11+。先将镜像、动态分区和平台工具放入目标 ROM 根目录，再在本仓库运行：

```powershell
python tools\generate_profile.py `
  --rom-root "E:\ROM-Package" `
  --codename shennong `
  --display-name "HarusoraOS for Xiaomi 14 Pro" `
  --author "Harusora Team" `
  --qq-group "123456789" `
  --note "首次刷入会清空全部用户数据"
```

脚本会扫描 `images/*.img` 和 `super/*.new.dat`，读取 `transfer.list` 计算动态分区大小，将当天日期写入 `build_date`，生成 `META-INF/com/android/flash_profile.toml`，并执行完整性校验。已有配置默认不会覆盖，确认重建时添加 `--force`。

无法可靠判断的镜像会导致生成失败。使用显式映射解决，不能依赖猜测：

```powershell
python tools\generate_profile.py ... `
  --partition-map "cust_boot.img=boot_ab" `
  --partition-map "special.img=special_a"
```

同一 ROM 支持多个代号时重复传入 `--codename`。需要 CRC 表时添加 `--flash-crc-lists`。

生成器默认在 `final_data` 添加 `erase metadata` 和 `erase userdata`，因此完整线刷会清空数据，而系统更新会自动跳过它们。可按包体需要使用 `--disable-root-menu`、`--post-flash-id`、`--no-reboot-after-success`、`--pre-erase PARTITION` 和 `--erase-if-present PARTITION`；这些可重复参数会直接写入配置。

### 可选扩容

默认生成 `enabled = false`，用户看不到扩容入口。确认机型 GPT 布局后才能启用，且必须明确提供目标大小：

```powershell
python tools\generate_profile.py ... `
  --enable-expansion `
  --expansion-target-bytes 17179869184 `
  --expansion-disk /dev/block/sda `
  --expansion-super-part 32 `
  --expansion-userdata-part 33
```

如果 Recovery 不自带 `sgdisk`，包内还须提供 `tools/sgdisk-arm64/{sgdisk,libext2_uuid.so,libc++.so}`。GPT 分区号绝不能跨机型照抄。

## 配置说明

生成后的配置分为：

- `[package]`：`author`、`qq_group`、`build_date`，在每个菜单页显示。
- `[device]`：机型代号、ROM 显示名和刷入须知。
- `[stages]`：`firmware`、`boot_chain`、`final_data` 镜像操作数组。
- `[dynamic]`：动态分区名称、DAT 大小、还原大小和槽位后缀。
- `[options]`：活动槽位、确认词、Root/ID 开关及重启行为。
- `[expansion]`：可选 GPT 扩容参数。

修改配置后执行只读语法检查：

```powershell
python -c "from pathlib import Path; from harusora_flash_tool.config import load_profile; r=Path(r'E:\ROM-Package'); print(load_profile(r, r/'META-INF/com/android/flash_profile.toml'))"
```

## 源码开发与构建

```powershell
python -m unittest discover -s tests -v
python -m harusora_flash_tool --version
python -m pip install pyinstaller
.\build.ps1
```

`build.ps1` 优先使用已安装的 Nuitka，否则使用 PyInstaller，产出 `dist\HarusoraFlashTool.exe` 并复制到仓库根目录。运行源码菜单可使用 `python -m harusora_flash_tool --rom-root "E:\ROM-Package"`。

发包前至少完成：生成器完整性校验通过、全部单测通过、EXE 位于 ROM 根目录、真实目标机执行只读检测、人工复核机型代号及所有分区映射。物理扩容还必须在对应机型上单独验证 GPT 备份与恢复流程。
