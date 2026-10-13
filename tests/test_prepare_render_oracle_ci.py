import hashlib
import io
from pathlib import Path
import sys
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock
import warnings
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
                prepare.HTTPS_OPENER, "open", return_value=response
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
                    prepare.HTTPS_OPENER, "open", return_value=response
                ):
                    with self.assertRaises(ValueError):
                        prepare.download(cache, "https://example.org/locked", expected)
                self.assertEqual(list(cache.iterdir()), [])

    def test_download_rejects_http_and_redirect_downgrade(self):
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary)
            with mock.patch.object(prepare.HTTPS_OPENER, "open") as network:
                with self.assertRaises(ValueError):
                    prepare.download(cache, "http://example.org/locked", identity(b"x"))
                network.assert_not_called()
            response = io.BytesIO(b"x")
            response.geturl = lambda: "http://example.org/redirect"
            with mock.patch.object(prepare.HTTPS_OPENER, "open", return_value=response):
                with self.assertRaises(ValueError):
                    prepare.download(
                        cache, "https://example.org/locked", identity(b"x")
                    )
            self.assertEqual(list(cache.iterdir()), [])

    def test_redirect_handler_rejects_downgrade_before_following(self):
        handler = prepare.HTTPSRedirectHandler()
        request = prepare.urllib.request.Request("https://example.org/source")
        for target in ("http://example.org/source", "ftp://example.org/source"):
            with (
                self.subTest(target=target),
                mock.patch.object(
                    prepare.urllib.request.HTTPRedirectHandler, "redirect_request"
                ) as follow,
                self.assertRaisesRegex(ValueError, "away from HTTPS"),
            ):
                handler.redirect_request(request, None, 302, "Found", {}, target)
            follow.assert_not_called()
        redirected = handler.redirect_request(
            request, None, 302, "Found", {}, "https://assets.example.org/source"
        )
        self.assertEqual(redirected.full_url, "https://assets.example.org/source")

    def test_failed_stream_removes_partial_download(self):
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary)
            response = mock.MagicMock()
            response.__enter__.return_value = response
            response.geturl.return_value = "https://example.org/locked"
            response.read.side_effect = [b"part", OSError("connection lost")]
            with mock.patch.object(prepare.HTTPS_OPENER, "open", return_value=response):
                with self.assertRaisesRegex(OSError, "connection lost"):
                    prepare.download(
                        cache, "https://example.org/locked", identity(b"partial")
                    )
            self.assertEqual(list(cache.iterdir()), [])

    def test_symlinked_cache_entry_is_rejected_without_network(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            cache = root / "cache"
            cache.mkdir()
            payload = b"locked source"
            source = root / "source"
            source.write_bytes(payload)
            expected = identity(payload)
            try:
                (cache / expected["sha256"]).symlink_to(source)
            except OSError as error:
                self.skipTest(f"symlinks unavailable: {error}")
            with mock.patch.object(prepare.HTTPS_OPENER, "open") as network:
                with self.assertRaises(ValueError):
                    prepare.download(cache, "https://example.org/locked", expected)
                network.assert_not_called()

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

    def test_duplicate_archive_member_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / "duplicate.zip"
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)
                with zipfile.ZipFile(archive, "w") as writer:
                    writer.writestr("OFL.txt", b"first")
                    writer.writestr("OFL.txt", b"second")
            with self.assertRaisesRegex(ValueError, "ambiguous"):
                prepare.archive_member(archive, "OFL.txt", 100)

    def test_prepare_rejects_existing_output_before_any_network(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "prepared"
            output.mkdir()
            with mock.patch.object(prepare.HTTPS_OPENER, "open") as network:
                with self.assertRaisesRegex(ValueError, "fresh"):
                    prepare.prepare(output, root / "cache")
                network.assert_not_called()

    def test_prepare_rejects_overlapping_cache_before_network(self):
        for cache_inside in (False, True):
            with (
                self.subTest(cache_inside=cache_inside),
                tempfile.TemporaryDirectory() as temporary,
            ):
                parent = Path(temporary) / "absent"
                output, cache = (
                    (parent, parent / "child")
                    if cache_inside
                    else (parent / "child", parent)
                )
                with mock.patch.object(prepare.HTTPS_OPENER, "open") as network:
                    with self.assertRaisesRegex(ValueError, "overlap"):
                        prepare.prepare(output, cache)
                    network.assert_not_called()
                self.assertFalse(parent.exists())

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

    def test_late_notice_failure_discards_staged_pack_wheels_and_context(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "fonts.zip"
            with zipfile.ZipFile(archive, "w") as writer:
                writer.writestr("Regular.ttf", b"font")
                writer.writestr("OFL.txt", b"license")
            source = {
                "repository": "example/fonts",
                "release_tag": "Test-v1",
                "asset": {"name": "fonts.zip", **identity(archive.read_bytes())},
            }
            base = {
                "families": [
                    {
                        "source": source,
                        "files": [
                            {
                                "name": "Regular.ttf",
                                "asset_member": "Regular.ttf",
                                **identity(b"font"),
                            }
                        ],
                    }
                ]
            }
            lock = SimpleNamespace(sha256=identity(b"lock")["sha256"])
            wheel = {
                "wheel": {
                    "name": "tool.whl",
                    "url": "https://example.org/tool",
                    **identity(archive.read_bytes()),
                }
            }
            runtime_lock = {
                "archive": {
                    "url": "https://example.org/runtime",
                    **identity(archive.read_bytes()),
                }
            }

            def stage_pack(fonts, licenses, output, lock):
                output.mkdir()
                (output / "MANIFEST.json").write_bytes(b"receipt")
                return {}

            def stage_context(source, output):
                output.mkdir()
                (output / "Containerfile").write_bytes(b"recipe")

            with (
                mock.patch.object(
                    prepare.base_fonts, "load_font_lock", return_value=base
                ),
                mock.patch.object(prepare.shared, "load_lock", return_value=lock),
                mock.patch.object(
                    prepare, "_load_json", return_value=({"additions": []}, b"lock")
                ),
                mock.patch.object(
                    prepare.runtime, "load_runtime_lock", return_value=runtime_lock
                ),
                mock.patch.object(prepare.attestation, "tool_lock", return_value=wheel),
                mock.patch.object(prepare.resources, "tool_lock", return_value=wheel),
                mock.patch.object(prepare, "download", return_value=archive),
                mock.patch.object(
                    prepare.shared, "prepare_pack", side_effect=stage_pack
                ) as pack,
                mock.patch.object(
                    prepare.runtime, "prepare_build", side_effect=stage_context
                ) as context,
                mock.patch.object(
                    prepare,
                    "notice_payloads",
                    side_effect=ValueError("notices changed"),
                ),
            ):
                with self.assertRaisesRegex(ValueError, "notices changed"):
                    prepare.prepare(root / "prepared", root / "cache")
                pack.assert_called_once()
                context.assert_called_once()
            self.assertEqual(list(root.iterdir()), [archive])


class NoticeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.notices = self.root / "licenses"
        self.notices.mkdir()
        self.base_name = "NotoTest-v1-OFL.txt"
        self.extra_name = "Extra-LICENSE.txt"
        self.payload = b"locked extra license"
        (self.notices / self.base_name).write_bytes(b"base archive license")
        (self.notices / self.extra_name).write_bytes(self.payload)
        base = {"families": [{"source": {"release_tag": "NotoTest-v1"}}]}
        lock = SimpleNamespace(
            licenses=({"name": self.extra_name, **identity(self.payload)},)
        )
        for patcher in (
            mock.patch.object(prepare.base_fonts, "load_font_lock", return_value=base),
            mock.patch.object(prepare.shared, "load_lock", return_value=lock),
        ):
            patcher.start()
            self.addCleanup(patcher.stop)

    def test_exact_notices_are_read_and_addition_identity_checked(self):
        self.assertEqual(
            set(prepare.notice_payloads(self.root)), {self.base_name, self.extra_name}
        )
        (self.notices / self.extra_name).write_bytes(b"x" * len(self.payload))
        with self.assertRaisesRegex(ValueError, "identity"):
            prepare.notice_payloads(self.root)

    def test_missing_extra_directory_and_oversized_notices_are_rejected(self):
        extra = self.notices / "unexpected.txt"
        extra.write_bytes(b"not a public notice")
        with self.assertRaisesRegex(ValueError, "file set"):
            prepare.notice_payloads(self.root)
        extra.unlink()
        base = self.notices / self.base_name
        base.unlink()
        with self.assertRaisesRegex(ValueError, "file set"):
            prepare.notice_payloads(self.root)
        base.mkdir()
        with self.assertRaises(ValueError):
            prepare.notice_payloads(self.root)
        base.rmdir()
        base.write_bytes(b"x" * (prepare.shared.MAX_LICENSE_BYTES + 1))
        with self.assertRaises(ValueError):
            prepare.notice_payloads(self.root)

    def test_notice_file_and_directory_symlinks_are_rejected(self):
        base = self.notices / self.base_name
        base.unlink()
        target = self.root / "outside.txt"
        target.write_bytes(b"unrelated bytes")
        try:
            base.symlink_to(target)
        except OSError as error:
            self.skipTest(f"symlinks unavailable: {error}")
        with self.assertRaises(ValueError):
            prepare.notice_payloads(self.root)
        base.unlink()
        actual = self.root / "actual-notices"
        self.notices.rename(actual)
        self.notices.symlink_to(actual, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "symlinked"):
            prepare.notice_payloads(self.root)

    def test_namespace_import_is_supported(self):
        result = subprocess.run(
            [
                sys.executable,
                "-I",
                "-B",
                "-c",
                "import sys; sys.path.insert(0,sys.argv[1]); from scripts import prepare_render_oracle_ci",
                str(Path(__file__).resolve().parents[1]),
            ],
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
