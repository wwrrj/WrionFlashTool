from __future__ import annotations

import tempfile
import unittest
import zipfile
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock, patch

from harusora_flash_tool.flash import (
    MIN_LOGICAL_FLASH_BYTES,
    convert_dat_to_sparse,
    flash_dynamic_partitions,
    is_overflow_flash_error,
    is_separate_id_partition,
    logical_flash_size,
    logical_size_for_sparse_payload,
)
from harusora_flash_tool.idflash import (
    IdFlashError,
    discover_id_candidates,
    flash_mi_ext_image,
    resolve_id_input,
)


def _write_transfer(path: Path, blocks: int) -> None:
    path.write_text(f"4\n{blocks}\n0\n0\nnew 2,0,{blocks}\n", encoding="utf-8")


def _make_id_zip(folder: Path, *, name: str = "IDpatch_test.zip", blocks: int = 2) -> Path:
    payload = b"ID" * (blocks * 2048)
    dat_name = "images/super/mi_ext.new.dat"
    list_name = "images/super/mi_ext.transfer.list"
    zip_path = folder / name
    with zipfile.ZipFile(zip_path, "w") as archive:
        archive.writestr(list_name, f"4\n{blocks}\n0\n0\nnew 2,0,{blocks}\n")
        archive.writestr(dat_name, payload)
        archive.writestr("images/super/mi_ext.patch.dat", b"")
        archive.writestr(
            "META-INF/com/google/android/updater-script",
            'ui_print("ID");\nblock_image_update(map_partition("mi_ext_a"));\n',
        )
    return zip_path


class LogicalFlashSizeTest(unittest.TestCase):
    def test_tiny_mi_ext_pads_to_1mib(self) -> None:
        self.assertEqual(logical_flash_size(139264, 139264), MIN_LOGICAL_FLASH_BYTES)

    def test_large_partition_unchanged(self) -> None:
        size = 5503029248
        self.assertEqual(logical_flash_size(size, size), size)

    def test_aligns_to_4k(self) -> None:
        self.assertEqual(logical_flash_size(100, None) % 4096, 0)

    def test_sparse_transport_overhead_gets_headroom(self) -> None:
        logical = 13_000_704
        grown = logical_size_for_sparse_payload(logical, logical + 40)
        self.assertGreater(grown, logical + 40)
        self.assertEqual(grown % 4096, 0)

    def test_sparse_transport_that_already_fits_is_unchanged(self) -> None:
        self.assertEqual(logical_size_for_sparse_payload(8192, 4096), 8192)

    def test_overflow_error_detected(self) -> None:
        self.assertTrue(
            is_overflow_flash_error(
                "Writing 'mi_ext_a' FAILED (remote: 'Value too large for defined data type')"
            )
        )
        self.assertFalse(is_overflow_flash_error("OKAY"))

    def test_separate_id_partition(self) -> None:
        self.assertTrue(is_separate_id_partition("mi_ext"))
        self.assertFalse(is_separate_id_partition("system"))


class IdZipResolveTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.work = self.root / "work"
        self.work.mkdir()

    def test_resolves_ota_zip_via_sdat2img(self) -> None:
        zip_path = _make_id_zip(self.root, blocks=2)
        image = resolve_id_input(zip_path, self.work)
        self.assertTrue(image.is_file())
        self.assertEqual(image.stat().st_size, 8192)
        self.assertEqual(image.read_bytes()[:2], b"ID")

    def test_rejects_zip_without_dat_or_image(self) -> None:
        zip_path = self.root / "empty.zip"
        with zipfile.ZipFile(zip_path, "w") as archive:
            archive.writestr("README.txt", "no id")
        with self.assertRaises(IdFlashError):
            resolve_id_input(zip_path, self.work)

    def test_discovers_zip_in_rom_root(self) -> None:
        zip_path = _make_id_zip(self.root, name="IDpatch_Sakiko.zip")
        found = discover_id_candidates(self.root)
        self.assertEqual([p.name for p in found], [zip_path.name])

    def test_resolves_raw_new_dat_with_transfer_list(self) -> None:
        dat = self.root / "mi_ext.new.dat"
        dat.write_bytes(b"X" * 8192)
        _write_transfer(self.root / "mi_ext.transfer.list", 2)
        image = resolve_id_input(dat, self.work)
        self.assertEqual(image.stat().st_size, 8192)
        self.assertEqual(image.read_bytes(), b"X" * 8192)


class IdFlashImageTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.work = Path(self.temporary.name)

    def test_pads_tiny_raw_and_recreates_logical_partitions(self) -> None:
        raw = self.work / "mi_ext.raw.img"
        raw.write_bytes(b"IDDATA" + b"\0" * 100)
        session = SimpleNamespace(logger=SimpleNamespace(write=lambda *_: None))
        calls: list[list[str]] = []

        def run_fastboot(args, **_kwargs):
            calls.append(list(args))
            return SimpleNamespace(code=0, text="OKAY")

        session.run_fastboot = run_fastboot
        flash_mi_ext_image(session, raw, MagicMock(), work=self.work)

        padded = self.work / "mi_ext-padded.img"
        self.assertTrue(padded.is_file())
        self.assertEqual(padded.stat().st_size, MIN_LOGICAL_FLASH_BYTES)
        self.assertEqual(padded.read_bytes()[:6], b"IDDATA")

        partitions = ["mi_ext_a", "mi_ext_b"]
        for part in partitions:
            self.assertIn(["delete-logical-partition", part], calls)
            self.assertIn(
                ["create-logical-partition", part, str(MIN_LOGICAL_FLASH_BYTES)],
                calls,
            )
            self.assertIn(["flash", part, str(padded)], calls)
        self.assertIn(["set_active", "a"], calls)


class SkipPlaceholderMiExtTest(unittest.TestCase):
    def test_post_flash_id_skips_rom_mi_ext(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        work = root / "work"
        work.mkdir()
        images = root / "images"
        images.mkdir()
        (images / "super_empty.img").write_bytes(b"empty")
        profile = SimpleNamespace(
            post_flash_id=True,
            slot_suffix="_a",
            work_dir=work,
            images_dir=images,
            wipe_super_image="super_empty.img",
            wipe_super_dat=None,
            super_dir=root / "super",
            dynamic_partitions=[
                SimpleNamespace(name="system", pad_to_bytes=8192, dat_bytes=4),
                SimpleNamespace(name="mi_ext", pad_to_bytes=139264, dat_bytes=100),
            ],
        )
        session = MagicMock()
        session.dry_run = True
        logger = MagicMock()
        with (
            patch("harusora_flash_tool.flash._wipe_or_init_super"),
            patch(
                "harusora_flash_tool.flash.convert_dat_to_sparse",
                return_value=8192,
            ) as convert,
            patch("harusora_flash_tool.flash.flash_dynamic_partition_image") as flash_img,
        ):
            flash_dynamic_partitions(profile, session, logger)
        self.assertEqual([call.args[2] for call in convert.call_args_list], ["system"])
        self.assertEqual([call.args[1] for call in flash_img.call_args_list], ["system_a"])


class SparseTransportPaddingTest(unittest.TestCase):
    def test_regenerates_sparse_when_encoded_payload_exceeds_logical_size(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        super_dir = root / "super"
        work = root / "work"
        super_dir.mkdir()
        work.mkdir()
        transfer = super_dir / "product.transfer.list"
        dat = super_dir / "product.new.dat"
        sparse = work / "product.img"
        _write_transfer(transfer, 512)
        dat.write_bytes(b"data")
        raw_bytes = 2 * 1024 * 1024
        calls = 0

        def fake_sdat2img(_transfer, _dat, raw, *, progress):
            raw.write_bytes(b"X" * raw_bytes)
            progress(raw_bytes, raw_bytes)
            return raw_bytes

        def fake_img2simg(_executable, args, **_kwargs):
            nonlocal calls
            calls += 1
            output = Path(args[1])
            if calls == 1:
                output.write_bytes(b"S" * (raw_bytes + 40))
            else:
                output.write_bytes(b"S" * (raw_bytes + 64))
            return SimpleNamespace(code=0, text="OKAY")

        profile = SimpleNamespace(work_dir=work, super_dir=super_dir, img2simg=Path("img2simg"))
        session = SimpleNamespace(dry_run=False)
        logger = MagicMock()
        with (
            patch("harusora_flash_tool.flash.sdat2img", side_effect=fake_sdat2img),
            patch("harusora_flash_tool.flash.run_process", side_effect=fake_img2simg),
        ):
            flashed_size = convert_dat_to_sparse(
                profile,
                session,
                "product",
                dat,
                sparse,
                None,
                logger,
            )

        self.assertEqual(calls, 2)
        self.assertGreater(flashed_size, raw_bytes + 40)
        self.assertLessEqual(sparse.stat().st_size, flashed_size)
        self.assertFalse((work / "product.raw.img").exists())


if __name__ == "__main__":
    unittest.main()
