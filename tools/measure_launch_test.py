import importlib.util
import json
import os
from pathlib import Path
import unittest
import uuid

spec = importlib.util.spec_from_file_location("measure_launch", Path(__file__).with_name("measure_launch.py"))
launcher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(launcher)


class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(os.environ["PINFEATHER_TEST_DATA_ROOT"]) / ("launcher-" + uuid.uuid4().hex)
        self.root.mkdir()
        self.data = self.root / "data"
        self.data.mkdir()
        self.exe = self.root / "synthetic.exe"
        self.exe.write_bytes(b"synthetic candidate; never executed")
        self.output = self.root / "run.ndjson"
        self.receipt = self.root / "launch.json"

    def test_native_tick_immediately_precedes_mock_spawn_and_never_creates_host_output(self):
        events = []
        def tick():
            events.append("qpc")
            return 123
        def spawn(args, **kwargs):
            events.append("spawn")
            self.assertEqual(kwargs, {"shell": False})
            self.assertEqual(args[-4:], ["--measure-cold-start-qpc", "123", "--measure-qpc-frequency", "10"])
            return type("Process", (), {"pid": 456})()
        launcher.launch(self.exe, self.data, self.output, self.receipt, "test", tick=tick, hz=lambda: 10, spawn=spawn)
        self.assertEqual(events, ["qpc", "spawn"])
        self.assertFalse(self.output.exists())
        self.assertEqual(json.loads(self.receipt.read_text())["candidate_sha256"], launcher.hashlib.sha256(self.exe.read_bytes()).hexdigest())
        with self.assertRaises(ValueError):
            launcher.launch(self.exe, self.data, self.output, self.receipt, "test", tick=tick, hz=lambda: 10, spawn=spawn)
        self.assertEqual(events, ["qpc", "spawn"])

    def test_existing_output_inside_data_and_spawn_failure_do_not_retry(self):
        calls = []
        def spawn(*args, **kwargs):
            calls.append(1)
            raise OSError("synthetic spawn failure")
        self.output.write_text("preserve")
        with self.assertRaises(ValueError):
            launcher.launch(self.exe, self.data, self.output, self.receipt, "test", spawn=spawn)
        self.assertEqual(self.output.read_text(), "preserve")
        with self.assertRaises(ValueError):
            launcher.launch(self.exe, self.data, self.data / "forbidden.ndjson", self.receipt, "test", spawn=spawn)
        with self.assertRaises(OSError):
            launcher.launch(self.exe, self.data, self.root / "unused.ndjson", self.receipt, "test", tick=lambda: 1, hz=lambda: 10, spawn=spawn)
        self.assertEqual(calls, [1])
        self.assertTrue(self.receipt.exists())
        self.assertEqual(self.receipt.read_bytes(), b"")


if __name__ == "__main__":
    unittest.main()
