from __future__ import annotations

import re
import tomllib
from collections.abc import Sequence
from dataclasses import dataclass
from datetime import date
from pathlib import Path
from typing import Any


class ProfileError(Exception):
    """Invalid or missing flash profile."""


@dataclass(frozen=True)
class ImageOp:
    action: str  # flash | erase
    partition: str
    file: str | None = None  # relative to images/


@dataclass(frozen=True)
class DynamicPartition:
    name: str
    dat_bytes: int
    pad_to_bytes: int | None = None


@dataclass(frozen=True)
class ExpansionConfig:
    """Physical GPT super expansion settings (from [expansion] in profile)."""

    enabled: bool
    codenames: list[str]
    target_bytes: int
    disk: str
    super_part: int
    userdata_part: int
    backup_dir: Path  # GPT backups under ROM root (e.g. backup/)
    sgdisk_dir: Path  # bundled ARM64 sgdisk (e.g. tools/sgdisk-arm64)
    remote_sgdisk_dir: str
    confirm_word: str
    restore_confirm_word: str


@dataclass
class FlashProfile:
    rom_root: Path
    profile_path: Path

    codenames: list[str]
    display_name: str
    notes: list[str]
    author: str
    qq_group: str
    build_date: str

    minimum_super_bytes: int
    anti_version_file: str | None
    erase_if_present: list[str]
    pre_erase: list[str]

    images_dir: Path
    super_dir: Path
    work_dir: Path
    logs_dir: Path
    expansion_dir: Path | None
    tools_dir: Path

    firmware: list[ImageOp]
    boot_chain: list[ImageOp]
    final_data: list[ImageOp]

    wipe_super_image: str | None
    wipe_super_dat: str | None
    slot_suffix: str
    dynamic_partitions: list[DynamicPartition]

    set_active_slot: str | None
    flash_crc_lists: bool
    crc_list_file: str
    sparse_crc_list_file: str
    require_wipe_confirmation: bool
    confirmation_word: str
    update_confirmation_word: str
    reboot_after_success: bool
    enable_root_menu: bool
    post_flash_id: bool

    expansion: ExpansionConfig | None

    @property
    def package_summary(self) -> str:
        return f"作者：{self.author}  |  QQ群：{self.qq_group}  |  构建日期：{self.build_date}"

    def resolve_tool(self, *parts: str) -> Path:
        return self.tools_dir.joinpath(*parts)

    @property
    def fastboot(self) -> Path:
        return self.resolve_tool("platform-tools", "fastboot.exe")

    @property
    def adb(self) -> Path:
        return self.resolve_tool("platform-tools", "adb.exe")

    @property
    def img2simg(self) -> Path:
        return self.resolve_tool("img2simg.exe")

    @property
    def cygwin_dll(self) -> Path:
        return self.resolve_tool("cygwin1.dll")


DEFAULT_PROFILE_REL = Path("META-INF") / "com" / "android" / "flash_profile.toml"
_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]*$")
_DEVICE_PATH_RE = re.compile(r"^/[A-Za-z0-9._/-]+$")


def _clean_path_arg(value: str | Path) -> Path:
    """Normalize CLI path args (strip quotes, trailing junk from broken bat quoting)."""
    text = str(value).strip().strip('"').strip("'")
    # cmd `"%dp0"` trailing-backslash bug can glue extra tokens onto the path
    # e.g. E:\Rom" --version  or  E:\Rom\
    if '"' in text:
        text = text.split('"', 1)[0]
    text = text.rstrip()
    return Path(text)


def _search_roots() -> list[Path]:
    """Candidate directories to walk upward when auto-detecting ROM root."""
    import sys

    starts: list[Path] = []
    try:
        starts.append(Path.cwd())
    except OSError:
        pass
    # Frozen exe / script location (not PyInstaller extract dir)
    try:
        if getattr(sys, "frozen", False):
            starts.append(Path(sys.executable).resolve().parent)
        else:
            starts.append(Path(__file__).resolve().parent.parent)
            if sys.argv and sys.argv[0]:
                starts.append(Path(sys.argv[0]).resolve().parent)
    except OSError:
        pass
    # De-dupe while preserving order
    seen: set[Path] = set()
    out: list[Path] = []
    for s in starts:
        try:
            r = s.resolve()
        except OSError:
            continue
        if r not in seen:
            seen.add(r)
            out.append(r)
    return out


