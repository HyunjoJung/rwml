import copy
import contextlib
import io
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import font_subset_attestation as attestation  # noqa: E402
import font_subset_worker as worker  # noqa: E402


def sample_request():
    return {
        "schema": "rwml.font-subset-request.v1",
        "source": {
            "bytes": 64,
            "sha256": "a" * 64,
            "postscript_name": "Locked-CJK",
            "sfnt_revision": 65536,
        },
        "subset": {"bytes": 128, "sha256": "b" * 64, "representation": "type1-pfa"},
        "worker_sha256": "c" * 64,
    }


def sample_result():
    request = sample_request()
    proof = {
        "matrix": [[1, 1000], [0, 1], [0, 1], [1, 1000], [0, 1], [0, 1]],
        "glyphs": [
            {
                "subset": ".notdef",
                "source": ".notdef",
                "width": [1000, 1],
                "outline_sha256": "d" * 64,
            },
            {
                "subset": "cid1",
                "source": "cid00001",
                "width": [1000, 1],
                "outline_sha256": "e" * 64,
            },
        ],
    }
    proof["outline_sha256"] = worker.digest(worker.canonical(proof))
    proof["glyph_count"] = 2
    return {
        "schema": "rwml.font-subset-worker.v1",
        "source": request["source"],
        "subset": request["subset"],
        "worker_sha256": request["worker_sha256"],
        "fonttools_version": worker.WHEEL_VERSION,
        "wheel_sha256": worker.WHEEL_SHA256,
        "python": worker.PYTHON_VERSION,
        "limits": worker.LIMITS,
        "proof": proof,
    }


