import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import inspect_oracle_image as inspect  # noqa: E402


def archive_bytes(entries):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w") as archive:
        for name, payload, mode in entries:
            entry = tarfile.TarInfo(name)
            entry.size, entry.mode, entry.mtime = len(payload), mode, 123
            archive.addfile(entry, io.BytesIO(payload))
    return stream.getvalue()


class OracleImageInspectionTests(unittest.TestCase):
    def image(self, *, payload=b"public bytes", mode=0o644):
        layer = archive_bytes([("etc/example", payload, mode)])
        config = b'{"architecture":"amd64","rootfs":{"diff_ids":[]}}'
        manifest = json.dumps(
            [{"Config": "config.json", "Layers": ["layer.tar"]}]
        ).encode()
        return archive_bytes(
            [
                ("manifest.json", manifest, 0o644),
                ("config.json", config, 0o644),
                ("layer.tar", layer, 0o644),
            ]
        )

    def inspect(self, payload):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "image.tar"
            path.write_bytes(payload)
            return inspect.inspect_archive(path)

    def test_records_bytes_and_metadata_without_extracting_paths(self):
        result = self.inspect(self.image())
        row = result["layers"][0]["entries"][0]
        self.assertEqual(row["name"], "etc/example")
        self.assertEqual(row["sha256"], hashlib.sha256(b"public bytes").hexdigest())
        self.assertEqual(row["mode"], 0o644)
        self.assertEqual(row["mtime"], 123)
        self.assertEqual(
            result["scope"], "image-build-diagnostic-not-runtime-acceptance"
        )

    def test_file_bytes_and_permissions_produce_distinct_diagnostics(self):
        normal = self.inspect(self.image())
        self.assertNotEqual(normal, self.inspect(self.image(payload=b"changed")))
        self.assertNotEqual(normal, self.inspect(self.image(mode=0o600)))

    def test_archive_member_and_layer_limits_are_enforced(self):
        from unittest import mock

        with mock.patch.object(inspect, "MAX_LAYERS", 0):
            with self.assertRaisesRegex(ValueError, "layer count"):
                self.inspect(self.image())
        with mock.patch.object(inspect, "MAX_ENTRIES", 0):
            with self.assertRaisesRegex(ValueError, "entry count"):
                self.inspect(self.image())


if __name__ == "__main__":
    unittest.main()
