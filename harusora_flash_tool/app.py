from __future__ import annotations

import os
import subprocess
import sys
import traceback
from pathlib import Path

from . import __app_name__, __version__
from .config import FlashProfile, ProfileError, load_profile, resolve_profile_path
from .device import DeviceError, DeviceSession
from .expand import ExpandError, ExpandSession
from .flash import FlashError, run_diagnostics, run_full_flash
from .idflash import (
    IdFlashError,
    discover_id_candidates,
    ensure_fastbootd,
    flash_mi_ext_image,
    format_userdata_metadata,
    prompt_id_path,
    resolve_id_input,
)
from .log import Logger
from .package import PackageError
from .rootflash import root_flash_ui
from .ui import (
    Colors,
    configure_banner,
    cprint,
    enable_windows_ansi,
    set_title,
    show_backup_safeguard_page,
    show_banner,
    show_menu,
    wait_key,
)


def _choose_factory(display_name: str):
    def choose(title: str, items: list[str]) -> int:
        return show_menu(title, items, display_name=display_name)

    return choose


def show_notice(profile: FlashProfile) -> None:
    show_banner(profile.display_name)
    cprint("  刷入须知", Colors.YELLOW)
    print()
    if profile.notes:
        for i, note in enumerate(profile.notes, start=1):
            print(f"  {i}. {note}")
    else:
        print("  （配置中未提供 notes）")
    print()
    cprint("  本工具不会执行锁 Bootloader 操作。", Colors.RED)
    print(f"  配置文件：{profile.profile_path}")
    print(f"  支持机型：{', '.join(profile.codenames)}")
    print(f"  作者：{profile.author}")
    print(f"  QQ群：{profile.qq_group}")
    print(f"  构建日期：{profile.build_date}")
    wait_key()


def show_diagnostics_ui(profile: FlashProfile, logger: Logger) -> None:
    show_banner(profile.display_name)
    cprint("  只读环境检测", Colors.YELLOW)
    print()
    try:
        if not logger.path:
            logger.start(profile.logs_dir)
        report = run_diagnostics(
            profile,
            logger,
            choose=_choose_factory(profile.display_name),
        )
        cprint("  [通过] 刷机包文件完整", Colors.GREEN)
        cprint(f"  [通过] fastboot 设备：{report.serial}", Colors.GREEN)
        cprint(f"  [通过] 设备代号：{report.product}", Colors.GREEN)
        if report.super_ready:
            cprint(
                f"  [通过] super 大小：{report.super_bytes} 字节 "
                f"(要求 ≥ {profile.minimum_super_bytes})",
                Colors.GREEN,
            )
        else:
            cprint(
                f"  [失败] super 大小：{report.super_bytes} 字节，"
                f"尚未达到 {profile.minimum_super_bytes}",
                Colors.RED,
            )
        mode = "fastbootd" if report.is_userspace else "bootloader fastboot"
        print(f"  当前模式：{mode}")
        print(f"  Anti rollback：{report.anti if report.anti else '未返回'}")
        print(f"  配置：{profile.profile_path}")
    except (FlashError, PackageError, DeviceError, ProfileError) as exc:
        cprint(f"  [检测失败] {exc}", Colors.RED)
    except Exception as exc:
        cprint(f"  [检测失败] {exc}", Colors.RED)
        logger.write(traceback.format_exc())
    wait_key()


def _apply_id_patch(profile: FlashProfile, session: DeviceSession, logger: Logger, id_path: Path) -> None:
    image = resolve_id_input(id_path, profile.work_dir)
    print(f"  解析到：{image}")
    ensure_fastbootd(session, logger)
    flash_mi_ext_image(session, image, logger, work=profile.work_dir)
    cprint("  ID 刷入完成。", Colors.GREEN)


