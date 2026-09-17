from __future__ import annotations

import re
import time
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from .log import Logger
from .process import ProcessResult, run_process


class DeviceError(Exception):
    pass


@dataclass
class AdbDevice:
    serial: str
    state: str


@dataclass
class EnvReport:
    serial: str
    product: str
    super_bytes: int
    super_ready: bool
    is_userspace: bool
    anti: str | None


class DeviceSession:
    def __init__(
        self,
        fastboot: Path,
        adb: Path,
        logger: Logger,
        *,
        dry_run: bool = False,
    ) -> None:
        self.fastboot = fastboot
        self.adb = adb
        self.logger = logger
        self.dry_run = dry_run
        self.serial: str | None = None

    def _fb_args(self, args: list[str]) -> list[str]:
        if self.serial:
            return ["-s", self.serial, *args]
        return list(args)

    def run_fastboot(
        self,
        args: list[str],
        *,
        label: str | None = None,
        allow_failure: bool = False,
        show_output: bool = False,
        timeout: float | None = None,
    ) -> ProcessResult:
        if self.dry_run and args and args[0] not in ("devices", "getvar"):
            # Still allow getvar/devices in dry-run for real checks if device present;
            # mutating ops are simulated.
            mutating = args[0] in (
                "flash",
                "erase",
                "reboot",
                "wipe-super",
                "set_active",
                "boot",
                "flashing",
                "oem",
                "snapshot-update",
                "delete-logical-partition",
                "create-logical-partition",
                "resize-logical-partition",
            )
            if mutating:
                msg = f"[dry-run] fastboot {' '.join(self._fb_args(args))}"
                self.logger.write(msg)
                if label:
                    print(f"  ├─ {label} (dry-run)")
                return ProcessResult(0, msg, msg, "")

        if label:
            print(f"  ├─ {label}")
        result = run_process(
            self.fastboot,
            self._fb_args(args),
            logger=self.logger,
            show_output=show_output or bool(label),
            timeout=timeout,
        )
        if result.code != 0 and not allow_failure:
            detail = result.text or f"exit {result.code}"
            raise DeviceError(
                f"{label or 'fastboot'} 失败（{result.code}）：{' '.join(args)}\n{detail}"
            )
        return result

    def getvar(self, name: str, *, allow_missing: bool = False) -> str | None:
        result = self.run_fastboot(["getvar", name], allow_failure=True)
        match = re.search(re.escape(name) + r":\s*(\S+)", result.text or "")
        if not match:
            if allow_missing:
                return None
            raise DeviceError(f"无法读取 fastboot 变量：{name}\n{result.text}")
        return match.group(1)

    def list_fastboot_serials(self) -> list[str]:
        result = run_process(self.fastboot, ["devices"], logger=self.logger)
        devices: list[str] = []
        for line in (result.text or "").splitlines():
            m = re.match(r"^(\S+)\s+fastboot", line.strip())
            if m:
                devices.append(m.group(1))
        return devices

    def list_adb_devices(self) -> list[AdbDevice]:
        if not self.adb.is_file():
            return []
        result = run_process(self.adb, ["devices"], logger=self.logger)
        devices: list[AdbDevice] = []
        for line in (result.text or "").splitlines():
            m = re.match(r"^(\S+)\s+(device|recovery|sideload)$", line.strip())
            if m:
                devices.append(AdbDevice(serial=m.group(1), state=m.group(2)))
        return devices

    def wait_for_any_fastboot(self, timeout_seconds: int = 90) -> list[str]:
        for _ in range(timeout_seconds):
            devices = self.list_fastboot_serials()
            if devices:
                return devices
            time.sleep(1)
        raise DeviceError("等待设备进入 fastboot 超时。")

    def wait_for_mode(self, want_userspace: bool, timeout_seconds: int = 90) -> None:
        for _ in range(timeout_seconds):
            time.sleep(1)
            result = self.run_fastboot(["getvar", "is-userspace"], allow_failure=True)
            if result.code == 0:
                is_us = bool(re.search(r"is-userspace:\s*yes", result.text or "", re.I))
                if is_us == want_userspace:
                    return
        if want_userspace:
            raise DeviceError("等待 fastbootd 超时。")
        raise DeviceError("等待 bootloader fastboot 超时。")

    def select_device(
        self,
        *,
        choose: Callable[[str, list[str]], int] | None = None,
    ) -> str:
        """
        choose(title, items) -> index, last item is cancel when multi-select.
        If choose is None, auto-pick single device or raise.
        """
        if not self.fastboot.is_file():
            raise DeviceError(f"缺少内置 fastboot：{self.fastboot}")

        self.serial = None
        devices = self.list_fastboot_serials()
        if not devices:
            adb_devices = self.list_adb_devices()
            if not adb_devices:
                raise DeviceError(
                    "未发现设备。请连接处于 Android、Recovery、bootloader fastboot 或 fastbootd 的手机。"
                )
            adb_dev = adb_devices[0]
            if len(adb_devices) > 1:
                if not choose:
                    raise DeviceError("检测到多个 ADB 设备，请在 TUI 中选择。")
                items = [f"使用 ADB 设备：{d.serial} [{d.state}]" for d in adb_devices] + ["取消"]
                idx = choose("检测到多个 ADB 设备", items)
                if idx >= len(adb_devices):
                    raise DeviceError("用户取消设备选择。")
                adb_dev = adb_devices[idx]

            if self.dry_run:
                raise DeviceError(
                    f"Dry-run 需要设备已处于 fastboot/fastbootd。"
                    f"当前仅发现 ADB/{adb_dev.state}（{adb_dev.serial}），不会自动重启设备。"
                )

            print(f"  [自动模式] 检测到 ADB/{adb_dev.state}，正在切换到 bootloader fastboot...")
            self.logger.write(f"Auto transition: adb/{adb_dev.state} -> bootloader")
            result = run_process(
                self.adb,
                ["-s", adb_dev.serial, "reboot", "bootloader"],
                logger=self.logger,
            )
            if result.code != 0:
                raise DeviceError(f"无法通过 ADB 自动进入 bootloader：{result.text}")
            devices = self.wait_for_any_fastboot()

        if len(devices) == 1:
            self.serial = devices[0]
            return self.serial

        if not choose:
            raise DeviceError(f"检测到多个 fastboot 设备：{', '.join(devices)}")
        items = [f"使用设备：{s}" for s in devices] + ["取消"]
        idx = choose("检测到多个 fastboot 设备", items)
        if idx >= len(devices):
            raise DeviceError("用户取消设备选择。")
        self.serial = devices[idx]
        return self.serial


def parse_size(value: str) -> int:
    value = value.strip()
    if value.lower().startswith("0x"):
        return int(value, 16)
    return int(value)