def find_rom_root(start: Path | None = None) -> Path | None:
    """Walk up from start (or cwd / exe dir) looking for flash_profile.toml."""
    starts = [start.resolve()] if start is not None else _search_roots()
    seen: set[Path] = set()
    for base in starts:
        for candidate in [base, *base.parents]:
            if candidate in seen:
                continue
            seen.add(candidate)
            if (candidate / DEFAULT_PROFILE_REL).is_file():
                return candidate
    return None


def resolve_profile_path(rom_root: Path | None, profile: Path | None) -> tuple[Path, Path]:
    """
    Return (rom_root, profile_path).
    If profile is given, rom_root defaults to ancestors of META-INF/com/android
    or --rom-root if provided.
    """
    if profile is not None:
        profile = _clean_path_arg(profile).resolve()
        if not profile.is_file():
            raise ProfileError(f"指定的配置文件不存在：{profile}")
        if rom_root is not None:
            return _clean_path_arg(rom_root).resolve(), profile
        # Heuristic: .../META-INF/com/android/flash_profile.toml -> rom root
        parts = profile.parts
        try:
            idx = parts.index("META-INF")
            if idx >= 1 and "com" in parts[idx:] and "android" in parts[idx:]:
                return Path(*parts[:idx]), profile
        except ValueError:
            pass
        # flash_profile.toml lives in META-INF/com/android -> up 3
        if profile.parent.name.lower() == "android":
            return profile.parent.parent.parent.parent.resolve(), profile
        return profile.parent.resolve(), profile

    if rom_root is None:
        found = find_rom_root()
        if found is None:
            raise ProfileError(
                "未找到 ROM 包配置。\n"
                f"需要：{{ROM根}}\\{DEFAULT_PROFILE_REL.as_posix().replace('/', chr(92))}\n"
                "请在刷机包根目录运行，或使用：\n"
                "  --rom-root \"刷机包完整路径\"\n"
                "  --profile \"...\\META-INF\\com\\android\\flash_profile.toml\""
            )
        rom_root = found
    else:
        rom_root = _clean_path_arg(rom_root).resolve()

    profile_path = rom_root / DEFAULT_PROFILE_REL
    if not profile_path.is_file():
        # One more try: maybe rom_root actually points at tools/ or similar
        found = find_rom_root(rom_root)
        if found is not None and (found / DEFAULT_PROFILE_REL).is_file():
            return found, found / DEFAULT_PROFILE_REL
        raise ProfileError(
            f"未找到刷机配置：{profile_path}\n"
            "请确认 META-INF\\com\\android\\flash_profile.toml 存在于刷机包内。\n"
            f"当前解析的 ROM 根目录：{rom_root}"
        )
    return rom_root, profile_path


def _require(data: dict[str, Any], key: str, ctx: str) -> Any:
    if key not in data:
        raise ProfileError(f"配置缺少必填项 [{ctx}].{key}")
    return data[key]


def _required_text(data: dict[str, Any], key: str, ctx: str) -> str:
    value = str(_require(data, key, ctx)).strip()
    if not value:
        raise ProfileError(f"[{ctx}].{key} 不能为空")
    return value


def _bool_option(data: dict[str, Any], key: str, default: bool, ctx: str) -> bool:
    value = data.get(key, default)
    if not isinstance(value, bool):
        raise ProfileError(f"[{ctx}].{key} 必须是 true 或 false")
    return value


def _validate_name(value: str, ctx: str) -> str:
    if not _NAME_RE.fullmatch(value):
        raise ProfileError(f"{ctx} 包含非法字符：{value!r}")
    return value


