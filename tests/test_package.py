from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

from harusora_flash_tool.config import DynamicPartition
from harusora_flash_tool.package import collect_required_paths, resolve_wipe_super_source


def _profile(root: Path, **overrides) -> SimpleNamespace:
    data = dict(
        fastboot=root / "tools" / "platform-tools" / "fastboot.exe",
        adb=root / "tools" / "platform-tools" / "adb.exe",
        img2simg=root / "tools" / "img2simg.exe",
        cygwin_dll=root / "tools" / "cygwin1.dll",
        images_dir=root / "images",
        super_dir=root / "super",
        wipe_super_image="super_empty_16g.img",
        wipe_super_dat=None,
        flash_crc_lists=False,
        crc_list_file="crclist.txt",
        sparse_crc_list_file="sparsecrclist.txt",
        firmware=[],
        boot_chain=[],
        final_data=[],
        dynamic_partitions=[DynamicPartition(name="system", dat_bytes=4)],
    )
    data.update(overrides)
    return SimpleNamespace(**data)


class WipeSuperSourceResolutionTest(unittest.TestCase):
    def test_prefers_existing_wipe_image(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            images = root / "images"
            images.mkdir()
            image = images / "super_empty_16g.img"
            image.write_bytes(b"img")
            source, transfer = resolve_wipe_super_source(_profile(root))
            self.assertEqual(source, image)
            self.assertIsNone(transfer)

    def test_prefers_super_empty_img_over_dat(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            images = root / "images"
            super_dir = root / "super"
            images.mkdir()
            super_dir.mkdir()
            image = images / "super_empty_16g.img"
            image.write_bytes(b"img")
            (super_dir / "super_empty.new.dat").write_bytes(b"dat")
            (super_dir / "super_empty.transfer.list").write_text(
                "4\n1\n0\n0\n", encoding="utf-8"
            )
            profile = _profile(root, wipe_super_dat="super_empty", wipe_super_image="")
            source, transfer = resolve_wipe_super_source(profile)
            self.assertEqual(source, image)
            self.assertIsNone(transfer)

    def test_falls_back_to_super_empty_dat(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            super_dir = root / "super"
            super_dir.mkdir()
            dat = super_dir / "super_empty.new.dat"
            transfer = super_dir / "super_empty.transfer.list"
            dat.write_bytes(b"dat")
            transfer.write_text("4\n1\n0\n0\n", encoding="utf-8")
            profile = _profile(root, wipe_super_dat="super_empty", wipe_super_image="")
            source, listed = resolve_wipe_super_source(profile)
            self.assertEqual(source, dat)
            self.assertEqual(listed, transfer)
            required = collect_required_paths(profile)
            self.assertIn(dat, required)
            self.assertIn(transfer, required)
            self.assertNotIn(root / "images" / "super_empty_16g.img", required)


if __name__ == "__main__":
    unittest.main()
