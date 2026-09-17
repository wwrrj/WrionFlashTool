from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock

from harusora_flash_tool.config import DynamicPartition
from harusora_flash_tool.flash import (
    MIN_DYNAMIC_IMAGE_BYTES,
    _create_logical_partitions_from_profile,
    _flash_empty_super_bootloader,
    _flash_logical_image,
    _logical_create_size,
    _logical_name_variants,
    _prepare_empty_super_flash_image,
    _prepare_wipe_super_source,
)
from harusora_flash_tool.process import ProcessResult


class FakeSession:
    def __init__(
        self,
        *,
        create_fail: set[str] | None = None,
        getvar_all: str = "",
        already_exists: set[str] | None = None,
    ) -> None:
        self.calls: list[list[str]] = []
        self.create_fail = create_fail or set()
        self.already_exists = already_exists or set()
        self.getvar_all = getvar_all
        self.getvars: dict[str, str | None] = {"is-userspace": "no"}
        self.dry_run = False
        self.logger = MagicMock()

    def run_fastboot(self, args: list[str], **_kwargs) -> ProcessResult:
        self.calls.append(list(args))
        cmd = args[0] if args else ""
        if cmd == "getvar" and args[1:] == ["all"]:
            return ProcessResult(0, self.getvar_all, self.getvar_all, "")
        if cmd == "create-logical-partition":
            name = args[1]
            if name in self.create_fail:
                text = "Creating 'system_a' FAILED (remote: 'Not enough space for partition')"
                return ProcessResult(1, text, "", text)
            if name in self.already_exists:
                text = "FAILED (remote: 'Partition already exists')"
                return ProcessResult(1, text, "", text)
        if cmd == "resize-logical-partition" and args[1] in self.already_exists:
            return ProcessResult(0, "OKAY", "OKAY", "")
        return ProcessResult(0, "OKAY", "OKAY", "")

    def getvar(self, name: str, *, allow_missing: bool = False) -> str | None:
        if name in self.getvars:
            return self.getvars[name]
        return None


def _profile(*names: str, slot: str = "_a") -> SimpleNamespace:
    return SimpleNamespace(
        slot_suffix=slot,
        dynamic_partitions=[
            DynamicPartition(name=name, dat_bytes=4096 * (index + 1))
            for index, name in enumerate(names)
        ],
    )


class LogicalPartitionFallbackTest(unittest.TestCase):
    def test_name_variants_include_other_slot_and_cow(self) -> None:
        names = _logical_name_variants(_profile("system", "vendor"))
        for expected in (
            "system_a",
            "system_b",
            "system_a-cow",
            "system_b-cow",
            "vendor_a",
            "vendor_b",
        ):
            self.assertIn(expected, names)

    def test_tiny_partition_pads_to_1mib(self) -> None:
        part = DynamicPartition(name="mi_ext", dat_bytes=163840)
        self.assertEqual(_logical_create_size(part), MIN_DYNAMIC_IMAGE_BYTES)

    def test_create_size_rounds_up_to_4k(self) -> None:
        part = DynamicPartition(name="vendor", dat_bytes=638136321)
        self.assertEqual(_logical_create_size(part), 638140416)

    def test_eoverflow_retries_padded_raw(self) -> None:
        session = FakeSession()

        def run_fastboot(args: list[str], **_kwargs) -> ProcessResult:
            session.calls.append(list(args))
            if args[:2] == ["flash", "mi_ext_a"] and not str(args[2]).endswith(".padded.img"):
                text = "Writing 'mi_ext_a' FAILED (remote: 'Value too large for defined data type')"
                return ProcessResult(1, text, "", text)
            return ProcessResult(0, "OKAY", "OKAY", "")

        session.run_fastboot = run_fastboot  # type: ignore[method-assign]
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / "mi_ext.img"
            image.write_bytes(b"\x00" * 163840)
            _flash_logical_image(
                session,  # type: ignore[arg-type]
                "mi_ext_a",
                image,
                DynamicPartition(name="mi_ext", dat_bytes=163840),
                MagicMock(),
            )
        flashes = [c for c in session.calls if c and c[0] == "flash"]
        self.assertGreaterEqual(len(flashes), 2)
        self.assertTrue(any(str(c[2]).endswith(".padded.img") for c in flashes))

    def test_returns_false_if_any_required_partition_fails(self) -> None:
        session = FakeSession(create_fail={"system_a"})
        ok = _create_logical_partitions_from_profile(
            _profile("system", "vendor"),
            session,  # type: ignore[arg-type]
            MagicMock(),
        )
        self.assertFalse(ok)
        creates = [c for c in session.calls if c and c[0] == "create-logical-partition"]
        self.assertTrue(creates)

    def test_deletes_other_slot_and_discovered_partitions_before_create(self) -> None:
        session = FakeSession(
            getvar_all="is-logical:system_b: yes\nis-logical:product_a-cow: yes\n"
        )
        ok = _create_logical_partitions_from_profile(
            _profile("system"),
            session,  # type: ignore[arg-type]
            MagicMock(),
        )
        self.assertTrue(ok)
        cmds = session.calls
        first_create = next(i for i, c in enumerate(cmds) if c[0] == "create-logical-partition")
        deleted = [c[1] for c in cmds[:first_create] if c[0] == "delete-logical-partition"]
        self.assertIn("system_b", deleted)
        self.assertIn("product_a-cow", deleted)
        self.assertIn("system_a", deleted)
        self.assertEqual(
            cmds[first_create],
            ["create-logical-partition", "system_a", str(MIN_DYNAMIC_IMAGE_BYTES)],
        )

    def test_resizes_when_partition_already_exists(self) -> None:
        session = FakeSession(already_exists={"system_a"})
        ok = _create_logical_partitions_from_profile(
            _profile("system"),
            session,  # type: ignore[arg-type]
            MagicMock(),
        )
        self.assertTrue(ok)
        self.assertIn(
            ["resize-logical-partition", "system_a", str(MIN_DYNAMIC_IMAGE_BYTES)],
            session.calls,
        )


