from __future__ import annotations

from pathlib import Path

from .config import FlashProfile, ImageOp


class PackageError(Exception):
    pass


def is_preserved_data_partition(partition: str) -> bool:
    """Return whether an operation would modify user data or encryption metadata."""
    name = partition.strip().lower()
    for suffix in ("_ab", "_a", "_b"):
        if name.endswith(suffix):
            name = name[: -len(suffix)]
            break
    return name in {"userdata", "metadata"}


def _image_path(profile: FlashProfile, op: ImageOp) -> Path | None:
    if op.action != "flash" or not op.file:
        return None
    return profile.images_dir / op.file


def discover_empty_super_image(profile: FlashProfile) -> Path | None:
    """Return a ready-to-flash empty super .img if one exists. Never a DAT."""
    if profile.wipe_super_image:
        configured = profile.images_dir / profile.wipe_super_image
        if configured.is_file():
            return configured
    matches = sorted(
        path for path in profile.images_dir.glob("super_empty*.img") if path.is_file()
    )
    if not matches:
        return None
    for match in matches:
        if "16g" in match.stem.lower():
            return match
    return matches[0]


def resolve_wipe_super_source(profile: FlashProfile) -> tuple[Path, Path | None]:
    """Return (image_or_dat, transfer_list_or_None).

    Prefer a super_empty*.img so it can be flashed to the physical super
    partition. DAT+transfer is only a fallback. transfer_list is set when the
    source is a DAT pair.
    """
    image = discover_empty_super_image(profile)
    if image is not None:
        return image, None
    stem = profile.wipe_super_dat or "super_empty"
    dat = profile.super_dir / f"{stem}.new.dat"
    transfer = profile.super_dir / f"{stem}.transfer.list"
    if dat.is_file() and transfer.is_file():
        return dat, transfer
    if profile.wipe_super_image:
        return profile.images_dir / profile.wipe_super_image, None
    return dat, transfer


def collect_required_paths(
    profile: FlashProfile,
    *,
    preserve_data: bool = False,
) -> list[Path]:
    required: list[Path] = [
        profile.fastboot,
        profile.adb,
    ]
    if profile.dynamic_partitions:
        required.append(profile.img2simg)
        required.append(profile.cygwin_dll)
        wipe_file, wipe_transfer = resolve_wipe_super_source(profile)
        required.append(wipe_file)
        if wipe_transfer is not None:
            required.append(wipe_transfer)

    if profile.flash_crc_lists:
        # CRC files optional at presence check only if options say so — keep soft:
        # original script required them always. Keep required when flash_crc_lists.
        required.append(profile.images_dir / profile.crc_list_file)
        required.append(profile.images_dir / profile.sparse_crc_list_file)

    for op in (*profile.firmware, *profile.boot_chain, *profile.final_data):
        if preserve_data and is_preserved_data_partition(op.partition):
            continue
        p = _image_path(profile, op)
        if p is not None:
            required.append(p)

    for part in profile.dynamic_partitions:
        required.append(profile.super_dir / f"{part.name}.new.dat")
        required.append(profile.super_dir / f"{part.name}.transfer.list")

    return required


def validate_package(profile: FlashProfile, *, preserve_data: bool = False) -> None:
    missing = [
        p
        for p in collect_required_paths(profile, preserve_data=preserve_data)
        if not p.is_file()
    ]
    if missing:
        lines = "\n".join(str(p) for p in missing)
        raise PackageError(f"刷机包文件不完整：\n{lines}")

    for part in profile.dynamic_partitions:
        path = profile.super_dir / f"{part.name}.new.dat"
        actual = path.stat().st_size
        if actual != part.dat_bytes:
            raise PackageError(
                f"{part.name}.new.dat 大小异常：预期 {part.dat_bytes}，实际 {actual}"
            )


def read_package_anti(profile: FlashProfile) -> int | None:
    """Return package anti level, or None to skip the check.

    Missing/unconfigured anti_version file must NOT be treated as 0, or devices
    with anti>=1 would be falsely rejected (common on houji card-flash packages).
    """
    if not profile.anti_version_file:
        return None
    path = profile.images_dir / profile.anti_version_file
    if not path.is_file():
        return None
    text = path.read_text(encoding="utf-8", errors="replace").strip()
    try:
        return int(text)
    except ValueError as exc:
        raise PackageError(f"无法解析 anti 版本文件：{path} -> {text!r}") from exc