def _validate_relative_path(value: str, ctx: str) -> str:
    path = Path(value)
    if path.is_absolute() or path.drive or ".." in path.parts:
        raise ProfileError(f"{ctx} 必须是刷机包内的相对路径：{value!r}")
    if not value or value in (".", "./", ".\\"):
        raise ProfileError(f"{ctx} 不能为空或指向当前目录")
    return value


def _resolve_package_path(rom_root: Path, value: str, ctx: str) -> Path:
    relative = _validate_relative_path(value, ctx)
    root = rom_root.resolve()
    candidate = (root / relative).resolve()
    try:
        candidate.relative_to(root)
    except ValueError as exc:
        raise ProfileError(f"{ctx} 超出刷机包根目录：{value!r}") from exc
    return candidate


def _validate_device_path(value: str, ctx: str) -> str:
    if not _DEVICE_PATH_RE.fullmatch(value) or ".." in Path(value).parts:
        raise ProfileError(f"{ctx} 必须是安全的设备绝对路径：{value!r}")
    return value


def _as_ops(items: Sequence[Any], stage: str) -> list[ImageOp]:
    ops: list[ImageOp] = []
    if not items:
        return ops
    for i, raw in enumerate(items):
        if not isinstance(raw, dict):
            raise ProfileError(f"stages.{stage}[{i}] 必须是表")
        action = str(raw.get("action", "")).strip().lower()
        partition = str(raw.get("partition", "")).strip()
        file_name = raw.get("file")
        if action not in ("flash", "erase"):
            raise ProfileError(f"stages.{stage}[{i}].action 必须是 flash 或 erase，收到：{action!r}")
        if not partition:
            raise ProfileError(f"stages.{stage}[{i}].partition 不能为空")
        _validate_name(partition, f"stages.{stage}[{i}].partition")
        if action == "flash":
            if not file_name:
                raise ProfileError(f"stages.{stage}[{i}] flash 必须指定 file")
            file_name = str(file_name).strip()
            _validate_relative_path(file_name, f"stages.{stage}[{i}].file")
        else:
            file_name = str(file_name).strip() if file_name else None
        # Safety: never allow lock-related partitions as free-form shell
        banned = ("lock", "oem lock", "flashing lock")
        blob = f"{action} {partition} {file_name or ''}".lower()
        if any(b in blob for b in banned) and "unlock" not in blob:
            if "lock" in partition.lower() or (file_name and "lock" in file_name.lower()):
                raise ProfileError(f"拒绝可能的锁 BL 操作：stages.{stage}[{i}]")
        ops.append(ImageOp(action=action, partition=partition, file=file_name))
    return ops