class EmptySuperBootloaderTest(unittest.TestCase):
    def test_erases_then_flashes_physical_super(self) -> None:
        session = FakeSession()
        image = Path("super_empty_16g.img")
        ok = _flash_empty_super_bootloader(session, image, MagicMock())  # type: ignore[arg-type]
        self.assertTrue(ok)
        self.assertIn(["erase", "super"], session.calls)
        self.assertIn(["flash", "super", str(image)], session.calls)
        erase_at = session.calls.index(["erase", "super"])
        flash_at = session.calls.index(["flash", "super", str(image)])
        self.assertLess(erase_at, flash_at)
        self.assertNotIn(["reboot", "fastboot"], session.calls)

    def test_returns_to_bootloader_from_fastbootd(self) -> None:
        session = FakeSession()
        session.getvars["is-userspace"] = "yes"
        session.wait_for_mode = MagicMock()  # type: ignore[method-assign]
        ok = _flash_empty_super_bootloader(  # type: ignore[arg-type]
            session, Path("super_empty_16g.img"), MagicMock()
        )
        self.assertTrue(ok)
        self.assertEqual(session.calls[0], ["reboot", "bootloader"])
        session.wait_for_mode.assert_called_once_with(False)


class WipeSuperSourceTest(unittest.TestCase):
    def test_uses_ready_img_without_converting_to_dat(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            images = root / "images"
            images.mkdir()
            img = images / "super_empty_16g.img"
            img.write_bytes(b"not-a-dat")
            profile = SimpleNamespace(
                wipe_super_image="super_empty_16g.img",
                wipe_super_dat="super_empty",
                images_dir=images,
                super_dir=root / "super",
                work_dir=root / "work",
                minimum_super_bytes=4096,
            )
            (root / "super").mkdir()
            path = _prepare_empty_super_flash_image(profile, MagicMock())  # type: ignore[arg-type]
            self.assertEqual(path, img)

    def test_uses_super_empty_dat_when_wipe_image_missing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            super_dir = root / "super"
            work_dir = root / "work"
            super_dir.mkdir()
            (super_dir / "super_empty.transfer.list").write_text(
                "4\n2\n0\n0\nnew 2,0,2\n", encoding="utf-8"
            )
            (super_dir / "super_empty.new.dat").write_bytes(b"C" * (2 * 4096))
            profile = SimpleNamespace(
                wipe_super_image="super_empty_16g.img",
                wipe_super_dat="super_empty",
                images_dir=root / "images",
                super_dir=super_dir,
                work_dir=work_dir,
            )
            (root / "images").mkdir()
            path = _prepare_wipe_super_source(profile, MagicMock())  # type: ignore[arg-type]
            self.assertTrue(path.is_file())
            self.assertLess(path.stat().st_size, 16 * 1024 * 1024)
            self.assertFalse((work_dir / "super_empty.raw.img").exists())


if __name__ == "__main__":
    unittest.main()
