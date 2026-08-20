"""Flash a user-supplied, already patched Root boot image."""

from __future__ import annotations

import time
from pathlib import Path

from .config import FlashProfile
from .device import DeviceError, DeviceSession, parse_size
from .flash import flash_image
from .log import Logger
from .ui import Colors, cprint, show_banner, show_menu, wait_key


class RootFlashError(Exception):
    pass


def _clean_image_path(text: str) -> Path:
    value = text.strip().strip('"').strip("'")
    if not value:
        raise RootFlashError("没有输入镜像路径。")
    path = Path(value).expanduser().resolve()
    if not path.is_file():
        raise RootFlashError(f"镜像不存在：{path}")
    if path.suffix.lower() != ".img":
        raise RootFlashError("只接受 .img 镜像。")
    if path.stat().st_size < 4096:
        raise RootFlashError("镜像文件过小，拒绝刷入。")
    with path.open("rb") as stream:
        magic = stream.read(8)
    if magic != b"ANDROID!":
        raise RootFlashError(
            "镜像没有 Android boot image 头（ANDROID!），不能作为 boot/init_boot 刷入。"
        )
    return path


def _verify_device(profile: FlashProfile, session: DeviceSession) -> str:
    product = session.getvar("product")
    if not product or product not in profile.codenames:
        raise RootFlashError(
            f"机型不匹配：{product or '无法读取'}；支持：{', '.join(profile.codenames)}"
        )
    if session.getvar("is-userspace", allow_missing=True) == "yes":
        print("  当前为 fastbootd，正在切换到 bootloader fastboot…")
        session.run_fastboot(
            ["reboot", "bootloader"],
            label="重启到 bootloader",
            allow_failure=True,
            show_output=True,
        )
        time.sleep(3)
        session.wait_for_any_fastboot()
        session.wait_for_mode(False)
    return product


def _partition_size(session: DeviceSession, target: str) -> int:
    values: list[int] = []
    for slot in ("a", "b"):
        raw = session.getvar(f"partition-size:{target}_{slot}", allow_missing=True)
        if raw:
            try:
                values.append(parse_size(raw))
            except ValueError:
                pass
    if not values:
        raw = session.getvar(f"partition-size:{target}", allow_missing=True)
        if raw:
            try:
                values.append(parse_size(raw))
            except ValueError:
                pass
    if not values:
        raise RootFlashError(f"无法读取 {target} 分区大小，拒绝盲刷。")
    return min(values)


def root_flash_ui(profile: FlashProfile) -> None:
    show_banner(profile.display_name)
    cprint("  刷入 Root 镜像 · 不清空用户数据", Colors.YELLOW)
    print()
    print("  本工具不会下载或修补 Root，只刷入你已经修补好的镜像。")
    print("  Magisk 通常选择 init_boot；KernelSU / APatch 通常选择 boot。")
    print()

    choice = show_menu(
        "选择 Root 镜像类型",
        [
            "Magisk：刷入已修补的 init_boot.img",
            "KernelSU / APatch：刷入已修补的 boot.img",
            "返回主菜单",
        ],
        display_name=profile.display_name,
    )
    if choice == 2:
        return
    target = "init_boot" if choice == 0 else "boot"
    logger = Logger()
    logger.start(profile.logs_dir, prefix="harusora-root")

    try:
        print()
        print("  请把修补后的 .img 拖到窗口内，或输入完整路径：")
        image = _clean_image_path(input("  镜像> "))
        session = DeviceSession(profile.fastboot, profile.adb, logger)
        session.select_device(
            choose=lambda title, items: show_menu(
                title, items, display_name=profile.display_name
            )
        )
        product = _verify_device(profile, session)
        limit = _partition_size(session, target)
        size = image.stat().st_size
        if size > limit:
            raise RootFlashError(
                f"镜像大于 {target} 分区：镜像 {size} 字节，分区 {limit} 字节。"
            )

        print()
        cprint(f"  设备：{product}", Colors.CYAN)
        cprint(f"  目标：{target}_a / {target}_b", Colors.CYAN)
        cprint(f"  镜像：{image}（{size} 字节）", Colors.CYAN)
        answer = input("  确认刷入请输入 ROOT> ").strip()
        if answer != "ROOT":
            print("  已取消，没有写入设备。")
            wait_key()
            return

        flash_image(
            session,
            f"{target}_ab",
            image,
            label=f"刷入 {target}_ab ← {image.name}",
        )
        cprint("  Root 镜像刷入完成。", Colors.GREEN)
        reboot = input("  输入 Y 立即重启系统，其他键保持 fastboot> ").strip().lower()
        if reboot == "y":
            session.run_fastboot(
                ["reboot"],
                label="重启系统",
                allow_failure=True,
                show_output=True,
            )
        if logger.path:
            print(f"  日志：{logger.path}")
    except (RootFlashError, DeviceError) as exc:
        cprint(f"  Root 刷入失败：{exc}", Colors.RED)
        logger.fatal(exc)
        if logger.path:
            print(f"  日志：{logger.path}")
    except Exception as exc:
        cprint(f"  Root 刷入失败：{exc}", Colors.RED)
        logger.fatal(exc)
        if logger.path:
            print(f"  日志：{logger.path}")
    wait_key()
