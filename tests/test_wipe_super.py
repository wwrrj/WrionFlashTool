from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock

from harusora_flash_tool.flash import prepare_wipe_super_source
from harusora_flash_tool.sparse import build_wipe_super_image


class DatOnlyWipeSuperTest(unittest.TestCase):
    def test_restores_dat_and_builds_small_wipe_metadata_image(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        super_dir = root / "super"
        work_dir = root / "work"
        super_dir.mkdir()
        payload = b"gDla" + bytes(4096 - 4)
        (super_dir / "super_empty.new.dat").write_bytes(payload)
        (super_dir / "super_empty.transfer.list").write_text(
            "4\n2\n0\n0\nnew 2,0,1\n", encoding="utf-8"
        )
        profile = SimpleNamespace(
            wipe_super_dat="super_empty",
            wipe_super_image=None,
            super_dir=super_dir,
            work_dir=work_dir,
        )
        session = SimpleNamespace(dry_run=False)
        source = prepare_wipe_super_source(profile, session, MagicMock())

        self.assertEqual(source.stat().st_size, 8192)
        self.assertEqual(source.read_bytes()[:4], b"gDla")
        wipe = build_wipe_super_image(source, work_dir / "wipe.img")
        wipe_bytes = wipe.read_bytes()
        self.assertEqual(wipe_bytes[:4], b"gDla")
        self.assertEqual(wipe_bytes[4096:4100], b"gDla")


if __name__ == "__main__":
    unittest.main()
