from __future__ import annotations

import os
import sys
from collections.abc import Sequence

_package_info = ""


def configure_banner(package_info: str) -> None:
    """Set ROM package attribution displayed on every interactive screen."""
    global _package_info
    _package_info = package_info.strip()


def enable_windows_ansi() -> None:
    if sys.platform != "win32":
        return
    try:
        import ctypes

        kernel32 = ctypes.windll.kernel32  # type: ignore[attr-defined]
        handle = kernel32.GetStdHandle(-11)
        mode = ctypes.c_uint32()
        if kernel32.GetConsoleMode(handle, ctypes.byref(mode)):
            kernel32.SetConsoleMode(handle, mode.value | 0x0004)
    except Exception:
        pass
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")  # type: ignore[attr-defined]
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")  # type: ignore[attr-defined]
    except Exception:
        pass


def set_title(title: str) -> None:
    if sys.platform == "win32":
        try:
            import ctypes

            ctypes.windll.kernel32.SetConsoleTitleW(title)  # type: ignore[attr-defined]
        except Exception:
            os.system(f"title {title}")  # noqa: S605
    else:
        sys.stdout.write(f"\33]0;{title}\a")
        sys.stdout.flush()


class Colors:
    RESET = "\033[0m"
    BOLD = "\033[1m"
    CYAN = "\033[36m"
    YELLOW = "\033[33m"
    GREEN = "\033[32m"
    RED = "\033[31m"
    BRIGHT_RED = "\033[91m"
    BRIGHT_YELLOW = "\033[93m"
    BRIGHT_WHITE = "\033[97m"
    MAGENTA = "\033[35m"
    DARK = "\033[90m"
    BLACK = "\033[30m"
    BG_CYAN = "\033[46m"
    BG_RED = "\033[41m"
    BG_YELLOW = "\033[43m"
    # High-visibility: white on red / black on yellow
    ALERT = "\033[1;97;41m"
    WARN_BAND = "\033[1;30;43m"


def cprint(text: str = "", color: str = "", end: str = "\n") -> None:
    if color:
        print(f"{color}{text}{Colors.RESET}", end=end)
    else:
        print(text, end=end)


def clear_screen() -> None:
    os.system("cls" if sys.platform == "win32" else "clear")  # noqa: S605


def window_width() -> int:
    try:
        return max(60, os.get_terminal_size().columns)
    except OSError:
        return 92