def _choose_id_source(profile: FlashProfile) -> Path | None:
    """Return an ID zip/image path, or None if the user skips."""
    bundled = discover_id_candidates(profile.rom_root)
    cprint("  重要：ROM 包内的 mi_ext 只是占位，需要刷独立 ID 补丁。", Colors.YELLOW)
    print("  当前 ID 是 Recovery OTA zip（含 mi_ext.new.dat），不是单独的 .img。")
    print("  未刷入 ID 可能一直停在开机动画；必须先刷完 ROM 再刷 ID。")
    print()
    if bundled:
        print(f"  发现刷机包内 ID 补丁：{bundled[0].name}")
        if len(bundled) > 1:
            for extra in bundled[1:]:
                print(f"           另外还有：{extra.name}")
        print("    [回车]  使用该 zip")
        print("    [p]     拖入其他 ID 文件")
        print("    [s]     跳过（开机可能停在动画）")
        ans = input("  选择> ").strip().lower()
        if ans in ("s", "skip"):
            return None
        if ans in ("p", "path", "n"):
            return prompt_id_path()
        return bundled[0]
    print("    [回车]  拖入 ID 补丁 zip")
    print("    [s]     跳过（开机可能停在动画）")
    ans = input("  选择> ").strip().lower()
    if ans in ("s", "skip"):
        return None
    return prompt_id_path()


def post_flash_id_and_format(profile: FlashProfile, session: DeviceSession, logger: Logger) -> None:
    """After ROM flash: ID (mi_ext zip), format data, reboot. No early reboot before this."""
    print()
    cprint("  ── 刷包完成（尚未重启）──", Colors.GREEN)
    print()
    id_path = _choose_id_source(profile)
    if id_path is None:
        print("  已跳过刷 ID。")
        logger.write("user skipped ID patch")
    else:
        print()
        try:
            _apply_id_patch(profile, session, logger, id_path)
        except (IdFlashError, DeviceError) as exc:
            cprint(f"  ID 刷入失败：{exc}", Colors.RED)
            logger.write(f"ID flash failed: {exc}")
            print("  将继续格式化 Data 并重启；也可稍后在菜单里单独再刷 ID。")

    print()
    try:
        # format data preferably in current mode; enter fastbootd if needed for consistency
        us = session.getvar("is-userspace", allow_missing=True)
        if us != "yes":
            try:
                ensure_fastbootd(session, logger)
            except Exception:
                print("  未能进入 fastbootd，尝试在当前 fastboot 下 erase…")
        format_userdata_metadata(session, logger)
        cprint("  Data 已格式化。", Colors.GREEN)
    except Exception as exc:
        cprint(f"  格式化 Data 时出现问题：{exc}", Colors.YELLOW)
        logger.write(f"format data issue: {exc}")

    print()
    print("  正在重启手机…")
    try:
        session.run_fastboot(["reboot"], label="reboot", show_output=True, allow_failure=True)
    except Exception as exc:
        print(f"  自动重启未成功（可手动重启）：{exc}")
    logger.write("post-flash flow finished (ID + format + reboot)")


def _show_flash_failure(profile: FlashProfile, logger: Logger, exc: BaseException) -> None:
    logger.fatal(exc)
    show_banner(profile.display_name)
    cprint(
        "  ╔══════════════════════════════════════════════════════════════════════╗",
        Colors.RED,
    )
    cprint(
        "  ║                              刷写失败                                ║",
        Colors.RED,
    )
    cprint(
        "  ╚══════════════════════════════════════════════════════════════════════╝",
        Colors.RED,
    )
    print()
    print(f"  {exc}")
    print()
    print("  设备不会被自动重启或锁定。")
    if logger.path:
        print(f"  完整日志：{logger.path}")
    print("  请保留当前 fastboot 画面和日志后再反馈。")
    wait_key()


def full_flash_ui(profile: FlashProfile) -> None:
    show_banner(profile.display_name)
    cprint("  完整线刷 · 清空全部用户数据", Colors.RED)
    print()

    logger = Logger()
    logger.start(profile.logs_dir)

    def confirm(msg: str) -> str:
        cprint(msg, Colors.YELLOW)
        return input("  确认> ").strip()

    try:
        session = run_full_flash(
            profile,
            logger,
            choose=_choose_factory(profile.display_name),
            confirm=confirm,
            reboot=False if profile.post_flash_id else None,
        )
        if session is not None and profile.post_flash_id:
            post_flash_id_and_format(profile, session, logger)
        wait_key("全部完成，按任意键返回")
    except FlashError as exc:
        if "已取消" in str(exc):
            print(f"  {exc}")
            wait_key()
            return
        _show_flash_failure(profile, logger, exc)
    except (DeviceError, PackageError) as exc:
        _show_flash_failure(profile, logger, exc)
    except KeyboardInterrupt:
        logger.write("Full flash interrupted by user (Ctrl+C)")
        cprint("\n  用户中断。请勿强拔数据线；设备保持 fastboot 即可。", Colors.YELLOW)
        if logger.path:
            print(f"  日志：{logger.path}")
        wait_key()
    except Exception as exc:
        logger.write(traceback.format_exc())
        _show_flash_failure(profile, logger, exc)


