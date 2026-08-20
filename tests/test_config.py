from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from harusora_flash_tool.config import ProfileError, load_profile

BASE_PROFILE = """
[package]
author = "Harusora Team"
qq_group = "123456"
build_date = "2026-08-20"

[device]
codenames = ["shennong"]
display_name = "Test ROM"

[requirements]
minimum_super_bytes = 1

[paths]
images = "images"
super = "super"
work = "work"
logs = "logs"
tools = "tools"

[stages]
firmware = [{ action = "flash", partition = "boot_ab", file = "boot.img" }]
boot_chain = []
final_data = []

[dynamic]
partitions = []

[options]
enable_root_menu = false
post_flash_id = false
"""


class ProfileMetadataTest(unittest.TestCase):
    def _load(self, text: str):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        profile = root / "flash_profile.toml"
        profile.write_text(text, encoding="utf-8")
        return load_profile(root, profile)

    def test_loads_package_metadata_and_disables_optional_features(self) -> None:
        profile = self._load(BASE_PROFILE)
        self.assertEqual(profile.author, "Harusora Team")
        self.assertEqual(profile.qq_group, "123456")
        self.assertEqual(profile.build_date, "2026-08-20")
        self.assertIsNone(profile.expansion)
        self.assertFalse(profile.enable_root_menu)
        self.assertFalse(profile.post_flash_id)

    def test_requires_package_metadata(self) -> None:
        with self.assertRaises(ProfileError):
            self._load(BASE_PROFILE.replace('author = "Harusora Team"\n', ""))

    def test_expansion_must_be_explicitly_enabled(self) -> None:
        profile = self._load(BASE_PROFILE + "\n[expansion]\nenabled = false\n")
        self.assertIsNone(profile.expansion)

    def test_requires_iso_build_date(self) -> None:
        with self.assertRaisesRegex(ProfileError, "YYYY-MM-DD"):
            self._load(BASE_PROFILE.replace("2026-08-20", "2026/08/20"))

    def test_rejects_image_path_outside_rom_package(self) -> None:
        unsafe = BASE_PROFILE.replace('file = "boot.img"', 'file = "../boot.img"')
        with self.assertRaisesRegex(ProfileError, "相对路径"):
            self._load(unsafe)

    def test_rejects_non_boolean_option(self) -> None:
        unsafe = BASE_PROFILE.replace(
            "enable_root_menu = false", 'enable_root_menu = "false"'
        )
        with self.assertRaisesRegex(ProfileError, "true 或 false"):
            self._load(unsafe)

    def test_rejects_unsafe_expansion_device_path(self) -> None:
        unsafe = (
            BASE_PROFILE
            + '\n[expansion]\nenabled = true\ndisk = "/dev/block/sda; reboot"\n'
        )
        with self.assertRaisesRegex(ProfileError, "安全的设备绝对路径"):
            self._load(unsafe)


if __name__ == "__main__":
    unittest.main()