def load_profile(rom_root: Path, profile_path: Path) -> FlashProfile:
    try:
        raw_text = profile_path.read_bytes()
        data = tomllib.loads(raw_text.decode("utf-8"))
    except tomllib.TOMLDecodeError as exc:
        raise ProfileError(f"TOML 解析失败：{profile_path}\n{exc}") from exc
    except OSError as exc:
        raise ProfileError(f"无法读取配置：{profile_path}\n{exc}") from exc

    if not isinstance(data, dict):
        raise ProfileError("配置根必须是表")

    package = data.get("package") or {}
    device = data.get("device") or {}
    requirements = data.get("requirements") or {}
    paths = data.get("paths") or {}
    stages = data.get("stages") or {}
    dynamic = data.get("dynamic") or {}
    options = data.get("options") or {}

    tables = {
        "package": package,
        "device": device,
        "requirements": requirements,
        "paths": paths,
        "stages": stages,
        "dynamic": dynamic,
        "options": options,
    }
    for name, table in tables.items():
        if not isinstance(table, dict):
            raise ProfileError(f"[{name}] 必须是表")
    author = _required_text(package, "author", "package")
    qq_group = _required_text(package, "qq_group", "package")
    build_date = _required_text(package, "build_date", "package")
    try:
        date.fromisoformat(build_date)
    except ValueError as exc:
        raise ProfileError("[package].build_date 必须是 YYYY-MM-DD 日期") from exc
    codenames = device.get("codenames")
    if not codenames or not isinstance(codenames, list) or not all(isinstance(c, str) and c.strip() for c in codenames):
        raise ProfileError("[device].codenames 必须是非空字符串数组")
    codenames = [_validate_name(c.strip(), "[device].codenames") for c in codenames]
    display_name = str(device.get("display_name") or "HarusoraFlashTool").strip()
    notes_raw = device.get("notes") or []
    if not isinstance(notes_raw, list) or not all(isinstance(note, str) for note in notes_raw):
        raise ProfileError("[device].notes 必须是字符串数组")
    notes = [note.strip() for note in notes_raw if note.strip()]

    min_super = requirements.get("minimum_super_bytes", 0)
    if not isinstance(min_super, int) or isinstance(min_super, bool):
        raise ProfileError("[requirements].minimum_super_bytes 必须是整数")
    minimum_super_bytes = min_super
    if minimum_super_bytes <= 0:
        raise ProfileError("[requirements].minimum_super_bytes 必须为正整数")

    anti = requirements.get("anti_version_file")
    anti_version_file = str(anti).strip() if anti else None
    if anti_version_file:
        _validate_relative_path(anti_version_file, "[requirements].anti_version_file")

    def _partition_list(key: str) -> list[str]:
        raw = requirements.get(key) or []
        if not isinstance(raw, list) or not all(isinstance(item, str) for item in raw):
            raise ProfileError(f"[requirements].{key} 必须是字符串数组")
        return [_validate_name(item.strip(), f"[requirements].{key}") for item in raw]

    erase_if_present = _partition_list("erase_if_present")
    pre_erase = _partition_list("pre_erase")

    def _path(key: str, default: str) -> Path:
        rel = str(paths.get(key, default)).strip() or default
        return _resolve_package_path(rom_root, rel, f"[paths].{key}")

    images_dir = _path("images", "images")
    super_dir = _path("super", "super")
    work_dir = _path("work", "work")
    logs_dir = _path("logs", "logs")
    tools_dir = _path("tools", "tools")
    # Legacy key still accepted but no longer required (tools live under tools/, backups under backup/)
    exp = paths.get("expansion_dir")
    expansion_dir = (
        _resolve_package_path(rom_root, str(exp), "[paths].expansion_dir")
        if exp
        else None
    )

    firmware = _as_ops(stages.get("firmware") or [], "firmware")
    boot_chain = _as_ops(stages.get("boot_chain") or [], "boot_chain")
    final_data = _as_ops(stages.get("final_data") or [], "final_data")
    if not firmware and not boot_chain and not (dynamic.get("partitions")):
        raise ProfileError("配置中没有任何刷写阶段（firmware / boot_chain / dynamic）")

    has_wipe_image = "wipe_super_image" in dynamic
    has_wipe_dat = "wipe_super_dat" in dynamic
    if has_wipe_image and has_wipe_dat:
        raise ProfileError(
            "[dynamic].wipe_super_image 与 [dynamic].wipe_super_dat 只能配置一个"
        )
    if has_wipe_dat:
        wipe_super_dat = str(dynamic.get("wipe_super_dat") or "").strip()
        if not wipe_super_dat:
            raise ProfileError("[dynamic].wipe_super_dat 不能为空")
        _validate_name(wipe_super_dat, "[dynamic].wipe_super_dat")
        wipe_super_image = None
    else:
        # Backward compatible default for existing packages.
        wipe_super_image = str(
            dynamic.get("wipe_super_image") or "super_empty_16g.img"
        ).strip()
        _validate_relative_path(wipe_super_image, "[dynamic].wipe_super_image")
        wipe_super_dat = None
    slot_suffix = str(dynamic.get("slot_suffix") if dynamic.get("slot_suffix") is not None else "_a")
    if slot_suffix not in ("", "_a", "_b"):
        raise ProfileError("[dynamic].slot_suffix 只能是空字符串、_a 或 _b")
    parts_raw = dynamic.get("partitions") or []
    dynamic_partitions: list[DynamicPartition] = []
    if not isinstance(parts_raw, list):
        raise ProfileError("[dynamic].partitions 必须是数组")
    for i, pr in enumerate(parts_raw):
        if not isinstance(pr, dict):
            raise ProfileError(f"dynamic.partitions[{i}] 必须是表")
        name = str(pr.get("name", "")).strip()
        if not name:
            raise ProfileError(f"dynamic.partitions[{i}].name 不能为空")
        _validate_name(name, f"dynamic.partitions[{i}].name")
        dat_bytes = pr.get("dat_bytes")
        if not isinstance(dat_bytes, int) or isinstance(dat_bytes, bool):
            raise ProfileError(f"dynamic.partitions[{i}].dat_bytes 必须是整数")
        if dat_bytes <= 0:
            raise ProfileError(f"dynamic.partitions[{i}].dat_bytes 必须为正")
        pad = pr.get("pad_to_bytes")
        if pad is not None and (not isinstance(pad, int) or isinstance(pad, bool)):
            raise ProfileError(f"dynamic.partitions[{i}].pad_to_bytes 必须是整数")
        pad_to = pad
        if pad_to is not None and pad_to < dat_bytes:
            raise ProfileError(f"dynamic.partitions[{i}].pad_to_bytes 不能小于 dat_bytes")
        dynamic_partitions.append(DynamicPartition(name=name, dat_bytes=dat_bytes, pad_to_bytes=pad_to))

    set_active = options.get("set_active_slot", "a")
    if set_active is None or str(set_active).strip() == "":
        set_active_slot = None
    else:
        set_active_slot = str(set_active).strip()
        if set_active_slot not in ("a", "b"):
            raise ProfileError("[options].set_active_slot 只能是 a、b 或空值")

    crc_list_file = str(options.get("crc_list_file") or "crclist.txt")
    sparse_crc_list_file = str(options.get("sparse_crc_list_file") or "sparsecrclist.txt")
    _validate_relative_path(crc_list_file, "[options].crc_list_file")
    _validate_relative_path(sparse_crc_list_file, "[options].sparse_crc_list_file")

    expansion = _load_expansion(
        data.get("expansion"),
        rom_root=rom_root.resolve(),
        expansion_dir=expansion_dir,
        device_codenames=codenames,
        minimum_super_bytes=minimum_super_bytes,
        tools_dir=tools_dir,
    )

    return FlashProfile(
        rom_root=rom_root.resolve(),
        profile_path=profile_path.resolve(),
        codenames=codenames,
        display_name=display_name,
        notes=notes,
        author=author,
        qq_group=qq_group,
        build_date=build_date,
        minimum_super_bytes=minimum_super_bytes,
        anti_version_file=anti_version_file,
        erase_if_present=erase_if_present,
        pre_erase=pre_erase,
        images_dir=images_dir,
        super_dir=super_dir,
        work_dir=work_dir,
        logs_dir=logs_dir,
        expansion_dir=expansion_dir,
        tools_dir=tools_dir,
        firmware=firmware,
        boot_chain=boot_chain,
        final_data=final_data,
        wipe_super_image=wipe_super_image,
        wipe_super_dat=wipe_super_dat,
        slot_suffix=slot_suffix,
        dynamic_partitions=dynamic_partitions,
        set_active_slot=set_active_slot,
        flash_crc_lists=_bool_option(options, "flash_crc_lists", True, "options"),
        crc_list_file=crc_list_file,
        sparse_crc_list_file=sparse_crc_list_file,
        require_wipe_confirmation=_bool_option(
            options, "require_wipe_confirmation", True, "options"
        ),
        confirmation_word=str(options.get("confirmation_word") or "WIPE"),
        update_confirmation_word=str(options.get("update_confirmation_word") or "UPDATE"),
        reboot_after_success=_bool_option(options, "reboot_after_success", True, "options"),
        enable_root_menu=_bool_option(options, "enable_root_menu", True, "options"),
        post_flash_id=_bool_option(options, "post_flash_id", False, "options"),
        expansion=expansion,
    )


