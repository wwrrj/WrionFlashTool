from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from harusora_flash_tool.sparse import (
    LP_DEVICE_RESERVED,
    LP_GEOMETRY_MAGIC,
    LP_GEOMETRY_SIZE,
    LP_METADATA_MAGIC,
    SPARSE_MAGIC,
    build_android_sparse,
    build_wipe_super_image,
    normalize_wipe_super_payload,
    sparse_logical_size,
)


def _block(magic: bytes, size: int = LP_GEOMETRY_SIZE) -> bytes:
    return magic + b"\0" * (size - len(magic))


def _device_layout() -> bytes:
    reserved = b"\0" * LP_DEVICE_RESERVED
    geom = _block(LP_GEOMETRY_MAGIC)
    meta = _block(LP_METADATA_MAGIC)
    return reserved + geom + geom + meta


def _image_layout() -> bytes:
    geom = _block(LP_GEOMETRY_MAGIC)
    meta = _block(LP_METADATA_MAGIC)
    return geom + geom + meta


def _missing_primary_image() -> bytes:
    zeros = b"\0" * LP_GEOMETRY_SIZE
    geom = _block(LP_GEOMETRY_MAGIC)
    meta = _block(LP_METADATA_MAGIC)
    return zeros + geom + meta


class WipeSuperLayoutTest(unittest.TestCase):
    def _assert_host_layout(self, data: bytes) -> None:
        self.assertTrue(data.startswith(LP_GEOMETRY_MAGIC))
        self.assertEqual(data[LP_GEOMETRY_SIZE : LP_GEOMETRY_SIZE + 4], LP_GEOMETRY_MAGIC)
        self.assertEqual(data[LP_GEOMETRY_SIZE * 2 : LP_GEOMETRY_SIZE * 2 + 4], LP_METADATA_MAGIC)

    def test_strips_device_reserved_prefix(self) -> None:
        rebuilt = normalize_wipe_super_payload(_device_layout())
        self._assert_host_layout(rebuilt)
        self.assertEqual(rebuilt, _image_layout())

    def test_keeps_host_image_layout(self) -> None:
        source = _image_layout()
        self.assertEqual(normalize_wipe_super_payload(source), source)

    def test_repairs_missing_primary_geometry(self) -> None:
        rebuilt = normalize_wipe_super_payload(_missing_primary_image())
        self._assert_host_layout(rebuilt)

    def test_does_not_place_geometry_where_metadata_belongs(self) -> None:
        rebuilt = normalize_wipe_super_payload(_device_layout())
        self.assertNotEqual(
            rebuilt[LP_GEOMETRY_SIZE * 2 : LP_GEOMETRY_SIZE * 2 + 4],
            LP_GEOMETRY_MAGIC,
        )

    def test_is_idempotent(self) -> None:
        once = normalize_wipe_super_payload(_device_layout())
        self.assertEqual(normalize_wipe_super_payload(once), once)

    def test_rejects_payload_without_metadata(self) -> None:
        with self.assertRaises(ValueError):
            normalize_wipe_super_payload(_block(LP_GEOMETRY_MAGIC) * 3)

    def test_build_wipe_super_image_from_device_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "super_empty.raw.img"
            out = root / "super_empty_wipe.img"
            source.write_bytes(_device_layout())
            build_wipe_super_image(source, out)
            self._assert_host_layout(out.read_bytes())

    def test_normalizes_real_super_empty_dat_if_present(self) -> None:
        rom = Path(
            r"E:\latest=rom\Xiaomi14_MiApps_ColorOS_16.0.10.501_HOUJIIN-Beta3_cfb46f3_build260907"
            r"\images\super\super_empty.new.dat"
        )
        if not rom.is_file():
            self.skipTest("sample ROM super_empty.new.dat not present")
        rebuilt = normalize_wipe_super_payload(rom.read_bytes())
        self._assert_host_layout(rebuilt)


class AndroidSparseBuilderTest(unittest.TestCase):
    def test_raw_prefix_and_dont_care_remainder(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "empty.sparse.img"
            prefix = b"A" * 8192
            build_android_sparse(prefix, 16 * 4096, out)
            self.assertEqual(sparse_logical_size(out), 16 * 4096)
            data = out.read_bytes()
            magic = int.from_bytes(data[:4], "little")
            self.assertEqual(magic, SPARSE_MAGIC)
            self.assertLess(out.stat().st_size, 16 * 4096)


if __name__ == "__main__":
    unittest.main()
