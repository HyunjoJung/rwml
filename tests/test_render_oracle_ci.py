import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import render_oracle_ci as ci  # noqa: E402


class CiTests(unittest.TestCase):
    def test_wrong_revision_fails_before_creating_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "evidence"
            with mock.patch.object(
                ci.capture.table_capture,
                "source_revision",
                side_effect=ValueError("revision differs"),
            ):
                with self.assertRaisesRegex(ValueError, "revision differs"):
                    ci.run(output, Path(temporary) / "prepared", "a" * 40)
            self.assertFalse(output.exists())

    def test_ci_keeps_fidelity_failure_but_requires_complete_repeated_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "evidence"
            prepared = root / "prepared"
            (prepared / "licenses").mkdir(parents=True)
            (prepared / "licenses/OFL.txt").write_bytes(b"license")
            report = {
                "schema": ci.contract.CAPTURE_EVIDENCE_SCHEMA,
                "summary": {"documents": 12, "measured": 12, "skipped": 0},
                "gate": {"passed": False},
            }
            pair = {
                "scope": ci.repeat.SCOPE,
                "source_revision": "a" * 40,
                "summary": {"documents": 12},
            }
            captured_paths = []

            def capture_run(manifest, path, *args):
                captured_paths.append(path)
                path.mkdir()
                return {}

            with (
                mock.patch.object(
                    ci.capture.table_capture, "source_revision", return_value="a" * 40
                ),
                mock.patch.object(ci.capture, "run", side_effect=capture_run),
                mock.patch.object(
                    ci.render, "captured_validation_report", return_value=report
                ) as measure,
                mock.patch.object(ci.contract, "validate_evidence_report") as validate,
                mock.patch.object(
                    ci.repeat, "verify_repeated_campaign", return_value=pair
                ) as repeat,
                contextlib.redirect_stdout(io.StringIO()),
            ):
                self.assertEqual(ci.run(output, prepared, "a" * 40), pair)
                self.assertEqual(len(set(captured_paths)), 2)
                self.assertEqual(measure.call_count, 2)
                self.assertEqual(validate.call_count, 2)
                repeat.assert_called_once()
                for label in ("a", "b"):
                    self.assertFalse(
                        json.loads((output / f"evidence-{label}.json").read_bytes())[
                            "gate"
                        ]["passed"]
                    )
                self.assertEqual(
                    json.loads((output / "repeatability.json").read_bytes()), pair
                )
                self.assertEqual((output / "licenses/OFL.txt").read_bytes(), b"license")
                with self.assertRaisesRegex(ValueError, "fresh"):
                    ci.run(output, prepared, "a" * 40)

    def test_incomplete_or_invalid_evidence_never_produces_a_repeat_receipt(self):
        for skipped in (1, 0):
            with (
                self.subTest(skipped=skipped),
                tempfile.TemporaryDirectory() as temporary,
            ):
                root = Path(temporary)
                output = root / "evidence"
                prepared = root / "prepared"
                (prepared / "licenses").mkdir(parents=True)
                report = {
                    "summary": {
                        "documents": 12,
                        "measured": 12 - skipped,
                        "skipped": skipped,
                    },
                    "gate": {"passed": False},
                }
                with (
                    mock.patch.object(
                        ci.capture.table_capture,
                        "source_revision",
                        return_value="a" * 40,
                    ),
                    mock.patch.object(ci.capture, "run"),
                    mock.patch.object(
                        ci.render, "captured_validation_report", return_value=report
                    ),
                    mock.patch.object(
                        ci.contract,
                        "validate_evidence_report",
                        side_effect=None if skipped else ValueError("invalid evidence"),
                    ),
                    mock.patch.object(ci.repeat, "verify_repeated_campaign") as repeat,
                ):
                    with self.assertRaises(ValueError):
                        ci.run(output, prepared, "a" * 40)
                    repeat.assert_not_called()
                    self.assertFalse((output / "repeatability.json").exists())


if __name__ == "__main__":
    unittest.main()