def update_flash_ui(profile: FlashProfile) -> None:
    show_banner(profile.display_name)
    cprint("  系统更新 · 保留全部用户数据", Colors.GREEN)
    print()
    cprint(
        "  将更新固件、启动链和系统动态分区。\n"
        "  userdata 与 metadata 不会被擦除、格式化或刷写。\n"
        "  仅适合同一 ROM 的后续版本更新；首次安装仍须使用完整线刷。",
        Colors.YELLOW,
    )
    print()

    logger = Logger()
    logger.start(profile.logs_dir, prefix="harusora-update")

    def confirm(msg: str) -> str:
        cprint(msg, Colors.YELLOW)
        return input("  确认> ").strip()

    try:
        run_full_flash(
            profile,
            logger,
            choose=_choose_factory(profile.display_name),
            confirm=confirm,
            preserve_data=True,
            reboot=None,
        )
        wait_key("更新完成，userdata / metadata 已保留。按任意键返回")
    except FlashError as exc:
        if "已取消" in str(exc):
            print(f"  {exc}")
        else:
            logger.fatal(exc)
            cprint(f"  更新失败：{exc}", Colors.RED)
            print("  设备不会被自动重启或锁定。")
            if logger.path:
                print(f"  完整日志：{logger.path}")
        wait_key()
    except (DeviceError, PackageError) as exc:
        logger.fatal(exc)
        cprint(f"  更新失败：{exc}", Colors.RED)
        if logger.path:
            print(f"  日志：{logger.path}")
        wait_key()
    except KeyboardInterrupt:
        logger.write("Update interrupted by user (Ctrl+C)")
        cprint("\n  用户中断。请勿强拔数据线；设备保持 fastboot 即可。", Colors.YELLOW)
        if logger.path:
            print(f"  日志：{logger.path}")
        wait_key()
    except Exception as exc:
        logger.fatal(exc)
        logger.write(traceback.format_exc())
        cprint(f"  更新失败：{exc}", Colors.RED)
        if logger.path:
            print(f"  日志：{logger.path}")
        wait_key()


def open_backup_folder(profile: FlashProfile) -> None:
    path = None
    if profile.expansion:
        path = profile.expansion.backup_dir
    if path is None:
        path = profile.rom_root / "backup"
    path.mkdir(parents=True, exist_ok=True)
    if sys.platform == "win32":
        os.startfile(str(path))  # type: ignore[attr-defined]
    else:
        subprocess.Popen(["xdg-open", str(path)])


def _expand_session(profile: FlashProfile, logger: Logger) -> ExpandSession:
    if not logger.path:
        logger.start(profile.logs_dir, prefix="harusora-expand")
    return ExpandSession(profile, logger)


def expansion_apply_ui(profile: FlashProfile) -> None:
    """Connect device, show plan (read-only preflight), then confirm and write GPT."""
    show_banner(profile.display_name)
    cprint("  super 物理扩容 · 清空数据", Colors.RED)
    print()
    assert profile.expansion is not None
    word = profile.expansion.confirm_word

    logger = Logger()
    try:
        session = _expand_session(profile, logger)
        cprint("  正在连接设备并计算分区方案（此阶段不写入）…", Colors.CYAN)
        print()
        plan = session.prepare_plan()
        print()
        if plan.current_super_bytes == plan.target_bytes:
            cprint("  super 已是目标大小，无需扩容。", Colors.GREEN)
            if logger.path:
                print(f"  日志：{logger.path}")
            wait_key()
            return
        if plan.current_super_bytes > plan.target_bytes:
            raise ExpandError("当前 super 大于目标大小，拒绝缩小。")

        cprint(
            f"  确认后将：备份 GPT → 格式化 userdata/metadata → 重启 Recovery "
            f"→ 修改物理分区表。\n"
            f"  手机中的全部用户数据都会被清空。\n"
            f"  请输入 {word} 后按回车继续；其他内容取消。",
            Colors.YELLOW,
        )
        answer = input("  确认> ").strip()
        if answer != word:
            print("  已取消，没有写入设备。")
            wait_key()
            return

        backup_dir = session.apply_expand(plan)
        if backup_dir is not None:
            # Full-screen, hard-to-miss reminder to keep recovery files
            show_backup_safeguard_page(
                str(backup_dir),
                backup_root=str(profile.expansion.backup_dir),
                log_path=str(logger.path) if logger.path else "",
            )
            # Open the specific backup folder so the user can copy it immediately
            try:
                if sys.platform == "win32":
                    os.startfile(str(backup_dir))  # type: ignore[attr-defined]
                else:
                    subprocess.Popen(["xdg-open", str(backup_dir)])
            except Exception:
                pass
            return
        cprint("  扩容流程结束（未生成新备份）。", Colors.GREEN)
        if logger.path:
            print(f"  日志：{logger.path}")
    except ExpandError as exc:
        cprint(f"  扩容失败：{exc}", Colors.RED)
        logger.fatal(exc)
        print("  请保留 Recovery 状态与 backup 目录，勿在未确认分区前启动系统。")
        if logger.path:
            print(f"  日志：{logger.path}")
    except Exception as exc:
        cprint(f"  扩容失败：{exc}", Colors.RED)
        logger.write(traceback.format_exc())
        if logger.path:
            print(f"  日志：{logger.path}")
    wait_key()


