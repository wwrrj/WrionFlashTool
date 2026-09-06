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


def is_android_sparse(path: Path) -> bool:
    try:
        with path.open("rb") as fh:
            magic = struct.unpack("<I", fh.read(4))[0]
        return magic == SPARSE_MAGIC
    except OSError:
        return False


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
            # primary geometry 4K + backup geometry 4K
            head = fh.read(8192)
        return b"gDla" in head
    except OSError:
        return False


def build_wipe_super_image(source: Path, out_path: Path, *, max_bytes: int = 16 * 1024 * 1024) -> Path:
    """
    Build a host-side image suitable for `fastboot wipe-super`.

    Sparse full-super empties often have primary geometry as zeros and real
    geometry only at the backup slot (offset 4096). Host liblp then rejects the
    file with 'invalid geometry magic signature'.

    We extract a prefix of the unsparsed super, locate the first gDla block, and
    rewrite: [primary geom][backup geom][following metadata bytes...].
    """
    out_path.parent.mkdir(parents=True, exist_ok=True)
    prefix = out_path.with_name(out_path.stem + ".prefix.bin")
    if is_android_sparse(source):
        extract_sparse_prefix(source, prefix, max_bytes=max_bytes)
    else:
        # DAT-restored raw super files are usually multi-GB sparse host files.
        # Read only the prefix needed by liblp instead of materializing all of it.
        with source.open("rb") as source_fh:
            data0 = source_fh.read(max_bytes)
        prefix.write_bytes(data0)

    data = prefix.read_bytes()
    idx = data.find(b"gDla")
    if idx < 0:
        raise ValueError(f"no LP geometry magic (gDla) in {source}")

    geom = data[idx : idx + 4096]
    if len(geom) < 4096:
        raise ValueError("truncated LP geometry block")
    tail = data[idx + 4096 :]
    # Primary + backup geometry, then whatever followed the first geometry block
    rebuilt = geom + geom + tail
    if not rebuilt.startswith(b"gDla"):
        raise ValueError("internal error: rebuilt wipe-super image missing gDla at 0")
    out_path.write_bytes(rebuilt)
    try:
        prefix.unlink(missing_ok=True)
    except OSError:
        pass
    return out_path
