from __future__ import annotations

import re
import shutil
from collections.abc import Callable
from pathlib import Path

from .config import FlashProfile, ImageOp
from .device import DeviceError, DeviceSession, EnvReport, parse_size
from .log import Logger
from .package import (
    PackageError,
    is_preserved_data_partition,
    read_package_anti,
    validate_package,
)
from .process import run_process
from .sdat2img import SdatError, sdat2img
from .sparse import (
    build_wipe_super_image,
    is_android_sparse,
)


class FlashError(Exception):
    pass


ChooseFn = Callable[[str, list[str]], int]
ConfirmFn = Callable[[str], str]

# Xiaomi/ColorOS fastbootd raises EOVERFLOW ("Value too large for defined data type")
# when auto-resizing tiny logical partitions such as placeholder mi_ext (~136KiB).
MIN_LOGICAL_FLASH_BYTES = 1024 * 1024
_SMALL_LOGICAL_RECREATE_BYTES = 8 * 1024 * 1024
_LOGICAL_BLOCK_BYTES = 4096


def logical_flash_size(out_size: int, pad_to_bytes: int | None = None) -> int:
    """Return the size we actually flash/create, padded up for tiny dynamic partitions."""
    size = max(int(out_size), int(pad_to_bytes or 0))
    if size < MIN_LOGICAL_FLASH_BYTES:
        size = MIN_LOGICAL_FLASH_BYTES
    if size % _LOGICAL_BLOCK_BYTES:
        size += _LOGICAL_BLOCK_BYTES - (size % _LOGICAL_BLOCK_BYTES)
    return size


def logical_size_for_sparse_payload(logical_size: int, sparse_bytes: int) -> int:
    """Leave enough logical space for fastbootd to accept the encoded sparse payload."""
    if sparse_bytes <= logical_size:
        return logical_size
    return logical_flash_size(sparse_bytes + _LOGICAL_BLOCK_BYTES)


def is_overflow_flash_error(text: str) -> bool:
    lowered = (text or "").lower()
    return "value too large" in lowered or "eoverflow" in lowered


def recreate_logical_partition(session: DeviceSession, name: str, size_bytes: int) -> None:
    """Delete and recreate a dynamic logical partition so flash can resize cleanly."""
    session.run_fastboot(
        ["delete-logical-partition", name],
        label=f"delete-logical-partition {name}",
        show_output=True,
        allow_failure=True,
    )
    result = session.run_fastboot(
        ["create-logical-partition", name, str(size_bytes)],
        label=f"create-logical-partition {name} {size_bytes}",
        show_output=True,
        allow_failure=True,
    )
    if result.code != 0:
        session.logger.write(f"create-logical-partition {name} failed: {result.text}")


def is_separate_id_partition(name: str) -> bool:
    """True when this dynamic partition is supplied by a post-flash ID zip, not the ROM stub."""
    return name.strip().lower() == "mi_ext"


def _flashed_partitions(text: str) -> list[str]:
    return re.findall(r"Partition\s+(\S+)\s+flashed successfully", text or "", flags=re.I)


def is_benign_flash_failure(partition: str, text: str) -> bool:
    """Xiaomi/Qcom sometimes returns remote 0000001B after a dual-slot write already OK."""
    t = text or ""
    if "0000001B" not in t and not re.search(r"Error flashing partition", t, re.I):
        return False
    ok = _flashed_partitions(t)
    if not ok:
        return False
    # recovery_ab / boot_ab → expect both sides if reported
    if partition.endswith("_ab"):
        base = partition[: -len("_ab")]
        has_a = any(p == f"{base}_a" or p.endswith("_a") for p in ok)
        has_b = any(p == f"{base}_b" or p.endswith("_b") for p in ok)
        if has_a and has_b:
            return True
        # Some BL only echo one line for both; still treat 1B+success as soft OK
        return "0000001B" in t and len(ok) >= 1
    return "0000001B" in t and len(ok) >= 1


