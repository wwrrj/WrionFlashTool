"""Convert Android block OTA .new.dat + transfer.list → raw partition image.

Mirrors the classic sdat2img logic (block_image_update on host).
Direct img2simg on .new.dat is WRONG: new.dat is a sequential stream for
non-contiguous block ranges listed in transfer.list, not a linear disk image.
"""

from __future__ import annotations

import re
from pathlib import Path


class SdatError(Exception):
    pass


_RANGE_RE = re.compile(r"(\d+(?:,\d+)*)")


def _parse_ranges(text: str) -> list[tuple[int, int]]:
    """
    Parse transfer.list range set after command word.
    Formats seen:
      new 2,0,1          → count=2, pairs (0,1)
      new 2,953857,954369
      new 4,0,1,2,3      → two pairs (0,1)(2,3)
    First number is the count of following integers (must be even for pairs).
    """
    text = text.strip()
    if not text:
        return []
    parts = [int(x) for x in text.split(",")]
    if len(parts) < 1:
        return []
    # Official format: <num_blocks_in_list>,start,end,start,end,...
    # where each pair is [start, end) in 4K blocks
    n = parts[0]
    rest = parts[1:]
    if n != len(rest):
        # Some lists omit count and only have pairs — try pair parse
        if len(parts) % 2 == 0:
            pairs = []
            for i in range(0, len(parts), 2):
                pairs.append((parts[i], parts[i + 1]))
            return pairs
        raise SdatError(f"range count mismatch: declared {n}, got {len(rest)} ints in {text!r}")
    if len(rest) % 2 != 0:
        raise SdatError(f"odd number of range values: {text!r}")
    pairs = []
    for i in range(0, len(rest), 2):
        pairs.append((rest[i], rest[i + 1]))
    return pairs


def _ranges_block_count(pairs: list[tuple[int, int]]) -> int:
    return sum(max(0, e - s) for s, e in pairs)


def sdat2img(
    transfer_list: Path,
    new_data: Path,
    output_img: Path,
    *,
    block_size: int = 4096,
    progress=None,
) -> int:
    """
    Build a raw partition image. Returns output size in bytes.
    """
    lines = transfer_list.read_text(encoding="utf-8", errors="replace").splitlines()
    if len(lines) < 4:
        raise SdatError(f"transfer.list too short: {transfer_list}")

    try:
        version = int(lines[0].strip())
    except ValueError as exc:
        raise SdatError(f"bad transfer.list version: {lines[0]!r}") from exc
    if version not in (1, 2, 3, 4):
        raise SdatError(f"unsupported transfer.list version {version}")

    try:
        total_blocks = int(lines[1].strip())
    except ValueError as exc:
        raise SdatError(f"bad total blocks: {lines[1]!r}") from exc

    # lines[2], lines[3] are stash entries for v2+; commands start at index 4 for v3/v4
    # For v1, commands start at line 2. For v2+, after stash size lines.
    if version == 1:
        cmd_start = 2
    else:
        cmd_start = 4

    output_img.parent.mkdir(parents=True, exist_ok=True)
    out_size = total_blocks * block_size

    # Pre-allocate sparse file on Windows via seek
    with output_img.open("wb") as out_fh, new_data.open("rb") as dat_fh:
        if out_size > 0:
            out_fh.seek(out_size - 1)
            out_fh.write(b"\0")
            out_fh.seek(0)

        dat_pos = 0
        dat_total = max(1, new_data.stat().st_size)
        last_report_pct = -1
        commands = lines[cmd_start:]

        def _maybe_report() -> None:
            nonlocal last_report_pct
            if not progress:
                return
            pct = min(100, int(dat_pos * 100 / dat_total))
            # report every 1% (and always at end)
            if pct != last_report_pct or dat_pos >= dat_total:
                last_report_pct = pct
                progress(dat_pos, dat_total)

        if progress:
            progress(0, dat_total)

        for raw in commands:
            line = raw.strip()
            if not line:
                continue

            # command word + rest
            if " " not in line:
                # e.g. erase with ranges attached? usually "erase 2,0,1"
                parts = line.split(None, 1)
            else:
                parts = line.split(None, 1)
            cmd = parts[0]
            arg = parts[1] if len(parts) > 1 else ""

            if cmd in ("erase", "zero"):
                # No need to write zeros: file pre-filled / sparse holes read as zero
                # (we wrote one byte at end; middle is zero on most FS)
                continue

            if cmd == "new":
                pairs = _parse_ranges(arg)
                for start, end in pairs:
                    if end < start:
                        raise SdatError(f"bad range {start}-{end}")
                    nblocks = end - start
                    nbytes = nblocks * block_size
                    data = dat_fh.read(nbytes)
                    if len(data) != nbytes:
                        raise SdatError(
                            f"new.dat truncated at offset {dat_pos}: need {nbytes}, got {len(data)}"
                        )
                    out_fh.seek(start * block_size)
                    out_fh.write(data)
                    dat_pos += nbytes
                    _maybe_report()
                continue

            if cmd in ("stash", "free", "crc"):
                # stash not needed for one-shot full image extract of pure-new lists
                continue

            # Ignore unknown commands that don't consume dat (or fail loud)
            if cmd.startswith("#"):
                continue
            raise SdatError(f"unsupported transfer command: {line!r}")

        # leftover data in new.dat is OK only if zero length
        leftover = dat_fh.read(1)
        if leftover:
            # Some packages pad; warn via size check only if significant
            more = len(dat_fh.read()) + 1
            if more > block_size:
                raise SdatError(
                    f"new.dat has {more} unexpected trailing bytes after applying transfer.list"
                )

    return out_size
