"""Minimal Android sparse image helpers for super_empty / wipe-super."""

from __future__ import annotations

import struct
from pathlib import Path

# ANDROID sparse magic
SPARSE_MAGIC = 0xED26FF3A
CHUNK_RAW = 0xCAC1
CHUNK_FILL = 0xCAC2
CHUNK_DONT_CARE = 0xCAC3
CHUNK_CRC32 = 0xCAC4

# liblp on-disk magics (little-endian)
LP_GEOMETRY_MAGIC = b"gDla"
LP_METADATA_MAGIC = struct.pack("<I", 0x414C5030)  # "ALP0" as uint32 LE → 0PLA
LP_GEOMETRY_SIZE = 4096
LP_DEVICE_RESERVED = 4096


def is_android_sparse(path: Path) -> bool:
    try:
        with path.open("rb") as fh:
            magic = struct.unpack("<I", fh.read(4))[0]
        return magic == SPARSE_MAGIC
    except OSError:
        return False


def sparse_logical_size(path: Path) -> int | None:
    """Return unsparsed size of an Android sparse image, or None if not sparse."""
    try:
        with path.open("rb") as fh:
            header = fh.read(28)
        if len(header) < 28:
            return None
        magic, _major, _minor, _file_hdr_sz, _chunk_hdr_sz, blk_sz, total_blks, _chunks, _crc = (
            struct.unpack("<IHHHHIIII", header)
        )
        if magic != SPARSE_MAGIC or blk_sz == 0:
            return None
        return total_blks * blk_sz
    except OSError:
        return None


def build_android_sparse(
    prefix: bytes,
    total_bytes: int,
    out_path: Path,
    *,
    block_size: int = 4096,
) -> Path:
    """
    Build a flashable Android sparse image: RAW prefix + DONT_CARE remainder.

    Used so an empty super DAT prefix can be flashed to the physical super
    partition without materializing a 16GiB raw file.
    """
    if block_size <= 0 or total_bytes <= 0 or total_bytes % block_size:
        raise ValueError(f"total_bytes must be a positive multiple of {block_size}")
    if not prefix:
        raise ValueError("empty super prefix is empty")
    pad = (-len(prefix)) % block_size
    if pad:
        prefix = prefix + b"\x00" * pad
    if len(prefix) > total_bytes:
        prefix = prefix[:total_bytes]
    total_blks = total_bytes // block_size
    raw_blks = len(prefix) // block_size
    rest_blks = total_blks - raw_blks
    chunks: list[tuple[int, int, bytes]] = [(CHUNK_RAW, raw_blks, prefix)]
    if rest_blks:
        chunks.append((CHUNK_DONT_CARE, rest_blks, b""))
    file_hdr_sz = 28
    chunk_hdr_sz = 12
    header = struct.pack(
        "<IHHHHIIII",
        SPARSE_MAGIC,
        1,
        0,
        file_hdr_sz,
        chunk_hdr_sz,
        block_size,
        total_blks,
        len(chunks),
        0,
    )
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("wb") as fh:
        fh.write(header)
        for ctype, chunk_sz, payload in chunks:
            total_sz = chunk_hdr_sz + len(payload)
            fh.write(struct.pack("<HHII", ctype, 0, chunk_sz, total_sz))
            fh.write(payload)
    return out_path