def expansion_restore_ui(profile: FlashProfile) -> None:
    show_banner(profile.display_name)
    cprint("  恢复官方物理分区布局（清空数据）", Colors.RED)
    print()
    cprint(
        "  仅接受本机扩容时自动生成的 GPT 备份。\n"
        "  恢复后必须立即刷官方完整线刷包，禁止直接启动移植系统。",
        Colors.YELLOW,
    )
    print()
    logger = Logger()
    try:
        session = _expand_session(profile, logger)
        if session.adb_transport_state() != "recovery":
            raise ExpandError("请先将手机进入 Recovery（支持 root ADB），再执行恢复。")
        device = session.adb_shell("getprop ro.product.device").strip().splitlines()
        device = device[0].strip() if device else ""
        if device not in profile.expansion.codenames:  # type: ignore[union-attr]
            raise ExpandError(f"机型不匹配：{device}")
        backups = session.list_backups_for_device(device)
        if not backups:
            raise ExpandError(
                f"没有找到与 {device} 匹配的扩容前 GPT 备份，拒绝盲目恢复。"
            )
        labels = [f"{b.name}  ({datetime_mtime(b)})" for b in backups] + ["取消"]
        idx = show_menu("选择扩容前备份", labels, display_name=profile.display_name)
        if idx >= len(backups):
            print("  已取消。")
            wait_key()
            return
        backup = backups[idx]

        show_banner(profile.display_name)
        word = profile.expansion.restore_confirm_word  # type: ignore[union-attr]
        cprint(
            f"  选中：{backup}\n"
            f"  确认恢复官方 GPT 并清空数据，请输入 {word}：",
            Colors.YELLOW,
        )
        answer = input("  确认> ").strip()
        if answer != word:
            print("  已取消，没有写入设备。")
            wait_key()
            return

        result = session.restore_official(backup)
        print()
        cprint("  官方分区表：已恢复", Colors.GREEN)
        if result.get("format_ok"):
            cprint("  数据分区：已格式化", Colors.GREEN)
        else:
            cprint("  数据分区：自动格式化未完成（不影响上面的分区表恢复）", Colors.YELLOW)
            print("  可在 Recovery 里手动 Format Data，或接着刷官方线刷包。")
        print()
        print("  可选择重启到 bootloader 以便刷官方线刷包。")
        reboot = input("  输入 Y 重启到 bootloader，其他键跳过> ").strip().lower()
        if reboot == "y":
            session.adb_run(["reboot", "bootloader"])
            print("  已发送 reboot bootloader。")
        if logger.path:
            print(f"  日志：{logger.path}")
    except ExpandError as exc:
        cprint(f"  未能完成：{exc}", Colors.YELLOW)
        logger.fatal(exc)
        print("  请保留 Recovery 与备份目录后再试。")
        if logger.path:
            print(f"  日志：{logger.path}")
    except Exception as exc:
        cprint(f"  未能完成：{exc}", Colors.YELLOW)
        logger.write(traceback.format_exc())
        if logger.path:
            print(f"  日志：{logger.path}")
    wait_key()


