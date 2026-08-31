import contextlib
import copy
import io
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import cff_mapping_worker as mapping  # noqa: E402
import font_subset_worker as common  # noqa: E402
import native_cff_attestation as native  # noqa: E402


def request():
    return {
        "schema": "rwml.cff-discovery-request.v1",
        "source": {
            "bytes": 100,
            "sha256": "a" * 64,
            "postscript_name": "Locked-CJK",
            "sfnt_revision": 65536,
        },
        "program": {"bytes": 20, "sha256": "b" * 64},
        "cmap": {"bytes": 30, "sha256": "c" * 64},
        "worker_sha256": "d" * 64,
        "helpers": {name: "e" * 64 for name in mapping.HELPERS},
    }


def result():
    return {
        **request(),
        "schema": "rwml.cff-discovery-worker.v1",
        "fonttools_version": common.WHEEL_VERSION,
        "fonttools_sha256": common.WHEEL_SHA256,
        "pypdf_version": mapping.pdf.WHEEL_VERSION,
        "pypdf_sha256": mapping.pdf.WHEEL_SHA256,
        "python": common.PYTHON_VERSION,
        "limits": {**common.LIMITS, **mapping.MAPPING_LIMITS},
        "glyphs": [[".notdef", ".notdef"], ["cid00001", "cid00002"]],
        "stats": {"source_glyphs": 3, "outline_commands": 12, "search_steps": 1},
    }


class NativeCFFTests(unittest.TestCase):
    def test_discovery_request_has_exact_bounded_input_identities(self):
        mapping.validate_request(request())
        for key, value in (
            ("program", {"bytes": True, "sha256": "b" * 64}),
            ("cmap", {"bytes": 65537, "sha256": "c" * 64}),
            ("helpers", {}),
            ("extra", True),
        ):
            with self.subTest(key=key), self.assertRaises(ValueError):
                mapping.validate_request({**request(), key: value})

    def test_discovery_result_requires_exact_inputs_tools_and_complete_mapping(self):
        native.validate_mapping_result(result(), request())
        for key, value in (
            ("cmap", {"bytes": 30, "sha256": "f" * 64}),
            ("fonttools_version", "unknown"),
            ("glyphs", [[".notdef", ".notdef"]]),
            ("helpers", {}),
            ("limits", {}),
        ):
            with self.subTest(key=key), self.assertRaises(ValueError):
                native.validate_mapping_result({**result(), key: value}, request())

    def test_work_statistics_are_typed_and_bounded(self):
        for key, value in (
            ("source_glyphs", True),
            ("source_glyphs", 1),
            ("outline_commands", -1),
            ("search_steps", mapping.MAPPING_LIMITS["candidate_search_steps"] + 1),
        ):
            invalid = result()
            invalid["stats"][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                native.validate_mapping_result(invalid, request())

    def test_no_cff_resources_are_not_reported_as_a_successful_proof(self):
        extraction = {"result": {"fonts": [], "blobs": []}}
        with mock.patch.object(
            native.resources, "extract_pdf", return_value=extraction
        ):
            with self.assertRaisesRegex(ValueError, "no native CFF"):
                native.attest_pdf(
                    b"pdf", b"source", {}, Path("fonttools"), Path("pypdf")
                )

    def test_batch_deadline_is_checked_before_starting_an_operation(self):
        with (
            mock.patch.object(native.time, "monotonic", side_effect=[0, 121]),
            mock.patch.object(native.resources, "extract_pdf") as extract,
        ):
            with self.assertRaisesRegex(ValueError, "timed out"):
                native.attest_pdf(
                    b"pdf", b"source", {}, Path("fonttools"), Path("pypdf")
                )
        extract.assert_not_called()

    def test_changed_discovered_map_fails_independent_receipt_verification(self):
        original = result()
        changed = copy.deepcopy(original)
        changed["glyphs"][1][1] = "cid00001"
        native.validate_mapping_result(changed, request())
        with self.assertRaisesRegex(ValueError, "recomputed"):
            native.resources.verify_receipt(common.canonical(changed), original)

    def test_cli_recomputes_and_does_not_rewrite_retained_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / "fonts").mkdir()
            font = directory / "fonts/NotoSansCJKkr-Regular.otf"
            font.write_bytes(b"source")
            pdf = directory / "input.pdf"
            pdf.write_bytes(b"%PDF-fixture")
            receipt = directory / "receipt.json"
            expected = {
                "cff_resources": [{"proof": {"result": {"proof": {"glyph_count": 2}}}}],
                "unverified_resources": [],
                "shared_font_lock_sha256": "a" * 64,
                "shared_base_lock_sha256": "b" * 64,
            }
            serialized = common.canonical(expected) + b"\n"
            receipt.write_bytes(serialized)
            entry = {"name": font.name, "bytes": 6}
            lock = SimpleNamespace(fonts=[entry], sha256="a" * 64, base_sha256="b" * 64)
            args = [
                "native_cff_attestation.py",
                "--pdf",
                str(pdf),
                "--font-pack",
                str(directory),
                "--fonttools-wheel",
                "fonttools.whl",
                "--pypdf-wheel",
                "pypdf.whl",
                "--verify",
                str(receipt),
            ]
            with (
                mock.patch.object(sys, "argv", args),
                mock.patch.object(native.shared, "load_lock", return_value=lock),
                mock.patch.object(native.shared, "verify_pack") as verify,
                mock.patch.object(
                    native, "attest_pdf", return_value=copy.deepcopy(expected)
                ) as attest,
                contextlib.redirect_stdout(io.StringIO()),
            ):
                self.assertEqual(native.main(), 0)
            attest.assert_called_once_with(
                b"%PDF-fixture",
                b"source",
                entry,
                Path("fonttools.whl"),
                Path("pypdf.whl"),
            )
            self.assertEqual(verify.call_count, 2)
            self.assertEqual(receipt.read_bytes(), serialized)

    def test_existing_output_is_preserved_before_reading_inputs(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "receipt.json"
            path.write_bytes(b"keep")
            args = [
                "native_cff_attestation.py",
                "--pdf",
                "missing.pdf",
                "--font-pack",
                "missing",
                "--fonttools-wheel",
                "missing.whl",
                "--pypdf-wheel",
                "missing.whl",
                "--output",
                str(path),
            ]
            with (
                mock.patch.object(sys, "argv", args),
                mock.patch.object(native.shared, "load_lock") as load,
                contextlib.redirect_stderr(io.StringIO()),
            ):
                self.assertEqual(native.main(), 1)
            load.assert_not_called()
            self.assertEqual(path.read_bytes(), b"keep")


if __name__ == "__main__":
    unittest.main()