def extract_sparse_prefix(sparse_path: Path, out_path: Path, max_bytes: int = 4 * 1024 * 1024) -> int:
    """
    Materialize the first max_bytes of the unsparsed image into out_path.

    Used so wipe-super can parse LP geometry (gDla) that lives at the start of
    a sparse empty super image without expanding a full 16GiB file.
    """
    with sparse_path.open("rb") as fh:
        header = fh.read(28)
        if len(header) < 28:
            raise ValueError(f"sparse header too short: {sparse_path}")
        magic, major, minor, file_hdr_sz, chunk_hdr_sz, blk_sz, total_blks, total_chunks, _crc = (
            struct.unpack("<IHHHHIIII", header)
        )
        if magic != SPARSE_MAGIC:
            raise ValueError(f"not an Android sparse image: {sparse_path}")
        if file_hdr_sz > 28:
            fh.read(file_hdr_sz - 28)

        out_path.parent.mkdir(parents=True, exist_ok=True)
        written = 0
        with out_path.open("wb") as out:
            for _ in range(total_chunks):
                if written >= max_bytes:
                    break
                ch = fh.read(chunk_hdr_sz)
                if len(ch) < 12:
                    raise ValueError("truncated sparse chunk header")
                chunk_type, _reserved, chunk_sz, total_sz = struct.unpack("<HHII", ch[:12])
                if chunk_hdr_sz > 12:
                    fh.read(chunk_hdr_sz - 12)

                data_size = total_sz - chunk_hdr_sz
                unsparsed_chunk = chunk_sz * blk_sz
                need = min(unsparsed_chunk, max_bytes - written)

                if chunk_type == CHUNK_RAW:
                    payload = fh.read(data_size)
                    if len(payload) != data_size:
                        raise ValueError("truncated raw chunk")
                    out.write(payload[:need])
                    if data_size > unsparsed_chunk:
                        # shouldn't happen
                        pass
                    written += need
                elif chunk_type == CHUNK_FILL:
                    fill = fh.read(4)
                    if len(fill) != 4:
                        raise ValueError("truncated fill chunk")
                    # skip rest of declared data if any
                    rest = data_size - 4
                    if rest > 0:
                        fh.read(rest)
                    out.write(fill * (need // 4) + fill[: need % 4])
                    written += need
                elif chunk_type == CHUNK_DONT_CARE:
                    if data_size:
                        fh.read(data_size)
                    out.write(b"\x00" * need)
                    written += need
                elif chunk_type == CHUNK_CRC32:
                    fh.read(data_size)
                else:
                    raise ValueError(f"unknown sparse chunk type 0x{chunk_type:x}")

        return written


def has_lp_geometry_magic(path: Path) -> bool:
    """True if file contains LP geometry magic 'gDla' (primary @0 or backup @4096)."""
    try:
        with path.open("rb") as fh:
            # reserved + primary geometry 4K + backup geometry 4K
            head = fh.read(LP_DEVICE_RESERVED + LP_GEOMETRY_SIZE * 2)
        return LP_GEOMETRY_MAGIC in head
    except OSError:
        return False


def _magic_at(data: bytes, offset: int, magic: bytes) -> bool:
    return data[offset : offset + len(magic)] == magic


def normalize_wipe_super_payload(data: bytes) -> bytes:
    """
    Convert a super prefix into the host image layout that `fastboot wipe-super`
    (liblp ReadFromImageFile) expects:

      offset 0:     primary geometry (gDla)
      offset 4096:  backup geometry (gDla)
      offset 8192:  metadata header (0PLA)

    A raw super partition / sdat2img of super_empty uses device layout instead:

      offset 0:     4K reserved
      offset 4096:  primary geometry
      offset 8192:  backup geometry
      offset 12288: metadata header

    Blindly duplicating the first gDla block puts a second geometry where
    metadata belongs; host liblp then reports 'invalid magic value'.
    """
    min_len = LP_GEOMETRY_SIZE * 2 + len(LP_METADATA_MAGIC)
    if len(data) < min_len:
        raise ValueError("wipe-super source too short for LP geometry")

    if _magic_at(data, 0, LP_GEOMETRY_MAGIC):
        rebuilt = data
    elif _magic_at(data, LP_DEVICE_RESERVED, LP_GEOMETRY_MAGIC) and _magic_at(
        data, LP_DEVICE_RESERVED + LP_GEOMETRY_SIZE, LP_GEOMETRY_MAGIC
    ):
        rebuilt = data[LP_DEVICE_RESERVED:]
    elif _magic_at(data, LP_GEOMETRY_SIZE, LP_GEOMETRY_MAGIC) and _magic_at(
        data, LP_GEOMETRY_SIZE * 2, LP_METADATA_MAGIC
    ):
        geom = data[LP_GEOMETRY_SIZE : LP_GEOMETRY_SIZE * 2]
        if len(geom) < LP_GEOMETRY_SIZE:
            raise ValueError("truncated LP geometry block")
        rebuilt = geom + geom + data[LP_GEOMETRY_SIZE * 2 :]
    else:
        idx = 0
        geom_off = -1
        while True:
            found = data.find(LP_GEOMETRY_MAGIC, idx)
            if found < 0:
                break
            if found % LP_GEOMETRY_SIZE == 0:
                geom_off = found
                break
            idx = found + 1
        if geom_off < 0:
            raise ValueError("no LP geometry magic (gDla) in wipe-super source")
        geom = data[geom_off : geom_off + LP_GEOMETRY_SIZE]
        if len(geom) < LP_GEOMETRY_SIZE:
            raise ValueError("truncated LP geometry block")
        rebuilt = geom + geom + data[geom_off + LP_GEOMETRY_SIZE :]

    if not rebuilt.startswith(LP_GEOMETRY_MAGIC):
        raise ValueError("internal error: rebuilt wipe-super image missing gDla at 0")
    if not _magic_at(rebuilt, LP_GEOMETRY_SIZE * 2, LP_METADATA_MAGIC):
        raise ValueError(
            "rebuilt wipe-super image missing LP metadata magic (0PLA) at offset 8192"
        )
    return rebuilt


def build_wipe_super_image(source: Path, out_path: Path, *, max_bytes: int = 16 * 1024 * 1024) -> Path:
    """
    Build a host-side image suitable for `fastboot wipe-super`.

    Accepts a sparse or raw super_empty, including a full-super device image
    whose geometry lives after the 4K reserved region.
    """
    out_path.parent.mkdir(parents=True, exist_ok=True)
    prefix = out_path.with_name(out_path.stem + ".prefix.bin")
    if is_android_sparse(source):
        extract_sparse_prefix(source, prefix, max_bytes=max_bytes)
    else:
        # DAT-restored raw super files are usually multi-GB sparse host files.
        # Read only the prefix needed by liblp instead of materializing all of it.
        with source.open("rb") as source_fh:
            prefix.write_bytes(source_fh.read(max_bytes))

    try:
        rebuilt = normalize_wipe_super_payload(prefix.read_bytes())
        out_path.write_bytes(rebuilt)
    finally:
        try:
            prefix.unlink(missing_ok=True)
        except OSError:
            pass
    return out_path