def center(text: str, color: str = Colors.DARK) -> None:
    width = window_width()
    # crude width: CJK counts as 2 for padding estimate
    visual = sum(2 if ord(ch) > 127 else 1 for ch in text)
    padding = max(0, (width - visual) // 2)
    cprint((" " * padding) + text, color)


def show_banner(display_name: str, subtitle: str = "") -> None:
    clear_screen()
    cprint(
        "  ╔══════════════════════════════════════════════════════════════════════════════╗",
        Colors.CYAN,
    )
    cprint(
        "  ║                   H A R U S O R A   F L A S H   T O O L                    ║",
        Colors.CYAN,
    )
    cprint(
        "  ╚══════════════════════════════════════════════════════════════════════════════╝",
        Colors.CYAN,
    )
    center(display_name, Colors.DARK)
    if _package_info:
        center(_package_info, Colors.DARK)
    if subtitle:
        center(subtitle, Colors.DARK)
    print()


def _fill_bar(text: str, style: str, width: int = 78) -> None:
    """Print a full-width colored bar with centered-ish text."""
    visual = sum(2 if ord(ch) > 127 else 1 for ch in text)
    pad = max(0, width - visual)
    left = pad // 2
    right = pad - left
    line = (" " * left) + text + (" " * right)
    if len(line) < width:
        line = line + (" " * (width - len(line)))
    print(f"  {style}{line[:width]}{Colors.RESET}")


def show_backup_safeguard_page(
    backup_dir: str,
    *,
    backup_root: str = "",
    log_path: str = "",
) -> None:
    """Post-expand reminder: keep the recovery backup folder."""
    clear_screen()
    print()
    cprint(
        "  ╔══════════════════════════════════════════════════════════════════════════════╗",
        Colors.GREEN,
    )
    cprint(
        "  ║                         扩  容  完  成                                       ║",
        Colors.GREEN,
    )
    cprint(
        "  ╚══════════════════════════════════════════════════════════════════════════════╝",
        Colors.GREEN,
    )
    print()
    cprint("  请保留本次生成的恢复备份（以后若要恢复官方分区表会用到）。", Colors.YELLOW)
    print()
    cprint("  ┌─ 备份文件夹 ───────────────────────────────────────────────────────┐", Colors.CYAN)
    cprint(f"  │  {backup_dir}", Colors.BRIGHT_WHITE)
    cprint("  └────────────────────────────────────────────────────────────────────┘", Colors.CYAN)
    if backup_root:
        print()
        print(f"  备份总目录：{backup_root}")
    print()
    print("  建议：")
    print("    · 复制整个文件夹到 U 盘或网盘另存一份")
    print("    · 保留其中的 gpt-sda-before.bin 与 plan.txt")
    print("    · 只使用本机生成的备份，不要混用其他设备的备份")
    print("    · 扩容后请继续刷入对应 ROM")
    print()
    if log_path:
        cprint(f"  日志：{log_path}", Colors.DARK)
        print()
    cprint("  按任意键打开备份文件夹并返回菜单", Colors.CYAN)
    wait_key("")


def wait_key(message: str = "按任意键返回主菜单") -> None:
    print()
    cprint(f"  {message}", Colors.DARK)
    try:
        if sys.platform == "win32":
            import msvcrt

            msvcrt.getch()
        else:
            import termios
            import tty

            fd = sys.stdin.fileno()
            old = termios.tcgetattr(fd)
            try:
                tty.setraw(fd)
                sys.stdin.read(1)
            finally:
                termios.tcsetattr(fd, termios.TCSADRAIN, old)
    except Exception:
        input("  (按 Enter 继续) ")


def _read_key() -> str:
    """Return logical key: up/down/enter/esc/other."""
    if sys.platform == "win32":
        import msvcrt

        ch = msvcrt.getch()
        if ch in (b"\x00", b"\xe0"):
            ch2 = msvcrt.getch()
            if ch2 == b"H":
                return "up"
            if ch2 == b"P":
                return "down"
            return "other"
        if ch in (b"\r", b"\n"):
            return "enter"
        if ch == b"\x1b":
            return "esc"
        if ch == b"\x03":
            raise KeyboardInterrupt
        return "other"

    import termios
    import tty

    fd = sys.stdin.fileno()
    old = termios.tcgetattr(fd)
    try:
        tty.setraw(fd)
        ch = sys.stdin.read(1)
        if ch == "\x1b":
            rest = sys.stdin.read(2)
            if rest == "[A":
                return "up"
            if rest == "[B":
                return "down"
            return "esc"
        if ch in ("\r", "\n"):
            return "enter"
        if ch == "\x03":
            raise KeyboardInterrupt
        return "other"
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old)


def show_menu(title: str, items: Sequence[str], display_name: str = "") -> int:
    selected = 0
    n = len(items)
    while True:
        show_banner(display_name or "HarusoraFlashTool")
        center(title, Colors.YELLOW)
        print()
        for i, item in enumerate(items):
            if i == selected:
                label = f"      ▶  {item}"
                print(f"{Colors.BLACK}{Colors.BG_CYAN}{label:<78}{Colors.RESET}")
            else:
                print(f"         {item}")
            print()
        cprint("  ↑/↓ 选择    Enter 确认    Esc 返回/退出", Colors.DARK)
        key = _read_key()
        if key == "up":
            selected = (selected - 1) % n
        elif key == "down":
            selected = (selected + 1) % n
        elif key == "enter":
            return selected
        elif key == "esc":
            return n - 1