def _load_expansion(
    raw: Any,
    *,
    rom_root: Path,
    expansion_dir: Path | None,
    device_codenames: list[str],
    minimum_super_bytes: int,
    tools_dir: Path,
) -> ExpansionConfig | None:
    """Parse optional [expansion] table.

    Layout (new):
      {ROM}/backup/              GPT backups
      {ROM}/tools/sgdisk-arm64/  bundled sgdisk
    Legacy paths.expansion_dir is ignored for assets (migration only).
    """
    if raw is None:
        return None
    if not isinstance(raw, dict):
        raise ProfileError("[expansion] 必须是表")

    enabled = _bool_option(raw, "enabled", True, "expansion")
    if not enabled:
        return None

    codes = raw.get("codenames")
    if codes is None:
        exp_codes = list(device_codenames)
    else:
        if not isinstance(codes, list) or not codes:
            raise ProfileError("[expansion].codenames 必须是非空字符串数组")
        if not all(isinstance(code, str) and code.strip() for code in codes):
            raise ProfileError("[expansion].codenames 必须是非空字符串数组")
        exp_codes = [
            _validate_name(code.strip(), "[expansion].codenames") for code in codes
        ]

    target = raw.get("target_bytes", minimum_super_bytes)
    if not isinstance(target, int) or isinstance(target, bool):
        raise ProfileError("[expansion].target_bytes 必须是整数")
    if target <= 0:
        raise ProfileError("[expansion].target_bytes 必须为正")

    disk = _validate_device_path(
        str(raw.get("disk") or "/dev/block/sda").strip(), "[expansion].disk"
    )
    super_part = raw.get("super_part", 32)
    userdata_part = raw.get("userdata_part", 33)
    if any(
        not isinstance(value, int) or isinstance(value, bool)
        for value in (super_part, userdata_part)
    ):
        raise ProfileError("[expansion].super_part / userdata_part 必须是整数")
    if super_part <= 0 or userdata_part <= 0 or super_part == userdata_part:
        raise ProfileError("[expansion] 分区号必须为不同的正整数")

    # Paths relative to ROM root (not a separate expand kit folder)
    backup_rel = str(
        raw.get("backup_dir") or raw.get("backup_subdir") or "backup"
    ).strip() or "backup"
    sgdisk_rel = str(
        raw.get("sgdisk_dir") or raw.get("sgdisk_subdir") or "tools/sgdisk-arm64"
    ).strip() or "tools/sgdisk-arm64"
    # If someone still writes bare "sgdisk-arm64", prefer under tools/
    if sgdisk_rel == "sgdisk-arm64":
        sgdisk_rel = "tools/sgdisk-arm64"

    backup_dir = _resolve_package_path(rom_root, backup_rel, "[expansion].backup_dir")
    sgdisk_dir = _resolve_package_path(rom_root, sgdisk_rel, "[expansion].sgdisk_dir")
    # Legacy fallback: old kit layout
    if not (sgdisk_dir / "sgdisk").is_file() and expansion_dir is not None:
        legacy = expansion_dir / "sgdisk-arm64"
        if (legacy / "sgdisk").is_file():
            sgdisk_dir = legacy.resolve()
    if not backup_dir.is_dir() and expansion_dir is not None:
        legacy_b = expansion_dir / "backup"
        if legacy_b.is_dir():
            backup_dir = legacy_b.resolve()

    remote = _validate_device_path(
        str(raw.get("remote_sgdisk_dir") or "/tmp/harusora-sgdisk").strip(),
        "[expansion].remote_sgdisk_dir",
    )
    confirm = str(raw.get("confirm_word") or "EXPAND").strip() or "EXPAND"
    restore_confirm = str(raw.get("restore_confirm_word") or "RESTORE").strip() or "RESTORE"

    return ExpansionConfig(
        enabled=True,
        codenames=exp_codes,
        target_bytes=target,
        disk=disk,
        super_part=super_part,
        userdata_part=userdata_part,
        backup_dir=backup_dir,
        sgdisk_dir=sgdisk_dir,
        remote_sgdisk_dir=remote,
        confirm_word=confirm,
        restore_confirm_word=restore_confirm,
    )
