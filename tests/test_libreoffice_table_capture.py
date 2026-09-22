import copy
import importlib
import pathlib
import sys
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
capture = importlib.import_module("libreoffice_table_capture")


class LibreOfficeTableCaptureTests(unittest.TestCase):
    def test_analysis_tools_bind_imported_distribution_payloads(self):
        expected = {"identity": "bound"}
        image = mock.Mock(__version__="12.3.0")
        fitz = mock.Mock(__version__="1.28.2")
        with (
            mock.patch.object(capture, "Image", image),
            mock.patch.object(capture, "fitz", fitz),
            mock.patch.object(
                capture.analysis, "analysis_identity", return_value=expected
            ) as build,
        ):
            self.assertIs(capture.analysis_tools(), expected)
        build.assert_called_once_with(
            {
                "pillow": ("Pillow", "12.3.0", image),
                "pymupdf": ("PyMuPDF", "1.28.2", fitz),
            }
        )

    def test_topology_binding_rejects_stale_nested_harness_tools_and_extractor(self):
        with (
            mock.patch.object(capture, "analysis_tools", return_value={}),
            mock.patch.object(
                capture.analysis,
                "tool_versions",
                return_value={"pymupdf": "1.28.2"},
            ),
        ):
            value = {
                "producer": {},
                "environment": {
                    "source_revision": "a" * 40,
                    "source_dirty": False,
                    "harness_sha256": capture.topology_harness_sha256(),
                    "tools": [{"name": "pymupdf", "version": "1.28.2"}],
                },
                "extractor": {
                    "identity_sha256": capture.harness_identity()[
                        "table_oracle_topology.py"
                    ]
                },
            }
            capture.validate_topology_binding(value, {}, "a" * 40)
            for field in ("harness_sha256", "tools", "source_dirty"):
                invalid = copy.deepcopy(value)
                invalid["environment"][field] = "stale"
                with self.subTest(field=field), self.assertRaises(ValueError):
                    capture.validate_topology_binding(invalid, {}, "a" * 40)
            value["extractor"]["identity_sha256"] = "b" * 64
            with self.assertRaises(ValueError):
                capture.validate_topology_binding(value, {}, "a" * 40)

    def test_executor_contract_rejects_missing_fields_and_wrong_types(self):
        valid = {
            "client": {
                "Version": "1",
                "ApiVersion": "1",
                "GitCommit": "abc",
                "Os": "darwin",
                "Arch": "arm64",
            },
            "server": {
                "Version": "1",
                "ApiVersion": "1",
                "GitCommit": "abc",
                "Os": "linux",
                "Arch": "arm64",
            },
            "kernel": "6.8",
            "client_sha256": "a" * 64,
        }
        capture.validate_executor(valid)
        for key, value in [
            ("client", None),
            ("server", {}),
            ("kernel", True),
            ("client_sha256", "unlocked"),
        ]:
            invalid = copy.deepcopy(valid)
            invalid[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                capture.validate_executor(invalid)

    def test_exact_contract_distinguishes_boolean_from_integer(self):
        with self.assertRaises(ValueError):
            capture.require_equal({"documents": 1}, {"documents": True}, "count")
        capture.require_equal({"documents": 1}, {"documents": 1}, "count")

    def test_font_stage_rejects_wrong_bytes_without_output(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            font = root / "font.ttf"
            font.write_bytes(b"wrong font")
            with self.assertRaises(ValueError):
                capture.stage_font(font, root / "fonts")
            self.assertFalse((root / "fonts").exists())

    def test_font_directory_rejects_unlisted_files_and_symlinks(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            (root / "font.ttf").write_bytes(b"font")
            expected = {"font.ttf": b"font"}
            capture.verify_directory(root, expected)
            (root / "extra.txt").write_bytes(b"extra")
            with self.assertRaises(ValueError):
                capture.verify_directory(root, expected)
            (root / "extra.txt").unlink()
            (root / "alias").symlink_to(root / "font.ttf")
            with self.assertRaises(ValueError):
                capture.verify_directory(root, expected)

    def test_capture_content_rejects_wrong_version_font_path_or_pdf_digest(self):
        pdf = b"%PDF-1.7\nexample"
        entries = {
            "output.pdf": pdf,
            "version.txt": (capture.runtime.VERSION_LINE + "\n").encode(),
            "fonts.txt": b"/oracle/fonts/NotoSans-Regular.ttf\n",
            "sha256.txt": (capture.runtime.sha256(pdf) + "  output.pdf\n").encode(),
            "warmup.log": b"",
            "conversion.log": b"conversion",
        }
        capture.validate_capture_content(entries)
        for key, wrong in [
            ("version.txt", b"wrong"),
            ("fonts.txt", b"unlocked"),
            ("sha256.txt", b"wrong"),
            ("output.pdf", b"not a PDF"),
        ]:
            with self.subTest(key=key):
                invalid = copy.deepcopy(entries)
                invalid[key] = wrong
                with self.assertRaises(ValueError):
                    capture.validate_capture_content(invalid)

    def test_source_identity_fails_dirty_or_wrong_revision(self):
        with mock.patch.object(
            capture.runtime, "run_bounded", side_effect=[b"a" * 40, b" M file"]
        ):
            with self.assertRaisesRegex(ValueError, "clean"):
                capture.source_revision()
        with mock.patch.object(capture.runtime, "run_bounded", return_value=b"a" * 40):
            with self.assertRaisesRegex(ValueError, "revision"):
                capture.source_revision("b" * 40)


if __name__ == "__main__":
    unittest.main()