class FontSubsetAttestationTests(unittest.TestCase):
    def test_worker_result_requires_exact_inputs_tools_and_proof(self):
        attestation.validate_result(sample_result(), sample_request())
        for key, value in (
            ("source", {}),
            ("python", "0.0.0"),
            ("worker_sha256", "f" * 64),
            ("limits", {}),
            ("fonttools_version", "unknown"),
        ):
            invalid = copy.deepcopy(sample_result())
            invalid[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                attestation.validate_result(invalid, sample_request())

    def test_proof_counts_numbers_names_and_aggregate_digest_are_strict(self):
        for change in (
            {"glyph_count": True},
            {"glyph_count": 2.0},
            {"glyph_count": 3},
            {"outline_sha256": "0" * 64},
            {"glyphs": []},
            {"matrix": [[1, 0]] * 6},
        ):
            invalid = copy.deepcopy(sample_result())
            invalid["proof"].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                attestation.validate_result(invalid, sample_request())

    def test_repaired_hash_cannot_hide_a_cid_alias(self):
        invalid = copy.deepcopy(sample_result())
        proof = invalid["proof"]
        proof["glyphs"][1]["source"] = ".notdef"
        proof["outline_sha256"] = worker.digest(
            worker.canonical({"matrix": proof["matrix"], "glyphs": proof["glyphs"]})
        )
        with self.assertRaises(ValueError):
            attestation.validate_result(invalid, sample_request())

    def test_cff_proof_must_follow_the_bound_map_not_identity_cids(self):
        request = sample_request()
        request["subset"]["representation"] = "cid-cff"
        request["subset"]["glyph_map"] = [
            [".notdef", ".notdef"],
            ["cid00001", "cid63157"],
        ]
        result = sample_result()
        result["subset"] = request["subset"]
        proof = result["proof"]
        proof["glyphs"][1].update(subset="cid00001", source="cid63157")
        proof["outline_sha256"] = worker.digest(
            worker.canonical({"matrix": proof["matrix"], "glyphs": proof["glyphs"]})
        )
        attestation.validate_result(result, request)
        proof["glyphs"][1]["source"] = "cid00001"
        proof["outline_sha256"] = worker.digest(
            worker.canonical({"matrix": proof["matrix"], "glyphs": proof["glyphs"]})
        )
        with self.assertRaisesRegex(ValueError, "mapping"):
            attestation.validate_result(result, request)

    def test_cff_map_requires_original_source_and_subset_digests(self):
        source, subset = b"source", b"subset"
        mapping = {
            "schema": "rwml.cff-glyph-map.v1",
            "source_sha256": worker.digest(source),
            "subset_sha256": worker.digest(subset),
            "glyphs": [[".notdef", ".notdef"], ["cid00001", "cid63157"]],
        }
        self.assertEqual(
            attestation.load_cff_map(worker.canonical(mapping), source, subset),
            mapping["glyphs"],
        )
        for key, value in (
            ("source_sha256", "a" * 64),
            ("subset_sha256", "b" * 64),
            ("extra", True),
        ):
            changed = {**mapping, key: value}
            with self.subTest(key=key), self.assertRaises(ValueError):
                attestation.load_cff_map(worker.canonical(changed), source, subset)

    def test_command_preserves_container_isolation_with_bundled_python(self):
        with tempfile.TemporaryDirectory() as temporary:
            command = attestation.worker_command(
                "sha256:" + "a" * 64, "rwml-oracle-" + "b" * 32, Path(temporary)
            )
        self.assertEqual(command[command.index("--memory") + 1], "2g")
        self.assertEqual(command[command.index("--network") + 1], "none")
        self.assertEqual(
            command[command.index("--entrypoint") + 1],
            "/opt/libreoffice26.2/program/python",
        )
        self.assertEqual(
            command[-5:], ["-B", "-s", "-S", "-P", "/oracle/source/worker.py"]
        )
        self.assertIn("--read-only", command)

    def test_deeply_nested_mapping_is_a_typed_failure(self):
        with self.assertRaises(ValueError):
            attestation.load_cff_map(
                b"[" * 2000 + b"0" + b"]" * 2000, b"source", b"subset"
            )

    def test_changed_or_missing_wheel_is_rejected_before_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "fonttools.whl"
            with self.assertRaises(ValueError):
                attestation.wheel_payload(path)
            path.write_bytes(b"wrong wheel")
            with self.assertRaises(ValueError):
                attestation.wheel_payload(path)

    def test_receipt_must_match_independently_recomputed_result(self):
        result = {
            "schema": "rwml.font-subset-attestation.v1",
            "result": sample_result(),
        }
        attestation.verify_receipt(worker.canonical(result), result)
        altered = copy.deepcopy(result)
        proof = altered["result"]["proof"]
        proof["glyphs"][1]["outline_sha256"] = "f" * 64
        proof["outline_sha256"] = worker.digest(
            worker.canonical({"matrix": proof["matrix"], "glyphs": proof["glyphs"]})
        )
        attestation.validate_result(altered["result"], sample_request())
        with self.assertRaisesRegex(ValueError, "recomputed"):
            attestation.verify_receipt(worker.canonical(altered), result)
        for payload in (
            b'{"a":1,"a":2}',
            b'{"a":NaN}',
            b"x" * (worker.MAX_RESULT_BYTES + 1),
        ):
            with self.subTest(payload=payload[:32]), self.assertRaises(ValueError):
                attestation.verify_receipt(payload, result)

    def test_existing_output_is_rejected_before_pack_or_worker_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "receipt.json"
            output.write_bytes(b"existing")
            args = [
                "font_subset_attestation.py",
                "--font-pack",
                temporary,
                "--fonttools-wheel",
                "wheel.whl",
                "--program",
                "font.pfa",
                "--output",
                str(output),
            ]
            with (
                mock.patch.object(sys, "argv", args),
                mock.patch.object(attestation.shared, "load_lock") as load,
                contextlib.redirect_stderr(io.StringIO()),
            ):
                self.assertEqual(attestation.main(), 1)
            load.assert_not_called()
            self.assertEqual(output.read_bytes(), b"existing")

    def test_verify_cli_recomputes_from_original_bytes_and_does_not_rewrite(self):
        result = {
            "schema": "rwml.font-subset-attestation.v1",
            "result": sample_result(),
        }
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / "fonts").mkdir()
            name = "NotoSansCJKkr-Regular.otf"
            (directory / "fonts" / name).write_bytes(b"source")
            program = directory / "font.pfa"
            program.write_bytes(b"subset")
            receipt = directory / "receipt.json"
            payload = worker.canonical(result) + b"\n"
            receipt.write_bytes(payload)
            entry = {"name": name, "bytes": 6}
            args = [
                "font_subset_attestation.py",
                "--font-pack",
                temporary,
                "--fonttools-wheel",
                "wheel.whl",
                "--program",
                str(program),
                "--verify",
                str(receipt),
            ]
            with (
                mock.patch.object(sys, "argv", args),
                mock.patch.object(
                    attestation.shared,
                    "load_lock",
                    return_value=SimpleNamespace(fonts=[entry]),
                ),
                mock.patch.object(attestation.shared, "verify_pack") as pack,
                mock.patch.object(
                    attestation, "attest_program", return_value=result
                ) as attest,
                contextlib.redirect_stdout(io.StringIO()),
            ):
                self.assertEqual(attestation.main(), 0)
            attest.assert_called_once_with(
                b"subset", b"source", entry, Path("wheel.whl")
            )
            self.assertEqual(pack.call_count, 2)
            self.assertEqual(receipt.read_bytes(), payload)


if __name__ == "__main__":
    unittest.main()