def flash_image(
    session: DeviceSession,
    partition: str,
    image_path: Path,
    *,
    label: str,
) -> None:
    """Flash one image with Xiaomi dual-slot / 0x1B quirks handling."""
    result = session.run_fastboot(
        ["flash", partition, str(image_path)],
        label=label,
        allow_failure=True,
        show_output=True,
    )
    if result.code == 0:
        return

    text = result.text or ""
    if is_benign_flash_failure(partition, text):
        print("  │  注：bootloader 已报告分区写入成功；远程错误 0000001B 在部分机型上可忽略。")
        session.logger.write(
            f"Soft-OK flash {partition}: treated remote failure as success (0000001B / dual-slot quirk)\n{text}"
        )
        return

    # Fallback for *_ab: flash slot A and B separately (often works when _ab returns 1B)
    if partition.endswith("_ab"):
        base = partition[: -len("_ab")]
        print(f"  │  {partition} 返回异常，尝试分别刷入 {base}_a / {base}_b …")
        session.logger.write(f"Retry flash {partition} as {base}_a + {base}_b")
        for slot in ("a", "b"):
            part = f"{base}_{slot}"
            r = session.run_fastboot(
                ["flash", part, str(image_path)],
                label=f"刷入 {part} ← {image_path.name}",
                allow_failure=True,
                show_output=True,
            )
            if r.code == 0:
                continue
            rt = r.text or ""
            if is_benign_flash_failure(part, rt) or (
                f"Partition {part} flashed successfully" in rt and "0000001B" in rt
            ):
                print(f"  │  注：{part} 已报告写入成功，忽略后续远程错误码。")
                session.logger.write(f"Soft-OK flash {part}:\n{rt}")
                continue
            # If the other path already wrote both via first attempt messages, don't hard-fail
            # unless this individual flash also looks dead
            raise DeviceError(
                f"刷入 {part} 失败（{r.code}）：flash {part} {image_path}\n{rt}"
            )
        return

    raise DeviceError(
        f"{label} 失败（{result.code}）：flash {partition} {image_path}\n{text}"
    )


def gather_environment(
    profile: FlashProfile,
    session: DeviceSession,
    *,
    choose: ChooseFn | None = None,
) -> EnvReport:
    session.select_device(choose=choose)
    product = session.getvar("product")
    assert product is not None
    if product not in profile.codenames:
        allowed = ", ".join(profile.codenames)
        raise FlashError(f"机型不匹配：检测到 {product}，本包仅支持 {allowed}。")

    size_text = session.getvar("partition-size:super")
    assert size_text is not None
    super_bytes = parse_size(size_text)
    is_userspace = (session.getvar("is-userspace", allow_missing=True) or "").lower() == "yes"
    anti = session.getvar("anti", allow_missing=True)

    return EnvReport(
        serial=session.serial or "",
        product=product,
        super_bytes=super_bytes,
        super_ready=super_bytes >= profile.minimum_super_bytes,
        is_userspace=is_userspace,
        anti=anti,
    )


def run_image_op(profile: FlashProfile, session: DeviceSession, op: ImageOp) -> None:
    if op.action == "erase":
        session.run_fastboot(["erase", op.partition], label=f"擦除 {op.partition}", show_output=True)
        return
    assert op.file
    path = profile.images_dir / op.file
    flash_image(
        session,
        op.partition,
        path,
        label=f"刷入 {op.partition} ← {op.file}",
    )


