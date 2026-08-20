"""Post-ROM ID (mi_ext) flash helpers — integrated into full line-flash flow."""

from __future__ import annotations

import shutil
import zipfile
from pathlib import Path

from .device import DeviceSession
from .log import Logger

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


def flash_mi_ext_image(session: DeviceSession, image: Path, logger: Logger) -> None:
    if not image.is_file():
        raise IdFlashError(f"ID 镜像不存在：{image}")
    print(f"  ID 镜像：{image}  ({image.stat().st_size} 字节)")
    logger.write(f"ID flash image: {image}")

    for part in ("mi_ext_a", "mi_ext_b"):
        result = session.run_fastboot(
            ["flash", part, str(image)],
            label=f"刷入 {part}",
            show_output=True,
            allow_failure=True,
        )
        if result.code == 0:
            print(f"  {part} 完成")
            continue
        print(f"  {part} 直接刷入未成功，尝试 1MiB 补零…")
        padded = _padded_image(image, image.parent)
        result2 = session.run_fastboot(
            ["flash", part, str(padded)],
            label=f"刷入 {part}（padded）",
            show_output=True,
            allow_failure=True,
        )
        if result2.code == 0:
            print(f"  {part} 完成（padded）")
            continue
        if part.endswith("_b"):
            print(f"  警告：{part} 跳过")
            logger.write(f"skip {part}: {result2.text}")
            continue
        raise IdFlashError(f"刷入 {part} 失败：\n{result2.text or result.text}")

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


def _padded_image(src: Path, work: Path) -> Path:
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
    target = 1024 * 1024
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
        return _pick_image_in_dir(p)
    suf = p.suffix.lower()
    if suf == ".zip" or zipfile.is_zipfile(p):
        return _from_zip(p, work)
    if suf == ".dat" or p.name.lower().endswith(".new.dat"):
        return _from_new_dat(p, work)
    if suf in {".img", ".simg", ".raw"}:
        return p
    raise IdFlashError(
        f"无法识别：{p.name}\n请提供 mi_ext.simg/.img、.new.dat 或 ID 补丁 zip"
    )


def _pick_image_in_dir(folder: Path) -> Path:
    imgs: list[Path] = []
    for pat in ("**/*.simg", "**/*.img"):
        imgs.extend(folder.glob(pat))
    imgs = [x for x in imgs if x.is_file() and x.stat().st_size > 0]
    if not imgs:
        raise IdFlashError(f"文件夹内无 .img/.simg：{folder}")
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


def _from_zip(zip_path: Path, work: Path) -> Path:
    dest = work / f"idzip_{zip_path.stem}"
    if dest.exists():
        shutil.rmtree(dest, ignore_errors=True)
    dest.mkdir(parents=True, exist_ok=True)
    print(f"  解压 ID zip：{zip_path.name}")
    with zipfile.ZipFile(zip_path, "r") as zf:
        zf.extractall(dest)
    return _pick_image_in_dir(dest)


def _from_new_dat(dat: Path, work: Path) -> Path:
    for name in ("mi_ext.simg", "mi_ext.img", dat.stem + ".simg", dat.stem + ".img"):
        p = dat.parent / name
        if p.is_file() and p.stat().st_size > 0:
            return p
    transfer = dat.with_name("mi_ext.transfer.list")
    if not transfer.is_file():
        transfer = dat.with_suffix(".transfer.list")
    if not transfer.is_file():
        print("  无 transfer.list，将 .new.dat 当作镜像尝试刷入")
        return dat
    # prefer sibling simg over full sdat2img for speed when present
    out = work / "mi_ext_from_dat.img"
    _sdat2img_simple(transfer, dat, out)
    return out


def _sdat2img_simple(transfer: Path, dat: Path, out: Path) -> None:
    lines = transfer.read_text(encoding="utf-8", errors="replace").splitlines()
    version = int(lines[0].strip())
    total_blocks = int(lines[1].strip())
    cmd_start = 2 if version == 1 else 4
    block = 4096
    out_size = total_blocks * block
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("wb") as ofh, dat.open("rb") as dfh:
        if out_size > 0:
            ofh.seek(out_size - 1)
            ofh.write(b"\0")
            ofh.seek(0)
        for raw in lines[cmd_start:]:
            line = raw.strip()
            if not line.startswith("new "):
                continue
            arg = line.split(None, 1)[1]
            nums = [int(x) for x in arg.split(",")]
            n, rest = nums[0], nums[1:]
            if n == len(rest) and len(rest) % 2 == 0:
                pairs = list(zip(rest[0::2], rest[1::2], strict=True))
            elif len(nums) % 2 == 0:
                pairs = list(zip(nums[0::2], nums[1::2], strict=True))
            else:
                raise IdFlashError(f"transfer range 无效：{arg}")
            for start, end in pairs:
                nbytes = (end - start) * block
                chunk = dfh.read(nbytes)
                if len(chunk) != nbytes:
                    raise IdFlashError("new.dat 与 transfer.list 不匹配")
                ofh.seek(start * block)
                ofh.write(chunk)


def prompt_id_path() -> Path:
    print("  请将 ID 文件拖入此窗口后回车，或粘贴完整路径：")
    while True:
        raw = input("  ID 路径> ").strip().strip('"')
        if not raw:
            continue
        p = Path(raw)
        if p.exists():
            return p
        print("  路径无效，请重试。")