def datetime_mtime(path: Path) -> str:
    from datetime import datetime

    return datetime.fromtimestamp(path.stat().st_mtime).strftime("%Y-%m-%d %H:%M:%S")


def expansion_menu(profile: FlashProfile) -> None:
    if not profile.expansion or not profile.expansion.enabled:
        show_banner(profile.display_name)
        cprint("  当前配置未启用物理扩容（[expansion]）。", Colors.YELLOW)
        wait_key()
        return

    while True:
        choice = show_menu(
            "super 物理扩容",
            [
                "开始扩容到目标大小（清空数据）",
                "恢复官方分区表（清空数据）",
                "打开备份文件夹（backup）",
                "返回主菜单",
            ],
            display_name=profile.display_name,
        )
        if choice == 0:
            expansion_apply_ui(profile)
        elif choice == 1:
            expansion_restore_ui(profile)
        elif choice == 2:
            open_backup_folder(profile)
        else:
            return


def id_flash_ui(profile: FlashProfile) -> None:
    """Standalone ID patch: flash mi_ext from an OTA zip. Does not wipe userdata."""
    show_banner(profile.display_name)
    cprint("  刷入 / 更新 ID 补丁 · 不清空用户数据", Colors.YELLOW)
    print()
    print("  请使用 ID 补丁 zip（含 images/super/mi_ext.new.dat + transfer.list）。")
    print("  必须先刷完 ROM，再刷 ID；否则补丁会被包内占位 mi_ext 覆盖。")
    print()

    logger = Logger()
    logger.start(profile.logs_dir, prefix="harusora-id")
    try:
        id_path = _choose_id_source(profile)
        if id_path is None:
            print("  已取消。")
            wait_key()
            return
        session = DeviceSession(profile.fastboot, profile.adb, logger)
        session.select_device(choose=_choose_factory(profile.display_name))
        product = session.getvar("product")
        if not product or product not in profile.codenames:
            raise IdFlashError(
                f"机型不匹配：{product or '无法读取'}；支持：{', '.join(profile.codenames)}"
            )
        print()
        cprint(f"  设备：{product}", Colors.CYAN)
        cprint(f"  ID 文件：{id_path}", Colors.CYAN)
        answer = input("  确认刷入请输入 ID> ").strip()
        if answer != "ID":
            print("  已取消，没有写入设备。")
            wait_key()
            return
        _apply_id_patch(profile, session, logger, id_path)
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
    except (IdFlashError, DeviceError) as exc:
        cprint(f"  ID 刷入失败：{exc}", Colors.RED)
        logger.fatal(exc)
        if logger.path:
            print(f"  日志：{logger.path}")
    except Exception as exc:
        cprint(f"  ID 刷入失败：{exc}", Colors.RED)
        logger.write(traceback.format_exc())
        if logger.path:
            print(f"  日志：{logger.path}")
    wait_key()


def clean_work(profile: FlashProfile) -> None:
    show_banner(profile.display_name)
    work = profile.work_dir
    if work.is_dir():
        import shutil

        shutil.rmtree(work, ignore_errors=True)
        cprint(f"  已清理工作目录：{work}", Colors.GREEN)
    else:
        print(f"  工作目录不存在，无需清理：{work}")
    wait_key()


def main_menu_entries(profile: FlashProfile) -> tuple[list[str], list[str]]:
    """Return aligned labels and action identifiers for the configured ROM package."""
    items = [
        "完整线刷（清空 Data）",
        "系统更新（保留 Data）",
        "只读检测设备、机型与 super 大小",
    ]
    actions = ["flash", "update", "check"]
    if profile.enable_root_menu:
        items.insert(2, "刷入 Root 镜像（不清空数据）")
        actions.insert(2, "root")
    if profile.post_flash_id:
        idx = actions.index("check")
        items.insert(idx, "刷入 / 更新 ID 补丁（mi_ext zip）")
        actions.insert(idx, "id")
    if profile.expansion and profile.expansion.enabled:
        items.append("super 物理扩容 / 恢复官方分区表")
        actions.append("expansion")
    items.extend(["查看刷入须知", "清理临时 work 目录", "退出"])
    actions.extend(["notice", "clean", "exit"])
    return items, actions