def convert_dat_to_sparse(
    profile: FlashProfile,
    session: DeviceSession,
    name: str,
    dat: Path,
    sparse: Path,
    pad_to_bytes: int | None,
    logger: Logger,
) -> int:
    """
    transfer.list + .new.dat → raw partition (sdat2img) → sparse (img2simg).

    Never img2simg the .new.dat directly: it is a block-OTA stream, not a linear image.
    """
    raw = profile.work_dir / f"{name}.raw.img"
    out_size = restore_dat_to_raw(profile, session, name, dat, raw, logger)
    if session.dry_run:
        print(f"  ├─ [dry-run] img2simg {raw.name} -> {sparse.name}")
        sparse.write_bytes(b"")
        return logical_flash_size(pad_to_bytes or 0)

    conversion_input = raw
    target_size = logical_flash_size(out_size, pad_to_bytes)
    if target_size > out_size:
        # Tiny logical partitions (especially mi_ext) must be ≥ 1MiB or fastbootd
        # fails with "Value too large for defined data type" while resizing.
        with raw.open("r+b") as fh:
            fh.truncate(target_size)
        logger.write(f"{name} raw padded {out_size} -> {target_size}")
        print(f"  │  {name} 已补零到 {target_size} 字节（避免 fastbootd 过小分区失败）")
        out_size = target_size

    for attempt in range(1, 4):
        logger.write(f"img2simg {conversion_input} {sparse} (attempt {attempt})")
        print(f"  │  img2simg：{name} → sparse…")
        result = run_process(
            profile.img2simg,
            [str(conversion_input), str(sparse)],
            logger=logger,
            show_output=True,
        )
        if result.code != 0:
            raise FlashError(f"img2simg 转换 {name} 失败：{result.text}")

        sparse_bytes = sparse.stat().st_size
        safe_size = logical_size_for_sparse_payload(out_size, sparse_bytes)
        logger.write(
            f"{name} sparse payload size={sparse_bytes}, logical size={out_size}, "
            f"safe size={safe_size}"
        )
        if safe_size == out_size:
            break

        # fastbootd checks the downloaded sparse file length before decoding it.
        # A nearly incompressible image can therefore be a few sparse-header bytes
        # larger than the logical partition and fail with EOVERFLOW even though its
        # expanded size is correct. Add zero tail room and regenerate the sparse file.
        with raw.open("r+b") as fh:
            fh.truncate(safe_size)
        sparse.unlink(missing_ok=True)
        logger.write(f"{name} raw padded for sparse transport {out_size} -> {safe_size}")
        print(
            f"  │  {name} sparse 封装比逻辑分区大，已补零到 {safe_size} 字节后重试"
        )
        out_size = safe_size
    else:
        raise FlashError(f"{name} sparse 封装尺寸连续超过逻辑分区，无法安全刷入")

    # free multi-GB raw ASAP
    try:
        raw.unlink(missing_ok=True)
    except OSError:
        pass
    return out_size


def restore_dat_to_raw(
    profile: FlashProfile,
    session: DeviceSession,
    name: str,
    dat: Path,
    raw: Path,
    logger: Logger,
) -> int:
    """Restore one block-OTA DAT stream to a sparse host-side raw file."""
    profile.work_dir.mkdir(parents=True, exist_ok=True)
    transfer = profile.super_dir / f"{name}.transfer.list"
    if not transfer.is_file():
        raise FlashError(f"缺少 {name}.transfer.list，无法正确还原分区镜像")
    if not dat.is_file():
        raise FlashError(f"缺少 {name}.new.dat，无法正确还原分区镜像")
    if session.dry_run:
        print(f"  ├─ [dry-run] sdat2img {transfer.name} + {dat.name} -> {raw.name}")
        raw.write_bytes(b"")
        return 0

    logger.write(f"sdat2img {transfer} {dat} -> {raw}")

    def _prog(done: int, total: int) -> None:
        # Same-line live bar (no trailing newline until finished)
        total = max(1, total)
        pct = min(100, int(done * 100 / total))
        width = 30
        filled = int(width * pct / 100)
        bar = "#" * filled + "-" * (width - filled)
        print(f"\r  │  sdat2img {name:<12} [{bar}] {pct:3d}%", end="", flush=True)

    try:
        out_size = sdat2img(transfer, dat, raw, progress=_prog)
    except SdatError as exc:
        print()  # break out of the progress line before error text
        raise FlashError(f"sdat2img 还原 {name} 失败：{exc}") from exc
    print(f"\r  │  sdat2img {name:<12} [{'#' * 30}] 100%", flush=True)
    logger.write(f"sdat2img {name} raw size={out_size}")
    return out_size


