"""Offline assembler tests; no SDK/device/network required."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("reference_assemble", HERE / "assemble.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class AssembleTests(unittest.TestCase):
    def setUp(self):
        build = HERE / "build"
        build.mkdir(exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(prefix="assembler-test-", dir=build)
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.bridge = self.root / "bridge.js"
        self.bridge.write_bytes(b"const bridge = {};\n")
        self.output = self.root / "out/reference.js"

    def valid_pin(self):
        return patch.object(module, "BRIDGE_SHA256", hashlib.sha256(self.bridge.read_bytes()).hexdigest())

    def test_pin_failure_creates_nothing(self):
        with self.assertRaises(ValueError):
            module.assemble(self.bridge, self.output)
        self.assertFalse(self.output.parent.exists())

    def test_assembly_hashes_owned_sources(self):
        with self.valid_pin():
            metadata = module.assemble(self.bridge, self.output)
        self.assertEqual(metadata["assembled_sha256"], hashlib.sha256(self.output.read_bytes()).hexdigest())
        self.assertEqual(metadata["assembled_bytes"], self.output.stat().st_size)
        self.assertEqual(metadata["device_actions"], 0)
        self.assertEqual(metadata["audio_arms"], 0)
        self.assertEqual(set(metadata["source_sha256"]), set(module.OWN_SOURCES))
        self.assertIn(b"Script.evaluate", self.output.read_bytes())
        stored = json.loads(self.output.with_suffix(".js.metadata.json").read_text(encoding="utf-8"))
        self.assertEqual(stored, metadata)

    def test_existing_output_is_not_overwritten(self):
        self.output.parent.mkdir()
        self.output.write_bytes(b"preserve")
        with self.valid_pin(), self.assertRaises(ValueError):
            module.assemble(self.bridge, self.output)
        self.assertEqual(self.output.read_bytes(), b"preserve")
        self.assertFalse(self.output.with_suffix(".js.metadata.json").exists())

    def test_existing_metadata_is_not_overwritten(self):
        self.output.parent.mkdir()
        metadata = self.output.with_suffix(".js.metadata.json")
        metadata.write_bytes(b"preserve")
        with self.valid_pin(), self.assertRaises(ValueError):
            module.assemble(self.bridge, self.output)
        self.assertEqual(metadata.read_bytes(), b"preserve")
        self.assertFalse(self.output.exists())

    def test_invalid_utf8_creates_nothing(self):
        self.bridge.write_bytes(b"\xff")
        with self.valid_pin(), self.assertRaises(UnicodeDecodeError):
            module.assemble(self.bridge, self.output)
        self.assertFalse(self.output.parent.exists())


if __name__ == "__main__":
    unittest.main()
