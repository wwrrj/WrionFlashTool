from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from harusora_flash_tool.sdat2img import sdat2img


class Sdat2ImgPrefixTest(unittest.TestCase):
    def test_prefix_extract_does_not_materialize_full_image(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            transfer = root / "super_empty.transfer.list"
            dat = root / "super_empty.new.dat"
            out = root / "prefix.img"
            # 100 blocks declared (409600 bytes) but only write the first 3
            transfer.write_text("4\n100\n0\n0\nnew 2,0,3\n", encoding="utf-8")
            payload = b"A" * (3 * 4096)
            dat.write_bytes(payload)
            written = sdat2img(transfer, dat, out, max_output_bytes=8192)
            self.assertEqual(written, 8192)
            self.assertEqual(out.stat().st_size, 8192)
            self.assertEqual(out.read_bytes(), payload[:8192])

    def test_full_extract_still_returns_declared_size(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            transfer = root / "part.transfer.list"
            dat = root / "part.new.dat"
            out = root / "part.img"
            transfer.write_text("4\n2\n0\n0\nnew 2,0,2\n", encoding="utf-8")
            dat.write_bytes(b"B" * (2 * 4096))
            written = sdat2img(transfer, dat, out)
            self.assertEqual(written, 2 * 4096)
            self.assertEqual(out.stat().st_size, 2 * 4096)


if __name__ == "__main__":
    unittest.main()