def prepare_wipe_super_source(
    profile: FlashProfile,
    session: DeviceSession,
    logger: Logger,
) -> Path:
    """Resolve a legacy IMG or restore a DAT-only empty-super source."""
    if profile.wipe_super_dat:
        name = profile.wipe_super_dat
        dat = profile.super_dir / f"{name}.new.dat"
        raw = profile.work_dir / f"{name}.raw.img"
        print(f"  │  正在还原空 super：{name}.new.dat")
        restore_dat_to_raw(profile, session, name, dat, raw, logger)
        return raw
    if profile.wipe_super_image:
        return profile.images_dir / profile.wipe_super_image
    raise FlashError("线刷配置未指定空 super 的 IMG 或 DAT")


def _wipe_or_init_super(
    profile: FlashProfile,
    session: DeviceSession,
    logger: Logger,
    wipe_img: Path,
) -> None:
    """
    Initialize empty super in fastbootd.

    Prefer a rebuilt wipe-super image (geometry at offset 0). flash super of a
    sparse full empty often returns 'No such file or directory' on Xiaomi
    fastbootd — we still retry briefly, then fall back to creating logical
    partitions from the profile so flashing can continue.
    """
    import time

    if not wipe_img.is_file():
        raise FlashError(f"找不到空 super 刷写源：{wipe_img}")

    # Confirm userspace fastbootd
    us = session.getvar("is-userspace", allow_missing=True)
    if us != "yes":
        print("  │  未检测到 fastbootd，尝试重新进入…")
        session.run_fastboot(
            ["reboot", "fastboot"],
            label="reboot fastboot",
            show_output=True,
            allow_failure=True,
        )
        time.sleep(3)
        session.wait_for_mode(True, timeout_seconds=90)

    profile.work_dir.mkdir(parents=True, exist_ok=True)
    candidates: list[tuple[str, Path]] = []

    # 1) Proper wipe-super metadata image (fix gDla @0)
    wipe_meta = profile.work_dir / "super_empty_wipe.img"
    try:
        build_wipe_super_image(wipe_img, wipe_meta)
        logger.write(
            f"built wipe-super image {wipe_meta} ({wipe_meta.stat().st_size} bytes) "
            f"from {wipe_img} sparse={is_android_sparse(wipe_img)}"
        )
        print(f"  │  已生成 wipe-super 元数据镜像（{wipe_meta.stat().st_size} 字节）")
        candidates.append(("wipe-super", wipe_meta))
    except Exception as exc:
        logger.write(f"build_wipe_super_image failed: {exc!r}")
        print(f"  │  生成 wipe-super 镜像失败：{exc}")
        if not is_android_sparse(wipe_img):
            candidates.append(("wipe-super", wipe_img))

    # 2) flash sparse super (may work on some builds; often fails on Xiaomi)
    if is_android_sparse(wipe_img):
        candidates.append(("flash-super", wipe_img))

    last_error = ""
    for mode, path in candidates:
        attempts = 3 if mode == "flash-super" else 2
        for attempt in range(1, attempts + 1):
            if mode == "wipe-super":
                label = f"wipe-super ← {path.name}（第 {attempt}/{attempts} 次）"
                result = session.run_fastboot(
                    ["wipe-super", str(path)],
                    label=label,
                    allow_failure=True,
                    show_output=True,
                )
            else:
                label = f"flash super ← {path.name}（第 {attempt}/{attempts} 次）"
                result = session.run_fastboot(
                    ["flash", "super", str(path)],
                    label=label,
                    allow_failure=True,
                    show_output=True,
                )
            if result.code == 0:
                logger.write(f"super init OK via {mode}: {path}")
                print(f"  │  空 super 初始化成功（{mode}）")
                return
            last_error = result.text or f"exit {result.code}"
            logger.write(f"super init failed via {mode} try {attempt}: {last_error}")
            # "No such file" right after reboot — wait and retry
            if "No such file" in last_error or "not found" in last_error.lower():
                time.sleep(2 + attempt)
            else:
                break  # geometry parse errors won't fix by retry
        print(f"  │  {mode} 未成功，尝试下一方案…")

    # 3) Last resort: create logical partitions from profile (no empty super image)
    print("  │  尝试按配置 create-logical-partition 创建逻辑分区…")
    if _create_logical_partitions_from_profile(profile, session, logger):
        print("  │  逻辑分区已创建，继续刷写动态分区")
        return

    raise FlashError(
        "在 fastbootd 初始化空 super 失败。\n"
        f"已尝试：wipe-super / flash super / create-logical-partition\n"
        f"最后错误：\n{last_error}\n"
        "请确认空 super 的 IMG/DAT 资源完整，且当前为 fastbootd（is-userspace:yes）。"
    )


