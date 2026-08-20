from __future__ import annotations

import ast
import unittest
from pathlib import Path


class ExpansionOrderTest(unittest.TestCase):
    def test_wipe_happens_before_gpt_rewrite(self) -> None:
        source = (Path(__file__).parents[1] / "harusora_flash_tool" / "expand.py").read_text(
            encoding="utf-8"
        )
        tree = ast.parse(source)
        method = next(
            node
            for node in ast.walk(tree)
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
            and node.name == "apply_expand"
        )
        wipe_line = next(
            node.lineno
            for node in ast.walk(method)
            if isinstance(node, ast.Call)
            and isinstance(node.func, ast.Attribute)
            and node.func.attr == "wipe_data_metadata_before_gpt"
        )
        rewrite_line = next(
            node.lineno
            for node in ast.walk(method)
            if isinstance(node, ast.Call)
            and isinstance(node.func, ast.Attribute)
            and node.func.attr == "_shell_step"
            and node.args
            and isinstance(node.args[0], ast.Constant)
            and node.args[0].value == "sgdisk 重写 super/userdata"
        )
        self.assertLess(wipe_line, rewrite_line)


if __name__ == "__main__":
    unittest.main()
