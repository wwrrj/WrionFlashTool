"""Physical super GPT expansion / restore (Recovery + sgdisk).

Ported from Expand-SuperTo16G.ps1 / Restore-OfficialPartitionLayout.ps1.
All device names, sizes, and paths come from FlashProfile.expansion.
"""

from __future__ import annotations

import re
import shutil
import tempfile
import time
import uuid
from collections.abc import Callable
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

from .config import ExpansionConfig, FlashProfile
from .log import Logger
from .process import run_process


class ExpandError(Exception):
    pass


@dataclass
class PartInfo:
    number: int
    code: str
    guid: str
    first: int
    last: int
    name: str
    lines: list[str]


@dataclass
class ExpandPlan:
    disk: str
    print_before: list[str]
    sector_bytes: int
    disk_sectors: int
    last_usable_sector: int
    target_bytes: int
    target_sectors: int
    super_info: PartInfo
    userdata_info: PartInfo
    current_super_bytes: int
    new_super_last: int
    new_userdata_first: int
    new_userdata_bytes: int
    super_part: int
    userdata_part: int


class ExpandSession:
    def __init__(
        self,
        profile: FlashProfile,
        logger: Logger,
        *,
        on_line: Callable[[str], None] | None = None,
    ) -> None:
        if not profile.expansion or not profile.expansion.enabled:
            raise ExpandError("当前配置未启用物理扩容（[expansion].enabled）。")
        self.profile = profile
        self.cfg: ExpansionConfig = profile.expansion
        self.logger = logger
        self.on_line = on_line
        self.device: str = ""
        self.sgdisk_cmd: str = "sgdisk"
        self.adb = profile.adb
        self.fastboot = profile.fastboot
        if not self.adb.is_file():
            raise ExpandError(f"缺少 adb：{self.adb}")
        if not self.fastboot.is_file():
            raise ExpandError(f"缺少 fastboot：{self.fastboot}")
        if not (self.cfg.sgdisk_dir / "sgdisk").is_file():
            # Recovery may still have system sgdisk; only warn via later init_sgdisk
            pass

    def _emit(self, text: str) -> None:
        print(f"  {text}")
        self.logger.write(text)
        if self.on_line:
            self.on_line(text)

    def adb_run(
        self,
        args: list[str],
        *,
        allow_fail: bool = False,
        timeout: float | None = 120.0,
    ) -> str:
        try:
            result = run_process(
                self.adb,
                args,
                logger=self.logger,
                show_output=False,
                timeout=timeout,
            )
        except TimeoutError as exc:
            if allow_fail:
                self.logger.write(f"adb timeout (allowed): {args} — {exc}")
                return ""
            raise ExpandError(f"ADB 超时：adb {' '.join(args)}") from exc
        if result.code != 0 and not allow_fail:
            raise ExpandError(
                f"ADB 失败（{result.code}）：adb {' '.join(args)}\n{result.text}"
            )
        return result.text or ""

    def adb_shell(
        self,
        command: str,
        *,
        allow_fail: bool = False,
        timeout: float | None = 180.0,
    ) -> str:
        return self.adb_run(["shell", command], allow_fail=allow_fail, timeout=timeout)

    def fb_run(self, args: list[str], *, allow_fail: bool = False, timeout: float | None = 60.0) -> str:
        try:
            result = run_process(
                self.fastboot,
                args,
                logger=self.logger,
                show_output=False,
                timeout=timeout,
            )
        except TimeoutError as exc:
            if allow_fail:
                return ""
            raise ExpandError(f"fastboot 超时：fastboot {' '.join(args)}") from exc
        if result.code != 0 and not allow_fail:
            raise ExpandError(
                f"fastboot 失败（{result.code}）：fastboot {' '.join(args)}\n{result.text}"
            )
        return result.text or ""

    def adb_transport_state(self) -> str:
        # Short timeout — used in tight poll loops
        text = self.adb_run(["devices"], allow_fail=True, timeout=15.0)
        matches = re.findall(r"(?m)^(\S+)\s+(recovery|device|sideload)\s*$", text)
        if len(matches) > 1:
            raise ExpandError("检测到多个 ADB 设备，请只连接一台手机后重试。")
        if len(matches) == 1:
            return matches[0][1]
        return ""

    def adb_devices_raw(self) -> str:
        return self.adb_run(["devices"], allow_fail=True, timeout=15.0)

    def fastboot_count(self) -> int:
        text = self.fb_run(["devices"], allow_fail=True, timeout=15.0)
        return len(re.findall(r"(?m)^\S+\s+fastboot\s*$", text))

    def wait_for_fastboot(self, timeout_seconds: int = 90) -> None:
        """Wait for exactly one fastboot device after leaving Recovery."""
        self._emit(f"正在等待 bootloader fastboot（最长 {timeout_seconds} 秒）…")
        deadline = time.monotonic() + timeout_seconds
        while time.monotonic() < deadline:
            count = self.fastboot_count()
            if count == 1:
                self._emit("bootloader fastboot 已就绪。")
                return
            if count > 1:
                raise ExpandError("检测到多个 fastboot 设备，请只连接一台手机后重试。")
            time.sleep(2)
        raise ExpandError("等待 bootloader fastboot 超时。")

    def verify_fastboot_product(self) -> str:
        text = self.fb_run(["getvar", "product"], allow_fail=True, timeout=20.0)
        match = re.search(r"(?im)^product:\s*(\S+)\s*$", text)
        if not match:
            raise ExpandError(f"无法读取 fastboot product。\n{text}")
        product = match.group(1)
        if product not in self.cfg.codenames:
            raise ExpandError(
                f"机型不匹配：{product}。扩容支持：{', '.join(self.cfg.codenames)}"
            )
        return product

    def reboot_to_recovery_and_wait(self, *, from_fastboot: bool = False) -> None:
        """
        Reboot into Recovery and wait until root ADB + super node are ready.
        Never use bare `adb wait-for-device` (it can hang forever after reboot).
        """
        self._emit("正在发送重启到 Recovery 的指令…")
        if from_fastboot:
            # reboot often returns non-zero as link drops — always allow fail + timeout
            self.fb_run(["reboot", "recovery"], allow_fail=True, timeout=20.0)
        else:
            # adb reboot blocks until disconnect; device death mid-command is normal
            self.adb_run(["reboot", "recovery"], allow_fail=True, timeout=25.0)

        self._emit("已发送重启指令，等待设备断开并重新进入 Recovery…")
        # Brief pause so USB disconnect settles
        time.sleep(3)
        self.wait_for_recovery_adb(timeout_seconds=180)

    def wait_for_recovery_adb(self, timeout_seconds: int = 180) -> None:
        """Poll until Recovery ADB is usable; print progress so UI never looks frozen."""
        self._emit(f"正在等待 Recovery ADB（最长 {timeout_seconds} 秒）…")
        deadline = time.monotonic() + timeout_seconds
        last_status = ""
        n = 0
        while time.monotonic() < deadline:
            n += 1
            elapsed = int(timeout_seconds - (deadline - time.monotonic()))
            remaining = max(0, int(deadline - time.monotonic()))

            raw = self.adb_devices_raw()
            state = ""
            m = re.search(r"(?m)^(\S+)\s+(recovery|device|sideload|offline|unauthorized)\s*$", raw)
            if m:
                state = m.group(2)

            status = f"[{elapsed}s / 剩余约 {remaining}s] adb 状态={state or '无设备'}"
            if state == "recovery":
                probe = self.adb_shell(
                    "id -u; getprop ro.product.device; "
                    "test -e /dev/block/by-name/super && echo SUPER_READY",
                    allow_fail=True,
                    timeout=20.0,
                )
                uid_ok = bool(re.search(r"(?m)^0\s*$", probe))
                super_ok = "SUPER_READY" in probe
                status += f"  root={uid_ok}  super节点={super_ok}"
                if uid_ok and super_ok:
                    self._emit("Recovery ADB 已就绪（root + super 节点可用）。")
                    return
                if not uid_ok:
                    status += "（等待 root shell）"
                elif not super_ok:
                    status += "（等待 /dev/block/by-name/super）"
            elif state == "device":
                status += "（系统模式，仍在重启中…）"
            elif state == "unauthorized":
                status += "（请在手机上允许 USB 调试）"
            elif state == "offline":
                status += "（offline，稍后重试）"

            if status != last_status or n % 5 == 1:
                self._emit(status)
                last_status = status

            time.sleep(2)

        raise ExpandError(
            "等待 Recovery ADB 超时。\n"
            "请确认：手机已进入 Recovery、USB 连接正常、Recovery 已开启 ADB，然后重试。"
        )

    def ensure_recovery(self) -> str:
        cfg = self.cfg
        adb_state = self.adb_transport_state()
        if adb_state == "recovery":
            self._emit("设备已处于 Recovery。")
        elif adb_state in ("device", "sideload"):
            self._emit(f"检测到 ADB/{adb_state}，正在重启到 Recovery...")
            self.reboot_to_recovery_and_wait(from_fastboot=False)
        else:
            fb_count = self.fastboot_count()
            if fb_count == 0:
                raise ExpandError("未检测到 Recovery ADB 或 fastboot 设备。")
            if fb_count > 1:
                raise ExpandError("检测到多个 fastboot 设备，请只连接一台手机后重试。")
            product_text = self.fb_run(["getvar", "product"], allow_fail=True)
            m = re.search(r"(?im)^product:\s*(\S+)\s*$", product_text)
            if not m:
                raise ExpandError(f"无法读取 fastboot product。\n{product_text}")
            product = m.group(1)
            if product not in cfg.codenames:
                raise ExpandError(
                    f"机型不匹配：{product}。扩容支持：{', '.join(cfg.codenames)}"
                )
            self._emit(f"检测到 {product} 的 fastboot，正在重启到 Recovery...")
            self.reboot_to_recovery_and_wait(from_fastboot=True)

        device = self.adb_shell("getprop ro.product.device", allow_fail=True).strip().splitlines()
        device = device[0].strip() if device else ""
        if device not in cfg.codenames:
            raise ExpandError(f"Recovery 中机型不匹配：{device}")
        self.device = device
        self._emit(f"已确认设备：{device}")
        return device

    def init_sgdisk(self) -> None:
        uid = self.adb_shell("id -u", allow_fail=True).strip().splitlines()
        uid_text = uid[0].strip() if uid else ""
        if uid_text != "0":
            raise ExpandError(
                f"当前 ADB shell 不是 root（uid={uid_text}）。"
                "请进入支持 ADB 的 Recovery 后再操作；禁止在正常系统中改 GPT。"
            )

        system_probe = self.adb_shell(
            "command -v sgdisk >/dev/null 2>&1 && sgdisk --version 2>&1",
            allow_fail=True,
        )
        if "GPT fdisk" in system_probe:
            self.sgdisk_cmd = "sgdisk"
            self._emit("使用 Recovery 内置 sgdisk。")
            return

        required = ["sgdisk", "libext2_uuid.so", "libc++.so"]
        for name in required:
            local = self.cfg.sgdisk_dir / name
            if not local.is_file():
                raise ExpandError(
                    f"Recovery 未提供 sgdisk，内置工具也不完整：缺少 {local}"
                )

        remote = self.cfg.remote_sgdisk_dir
        self.adb_shell(f"rm -rf {remote}; mkdir -p {remote}")
        for name in required:
            self.adb_run(["push", str(self.cfg.sgdisk_dir / name), f"{remote}/{name}"])
        self.adb_shell(
            f"chmod 755 {remote}/sgdisk; chmod 644 {remote}/libext2_uuid.so {remote}/libc++.so"
        )
        self.sgdisk_cmd = (
            f"LD_LIBRARY_PATH={remote}:/system/lib64:/vendor/lib64 {remote}/sgdisk"
        )
        probe = self.adb_shell(f"{self.sgdisk_cmd} --version 2>&1", allow_fail=True)
        if "GPT fdisk" not in probe:
            raise ExpandError(f"内置 sgdisk 无法在当前 Recovery 中运行。\n{probe}")
        self._emit("Recovery 未内置 sgdisk，已启用包内 ARM64 版本。")

    def pull_file(self, remote: str, local: Path) -> None:
        """Pull via ASCII temp path to avoid Chinese path issues with adb on Windows."""
        local.parent.mkdir(parents=True, exist_ok=True)
        staging_root = Path(tempfile.gettempdir()) / "harusora-adb-pull"
        staging_root.mkdir(parents=True, exist_ok=True)
        staging = staging_root / f"{uuid.uuid4().hex}.bin"
        try:
            self.adb_run(["pull", remote, str(staging)])
            if not staging.is_file():
                raise ExpandError(f"ADB pull 未生成文件：{remote}")
            shutil.move(str(staging), str(local))
        finally:
            if staging.exists():
                staging.unlink(missing_ok=True)

    def push_file(self, local: Path, remote: str) -> None:
        staging_root = Path(tempfile.gettempdir()) / "harusora-adb-push"
        staging_root.mkdir(parents=True, exist_ok=True)
        staging = staging_root / f"{uuid.uuid4().hex}{local.suffix or '.bin'}"
        try:
            shutil.copy2(local, staging)
            self.adb_run(["push", str(staging), remote])
        finally:
            if staging.exists():
                staging.unlink(missing_ok=True)

    def parse_part_info(self, part_number: int, disk: str) -> PartInfo:
        text = self.adb_shell(f"{self.sgdisk_cmd} --info={part_number} {disk}")
        lines = text.splitlines()
        joined = "\n".join(lines)
        code = re.search(r"Partition GUID code:\s*([0-9A-Fa-f-]+)", joined)
        guid = re.search(r"Partition unique GUID:\s*([0-9A-Fa-f-]+)", joined)
        first = re.search(r"First sector:\s*(\d+)", joined)
        last = re.search(r"Last sector:\s*(\d+)", joined)
        name = re.search(r"Partition name:\s*'([^']*)'", joined)
        if not (code and guid and first and last and name):
            raise ExpandError(f"无法解析分区 {part_number} 的 sgdisk info。\n{joined}")
        return PartInfo(
            number=part_number,
            code=code.group(1).upper(),
            guid=guid.group(1).upper(),
            first=int(first.group(1)),
            last=int(last.group(1)),
            name=name.group(1),
            lines=lines,
        )

    def parse_disk_geometry(self, print_text: str) -> tuple[int, int, int, int]:
        sector = re.search(
            r"Sector size \(logical/physical\):\s*(\d+)/(\d+) bytes", print_text
        )
        last_u = re.search(r"last usable sector is\s*(\d+)", print_text)
        disk_sec = re.search(r"Disk\s+\S+:\s*(\d+)\s+sectors", print_text)
        if not (sector and last_u and disk_sec):
            raise ExpandError("无法从 sgdisk --print 解析磁盘几何信息。")
        return (
            int(sector.group(1)),
            int(sector.group(2)),
            int(last_u.group(1)),
            int(disk_sec.group(1)),
        )

    def build_plan(self) -> ExpandPlan:
        cfg = self.cfg
        print_text = self.adb_shell(f"{self.sgdisk_cmd} --print {cfg.disk}")
        print_lines = print_text.splitlines()
        logical, _phys, last_usable, disk_sectors = self.parse_disk_geometry(print_text)
        super_info = self.parse_part_info(cfg.super_part, cfg.disk)
        userdata_info = self.parse_part_info(cfg.userdata_part, cfg.disk)

        if super_info.name != "super":
            raise ExpandError(
                f"分区 {cfg.super_part} 名称是 '{super_info.name}'，不是 super。"
            )
        if userdata_info.name != "userdata":
            raise ExpandError(
                f"分区 {cfg.userdata_part} 名称是 '{userdata_info.name}'，不是 userdata。"
            )
        if (super_info.last + 1) != userdata_info.first:
            raise ExpandError(
                f"super 与 userdata 不相邻：super_last={super_info.last}, "
                f"userdata_first={userdata_info.first}。"
            )
        if cfg.target_bytes % logical != 0:
            raise ExpandError(
                f"target_bytes={cfg.target_bytes} 不能被扇区大小 {logical} 整除。"
            )

        target_sectors = cfg.target_bytes // logical
        new_super_last = super_info.first + target_sectors - 1
        new_userdata_first = new_super_last + 1
        if new_super_last > last_usable:
            raise ExpandError(
                f"目标 super 末端 {new_super_last} 超过 last usable {last_usable}。"
            )
        if new_userdata_first > userdata_info.last:
            raise ExpandError(
                f"目标 super 会吞掉全部 userdata：new_userdata_first={new_userdata_first}。"
            )

        current_super = (super_info.last - super_info.first + 1) * logical
        new_ud_bytes = (userdata_info.last - new_userdata_first + 1) * logical

        return ExpandPlan(
            disk=cfg.disk,
            print_before=print_lines,
            sector_bytes=logical,
            disk_sectors=disk_sectors,
            last_usable_sector=last_usable,
            target_bytes=cfg.target_bytes,
            target_sectors=target_sectors,
            super_info=super_info,
            userdata_info=userdata_info,
            current_super_bytes=current_super,
            new_super_last=new_super_last,
            new_userdata_first=new_userdata_first,
            new_userdata_bytes=new_ud_bytes,
            super_part=cfg.super_part,
            userdata_part=cfg.userdata_part,
        )

    def show_plan(self, plan: ExpandPlan) -> None:
        self._emit("分区方案：")
        self._emit(f"  disk: {plan.disk}")
        self._emit(f"  logical sector: {plan.sector_bytes} bytes")
        self._emit(
            f"  current super: part {plan.super_part}, "
            f"{plan.super_info.first}-{plan.super_info.last}, {plan.current_super_bytes} bytes"
        )
        self._emit(
            f"  target super:  part {plan.super_part}, "
            f"{plan.super_info.first}-{plan.new_super_last}, {plan.target_bytes} bytes"
        )
        self._emit(
            f"  current userdata: part {plan.userdata_part}, "
            f"{plan.userdata_info.first}-{plan.userdata_info.last}"
        )
        self._emit(
            f"  target userdata:  part {plan.userdata_part}, "
            f"{plan.new_userdata_first}-{plan.userdata_info.last}, {plan.new_userdata_bytes} bytes"
        )
        if plan.current_super_bytes == plan.target_bytes:
            self._emit("  status: super 已是目标大小。")
        elif plan.current_super_bytes > plan.target_bytes:
            self._emit("  status: 当前 super 大于目标，脚本不会缩小。")
        else:
            self._emit("  status: 扩容需移动 userdata 起始扇区，并清空 /data。")

    def _save_lines(self, path: Path, content: str | list[str]) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, list):
            text = "\n".join(content)
        else:
            text = content
        path.write_text(text + ("" if text.endswith("\n") or not text else "\n"), encoding="utf-8")

    def _backup_block_if_exists(self, by_name: str, out_dir: Path) -> None:
        exists = self.adb_shell(
            f"[ -e /dev/block/by-name/{by_name} ] && echo yes || echo no",
            allow_fail=True,
        ).strip()
        if exists.splitlines()[0].strip() != "yes":
            return
        remote = f"/tmp/harusora-{by_name}.img"
        self.adb_shell(
            f"rm -f {remote}; dd if=/dev/block/by-name/{by_name} of={remote} "
            f"bs=1048576 2>/tmp/harusora-{by_name}.dd.log; cat /tmp/harusora-{by_name}.dd.log",
            allow_fail=True,
        )
        self.pull_file(remote, out_dir / f"{by_name}.img")
        self.adb_shell(
            f"rm -f {remote} /tmp/harusora-{by_name}.dd.log",
            allow_fail=True,
        )

    def create_backup(self, plan: ExpandPlan) -> Path:
        stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
        device_name = self.device or "unknown-device"
        out = self.cfg.backup_dir / f"{device_name}-super-expand-{stamp}"
        out.mkdir(parents=True, exist_ok=True)
        self._emit(f"正在备份到：{out}")

        self._save_lines(out / "adb-devices.txt", self.adb_run(["devices"], allow_fail=True))
        self._save_lines(
            out / "device-info.txt",
            self.adb_shell(
                "id; getprop ro.product.device; getprop ro.product.model; "
                "getprop ro.product.vendor.device; getprop ro.boot.slot_suffix; "
                "getprop ro.crypto.state; getprop ro.crypto.type; uname -a",
                allow_fail=True,
            ),
        )
        self._save_lines(out / "sgdisk-print-before.txt", plan.print_before)
        self._save_lines(out / "sgdisk-info-super-before.txt", plan.super_info.lines)
        self._save_lines(
            out / "sgdisk-info-userdata-before.txt", plan.userdata_info.lines
        )
        self._save_lines(
            out / "sgdisk-verify-before.txt",
            self.adb_shell(f"{self.sgdisk_cmd} --verify {plan.disk} 2>&1", allow_fail=True),
        )
        self._save_lines(
            out / "lpdump-before.txt",
            self.adb_shell("lpdump 2>/dev/null || true", allow_fail=True),
        )
        layout_cmd = (
            'ls -l /dev/block/by-name; for p in /sys/class/block/sda*/partition; do '
            'b=${p%/partition}; n=${b##*/}; echo "$n $(cat $b/start) $(cat $b/size) $(cat $p)"; '
            "done | sort -k2,2n"
        )
        self._save_lines(out / "by-name-and-sda-layout-before.txt", self.adb_shell(layout_cmd, allow_fail=True))
        self._save_lines(
            out / "block-sizes-before.txt",
            self.adb_shell(
                f"blockdev --getsize64 {plan.disk}; blockdev --getss {plan.disk} 2>/dev/null || true; "
                "cat /sys/class/block/sda/queue/logical_block_size; cat /sys/class/block/sda/size",
                allow_fail=True,
            ),
        )
        self._save_lines(out / "mount-before.txt", self.adb_shell("mount", allow_fail=True))
        self._save_lines(out / "getprop-before.txt", self.adb_shell("getprop", allow_fail=True))

        remote_gpt = "/tmp/harusora-gpt-sda-before.bin"
        self.adb_shell(
            f"rm -f {remote_gpt}; {self.sgdisk_cmd} --backup={remote_gpt} {plan.disk}; ls -l {remote_gpt}"
        )
        self.pull_file(remote_gpt, out / "gpt-sda-before.bin")

        raw_count = 16777216 // plan.sector_bytes
        tail_skip = plan.disk_sectors - raw_count
        self.adb_shell(
            f"rm -f /tmp/harusora-sda-head-16m.img /tmp/harusora-sda-tail-16m.img; "
            f"dd if={plan.disk} of=/tmp/harusora-sda-head-16m.img bs={plan.sector_bytes} count={raw_count}; "
            f"dd if={plan.disk} of=/tmp/harusora-sda-tail-16m.img bs={plan.sector_bytes} "
            f"skip={tail_skip} count={raw_count}"
        )
        self.pull_file("/tmp/harusora-sda-head-16m.img", out / "sda-head-16m.img")
        self.pull_file("/tmp/harusora-sda-tail-16m.img", out / "sda-tail-16m.img")
        self.adb_shell(
            f"rm -f {remote_gpt} /tmp/harusora-sda-head-16m.img /tmp/harusora-sda-tail-16m.img",
            allow_fail=True,
        )

        for name in (
            "persist",
            "metadata",
            "misc",
            "frp",
            "vbmeta_a",
            "vbmeta_b",
            "vbmeta_system_a",
            "vbmeta_system_b",
        ):
            self._backup_block_if_exists(name, out)

        plan_lines = [
            f"disk={plan.disk}",
            f"device={device_name}",
            f"sector_bytes={plan.sector_bytes}",
            f"target_bytes={plan.target_bytes}",
            f"target_sectors={plan.target_sectors}",
            f"super_old={plan.super_info.first}-{plan.super_info.last}",
            f"super_new={plan.super_info.first}-{plan.new_super_last}",
            f"userdata_old={plan.userdata_info.first}-{plan.userdata_info.last}",
            f"userdata_new={plan.new_userdata_first}-{plan.userdata_info.last}",
            "backup_super=False",
        ]
        self._save_lines(out / "plan.txt", plan_lines)
        self._emit(f"备份完成：{out}")
        return out

    def prepare_plan(self) -> ExpandPlan:
        """Connect Recovery, init sgdisk, compute layout (no GPT write)."""
        self.ensure_recovery()
        self.init_sgdisk()
        plan = self.build_plan()
        self.show_plan(plan)
        return plan

    def _shell_step(
        self,
        title: str,
        command: str,
        *,
        critical: bool = False,
        timeout: float = 120.0,
    ) -> str:
        """Run one adb shell command at a time (never chain umount||exit with sgdisk)."""
        self._emit(f"→ {title}")
        self.logger.write(f"step: {title} :: {command}")
        try:
            # non-critical: never raise (umount busy is normal)
            # critical: raise on non-zero adb exit
            out = self.adb_shell(command, allow_fail=not critical, timeout=timeout)
        except ExpandError as exc:
            if critical:
                raise ExpandError(f"步骤失败：{title}\n{exc}") from exc
            out = str(exc)
        for line in (out or "").splitlines():
            if line.strip():
                print(f"     {line}")
        return out or ""

    def soft_unmount_data_metadata(self) -> None:
        """Best-effort unmount only — busy /data is common on TWRP and is NOT fatal."""
        self._emit("尝试卸载 /data、/metadata（失败可忽略，继续写 GPT）…")
        # One command per call so a busy mount never aborts the whole expand
        for title, cmd in (
            ("sync", "sync"),
            ("lazy umount /sdcard", "umount -l /sdcard 2>/dev/null; true"),
            ("lazy umount /data", "umount -l /data 2>/dev/null; true"),
            ("umount /data", "umount /data 2>/dev/null; true"),
            ("lazy umount /metadata", "umount -l /metadata 2>/dev/null; true"),
            ("umount /metadata", "umount /metadata 2>/dev/null; true"),
            ("sync again", "sync"),
        ):
            self._shell_step(title, cmd, critical=False, timeout=30.0)
        # show mount state for log only
        self._shell_step(
            "查看挂载状态",
            "grep -E ' /data | /metadata ' /proc/mounts 2>/dev/null || echo 'data/metadata: not mounted'",
            critical=False,
            timeout=15.0,
        )

    def wipe_data_metadata_before_gpt(self) -> None:
        """Wipe data/metadata, reboot Recovery, then require both to be unmounted.

        TWRP owns several /data users, so a plain umount can remain busy forever.
        Let Recovery perform its native wipe first. If that command is unavailable
        or fails, use bootloader fastboot erase as the deterministic fallback.
        """
        self._emit("写入 GPT 前先格式化 userdata / metadata…")
        probe = self.adb_shell(
            "command -v twrp >/dev/null 2>&1 && echo TWRP_READY",
            allow_fail=True,
            timeout=15.0,
        )
        wiped_by_twrp = False
        if "TWRP_READY" in probe:
            script = """#!/system/bin/sh
set +e
sync
twrp unmount data >/tmp/harusora-twrp-data-unmount.log 2>&1
twrp unmount metadata >/tmp/harusora-twrp-metadata-unmount.log 2>&1
twrp wipe data >/tmp/harusora-twrp-data-wipe.log 2>&1
DATA_EC=$?
twrp wipe metadata >/tmp/harusora-twrp-metadata-wipe.log 2>&1
META_EC=$?
cat /tmp/harusora-twrp-data-unmount.log
cat /tmp/harusora-twrp-metadata-unmount.log
cat /tmp/harusora-twrp-data-wipe.log
cat /tmp/harusora-twrp-metadata-wipe.log
echo "HARUSORA_TWRP_WIPE data=$DATA_EC metadata=$META_EC"
rm -f /tmp/harusora-twrp-*-unmount.log /tmp/harusora-twrp-*-wipe.log
sync
exit 0
"""
            out = self._run_device_script("wipe-before-gpt", script, timeout=600.0)
            self.logger.write(out)
            for line in out.splitlines():
                if line.strip():
                    print(f"     {line}")
            wiped_by_twrp = "HARUSORA_TWRP_WIPE data=0 metadata=0" in out

        if wiped_by_twrp:
            self._emit("Recovery 已完成 userdata / metadata 格式化。")
            self.reboot_to_recovery_and_wait(from_fastboot=False)
        else:
            self._emit("Recovery 原生格式化不可用，改用 bootloader fastboot erase…")
            self.adb_run(["reboot", "bootloader"], allow_fail=True, timeout=25.0)
            time.sleep(3)
            self.wait_for_fastboot()
            product = self.verify_fastboot_product()
            self._emit(f"已确认 fastboot 设备：{product}")
            for partition in ("userdata", "metadata"):
                self._emit(f"正在擦除 {partition}…")
                self.fb_run(["erase", partition], allow_fail=False, timeout=300.0)
            self.reboot_to_recovery_and_wait(from_fastboot=True)

        self.init_sgdisk()
        self.soft_unmount_data_metadata()
        mounts = self.adb_shell(
            "grep -E ' /data | /metadata ' /proc/mounts 2>/dev/null || true",
            allow_fail=True,
            timeout=15.0,
        ).strip()
        if mounts:
            raise ExpandError(
                "格式化并重启 Recovery 后，userdata/metadata 仍被挂载，拒绝写入 GPT。\n"
                f"{mounts}"
            )
        self._emit("userdata / metadata 已释放，可以安全修改 GPT。")

    def _run_device_script(self, name: str, script_body: str, *, timeout: float = 120.0) -> str:
        """Push a small shell script to /tmp and execute it (avoids quoting hell)."""
        remote = f"/tmp/harusora-{name}.sh"
        staging = Path(tempfile.gettempdir()) / f"harusora-{name}-{uuid.uuid4().hex}.sh"
        try:
            staging.write_text(script_body.replace("\r\n", "\n"), encoding="utf-8", newline="\n")
            self.push_file(staging, remote)
            self.adb_shell(f"chmod 755 {remote}", allow_fail=True, timeout=15.0)
            out = self.adb_shell(f"sh {remote}; EC=$?; rm -f {remote}; exit $EC", allow_fail=True, timeout=timeout)
            return out
        finally:
            if staging.exists():
                staging.unlink(missing_ok=True)

    def force_unmount_partition(
        self,
        by_name: str,
        mount_hints: tuple[str, ...] = (),
    ) -> None:
        """Aggressively free a block device so make_f2fs will not see 'In use by the system'."""
        hints_list = " ".join(mount_hints)
        script = f"""#!/system/bin/sh
set +e
NAME="{by_name}"
HINTS="{hints_list}"
BYNODE="/dev/block/by-name/$NAME"
BLK=$(readlink -f "$BYNODE" 2>/dev/null)
echo "UNMOUNT_TARGET name=$NAME blk=$BLK"

killall -9 recovery-refresh 2>/dev/null
killall -9 updater 2>/dev/null

# Nested / fuse mounts first
for mp in /sdcard /storage/emulated /storage/self/primary /mnt/sdcard /mnt/user/0/emulated; do
  umount -l "$mp" 2>/dev/null
  umount -f "$mp" 2>/dev/null
  umount "$mp" 2>/dev/null
done

for round in 1 2 3 4 5 6 7 8 9 10 11 12; do
  MPS=""
  if [ -n "$BLK" ]; then
    MPS=$(awk -v b="$BLK" -v n="$BYNODE" '
      $1 == b || $1 == n {{ print length($2), $2 }}
    ' /proc/mounts 2>/dev/null | sort -rn | awk '{{ print $2 }}')
  fi
  # Also catch mountpoints by path even if device string differs slightly
  EXTRA=$(awk '
    $2 == "/data" || $2 == "/metadata" || $2 == "/sdcard" {{ print length($2), $2 }}
  ' /proc/mounts 2>/dev/null | sort -rn | awk '{{ print $2 }}')
  if [ -n "$EXTRA" ]; then
    MPS=$(printf "%s\\n%s\\n" "$MPS" "$EXTRA" | awk 'NF' | awk '!a[$0]++')
  fi
  if [ -z "$MPS" ]; then
    # path hints still mounted?
    busy=0
    for mp in $HINTS; do
      if grep -q " $mp " /proc/mounts 2>/dev/null; then busy=1; break; fi
    done
    if [ "$busy" -eq 0 ]; then
      break
    fi
  fi
  echo "ROUND $round:"
  echo "$MPS"
  echo "$MPS" | while read -r mp; do
    [ -z "$mp" ] && continue
    echo "  umount $mp"
    command -v fuser >/dev/null 2>&1 && fuser -km "$mp" 2>/dev/null
    umount "$mp" 2>/dev/null
    umount -d "$mp" 2>/dev/null
    umount -f "$mp" 2>/dev/null
    umount -l "$mp" 2>/dev/null
  done
  for mp in $HINTS; do
    command -v fuser >/dev/null 2>&1 && fuser -km "$mp" 2>/dev/null
    umount "$mp" 2>/dev/null
    umount -f "$mp" 2>/dev/null
    umount -l "$mp" 2>/dev/null
  done
  if [ -n "$BLK" ]; then
    umount "$BLK" 2>/dev/null
    umount -l "$BLK" 2>/dev/null
  fi
  umount "$BYNODE" 2>/dev/null
  umount -l "$BYNODE" 2>/dev/null
  sleep 1
done

echo "--- remaining relevant mounts ---"
grep -E "userdata|metadata|/data|/sdcard|/metadata|$BLK" /proc/mounts 2>/dev/null || echo "(none)"

if [ -n "$BLK" ] && grep -q "$BLK" /proc/mounts 2>/dev/null; then
  echo "UNMOUNT_FAIL still_busy=$BLK"
  exit 11
fi
# /data path still mounted on something else?
if grep -q " /data " /proc/mounts 2>/dev/null && [ "$NAME" = "userdata" ]; then
  echo "UNMOUNT_FAIL /data still mounted"
  exit 12
fi
if grep -q " /metadata " /proc/mounts 2>/dev/null && [ "$NAME" = "metadata" ]; then
  echo "UNMOUNT_FAIL /metadata still mounted"
  exit 13
fi
echo "UNMOUNT_OK"
exit 0
"""
        self._emit(f"正在强制卸载 {by_name}（释放占用后才能格式化）…")
        out = self._run_device_script(f"umount-{by_name}", script, timeout=120.0)
        for line in out.splitlines():
            if line.strip():
                print(f"     {line}")
        self.logger.write(out)
        if "UNMOUNT_OK" not in out:
            self._emit(f"警告：{by_name} 可能仍有挂载残留，将再尝试懒卸载…")
            self.adb_shell(
                "umount -l /data /metadata /sdcard /storage/emulated "
                f"/dev/block/by-name/{by_name} 2>/dev/null; sync; sleep 2",
                allow_fail=True,
                timeout=30.0,
            )

    def format_partition_f2fs(
        self,
        by_name: str,
        *,
        mount_hints: tuple[str, ...] = (),
    ) -> bool:
        """Unmount then make_f2fs -f. Returns True on success, False if still busy.

        Failure here only means the filesystem wipe did not finish — it does NOT
        roll back an already-written GPT.
        """
        dev = f"/dev/block/by-name/{by_name}"
        last_out = ""
        for attempt in range(1, 6):
            self.force_unmount_partition(by_name, mount_hints)
            self._emit(f"正在格式化 {by_name}（第 {attempt}/5 次）…")
            script = f"""#!/system/bin/sh
set +e
DEV="{dev}"
BLK=$(readlink -f "$DEV" 2>/dev/null)
echo "format target $DEV -> $BLK"
# Final free
for mp in /data /metadata /sdcard /storage/emulated; do
  umount -l "$mp" 2>/dev/null
  umount -f "$mp" 2>/dev/null
done
umount -l "$DEV" 2>/dev/null
umount -l "$BLK" 2>/dev/null
sync
sleep 1
if command -v make_f2fs >/dev/null 2>&1; then
  make_f2fs -f "$DEV"
  EC=$?
elif command -v mkfs.f2fs >/dev/null 2>&1; then
  mkfs.f2fs -f "$DEV"
  EC=$?
else
  echo "NO_MKFS_F2FS"
  exit 99
fi
echo "MKFS_EXIT=$EC"
exit $EC
"""
            last_out = self._run_device_script(f"mkfs-{by_name}-{attempt}", script, timeout=300.0)
            for line in last_out.splitlines():
                if line.strip():
                    print(f"     {line}")
            self.logger.write(last_out)
            low = last_out.lower()
            if "no_mkfs_f2fs" in low:
                self._emit(f"Recovery 里没有 make_f2fs，跳过自动格式化 {by_name}。")
                return False
            if "in use by the system" in low:
                self._emit(f"{by_name} 仍被占用，稍后重试…")
                time.sleep(2)
                continue
            m = re.search(r"MKFS_EXIT=(\d+)", last_out)
            if m and m.group(1) == "0":
                self._emit(f"{by_name} 格式化完成。")
                return True
            if m and m.group(1) != "0":
                self._emit(f"格式化退出码 {m.group(1)}，准备重试…")
                time.sleep(2)
                continue
            if "Error:" not in last_out and "error:" not in last_out and "In use" not in last_out:
                self._emit(f"{by_name} 格式化完成。")
                return True
            time.sleep(2)

        self._emit(
            f"{by_name} 自动格式化未完成（多半是 Recovery 仍挂着分区）。"
            "这不影响已经写好的 GPT；可在 Recovery 里手动卸载后格式化 Data。"
        )
        self.logger.write(f"format {by_name} incomplete:\n{last_out}")
        return False

    def apply_expand(self, plan: ExpandPlan | None = None) -> Path | None:
        """Backup + write GPT + format data. Pass plan from prepare_plan() when available."""
        if plan is None:
            plan = self.prepare_plan()

        if plan.current_super_bytes > plan.target_bytes:
            raise ExpandError("当前 super 大于目标大小，拒绝缩小。")
        if plan.current_super_bytes == plan.target_bytes:
            self._emit("super 已是目标大小，无需修改 GPT。")
            return None

        backup_dir = self.create_backup(plan)

        # TWRP can keep /data busy through its own services. Format first and
        # restart Recovery so GPT is never rewritten while either partition is mounted.
        self.wipe_data_metadata_before_gpt()

        refreshed = self.build_plan()
        if (
            refreshed.super_info.first != plan.super_info.first
            or refreshed.super_info.last != plan.super_info.last
            or refreshed.userdata_info.first != plan.userdata_info.first
            or refreshed.userdata_info.last != plan.userdata_info.last
            or refreshed.sector_bytes != plan.sector_bytes
        ):
            raise ExpandError("格式化后检测到 GPT 布局发生意外变化，拒绝继续写入。")
        plan = refreshed

        # sgdisk partition rewrite must stay ONE sgdisk invocation (GPT atomicity),
        # but separate from umount/verify/sync as individual adb shell calls.
        sp, up = plan.super_part, plan.userdata_part
        sg = self.sgdisk_cmd
        disk = plan.disk
        rewrite = (
            f"{sg} "
            f"--delete={sp} --delete={up} "
            f"--new={sp}:{plan.super_info.first}:{plan.new_super_last} "
            f"--typecode={sp}:{plan.super_info.code} "
            f"--partition-guid={sp}:{plan.super_info.guid} "
            f"--change-name={sp}:{plan.super_info.name} "
            f"--new={up}:{plan.new_userdata_first}:{plan.userdata_info.last} "
            f"--typecode={up}:{plan.userdata_info.code} "
            f"--partition-guid={up}:{plan.userdata_info.guid} "
            f"--change-name={up}:{plan.userdata_info.name} "
            f"{disk}"
        )
        self._emit("正在写入 GPT（分步执行）…")
        self._shell_step("sync（写前）", "sync", critical=False, timeout=30.0)
        rewrite_out = self._shell_step(
            "sgdisk 重写 super/userdata",
            rewrite,
            critical=True,
            timeout=300.0,
        )
        # treat obvious failures
        if re.search(r"(?i)error|failed|invalid", rewrite_out) and "The operation" not in rewrite_out:
            # sgdisk often prints warnings; only fail if exit was non-zero via critical
            pass
        self._shell_step(
            "sgdisk --verify",
            f"{sg} --verify {disk}",
            critical=False,
            timeout=120.0,
        )
        self._shell_step("sync（写后）", "sync", critical=False, timeout=30.0)

        self._emit("正在重启 Recovery 以重新加载 GPT...")
        self.reboot_to_recovery_and_wait(from_fastboot=False)
        self.init_sgdisk()

        self._emit("正在验证分区大小...")
        self._shell_step("readlink super", "readlink -f /dev/block/by-name/super", critical=False)
        self._shell_step("readlink userdata", "readlink -f /dev/block/by-name/userdata", critical=False)
        sizes = self._shell_step(
            "blockdev super size",
            "blockdev --getsize64 /dev/block/by-name/super",
            critical=True,
            timeout=30.0,
        )
        self._shell_step(
            "blockdev userdata size",
            "blockdev --getsize64 /dev/block/by-name/userdata",
            critical=False,
            timeout=30.0,
        )
        actual_super = None
        for ln in sizes.splitlines():
            ln = ln.strip()
            if ln.isdigit():
                actual_super = int(ln)
                break
        if actual_super != plan.target_bytes:
            raise ExpandError(
                f"扩容后 super 大小不匹配：预期 {plan.target_bytes}，实际 {actual_super}"
            )

        ud_ok = self.format_partition_f2fs(
            "userdata",
            mount_hints=("/data", "/sdcard", "/tmp/userdata", "/mnt/userdata"),
        )
        self._emit("GPT 扩容已完成。")
        if not ud_ok:
            missing = []
            if not ud_ok:
                missing.append("userdata")
            self._emit(
                "分区表已经改好；仅 "
                + "、".join(missing)
                + " 自动格式化没做完。可在 Recovery 里手动 Format Data，或继续刷 ROM（线刷也会写 userdata）。"
            )
        else:
            self._emit("userdata / metadata 已格式化。请保持 Recovery，继续刷 ROM。")
        self._emit(f"备份目录：{backup_dir}")
        return backup_dir

    # ----- restore -----

    def _backup_device_of(self, directory: Path) -> str:
        plan_path = directory / "plan.txt"
        if plan_path.is_file():
            for line in plan_path.read_text(encoding="utf-8", errors="replace").splitlines():
                if line.startswith("device="):
                    dev = line.split("=", 1)[1].strip()
                    if dev in self.cfg.codenames:
                        return dev
        info = directory / "device-info.txt"
        if info.is_file():
            for line in info.read_text(encoding="utf-8", errors="replace").splitlines():
                s = line.strip()
                if s in self.cfg.codenames:
                    return s
        for code in self.cfg.codenames:
            if directory.name.startswith(f"{code}-"):
                return code
        return ""

    def list_backups_for_device(self, device: str) -> list[Path]:
        root = self.cfg.backup_dir
        if not root.is_dir():
            return []
        found: list[Path] = []
        for d in root.iterdir():
            if not d.is_dir():
                continue
            if not (d / "gpt-sda-before.bin").is_file() or not (d / "plan.txt").is_file():
                continue
            if self._backup_device_of(d) == device:
                found.append(d)
        found.sort(key=lambda p: p.stat().st_mtime, reverse=True)
        return found

    def read_original_layout(self, backup: Path) -> tuple[int, int]:
        values: dict[str, str] = {}
        for line in (backup / "plan.txt").read_text(encoding="utf-8", errors="replace").splitlines():
            if "=" in line:
                k, v = line.split("=", 1)
                values[k.strip()] = v.strip()
        if "sector_bytes" not in values or "super_old" not in values:
            raise ExpandError("plan.txt 缺少 sector_bytes 或 super_old。")
        m = re.match(r"^(\d+)-(\d+)$", values["super_old"])
        if not m:
            raise ExpandError("无法解析 plan.txt 中的 super_old。")
        sector = int(values["sector_bytes"])
        first, last = int(m.group(1)), int(m.group(2))
        super_bytes = (last - first + 1) * sector
        return super_bytes, sector

    def wait_recovery_super(self, expected: int, timeout_seconds: int = 180) -> int:
        last_state = "尚未检测到 Recovery ADB"
        self._emit(f"正在核对 super 大小（期望 {expected} 字节，最长 {timeout_seconds}s）…")
        deadline = time.monotonic() + timeout_seconds
        n = 0
        while time.monotonic() < deadline:
            n += 1
            text = self.adb_devices_raw()
            if re.search(r"(?m)^\S+\s+recovery\s*$", text):
                size_text = self.adb_shell(
                    "test -e /dev/block/by-name/super && blockdev --getsize64 /dev/block/by-name/super",
                    allow_fail=True,
                    timeout=20.0,
                ).strip()
                # may be multi-line
                for line in size_text.splitlines():
                    line = line.strip()
                    if re.match(r"^\d+$", line):
                        actual = int(line)
                        last_state = f"Recovery 已连接，super={actual} 字节"
                        if n % 3 == 1:
                            self._emit(last_state)
                        if actual == expected:
                            self._emit(f"super 大小已符合预期：{actual}")
                            return actual
                        break
                else:
                    last_state = "Recovery 已连接，但 super 节点尚未就绪"
                    if n % 3 == 1:
                        self._emit(last_state)
            else:
                last_state = "等待 Recovery ADB 重新上线…"
                if n % 3 == 1:
                    self._emit(last_state)
            time.sleep(2)
        raise ExpandError(f"等待 Recovery 加载官方分区表超时。最后状态：{last_state}")

    def restore_official(self, backup: Path) -> dict:
        """Restore official GPT from backup. Returns status dict.

        format_ok=False only means data wipe step failed — GPT restore can still be OK.
        """
        # Recovery-only for restore (same as original script)
        if self.adb_transport_state() != "recovery":
            raise ExpandError("请先进入 Recovery（支持 root ADB），再恢复分区表。")
        device = self.adb_shell("getprop ro.product.device").strip().splitlines()
        device = device[0].strip() if device else ""
        if device not in self.cfg.codenames:
            raise ExpandError(f"机型不匹配：{device}")
        self.device = device
        self._emit(f"已确认设备：{device}")

        if self._backup_device_of(backup) != device:
            raise ExpandError("所选备份与当前机型不匹配，已取消。")

        expected_super, _ = self.read_original_layout(backup)
        gpt = backup / "gpt-sda-before.bin"
        if not gpt.is_file():
            raise ExpandError(f"缺少 GPT 备份文件：{gpt}")

        self.init_sgdisk()
        current = int(
            self.adb_shell("blockdev --getsize64 /dev/block/by-name/super").strip().splitlines()[0]
        )
        self._emit(f"官方 super 大小：{expected_super} 字节")
        self._emit(f"当前 super 大小：{current} 字节")
        self._emit(f"选中备份：{backup}")

        if current == expected_super:
            self._emit("当前已是官方 super 大小，跳过重复写入 GPT。")
            actual = current
        else:
            remote = "/tmp/harusora-gpt-restore.bin"
            self.push_file(gpt, remote)
            self.soft_unmount_data_metadata()
            self._emit("正在写入官方 GPT（分步）…")
            self._shell_step("sync（写前）", "sync", critical=False)
            self._shell_step(
                "sgdisk --load-backup",
                f"{self.sgdisk_cmd} --load-backup={remote} {self.cfg.disk}",
                critical=True,
                timeout=300.0,
            )
            self._shell_step(
                "sgdisk --verify",
                f"{self.sgdisk_cmd} --verify {self.cfg.disk}",
                critical=False,
            )
            self._shell_step("sync（写后）", "sync", critical=False)
            self._shell_step(f"清理 {remote}", f"rm -f {remote}", critical=False)
            self._emit("正在重启 Recovery 并核对 super 大小…")
            self.reboot_to_recovery_and_wait(from_fastboot=False)
            actual = self.wait_recovery_super(expected_super)
            self.init_sgdisk()

        if actual != expected_super:
            raise ExpandError(
                f"GPT 恢复后 super 大小不符：预期 {expected_super}，实际 {actual}"
            )

        self._emit("官方物理分区表已就绪。")
        ud_ok = self.format_partition_f2fs(
            "userdata",
            mount_hints=("/data", "/sdcard", "/tmp/userdata", "/mnt/userdata"),
        )
        md_ok = self.format_partition_f2fs(
            "metadata",
            mount_hints=("/metadata", "/tmp/metadata", "/mnt/metadata"),
        )
        self.adb_shell("sync", allow_fail=True)

        format_ok = ud_ok and md_ok
        if format_ok:
            self._emit("userdata / metadata 已格式化。")
        else:
            missing = []
            if not ud_ok:
                missing.append("userdata")
            if not md_ok:
                missing.append("metadata")
            self._emit(
                "分区表已经恢复成功；只是 "
                + "、".join(missing)
                + " 自动格式化没做完（常见原因：Recovery 仍挂着 /data）。"
            )
            self._emit(
                "可在 Recovery 菜单里手动 Format Data，或直接刷官方线刷包（也会处理 userdata）。"
            )
        self._emit(f"super 大小：{actual} 字节。建议接着刷官方完整线刷包。")
        return {
            "gpt_ok": True,
            "super_bytes": actual,
            "format_ok": format_ok,
            "userdata_formatted": ud_ok,
            "metadata_formatted": md_ok,
        }