def _create_logical_partitions_from_profile(
    profile: FlashProfile,
    session: DeviceSession,
    logger: Logger,
) -> bool:
    """Best-effort: delete+create each logical partition so flash can proceed."""
    if not profile.dynamic_partitions:
        return False
    ok_any = False
    suffix = profile.slot_suffix or "_a"
    for part in profile.dynamic_partitions:
        name = f"{part.name}{suffix}"
        size = logical_flash_size(part.pad_to_bytes or part.dat_bytes)
        session.run_fastboot(
            ["delete-logical-partition", name],
            label=f"delete-logical-partition {name}",
            show_output=True,
            allow_failure=True,
        )
        result = session.run_fastboot(
            ["create-logical-partition", name, str(size)],
            label=f"create-logical-partition {name} {size}",
            show_output=True,
            allow_failure=True,
        )
        if result.code == 0:
            ok_any = True
            logger.write(f"created logical {name} size={size}")
        else:
            logger.write(f"create logical {name} failed: {result.text}")
    return ok_any


def flash_dynamic_partition_image(
    session: DeviceSession,
    partition: str,
    image: Path,
    logger: Logger,
    *,
    logical_size: int,
) -> None:
    """Flash one dynamic partition, recreating tiny logicals and retrying EOVERFLOW."""
    small = logical_size <= _SMALL_LOGICAL_RECREATE_BYTES
    if small:
        recreate_logical_partition(session, partition, logical_size)
    result = session.run_fastboot(
        ["flash", partition, str(image)],
        label=f"刷入 {partition}",
        show_output=True,
        allow_failure=True,
    )
    if result.code == 0:
        return
    text = result.text or ""
    if is_overflow_flash_error(text) or small:
        retry_size = max(logical_size, MIN_LOGICAL_FLASH_BYTES)
        print("  │  逻辑分区写入失败，正在重建后重试…")
        logger.write(f"retry flash {partition} after logical recreate: {text}")
        recreate_logical_partition(session, partition, retry_size)
        result = session.run_fastboot(
            ["flash", partition, str(image)],
            label=f"刷入 {partition}（重试）",
            show_output=True,
            allow_failure=True,
        )
        if result.code == 0:
            return
        text = result.text or ""
    raise DeviceError(
        f"刷入 {partition} 失败（{result.code}）：flash {partition} {image}\n{text}"
    )


