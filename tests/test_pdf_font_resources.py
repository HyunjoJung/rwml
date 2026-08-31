import base64
import copy
import contextlib
import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import font_subset_worker as common  # noqa: E402
import pdf_font_resources as resources  # noqa: E402
import pdf_font_worker as worker  # noqa: E402


def request():
    return {
        "schema": "rwml.pdf-font-request.v1",
        "pdf": {"bytes": 100, "sha256": "a" * 64},
        "worker_sha256": "b" * 64,
        "helper_sha256": "c" * 64,
    }


def result():
    program = b"%!FontType1-fixture"
    return {
        **request(),
        "schema": "rwml.pdf-font-worker.v1",
        "parser_version": worker.WHEEL_VERSION,
        "wheel_sha256": worker.WHEEL_SHA256,
        "python": common.PYTHON_VERSION,
        "limits": {**common.LIMITS, **worker.PDF_LIMITS},
        "fonts": [
            {
                "ref": [4, 0],
                "subtype": "Type1",
                "base_font": "AAAAAA+Fixture",
                "descriptor_font": "AAAAAA+Fixture",
                "descendant_ref": None,
                "descendant_subtype": None,
                "encoding_kind": "absent",
                "program": [6, 0],
                "to_unicode": None,
            }
        ],
        "blobs": [
            {
                "ref": [6, 0],
                "kind": "type1-pfa",
                "bytes": len(program),
                "sha256": common.digest(program),
                "base64": base64.b64encode(program).decode(),
            }
        ],
    }


class PDFFontResourceTests(unittest.TestCase):
    def test_request_requires_bounded_pdf_and_exact_identities(self):
        worker.validate_request(request())
        for change in (
            {"pdf": {"bytes": True, "sha256": "a" * 64}},
            {"pdf": {"bytes": worker.MAX_PDF_BYTES + 1, "sha256": "a" * 64}},
            {"helper_sha256": "unknown"},
            {"extra": True},
        ):
            with self.subTest(change=change), self.assertRaises(ValueError):
                worker.validate_request({**request(), **change})

    def test_result_rejects_changed_inputs_and_parser(self):
        resources.validate_result(result(), request())
        for change in (
            {"pdf": {"bytes": 101, "sha256": "a" * 64}},
            {"parser_version": "unknown"},
            {"helper_sha256": "d" * 64},
            {"limits": {}},
        ):
            with self.subTest(change=change), self.assertRaises(ValueError):
                resources.validate_result({**result(), **change}, request())

    def test_result_rejects_missing_or_aliased_resources(self):
        for mutation in ("missing", "duplicate", "wrong-kind", "unreferenced"):
            changed = result()
            if mutation == "missing":
                changed["blobs"] = []
            elif mutation == "duplicate":
                changed["fonts"] *= 2
            elif mutation == "wrong-kind":
                changed["blobs"][0]["kind"] = "truetype"
            else:
                changed["fonts"] = []
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                resources.validate_result(changed, request())

    def test_blob_hash_size_and_canonical_base64_are_checked(self):
        for change in (
            {"sha256": "f" * 64},
            {"bytes": True},
            {"bytes": 100},
            {"base64": "%%%%"},
            {"base64": result()["blobs"][0]["base64"] + "\n"},
        ):
            changed = result()
            changed["blobs"][0].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                resources.validate_result(changed, request())

    def test_receipt_requires_independent_recomputation(self):
        original = result()
        resources.verify_receipt(common.canonical(original), original)
        changed = copy.deepcopy(original)
        changed["fonts"][0]["base_font"] = "Another-Font"
        resources.validate_result(changed, request())
        with self.assertRaisesRegex(ValueError, "recomputed"):
            resources.verify_receipt(common.canonical(changed), original)
        for payload in (b'{"a":1,"a":2}', b'{"a":NaN}'):
            with self.assertRaises(ValueError):
                resources.verify_receipt(payload, original)

    def test_changed_wheel_fails_before_container_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "pypdf.whl"
            path.write_bytes(b"not the pinned wheel")
            with self.assertRaises(ValueError):
                resources.wheel_payload(path)

    def test_json_complexity_fails_before_canonicalization(self):
        with self.assertRaisesRegex(ValueError, "depth"):
            resources.verify_receipt(b"[" * 2000 + b"0" + b"]" * 2000, {})

    def test_cli_recomputes_and_never_rewrites_a_retained_receipt(self):
        receipt = {"schema": "rwml.pdf-font-extraction.v1", "result": result()}
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            pdf, retained = directory / "input.pdf", directory / "receipt.json"
            pdf.write_bytes(b"%PDF-fixture")
            payload = common.canonical(receipt) + b"\n"
            retained.write_bytes(payload)
            args = [
                "pdf_font_resources.py",
                "--pdf",
                str(pdf),
                "--pypdf-wheel",
                "pypdf.whl",
                "--verify",
                str(retained),
            ]
            with (
                mock.patch.object(sys, "argv", args),
                mock.patch.object(
                    resources, "extract_pdf", return_value=receipt
                ) as extract,
                contextlib.redirect_stdout(io.StringIO()),
            ):
                self.assertEqual(resources.main(), 0)
            extract.assert_called_once_with(b"%PDF-fixture", Path("pypdf.whl"))
            self.assertEqual(retained.read_bytes(), payload)

    def test_existing_output_fails_before_parser_or_wheel_use(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "receipt.json"
            path.write_bytes(b"retained")
            args = [
                "pdf_font_resources.py",
                "--pdf",
                "missing.pdf",
                "--pypdf-wheel",
                "missing.whl",
                "--output",
                str(path),
            ]
            with (
                mock.patch.object(sys, "argv", args),
                mock.patch.object(resources, "extract_pdf") as extract,
                contextlib.redirect_stderr(io.StringIO()),
            ):
                self.assertEqual(resources.main(), 1)
            extract.assert_not_called()
            self.assertEqual(path.read_bytes(), b"retained")

    def test_empty_inventory_is_explicitly_permitted(self):
        value = result()
        value.update(fonts=[], blobs=[])
        resources.validate_result(value, request())

    def test_reference_and_font_name_types_are_strict(self):
        for key, value in (
            ("ref", [True, 0]),
            ("ref", [0, 0]),
            ("ref", [4, 65536]),
            ("base_font", "/private/font"),
            ("subtype", []),
            ("descendant_ref", [5, 0]),
        ):
            changed = result()
            changed["fonts"][0][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                resources.validate_result(changed, request())


if __name__ == "__main__":
    unittest.main()
