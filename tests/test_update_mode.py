from __future__ import annotations

import unittest

from harusora_flash_tool.package import is_preserved_data_partition


class UpdateModeTest(unittest.TestCase):
    def test_preserves_userdata_and_metadata_slots(self) -> None:
        for name in (
            "userdata",
            "userdata_a",
            "userdata_b",
            "userdata_ab",
            "metadata",
            "metadata_a",
            "metadata_b",
            "metadata_ab",
        ):
            with self.subTest(name=name):
                self.assertTrue(is_preserved_data_partition(name))

    def test_does_not_skip_required_system_partitions(self) -> None:
        for name in ("misc", "system_a", "vendor_a", "mi_ext_a", "boot_ab"):
            with self.subTest(name=name):
                self.assertFalse(is_preserved_data_partition(name))


if __name__ == "__main__":
    unittest.main()
