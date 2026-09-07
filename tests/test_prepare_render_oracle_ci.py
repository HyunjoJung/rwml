import hashlib
import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import prepare_render_oracle_ci as prepare  # noqa: E402


def identity(payload):
    return {"bytes": len(payload), "sha256": hashlib.sha256(payload).hexdigest()}


class DownloadTests(unittest.TestCase):
    def test_verified_download_reuses_only_matching_cached_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary)
            payload = b"locked source"
            expected = identity(payload)
            response = io.BytesIO(payload)
            response.geturl = lambda: "https://example.org/locked"
            with mock.patch.object(
                prepare.urllib.request, "urlopen", return_value=response
            ) as network:
                path = prepare.download(cache, "https://example.org/locked", expected)
                self.assertEqual(path.read_bytes(), payload)
                self.assertEqual(
                    prepare.download(cache, "https://example.org/locked", expected),
                    path,
                )
                self.assertEqual(network.call_count, 1)
                path.write_bytes(b"changed")
                with self.assertRaises(ValueError):
                    prepare.download(cache, "https://example.org/locked", expected)
                self.assertEqual(network.call_count, 1)

    def test_download_rejects_truncated_oversized_and_wrong_digest_responses(self):
        expected = identity(b"expected")
        for payload in (b"short", b"too long payload", b"modified"):
            with (
                self.subTest(payload=payload),
                tempfile.TemporaryDirectory() as temporary,
            ):
                cache = Path(temporary)
                response = io.BytesIO(payload)
                response.geturl = lambda: "https://example.org/locked"
                with mock.patch.object(
                    prepare.urllib.request, "urlopen", return_value=response
                ):
                    with self.assertRaises(ValueError):
                        prepare.download(cache, "https://example.org/locked", expected)
                self.assertEqual(list(cache.iterdir()), [])

    def test_download_rejects_http_and_redirect_downgrade(self):
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary)
            with mock.patch.object(prepare.urllib.request, "urlopen") as network:
                with self.assertRaises(ValueError):
                    prepare.download(cache, "http://example.org/locked", identity(b"x"))
                network.assert_not_called()
            response = io.BytesIO(b"x")
            response.geturl = lambda: "http://example.org/redirect"
            with mock.patch.object(
                prepare.urllib.request, "urlopen", return_value=response
            ):
                with self.assertRaises(ValueError):
                    prepare.download(
                        cache, "https://example.org/locked", identity(b"x")
                    )
            self.assertEqual(list(cache.iterdir()), [])

    def test_archive_member_is_bounded_and_never_extracts_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / "fonts.zip"
            with zipfile.ZipFile(archive, "w") as writer:
                writer.writestr("../OFL.txt", b"license")
                writer.writestr("fonts/Regular.ttf", b"font")
            self.assertEqual(
                prepare.archive_member(archive, "../OFL.txt", 7), b"license"
            )
            self.assertFalse((archive.parent.parent / "OFL.txt").exists())
            with self.assertRaises(ValueError):
                prepare.archive_member(archive, "fonts/Regular.ttf", 3)
            with self.assertRaises(ValueError):
                prepare.archive_member(archive, "missing", 100)

    def test_prepare_rejects_existing_output_before_any_network(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "prepared"
            output.mkdir()
            with mock.patch.object(prepare.urllib.request, "urlopen") as network:
                with self.assertRaisesRegex(ValueError, "fresh"):
                    prepare.prepare(output, root / "cache")
                network.assert_not_called()

    def test_failed_source_never_publishes_a_partial_prepared_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "prepared"
            with mock.patch.object(
                prepare, "download", side_effect=ValueError("source changed")
            ):
                with self.assertRaisesRegex(ValueError, "source changed"):
                    prepare.prepare(output, root / "cache")
            self.assertFalse(output.exists())
            self.assertEqual(list(root.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
