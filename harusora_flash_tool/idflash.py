"""Post-ROM ID (mi_ext) flash helpers — OTA zip or image, after the ROM stub."""

from __future__ import annotations

import shutil
import zipfile
from pathlib import Path

from .device import DeviceSession
from .flash import (
    MIN_LOGICAL_FLASH_BYTES,
    is_overflow_flash_error,
    logical_flash_size,
    recreate_logical_partition,
)
from .log import Logger
from .sdat2img import SdatError, sdat2img

SPARSE_MAGIC = b"\x3a\xff\x26\xed"


class IdFlashError(Exception):
    pass


def ensure_fastbootd(session: DeviceSession, logger: Logger) -> None:
    us = session.getvar("is-userspace", allow_missing=True)
    if us and us.lower() == "yes":
        print("  已在 fastbootd。")
        return
    print("  进入 fastbootd…")
    session.run_fastboot(
        ["reboot", "fastboot"],
        label="reboot fastboot",
        show_output=True,
        allow_failure=True,
    )
    import time

    time.sleep(2)
    # re-discover serial if needed
    if not session.list_fastboot_serials():
        for _ in range(90):
            devs = session.list_fastboot_serials()
            if devs:
                if not session.serial or session.serial not in devs:
                    session.serial = devs[0]
                break
            time.sleep(1)
    session.wait_for_mode(True, timeout_seconds=90)
    logger.write("entered fastbootd for ID flash")


def flash_mi_ext_image(
    session: DeviceSession,
    image: Path,
    logger: Logger,
    *,
    work: Path | None = None,
) -> None:
    if not image.is_file():
        raise IdFlashError(f"ID 镜像不存在：{image}")
    work_dir = work or image.parent
    flash_path = _prepare_mi_ext_flash_image(image, work_dir)
    size = logical_flash_size(flash_path.stat().st_size)
    print(f"  ID 镜像：{flash_path}  ({flash_path.stat().st_size} 字节，逻辑分区 {size})")
    logger.write(f"ID flash image: {flash_path} logical_size={size}")

    for part in ("mi_ext_a", "mi_ext_b"):
        recreate_logical_partition(session, part, size)
        result = session.run_fastboot(
            ["flash", part, str(flash_path)],
            label=f"刷入 {part}",
            show_output=True,
            allow_failure=True,
        )
        if result.code == 0:
            print(f"  {part} 完成")
            continue

        text = result.text or ""
        if is_overflow_flash_error(text) and not _is_sparse(flash_path):
            print(f"  {part} 仍过小，尝试 8MiB 补零…")
            padded = _padded_image(flash_path, work_dir, target=8 * 1024 * 1024)
            retry_size = logical_flash_size(padded.stat().st_size)
            recreate_logical_partition(session, part, retry_size)
            result2 = session.run_fastboot(
                ["flash", part, str(padded)],
                label=f"刷入 {part}（8MiB padded）",
                show_output=True,
                allow_failure=True,
            )
            if result2.code == 0:
                print(f"  {part} 完成（padded）")
                continue
            text = result2.text or text

        if part.endswith("_b"):
            print(f"  警告：{part} 跳过")
            logger.write(f"skip {part}: {text}")
            continue
        raise IdFlashError(f"刷入 {part} 失败：\n{text}")

    session.run_fastboot(
        ["set_active", "a"],
        label="set_active a",
        show_output=True,
        allow_failure=True,
    )


def format_userdata_metadata(session: DeviceSession, logger: Logger) -> None:
    """Erase userdata + metadata (clean first boot after ID)."""
    print("  格式化 Data（erase userdata / metadata）…")
    for part in ("userdata", "metadata"):
        session.run_fastboot(
            ["erase", part],
            label=f"erase {part}",
            show_output=True,
            allow_failure=True,
        )
    logger.write("erased userdata + metadata after ID step")


def _prepare_mi_ext_flash_image(image: Path, work: Path) -> Path:
    """Pad a raw mi_ext image to the fastbootd-safe minimum. Sparse files are left as-is."""
    if _is_sparse(image):
        return image
    size = logical_flash_size(image.stat().st_size)
    if image.stat().st_size >= size:
        return image
    return _padded_image(image, work, target=size)


def _padded_image(src: Path, work: Path, *, target: int = MIN_LOGICAL_FLASH_BYTES) -> Path:
    data = src.read_bytes()
    if data[:4] == SPARSE_MAGIC:
        for alt in (src.with_suffix(".img"), src.parent / "mi_ext.img"):
            if alt.is_file():
                raw = alt.read_bytes()
                if raw[:4] != SPARSE_MAGIC:
                    data = raw
                    break
        else:
            raise IdFlashError("sparse 刷入失败且无 raw .img 可补零")
    if len(data) < target:
        data = data + b"\x00" * (target - len(data))
    work.mkdir(parents=True, exist_ok=True)
    out = work / "mi_ext-padded.img"
    out.write_bytes(data)
    return out


def resolve_id_input(user_path: Path, work: Path) -> Path:
    p = user_path.expanduser().resolve()
    if not p.exists():
        raise IdFlashError(f"路径不存在：{p}")
    work.mkdir(parents=True, exist_ok=True)

    if p.is_dir():
        return _from_dir(p, work)
    suf = p.suffix.lower()
    if suf == ".zip" or zipfile.is_zipfile(p):
        return _from_zip(p, work)
    if suf == ".dat" or p.name.lower().endswith(".new.dat"):
        return _from_new_dat(p, work)
    if suf in {".img", ".simg", ".raw"}:
        return p
    raise IdFlashError(
        f"无法识别：{p.name}\n"
        "请提供 ID 补丁 zip（含 images/super/mi_ext.new.dat）、.new.dat 或 mi_ext.img/.simg"
    )


