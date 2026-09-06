"""Generate and validate a HarusoraFlashTool ROM profile.

Run this script before publishing a ROM package. It scans images/ and super/,
then writes META-INF/com/android/flash_profile.toml in the ROM root.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from dataclasses import dataclass
from datetime import date
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

from harusora_flash_tool.config import load_profile  # noqa: E402
from harusora_flash_tool.package import validate_package  # noqa: E402

PROFILE_REL = Path("META-INF") / "com" / "android" / "flash_profile.toml"
GIB = 1024**3

BOOT_CHAIN = {
    "boot",
    "dtbo",
    "init_boot",
    "recovery",
    "vendor_boot",
    "vendor_kernel_boot",
    "vbmeta",
    "vbmeta_system",
    "vbmeta_vendor",
}
FIRMWARE = {
    "abl",
    "aop",
    "aop_config",
    "bluetooth",
    "cpucp",
    "cpucp_dtb",
    "devcfg",
    "dsp",
    "featenabler",
    "hyp",
    "imagefv",
    "keymaster",
    "modem",
    "multiimgoem",
    "qupfw",
    "shrm",
    "tz",
    "uefi",
    "uefisecapp",
    "xbl",
    "xbl_config",
    "xbl_ramdump",
    "xbl_sc_test_mode",
    "xbl_uefi_sec",
}
FINAL_DATA = {"metadata", "userdata"}
AB_PARTITIONS = BOOT_CHAIN | FIRMWARE


class GeneratorError(Exception):
    """Profile generation cannot continue safely."""


@dataclass(frozen=True)
class ImageEntry:
    stage: str
    partition: str
    file: str


@dataclass(frozen=True)
class DynamicEntry:
    name: str
    dat_bytes: int
    raw_bytes: int


def _toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def _toml_array(values: list[str]) -> str:
    return "[" + ", ".join(_toml_string(value) for value in values) + "]"


def _base_partition(stem: str) -> str:
    for suffix in ("_ab", "_a", "_b"):
        if stem.endswith(suffix):
            return stem[: -len(suffix)]
    return stem


def _parse_partition_maps(values: list[str]) -> dict[str, str]:
    mappings: dict[str, str] = {}
    for value in values:
        file_name, separator, partition = value.partition("=")
        if not separator or not file_name.strip() or not partition.strip():
            raise GeneratorError(
                f"无效 --partition-map {value!r}；格式应为 image.img=partition"
            )
        mappings[file_name.strip().lower()] = partition.strip()
    return mappings


def discover_images(images_dir: Path, mappings: dict[str, str]) -> list[ImageEntry]:
    entries: list[ImageEntry] = []
    unknown: list[str] = []
    for path in sorted(images_dir.glob("*.img"), key=lambda item: item.name.lower()):
        stem = path.stem.lower()
        if stem.startswith("super_empty") or stem in {"super", "blank_super"}:
            continue
        mapped = mappings.get(path.name.lower())
        base = _base_partition(stem)
        if mapped:
            partition = mapped
        elif base in AB_PARTITIONS and not stem.endswith(("_a", "_b", "_ab")):
            partition = f"{base}_ab"
        elif base in BOOT_CHAIN | FIRMWARE | FINAL_DATA:
            partition = stem
        else:
            unknown.append(path.name)
            continue

        target_base = _base_partition(partition.lower())
        if target_base in FINAL_DATA:
            stage = "final_data"
        elif target_base in BOOT_CHAIN:
            stage = "boot_chain"
        else:
            stage = "firmware"
        entries.append(ImageEntry(stage, partition, path.name))

    unused = sorted(set(mappings) - {entry.file.lower() for entry in entries})
    if unused:
        raise GeneratorError("--partition-map 引用了不存在的镜像：" + ", ".join(unused))
    if unknown:
        examples = " ".join(f'--partition-map "{name}={Path(name).stem}_ab"' for name in unknown)
        raise GeneratorError(
            "以下镜像无法安全推断目标分区："
            + ", ".join(unknown)
            + "\n请逐项指定映射，例如：\n  "
            + examples
        )
    return entries


def _transfer_raw_bytes(path: Path) -> int:
    lines = path.read_text(encoding="utf-8", errors="strict").splitlines()
    if len(lines) < 2:
        raise GeneratorError(f"transfer.list 格式不完整：{path}")
    try:
        blocks = int(lines[1].strip())
    except ValueError as exc:
        raise GeneratorError(f"transfer.list 总块数无效：{path}") from exc
    if blocks <= 0:
        raise GeneratorError(f"transfer.list 总块数必须为正：{path}")
    return blocks * 4096


def discover_dynamic(super_dir: Path) -> list[DynamicEntry]:
    entries: list[DynamicEntry] = []
    for dat in sorted(super_dir.glob("*.new.dat"), key=lambda item: item.name.lower()):
        name = dat.name[: -len(".new.dat")]
        if name == "super_empty":
            continue
        transfer = super_dir / f"{name}.transfer.list"
        if not transfer.is_file():
            raise GeneratorError(f"缺少动态分区传输列表：{transfer}")
        entries.append(DynamicEntry(name, dat.stat().st_size, _transfer_raw_bytes(transfer)))
    return entries


def _minimum_super_bytes(dynamic: list[DynamicEntry], explicit: int | None) -> int:
    if explicit is not None:
        if explicit <= 0:
            raise GeneratorError("--minimum-super-bytes 必须为正整数")
        return explicit
    if not dynamic:
        return 1
    required = sum(entry.raw_bytes for entry in dynamic)
    return max(GIB, math.ceil(required / GIB) * GIB)


def _find_wipe_source(
    images_dir: Path,
    super_dir: Path,
    requested_image: str | None,
    requested_dat: str | None,
) -> tuple[str, str]:
    if requested_image and requested_dat:
        raise GeneratorError("--wipe-super-image 与 --wipe-super-dat 只能指定一个")
    if requested_dat:
        dat = super_dir / f"{requested_dat}.new.dat"
        transfer = super_dir / f"{requested_dat}.transfer.list"
        if not dat.is_file() or not transfer.is_file():
            raise GeneratorError(
                f"空 super DAT 必须同时包含：{dat} 与 {transfer}"
            )
        return "wipe_super_dat", requested_dat
    if requested_image:
        path = images_dir / requested_image
        if not path.is_file():
            raise GeneratorError(f"空 super 镜像不存在：{path}")
        return "wipe_super_image", requested_image
    candidates = sorted(images_dir.glob("super_empty*.img"))
    if len(candidates) == 1:
        return "wipe_super_image", candidates[0].name
    if len(candidates) > 1:
        raise GeneratorError("images/ 中存在多个 super_empty*.img，请明确指定")
    dat = super_dir / "super_empty.new.dat"
    transfer = super_dir / "super_empty.transfer.list"
    if dat.is_file() and transfer.is_file():
        return "wipe_super_dat", "super_empty"
    raise GeneratorError(
        "检测到动态分区时，必须提供一个 super_empty*.img，或在 super/ 中提供 "
        "super_empty.new.dat 与 super_empty.transfer.list"
    )


def _inline_ops(
    entries: list[ImageEntry],
    stage: str,
    *,
    erase_partitions: list[str] | None = None,
) -> str:
    values = [
        "{ action = \"flash\", partition = "
        + _toml_string(entry.partition)
        + ", file = "
        + _toml_string(entry.file)
        + " }"
        for entry in entries
        if entry.stage == stage
    ]
    values.extend(
        "{ action = \"erase\", partition = " + _toml_string(partition) + " }"
        for partition in (erase_partitions or [])
    )
    return "[" + ",\n  ".join(values) + "]"


def render_profile(args: argparse.Namespace) -> tuple[str, list[ImageEntry], list[DynamicEntry]]:
    root = args.rom_root.resolve()
    images_dir = root / "images"
    super_dir = root / "super"
    if not images_dir.is_dir():
        raise GeneratorError(f"缺少镜像目录：{images_dir}")
    super_dir.mkdir(exist_ok=True)

    images = discover_images(images_dir, _parse_partition_maps(args.partition_map))
    dynamic = discover_dynamic(super_dir)
    if not images and not dynamic:
        raise GeneratorError("未在 images/*.img 或 super/*.new.dat 中发现可刷入内容")

    final_targets = {
        _base_partition(entry.partition.lower())
        for entry in images
        if entry.stage == "final_data"
    }
    final_erases = [
        partition for partition in ("metadata", "userdata") if partition not in final_targets
    ]

    minimum_super = _minimum_super_bytes(dynamic, args.minimum_super_bytes)
    wipe_key, wipe_value = (
        _find_wipe_source(
            images_dir,
            super_dir,
            args.wipe_super_image,
            args.wipe_super_dat,
        )
        if dynamic
        else ("wipe_super_image", "")
    )
    anti = args.anti_version_file or ""

    lines = [
        "# Generated by tools/generate_profile.py; review before release.",
        "",
        "[package]",
        f"author = {_toml_string(args.author)}",
        f"qq_group = {_toml_string(args.qq_group)}",
        f"build_date = {_toml_string(args.build_date)}",
        "",
        "[device]",
        f"codenames = {_toml_array(args.codename)}",
        f"display_name = {_toml_string(args.display_name or root.name)}",
        f"notes = {_toml_array(args.note)}",
        "",
        "[requirements]",
        f"minimum_super_bytes = {minimum_super}",
        f"anti_version_file = {_toml_string(anti)}",
        f"erase_if_present = {_toml_array(args.erase_if_present)}",
        f"pre_erase = {_toml_array(args.pre_erase)}",
        "",
        "[paths]",
        'images = "images"',
        'super = "super"',
        'work = "work"',
        'logs = "logs"',
        'tools = "tools"',
        "",
        "[stages]",
        f"firmware = {_inline_ops(images, 'firmware')}",
        f"boot_chain = {_inline_ops(images, 'boot_chain')}",
        f"final_data = {_inline_ops(images, 'final_data', erase_partitions=final_erases)}",
        "",
        "[dynamic]",
        f"{wipe_key} = {_toml_string(wipe_value)}",
        f"slot_suffix = {_toml_string(args.dynamic_slot_suffix)}",
        "partitions = [",
    ]
    for entry in dynamic:
        pad = entry.raw_bytes
        # Tiny mi_ext placeholders overflow Xiaomi fastbootd unless padded to ≥ 1MiB.
        if entry.name.lower() == "mi_ext":
            pad = max(pad, 1024 * 1024)
        lines.append(
            "  { name = "
            + _toml_string(entry.name)
            + f", dat_bytes = {entry.dat_bytes}, pad_to_bytes = {pad} }},"
        )
    lines.extend(
        [
            "]",
            "",
            "[options]",
            f"set_active_slot = {_toml_string(args.set_active_slot)}",
            f"flash_crc_lists = {'true' if args.flash_crc_lists else 'false'}",
            'crc_list_file = "crclist.txt"',
            'sparse_crc_list_file = "sparsecrclist.txt"',
            "require_wipe_confirmation = true",
            'confirmation_word = "WIPE"',
            'update_confirmation_word = "UPDATE"',
            f"reboot_after_success = {'false' if args.no_reboot_after_success else 'true'}",
            f"enable_root_menu = {'false' if args.disable_root_menu else 'true'}",
            f"post_flash_id = {'true' if args.post_flash_id else 'false'}",
        ]
    )
    if args.enable_expansion:
        if args.expansion_target_bytes is None:
            raise GeneratorError("启用扩容时必须明确指定 --expansion-target-bytes")
        lines.extend(
            [
                "",
                "[expansion]",
                "enabled = true",
                f"codenames = {_toml_array(args.codename)}",
                f"target_bytes = {args.expansion_target_bytes}",
                f"disk = {_toml_string(args.expansion_disk)}",
                f"super_part = {args.expansion_super_part}",
                f"userdata_part = {args.expansion_userdata_part}",
                'backup_dir = "backup"',
                'sgdisk_dir = "tools/sgdisk-arm64"',
                'remote_sgdisk_dir = "/tmp/harusora-sgdisk"',
                'confirm_word = "EXPAND"',
                'restore_confirm_word = "RESTORE"',
            ]
        )
    else:
        lines.extend(["", "[expansion]", "enabled = false"])
    return "\n".join(lines) + "\n", images, dynamic


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="扫描 ROM 包并生成 HarusoraFlashTool 配置")
    parser.add_argument("--rom-root", type=Path, default=Path.cwd(), help="刷机包根目录")
    parser.add_argument("--codename", action="append", required=True, help="支持机型代号，可重复")
    parser.add_argument("--display-name", default="", help="面向用户显示的 ROM 名称")
    parser.add_argument("--author", required=True, help="ROM 作者或维护团队")
    parser.add_argument("--qq-group", required=True, help="用户支持 QQ 群")
    parser.add_argument("--build-date", default=date.today().isoformat(), help="构建日期，默认今天")
    parser.add_argument("--note", action="append", default=[], help="刷入须知，可重复")
    parser.add_argument("--minimum-super-bytes", type=int, help="最小 super 大小；默认按动态分区推算")
    parser.add_argument("--partition-map", action="append", default=[], help="无法推断时指定 image.img=partition")
    parser.add_argument("--wipe-super-image", help="空 super 镜像文件名")
    parser.add_argument("--wipe-super-dat", help="空 super DAT 基名，例如 super_empty")
    parser.add_argument("--dynamic-slot-suffix", default="_a", help="动态分区目标后缀")
    parser.add_argument("--set-active-slot", default="a", help="刷完后激活的槽位")
    parser.add_argument("--anti-version-file", help="images/ 下的 anti 版本文件")
    parser.add_argument("--erase-if-present", action="append", default=[], help="存在时擦除的分区，可重复")
    parser.add_argument("--pre-erase", action="append", default=[], help="刷写前强制擦除的分区，可重复")
    parser.add_argument("--flash-crc-lists", action="store_true", help="刷入 crclist.txt 与 sparsecrclist.txt")
    parser.add_argument("--disable-root-menu", action="store_true", help="隐藏用户 Root 镜像入口")
    parser.add_argument(
        "--post-flash-id",
        action="store_true",
        help="启用 ColorOS ID 后处理：跳过包内占位 mi_ext，刷完后接受 ID 补丁 zip",
    )
    parser.add_argument("--no-reboot-after-success", action="store_true", help="刷写成功后停留在当前模式")
    parser.add_argument("--enable-expansion", action="store_true", help="在用户菜单显示物理扩容入口")
    parser.add_argument("--expansion-target-bytes", type=int, help="扩容后的 super 目标大小")
    parser.add_argument("--expansion-disk", default="/dev/block/sda")
    parser.add_argument("--expansion-super-part", type=int, default=32)
    parser.add_argument("--expansion-userdata-part", type=int, default=33)
    parser.add_argument("--force", action="store_true", help="覆盖现有配置")
    parser.add_argument("--no-validate", action="store_true", help="只生成，不检查完整包资源")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    output = args.rom_root.resolve() / PROFILE_REL
    if output.exists() and not args.force:
        print(f"拒绝覆盖现有配置：{output}\n如确认重建，请添加 --force。", file=sys.stderr)
        return 2
    temporary = output.with_suffix(".toml.tmp")
    try:
        content, images, dynamic = render_profile(args)
        output.parent.mkdir(parents=True, exist_ok=True)
        temporary.write_text(content, encoding="utf-8", newline="\n")
        profile = load_profile(args.rom_root.resolve(), temporary)
        if not args.no_validate:
            validate_package(profile)
        temporary.replace(output)
    except Exception as exc:
        temporary.unlink(missing_ok=True)
        print(f"生成失败：{exc}", file=sys.stderr)
        return 1

    print(f"已生成：{output}")
    print(f"机型：{', '.join(profile.codenames)}")
    print(f"镜像操作：{len(images)}；动态分区：{len(dynamic)}")
    print(f"扩容入口：{'启用' if profile.expansion else '关闭'}")
    print("完整性校验：" + ("已跳过" if args.no_validate else "通过"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
