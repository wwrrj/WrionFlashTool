from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from harusora_flash_tool.config import load_profile
from tools.generate_profile import PROFILE_REL, GeneratorError, discover_images, main


class GenerateProfileTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "images").mkdir()
        (self.root / "super").mkdir()

    def test_generates_profile_from_rom_tree(self) -> None:
        (self.root / "images" / "boot.img").write_bytes(b"ANDROID!")
        (self.root / "images" / "super_empty_8g.img").write_bytes(b"empty")
        (self.root / "super" / "system.new.dat").write_bytes(b"data")
        (self.root / "super" / "system.transfer.list").write_text(
            "4\n2\n0\n0\n", encoding="utf-8"
        )

        result = main(
            [
                "--rom-root",
                str(self.root),
                "--codename",
                "shennong",
                "--display-name",
                "Harusora Test ROM",
                "--author",
                "Harusora Team",
                "--qq-group",
                "123456",
                "--build-date",
                "2026-08-20",
                "--no-validate",
            ]
        )

        self.assertEqual(result, 0)
        output = self.root / PROFILE_REL
        profile = load_profile(self.root, output)
        self.assertEqual(profile.codenames, ["shennong"])
        self.assertEqual(profile.boot_chain[0].partition, "boot_ab")
        self.assertEqual(profile.dynamic_partitions[0].name, "system")
        self.assertEqual(profile.dynamic_partitions[0].dat_bytes, 4)
        self.assertEqual(profile.dynamic_partitions[0].pad_to_bytes, 8192)
        self.assertEqual(
            [(operation.action, operation.partition) for operation in profile.final_data],
            [("erase", "metadata"), ("erase", "userdata")],
        )
        self.assertIsNone(profile.expansion)

    def test_mi_ext_is_padded_to_at_least_1mib(self) -> None:
        (self.root / "images" / "boot.img").write_bytes(b"ANDROID!")
        (self.root / "images" / "super_empty_8g.img").write_bytes(b"empty")
        (self.root / "super" / "mi_ext.new.dat").write_bytes(b"x" * 100)
        (self.root / "super" / "mi_ext.transfer.list").write_text(
            "4\n34\n0\n0\nnew 2,0,34\n", encoding="utf-8"
        )
        result = main(
            [
                "--rom-root",
                str(self.root),
                "--codename",
                "houji",
                "--author",
                "Harusora Team",
                "--qq-group",
                "123456",
                "--post-flash-id",
                "--no-validate",
            ]
        )
        self.assertEqual(result, 0)
        profile = load_profile(self.root, self.root / PROFILE_REL)
        self.assertEqual(profile.dynamic_partitions[0].name, "mi_ext")
        self.assertEqual(profile.dynamic_partitions[0].pad_to_bytes, 1048576)
        self.assertTrue(profile.post_flash_id)

    def test_generates_profile_with_dat_only_wipe_super(self) -> None:
        (self.root / "images" / "boot.img").write_bytes(b"ANDROID!")
        platform_tools = self.root / "tools" / "platform-tools"
        platform_tools.mkdir(parents=True)
        (platform_tools / "adb.exe").write_bytes(b"tool")
        (platform_tools / "fastboot.exe").write_bytes(b"tool")
        (self.root / "tools" / "img2simg.exe").write_bytes(b"tool")
        (self.root / "tools" / "cygwin1.dll").write_bytes(b"tool")
        (self.root / "super" / "system.new.dat").write_bytes(b"data")
        (self.root / "super" / "system.transfer.list").write_text(
            "4\n2\n0\n0\n", encoding="utf-8"
        )
        (self.root / "super" / "super_empty.new.dat").write_bytes(b"empty")
        (self.root / "super" / "super_empty.transfer.list").write_text(
            "4\n4194304\n0\n0\nnew 2,0,1\n", encoding="utf-8"
        )

        result = main(
            [
                "--rom-root",
                str(self.root),
                "--codename",
                "houji",
                "--author",
                "Harusora Team",
                "--qq-group",
                "123456",
            ]
        )

        self.assertEqual(result, 0)
        profile = load_profile(self.root, self.root / PROFILE_REL)
        self.assertEqual(profile.wipe_super_dat, "super_empty")
        self.assertIsNone(profile.wipe_super_image)
        self.assertEqual([part.name for part in profile.dynamic_partitions], ["system"])

    def test_full_validation_passes_for_complete_package(self) -> None:
        (self.root / "images" / "boot.img").write_bytes(b"ANDROID!")
        platform_tools = self.root / "tools" / "platform-tools"
        platform_tools.mkdir(parents=True)
        (platform_tools / "adb.exe").write_bytes(b"tool")
        (platform_tools / "fastboot.exe").write_bytes(b"tool")

        result = main(
            [
                "--rom-root",
                str(self.root),
                "--codename",
                "shennong",
                "--author",
                "Harusora Team",
                "--qq-group",
                "123456",
            ]
        )

        self.assertEqual(result, 0)
        self.assertTrue((self.root / PROFILE_REL).is_file())

    def test_can_explicitly_enable_expansion(self) -> None:
        (self.root / "images" / "boot.img").write_bytes(b"ANDROID!")
        result = main(
            [
                "--rom-root",
                str(self.root),
                "--codename",
                "shennong",
                "--author",
                "Harusora Team",
                "--qq-group",
                "123456",
                "--enable-expansion",
                "--expansion-target-bytes",
                "17179869184",
                "--no-validate",
            ]
        )

        self.assertEqual(result, 0)
        profile = load_profile(self.root, self.root / PROFILE_REL)
        self.assertIsNotNone(profile.expansion)
        assert profile.expansion is not None
        self.assertEqual(profile.expansion.target_bytes, 17179869184)

    def test_expansion_requires_explicit_target_size(self) -> None:
        (self.root / "images" / "boot.img").write_bytes(b"ANDROID!")
        result = main(
            [
                "--rom-root",
                str(self.root),
                "--codename",
                "shennong",
                "--author",
                "Harusora Team",
                "--qq-group",
                "123456",
                "--enable-expansion",
                "--no-validate",
            ]
        )
        self.assertEqual(result, 1)
        self.assertFalse((self.root / PROFILE_REL).exists())

    def test_supports_multiple_devices_and_explicit_partition_mapping(self) -> None:
        (self.root / "images" / "custom.img").write_bytes(b"data")
        result = main(
            [
                "--rom-root",
                str(self.root),
                "--codename",
                "shennong",
                "--codename",
                "houji",
                "--author",
                "Harusora Team",
                "--qq-group",
                "123456",
                "--partition-map",
                "custom.img=custom_ab",
                "--disable-root-menu",
                "--no-reboot-after-success",
                "--no-validate",
            ]
        )
        self.assertEqual(result, 0)
        profile = load_profile(self.root, self.root / PROFILE_REL)
        self.assertEqual(profile.codenames, ["shennong", "houji"])
        self.assertEqual(profile.firmware[0].partition, "custom_ab")
        self.assertFalse(profile.enable_root_menu)
        self.assertFalse(profile.reboot_after_success)

    def test_rejects_unknown_image_without_mapping(self) -> None:
        image = self.root / "images" / "device_specific.img"
        image.write_bytes(b"data")
        with self.assertRaises(GeneratorError):
            discover_images(self.root / "images", {})

    def test_does_not_overwrite_existing_profile_without_force(self) -> None:
        output = self.root / PROFILE_REL
        output.parent.mkdir(parents=True)
        output.write_text("original", encoding="utf-8")
        result = main(
            [
                "--rom-root",
                str(self.root),
                "--codename",
                "test",
                "--author",
                "team",
                "--qq-group",
                "123",
                "--no-validate",
            ]
        )
        self.assertEqual(result, 2)
        self.assertEqual(output.read_text(encoding="utf-8"), "original")

    def test_force_preserves_existing_profile_when_validation_fails(self) -> None:
        (self.root / "images" / "boot.img").write_bytes(b"ANDROID!")
        output = self.root / PROFILE_REL
        output.parent.mkdir(parents=True)
        output.write_text("original", encoding="utf-8")

        result = main(
            [
                "--rom-root",
                str(self.root),
                "--codename",
                "test",
                "--author",
                "team",
                "--qq-group",
                "123",
                "--force",
            ]
        )

        self.assertEqual(result, 1)
        self.assertEqual(output.read_text(encoding="utf-8"), "original")
        self.assertFalse(output.with_suffix(".toml.tmp").exists())


if __name__ == "__main__":
    unittest.main()