def discover_id_candidates(rom_root: Path) -> list[Path]:
    """Find ID OTA zips placed next to the ROM package (optional convenience)."""
    found: list[Path] = []
    if not rom_root.is_dir():
        return found
    search_roots = [rom_root]
    for extra in ("id", "ID", "idpatch", "IDpatch"):
        folder = rom_root / extra
        if folder.is_dir():
            search_roots.append(folder)
    seen: set[Path] = set()
    for root in search_roots:
        for path in sorted(root.glob("*.zip")):
            try:
                resolved = path.resolve()
            except OSError:
                continue
            if resolved in seen:
                continue
            if _zip_contains_mi_ext(path):
                seen.add(resolved)
                found.append(path)
    return found


def _zip_contains_mi_ext(path: Path) -> bool:
    try:
        with zipfile.ZipFile(path, "r") as zf:
            names = [name.replace("\\", "/").lower() for name in zf.namelist()]
    except (OSError, zipfile.BadZipFile):
        return False
    return any(
        name.endswith(("mi_ext.new.dat", "mi_ext.img", "mi_ext.simg"))
        or name.endswith("/mi_ext.transfer.list")
        for name in names
    )


def _from_dir(folder: Path, work: Path) -> Path:
    dats = _find_mi_ext_dats(folder)
    if dats:
        return _from_new_dat(dats[0], work)
    return _pick_image_in_dir(folder)


def _pick_image_in_dir(folder: Path) -> Path:
    imgs: list[Path] = []
    for pat in ("**/*.simg", "**/*.img"):
        imgs.extend(folder.glob(pat))
    imgs = [x for x in imgs if x.is_file() and x.stat().st_size > 0]
    if not imgs:
        raise IdFlashError(
            f"文件夹内无 mi_ext.new.dat / .img / .simg：{folder}\n"
            "当前 ID 补丁是 Recovery OTA zip，请直接拖入该 zip。"
        )
    scored: list[tuple[int, Path]] = []
    for p in imgs:
        n = p.name.lower()
        s = 0
        if "mi_ext" in n:
            s += 50
        if n.endswith(".simg") or _is_sparse(p):
            s += 30
        scored.append((s, p))
    scored.sort(key=lambda t: (-t[0], t[1].name))
    return scored[0][1]


def _is_sparse(p: Path) -> bool:
    try:
        return p.read_bytes()[:4] == SPARSE_MAGIC
    except OSError:
        return False


def _safe_extract_zip(zip_path: Path, dest: Path) -> None:
    dest = dest.resolve()
    with zipfile.ZipFile(zip_path, "r") as zf:
        for info in zf.infolist():
            target = (dest / info.filename).resolve()
            if not str(target).startswith(str(dest)):
                raise IdFlashError(f"zip 内含不安全路径：{info.filename}")
        zf.extractall(dest)


def _from_zip(zip_path: Path, work: Path) -> Path:
    dest = work / f"idzip_{zip_path.stem}"
    if dest.exists():
        shutil.rmtree(dest, ignore_errors=True)
    dest.mkdir(parents=True, exist_ok=True)
    print(f"  解压 ID zip：{zip_path.name}")
    _safe_extract_zip(zip_path, dest)
    return _from_dir(dest, work)


def _find_mi_ext_dats(folder: Path) -> list[Path]:
    dats = [p for p in folder.rglob("*.new.dat") if p.is_file() and p.stat().st_size > 0]
    preferred = [p for p in dats if "mi_ext" in p.name.lower()]
    return preferred or dats


def _from_new_dat(dat: Path, work: Path) -> Path:
    for name in ("mi_ext.simg", "mi_ext.img", dat.stem + ".simg", dat.stem + ".img"):
        p = dat.parent / name
        if p.is_file() and p.stat().st_size > 0 and p.suffix.lower() in {".img", ".simg", ".raw"}:
            # Prefer a real image sitting next to the dat; skip empty patch placeholders.
            if p.name.lower().endswith(".patch.dat"):
                continue
            return p
    transfer = dat.with_name("mi_ext.transfer.list")
    if not transfer.is_file():
        stem = dat.name[: -len(".new.dat")] if dat.name.lower().endswith(".new.dat") else dat.stem
        transfer = dat.with_name(f"{stem}.transfer.list")
    if not transfer.is_file():
        transfer = dat.with_suffix(".transfer.list")
    if not transfer.is_file():
        raise IdFlashError(
            f"找到 {dat.name} 但缺少 transfer.list，无法还原 ID 镜像。\n"
            "请使用完整 ID 补丁 zip（需同时包含 mi_ext.new.dat 与 mi_ext.transfer.list）。"
        )
    out = work / "mi_ext_from_dat.img"
    print(f"  sdat2img：{dat.name} + {transfer.name}")
    try:
        size = sdat2img(transfer, dat, out)
    except SdatError as exc:
        raise IdFlashError(f"还原 ID 镜像失败：{exc}") from exc
    print(f"  已还原 mi_ext raw：{size} 字节")
    return out


def prompt_id_path() -> Path:
    print("  请将 ID 补丁 zip 拖入此窗口后回车，或粘贴完整路径：")
    print("  （当前包是 OTA zip：含 images/super/mi_ext.new.dat，不是单独的 .img）")
    while True:
        raw = input("  ID 路径> ").strip().strip('"')
        if not raw:
            continue
        p = Path(raw)
        if p.exists():
            return p
        print("  路径无效，请重试。")