def flash_dynamic_partitions(
    profile: FlashProfile,
    session: DeviceSession,
    logger: Logger,
) -> None:
    session.run_fastboot(["reboot", "fastboot"], label="重启到 fastbootd", show_output=True)
    if not session.dry_run:
        session.wait_for_mode(True)

    wipe_img = prepare_wipe_super_source(profile, session, logger)
    _wipe_or_init_super(profile, session, logger, wipe_img)

    if profile.work_dir.exists():
        shutil.rmtree(profile.work_dir, ignore_errors=True)
    profile.work_dir.mkdir(parents=True, exist_ok=True)

    try:
        total = len(profile.dynamic_partitions)
        for index, part in enumerate(profile.dynamic_partitions, start=1):
            target = f"{part.name}{profile.slot_suffix}"
            if profile.post_flash_id and is_separate_id_partition(part.name):
                print(
                    f"  ├─ [{index}/{total}] 跳过 {part.name} "
                    "（由独立 ID 补丁刷入，避免覆盖）"
                )
                logger.write(
                    f"skipped placeholder {part.name}; ID patch is applied separately"
                )
                continue
            dat = profile.super_dir / f"{part.name}.new.dat"
            sparse = profile.work_dir / f"{part.name}.img"
            print(f"  ├─ [{index}/{total}] 转换 {part.name} DAT → sparse")
            flashed_size = convert_dat_to_sparse(
                profile,
                session,
                part.name,
                dat,
                sparse,
                part.pad_to_bytes,
                logger,
            )
            flash_dynamic_partition_image(
                session,
                target,
                sparse,
                logger,
                logical_size=flashed_size,
            )
            for junk in (sparse, profile.work_dir / f"{part.name}.raw.img"):
                try:
                    if junk.exists():
                        junk.unlink(missing_ok=True)
                except OSError:
                    pass
    finally:
        if profile.work_dir.exists():
            shutil.rmtree(profile.work_dir, ignore_errors=True)

    session.run_fastboot(["reboot", "bootloader"], label="返回 bootloader", show_output=True)
    if not session.dry_run:
        session.wait_for_mode(False)


def initialize_crc(profile: FlashProfile, session: DeviceSession) -> None:
    if not profile.flash_crc_lists:
        return
    crc = session.getvar("crc", allow_missing=True)
    if crc == "1":
        session.run_fastboot(
            ["flash", "crclist", str(profile.images_dir / profile.crc_list_file)],
            label="加载普通镜像 CRC 表",
            show_output=True,
        )
        session.run_fastboot(
            [
                "flash",
                "sparsecrclist",
                str(profile.images_dir / profile.sparse_crc_list_file),
            ],
            label="加载 sparse 镜像 CRC 表",
            show_output=True,
        )


