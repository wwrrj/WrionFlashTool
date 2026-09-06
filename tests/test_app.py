from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock, patch

from harusora_flash_tool import app
from harusora_flash_tool.device import DeviceError


class MainMenuTest(unittest.TestCase):
    def test_optional_entries_are_hidden(self) -> None:
        profile = SimpleNamespace(enable_root_menu=False, expansion=None, post_flash_id=False)
        items, actions = app.main_menu_entries(profile)
        self.assertEqual(len(items), len(actions))
        self.assertNotIn("root", actions)
        self.assertNotIn("expansion", actions)
        self.assertNotIn("id", actions)

    def test_optional_entries_are_shown_when_enabled(self) -> None:
        profile = SimpleNamespace(
            enable_root_menu=True,
            expansion=SimpleNamespace(enabled=True),
            post_flash_id=True,
        )
        items, actions = app.main_menu_entries(profile)
        self.assertEqual(len(items), len(actions))
        self.assertIn("root", actions)
        self.assertIn("expansion", actions)
        self.assertIn("id", actions)
        self.assertEqual(items[actions.index("id")], "刷入 / 更新 ID 补丁（mi_ext zip）")


class FlashUiTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.profile = SimpleNamespace(
            display_name="Test ROM",
            logs_dir=Path(self.temporary.name),
            post_flash_id=False,
        )

    def _ui_patches(self):
        return patch.multiple(
            app,
            show_banner=MagicMock(),
            cprint=MagicMock(),
            wait_key=MagicMock(),
        )

    def test_full_flash_uses_configured_reboot_behavior(self) -> None:
        with (
            self._ui_patches(),
            patch.object(app, "Logger"),
            patch.object(app, "run_full_flash", return_value=None) as run,
        ):
            app.full_flash_ui(self.profile)
        self.assertIsNone(run.call_args.kwargs["reboot"])

    def test_post_flash_id_holds_reboot_until_post_processing(self) -> None:
        self.profile.post_flash_id = True
        session = object()
        with (
            self._ui_patches(),
            patch.object(app, "Logger"),
            patch.object(app, "run_full_flash", return_value=session) as run,
            patch.object(app, "post_flash_id_and_format") as post,
        ):
            app.full_flash_ui(self.profile)
        self.assertFalse(run.call_args.kwargs["reboot"])
        post.assert_called_once()

    def test_full_flash_handles_device_errors(self) -> None:
        with (
            self._ui_patches(),
            patch.object(app, "Logger"),
            patch.object(app, "run_full_flash", side_effect=DeviceError("offline")),
            patch.object(app, "_show_flash_failure") as failure,
        ):
            app.full_flash_ui(self.profile)
        failure.assert_called_once()

    def test_update_uses_configured_reboot_behavior(self) -> None:
        with (
            self._ui_patches(),
            patch.object(app, "Logger"),
            patch.object(app, "run_full_flash", return_value=None) as run,
        ):
            app.update_flash_ui(self.profile)
        self.assertTrue(run.call_args.kwargs["preserve_data"])
        self.assertIsNone(run.call_args.kwargs["reboot"])


if __name__ == "__main__":
    unittest.main()
