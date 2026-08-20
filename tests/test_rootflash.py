from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from harusora_flash_tool.rootflash import RootFlashError, _clean_image_path, _partition_size


class FakeSession:
    def __init__(self, values: dict[str, str | None]) -> None:
        self.values = values

    def getvar(self, name: str, *, allow_missing: bool = False) -> str | None:
        return self.values.get(name)


class RootImageValidationTest(unittest.TestCase):
    def test_accepts_android_boot_image(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / "magisk_patched.img"
            image.write_bytes(b"ANDROID!" + b"\0" * 8192)
            self.assertEqual(_clean_image_path(str(image)), image.resolve())

    def test_rejects_non_boot_image(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / "invalid.img"
            image.write_bytes(b"NOTBOOT!" + b"\0" * 8192)
            with self.assertRaises(RootFlashError):
                _clean_image_path(str(image))

    def test_uses_smallest_slot_size(self) -> None:
        session = FakeSession(
            {
                "partition-size:init_boot_a": "0x800000",
                "partition-size:init_boot_b": "0x700000",
            }
        )
        self.assertEqual(_partition_size(session, "init_boot"), 0x700000)

    def test_rejects_unknown_partition_size(self) -> None:
        with self.assertRaises(RootFlashError):
            _partition_size(FakeSession({}), "boot")


if __name__ == "__main__":
    unittest.main()