def run_full_flash(
    profile: FlashProfile,
    logger: Logger,
    *,
    choose: ChooseFn | None = None,
    confirm: ConfirmFn | None = None,
    dry_run: bool = False,
    skip_confirm: bool = False,
    preserve_data: bool = False,
    reboot: bool | None = None,
) -> DeviceSession | None:
    """Run full line flash. Returns DeviceSession (still on device) for post steps.

    reboot:
      None  → use profile.reboot_after_success
      False → never reboot here (caller does ID / format / reboot)
      True  → always reboot at end
    """
    if preserve_data and not skip_confirm and not dry_run:
        word = profile.update_confirmation_word
        if confirm:
            answer = confirm(
                "更新模式会覆盖系统、固件和启动分区，但保留 userdata / metadata。\n"
                "更新前仍建议备份重要资料。\n"
                f"请输入 {word} 后按回车继续；输入其他内容将取消。"
            )
        else:
            print(f"  请输入 {word} 确认保留数据更新：")
            answer = input("  确认> ").strip()
        if answer != word:
            raise FlashError("已取消，没有写入设备。")
    elif profile.require_wipe_confirmation and not skip_confirm and not dry_run:
        word = profile.confirmation_word
        if confirm:
            answer = confirm(
                f"最后确认：本操作会格式化 userdata 和 metadata。\n"
                f"请输入 {word} 后按回车继续；输入其他内容将取消。"
            )
        else:
            print(f"  请输入 {word} 确认完整线刷（清空 Data）：")
            answer = input("  确认> ").strip()
        if answer != word:
            raise FlashError("已取消，没有写入设备。")

    session = DeviceSession(profile.fastboot, profile.adb, logger, dry_run=dry_run)

    print()
    print("  阶段 1/6 · 安全预检")
    try:
        validate_package(profile, preserve_data=preserve_data)
    except PackageError as exc:
        raise FlashError(str(exc)) from exc

    report = gather_environment(profile, session, choose=choose)
    if not report.super_ready:
        raise FlashError(
            f"super 只有 {report.super_bytes} 字节，未达到要求的 "
            f"{profile.minimum_super_bytes} 字节。请先运行独立扩容工具。"
        )
    if report.is_userspace:
        session.run_fastboot(
            ["reboot", "bootloader"],
            label="从 fastbootd 返回 bootloader",
            show_output=True,
        )
        if not dry_run:
            session.wait_for_mode(False)

    package_anti = read_package_anti(profile)
    if package_anti is None:
        logger.write(
            f"Anti check skipped (no package anti file); device anti={report.anti!r}"
        )
    else:
        device_anti = 0
        if report.anti and str(report.anti).isdigit():
            device_anti = int(report.anti)
        if device_anti > package_anti:
            raise FlashError(
                f"Anti rollback 检查失败：设备={device_anti}，刷机包={package_anti}。"
            )
        logger.write(f"Anti check OK: device={device_anti}, package={package_anti}")

    logger.write(
        f"Preflight passed: serial={report.serial}, product={report.product}, "
        f"super={report.super_bytes}, dry_run={dry_run}, preserve_data={preserve_data}"
    )
    initialize_crc(profile, session)

    print()
    print("  阶段 2/6 · 固件与启动链前半段")
    for name in profile.erase_if_present:
        ptype = session.getvar(f"partition-type:{name}", allow_missing=True)
        if ptype == "raw":
            session.run_fastboot(["erase", name], label=f"擦除 {name}", show_output=True)
    for name in profile.pre_erase:
        session.run_fastboot(["erase", name], label=f"擦除 {name}", show_output=True)

    total_fw = len(profile.firmware)
    for i, op in enumerate(profile.firmware, start=1):
        print(f"  · 固件 {i}/{total_fw}")
        run_image_op(profile, session, op)

    print()
    print("  阶段 3/6 · 补齐 fastbootd 启动链")
    total_bc = len(profile.boot_chain)
    for i, op in enumerate(profile.boot_chain, start=1):
        print(f"  · 启动链 {i}/{total_bc}")
        run_image_op(profile, session, op)
    if profile.set_active_slot:
        session.run_fastboot(
            ["set_active", profile.set_active_slot],
            label=f"预设活动槽位 {profile.set_active_slot}",
            show_output=True,
        )

    print()
    print("  阶段 4/6 · super 与动态分区")
    if profile.dynamic_partitions:
        flash_dynamic_partitions(profile, session, logger)
    else:
        print("  （配置中无动态分区，跳过）")

    print()
    if preserve_data:
        print("  阶段 5/6 · 保留数据与活动槽位")
    else:
        print("  阶段 5/6 · 数据分区与活动槽位")
    for op in profile.final_data:
        if preserve_data and is_preserved_data_partition(op.partition):
            print(f"  ├─ [更新模式] 跳过 {op.action} {op.partition}")
            logger.write(
                f"Update preserve-data: skipped {op.action} {op.partition} {op.file or ''}"
            )
            continue
        run_image_op(profile, session, op)
    if profile.set_active_slot:
        session.run_fastboot(
            ["set_active", profile.set_active_slot],
            label=f"设置活动槽位 {profile.set_active_slot}",
            show_output=True,
        )

    print()
    print("  阶段 6/6 · 包体刷写完成")
    result_text = "固件与动态分区均已成功。"
    if preserve_data:
        result_text += "userdata / metadata 已保留。"
    print("  " + result_text + ("（dry-run）" if dry_run else ""))
    if logger.path:
        print(f"  日志：{logger.path}")

    do_reboot = profile.reboot_after_success if reboot is None else reboot
    if do_reboot and not dry_run:
        session.run_fastboot(["reboot"], label="重启设备", show_output=True)
    return session


def run_diagnostics(
    profile: FlashProfile,
    logger: Logger,
    *,
    choose: ChooseFn | None = None,
) -> EnvReport:
    validate_package(profile)
    session = DeviceSession(profile.fastboot, profile.adb, logger, dry_run=False)
    return gather_environment(profile, session, choose=choose)
