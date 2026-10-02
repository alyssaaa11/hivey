"""Tests for pet.py (no building, launching or quitting: those calls are stubbed)."""
import io
import os
import plistlib
import tempfile
import time
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

import pet


class PetTest(unittest.TestCase):
    def setUp(self):
        self.home = tempfile.TemporaryDirectory()
        self.env = mock.patch.dict(os.environ, {"HOME": self.home.name})
        self.env.start()

    def tearDown(self):
        self.env.stop()
        self.home.cleanup()

    def test_choice_is_remembered(self):
        self.assertIsNone(pet.chosen())
        pet.save_choice("hiver-dot")
        self.assertEqual(pet.chosen(), "hiver-dot")
        pet.save_choice(None)
        self.assertIsNone(pet.chosen())

    def test_login_item_opens_the_installed_app(self):
        pet.set_login("hiver-h", True)
        plist = plistlib.loads(pet.launch_agent("hiver-h").read_bytes())
        self.assertEqual(plist["Label"], "com.hiver.h")
        self.assertEqual(plist["ProgramArguments"][-1], str(pet.installed_app("hiver-h")))
        self.assertTrue(plist["RunAtLoad"])
        pet.set_login("hiver-h", False)
        self.assertFalse(pet.launch_agent("hiver-h").exists())

    def test_outdated_until_built_after_the_source(self):
        self.assertTrue(pet.outdated("hiver-h"))   # not installed
        exe = pet.installed_app("hiver-h") / "Contents" / "MacOS" / "hiver-h"
        exe.parent.mkdir(parents=True)
        exe.write_text("")
        newest = max(src.stat().st_mtime for src in pet.sources("hiver-h"))
        os.utime(exe, (newest + 10, newest + 10))
        self.assertFalse(pet.outdated("hiver-h"))
        os.utime(exe, (newest - 10, newest - 10))
        self.assertTrue(pet.outdated("hiver-h"))

    def test_use_switches_and_retires_the_others(self):
        calls = []
        with mock.patch.object(pet, "check_mac"), \
                mock.patch.object(pet, "outdated", return_value=False), \
                mock.patch.object(pet, "quit_pet", side_effect=lambda p: calls.append(("quit", p))), \
                mock.patch.object(pet.subprocess, "run") as run, \
                redirect_stdout(io.StringIO()):
            pet.set_login("hiver-dot", True)
            pet.use("hiver-prompt")
        self.assertEqual(pet.chosen(), "hiver-prompt")
        self.assertFalse(pet.launch_agent("hiver-dot").exists())
        self.assertTrue(pet.launch_agent("hiver-prompt").exists())
        self.assertEqual({p for _, p in calls}, set(pet.ORDER))
        self.assertEqual(run.call_args.args[0][:2], ["open", "-a"])

    def test_unknown_pet_is_refused(self):
        with self.assertRaises(SystemExit) as stop:
            pet.use("cat")
        self.assertIn("hiver-h", str(stop.exception))

    def test_off_forgets_the_pet(self):
        pet.save_choice("hiver-h")
        pet.set_login("hiver-h", True)
        with mock.patch.object(pet, "quit_pet"), redirect_stdout(io.StringIO()):
            pet.off()
        self.assertIsNone(pet.chosen())
        self.assertFalse(pet.launch_agent("hiver-h").exists())

    def test_choose(self):
        cases = [("2", ("use", "hiver-dot")), ("0", ("off",)), ("", None), ("9", None)]
        for answer, expected in cases:
            with mock.patch("builtins.input", return_value=answer), \
                    mock.patch.object(pet, "use") as use, mock.patch.object(pet, "off") as off, \
                    redirect_stdout(io.StringIO()):
                pet.choose()
            if expected is None:
                use.assert_not_called()
                off.assert_not_called()
            elif expected[0] == "use":
                use.assert_called_once_with(expected[1])
            else:
                off.assert_called_once()

    def test_refresh_only_rebuilds_a_chosen_outdated_pet(self):
        with mock.patch.object(pet, "use") as use, mock.patch.object(pet.platform, "system", return_value="Darwin"), \
                mock.patch.object(pet.shutil, "which", return_value="/usr/bin/swiftc"):
            pet.refresh()                       # no pet chosen
            pet.save_choice("hiver-h")
            with mock.patch.object(pet, "outdated", return_value=False):
                pet.refresh()
            use.assert_not_called()
            with mock.patch.object(pet, "outdated", return_value=True):
                pet.refresh()
            use.assert_called_once_with("hiver-h")


if __name__ == "__main__":
    unittest.main()