def run_tui(profile: FlashProfile) -> int:
    enable_windows_ansi()
    set_title(f"{__app_name__} - {profile.display_name}")
    logger = Logger()

    while True:
        items, actions = main_menu_entries(profile)
        choice = show_menu(
            "请选择操作",
            items,
            display_name=profile.display_name,
        )
        action = actions[choice]
        if action == "flash":
            full_flash_ui(profile)
        elif action == "update":
            update_flash_ui(profile)
        elif action == "root":
            root_flash_ui(profile)
        elif action == "id":
            id_flash_ui(profile)
        elif action == "check":
            show_diagnostics_ui(profile, logger)
        elif action == "expansion":
            expansion_menu(profile)
        elif action == "notice":
            show_notice(profile)
        elif action == "clean":
            clean_work(profile)
        else:
            return 0


def load_from_args(rom_root: Path | None, profile: Path | None) -> FlashProfile:
    root, path = resolve_profile_path(rom_root, profile)
    return load_profile(root, path)


def main(argv: list[str] | None = None) -> int:
    import argparse

    parser = argparse.ArgumentParser(
        prog=__app_name__,
        description="Harusora 团队配置驱动的 Android fastboot 刷机工具",
    )
    parser.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    parser.add_argument(
        "--rom-root",
        type=Path,
        default=None,
        help="ROM 包根目录（含 images/ super/ META-INF/）",
    )
    parser.add_argument(
        "--profile",
        type=Path,
        default=None,
        help="flash_profile.toml 路径（默认 META-INF/com/android/flash_profile.toml）",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="只读检测包与设备后退出",
    )
    parser.add_argument(
        "--flash",
        action="store_true",
        help="无 TUI 直接完整线刷（需 --confirm WIPE）",
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="无 TUI 保留 userdata/metadata 更新（需 --confirm UPDATE）",
    )
    parser.add_argument(
        "--confirm",
        type=str,
        default="",
        help="与 --flash / --update 联用，须等于对应确认词",
    )
    parser.add_argument(
        "--no-tui",
        action="store_true",
        help="不进入菜单（需配合 --check / --flash）",
    )

    args = parser.parse_args(argv)
    enable_windows_ansi()

    if args.flash and args.update:
        parser.error("--flash 与 --update 不能同时使用")

    try:
        profile = load_from_args(args.rom_root, args.profile)
    except ProfileError as exc:
        print(f"配置错误：{exc}", file=sys.stderr)
        return 2

    configure_banner(profile.package_summary)

    logger = Logger()

    if args.check:
        logger.start(profile.logs_dir)
        try:
            report = run_diagnostics(profile, logger)
            print("OK package + device")
            print(f"  serial={report.serial}")
            print(f"  product={report.product}")
            print(f"  super={report.super_bytes} ready={report.super_ready}")
            print(f"  mode={'fastbootd' if report.is_userspace else 'bootloader'}")
            print(f"  anti={report.anti}")
            print(f"  {profile.package_summary}")
            return 0 if report.super_ready else 3
        except Exception as exc:
            print(f"CHECK FAILED: {exc}", file=sys.stderr)
            return 1

    if args.flash:
        logger.start(profile.logs_dir)
        word = profile.confirmation_word
        if args.confirm != word:
            print(
                f"拒绝：完整线刷需 --confirm {word}",
                file=sys.stderr,
            )
            return 2
        try:
            run_full_flash(
                profile,
                logger,
                skip_confirm=True,
            )
            return 0
        except Exception as exc:
            print(f"FLASH FAILED: {exc}", file=sys.stderr)
            if logger.path:
                print(f"log: {logger.path}", file=sys.stderr)
            return 1

    if args.update:
        logger.start(profile.logs_dir, prefix="harusora-update")
        word = profile.update_confirmation_word
        if args.confirm != word:
            print(f"拒绝：保留数据更新需 --confirm {word}", file=sys.stderr)
            return 2
        try:
            run_full_flash(
                profile,
                logger,
                skip_confirm=True,
                preserve_data=True,
            )
            return 0
        except Exception as exc:
            print(f"UPDATE FAILED: {exc}", file=sys.stderr)
            if logger.path:
                print(f"log: {logger.path}", file=sys.stderr)
            return 1

    if args.no_tui:
        print("未指定 --check、--flash 或 --update，且使用了 --no-tui。", file=sys.stderr)
        return 2

    return run_tui(profile)
