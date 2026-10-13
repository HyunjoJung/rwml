import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import render_oracle_ci as ci


class CiTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.output = self.root / "evidence"
        self.prepared = self.root / "prepared"
        self.revision = "a" * 40
        self.report = {
            "schema": ci.contract.CAPTURE_EVIDENCE_SCHEMA,
            "summary": {"documents": 12, "measured": 12, "skipped": 0},
            "gate": {"passed": False},
        }
        self.pair = {
            "schema": ci.metrics.REPEAT_SCHEMA,
            "scope": "diagnostic-repeat-not-release-acceptance",
            "source_revision": self.revision,
            "summary": {
                "documents": 12,
                "reference_pages": 16,
                "exact_native_pairs": 12,
                "exact_reference_rasters": 12,
                "metrics_recomputed": True,
                "fidelity_gate_passed": False,
            },
        }
        self.stack = contextlib.ExitStack()
        self.addCleanup(self.stack.close)
        self.source = self.stack.enter_context(
            mock.patch.object(
                ci.capture.table_capture, "source_revision", return_value=self.revision
            )
        )
        self.notices = self.stack.enter_context(
            mock.patch.object(
                ci.prepare, "notice_payloads", return_value={"OFL.txt": b"license"}
            )
        )
        self.capture = self.stack.enter_context(
            mock.patch.object(ci.capture, "run", side_effect=self.capture_run)
        )
        self.measure = self.stack.enter_context(
            mock.patch.object(ci.metrics, "run", return_value=self.report)
        )
        self.validate = self.stack.enter_context(
            mock.patch.object(ci.contract, "validate_evidence_report")
        )
        self.repeat = self.stack.enter_context(
            mock.patch.object(ci.metrics, "verify_repeat", return_value=self.pair)
        )
        self.stack.enter_context(contextlib.redirect_stdout(io.StringIO()))

    @staticmethod
    def capture_run(manifest, path, *args):
        path.mkdir()
        (path / "partial.bin").write_bytes(b"retained diagnostic")
        return {}

    def run_ci(self):
        return ci.run(self.output, self.prepared, self.revision)

    def test_wrong_revision_fails_before_creating_artifacts(self):
        self.source.side_effect = ValueError("revision differs")
        with self.assertRaisesRegex(ValueError, "revision differs"):
            self.run_ci()
        self.assertFalse(self.output.exists())
        self.capture.assert_not_called()

    def test_stale_smoke_and_invalid_notices_fail_before_capture(self):
        with mock.patch.object(ci.smoke, "check", return_value=False):
            with self.assertRaisesRegex(ValueError, "stale"):
                self.run_ci()
        self.notices.side_effect = ValueError("invalid notices")
        with self.assertRaisesRegex(ValueError, "invalid notices"):
            self.run_ci()
        self.assertFalse(self.output.exists())
        self.capture.assert_not_called()

    def test_ci_keeps_fidelity_failure_and_verifies_both_complete_captures(self):
        self.assertEqual(self.run_ci(), self.pair)
        captures = tuple(self.output / f"capture-{label}" for label in ("a", "b"))
        evidence = tuple(self.output / f"evidence-{label}.json" for label in ("a", "b"))
        pack = self.prepared / "shared-font-pack"
        fonttools = self.prepared / ci.capture.attestation.tool_lock()["wheel"]["name"]
        pypdf = self.prepared / ci.capture.resources.tool_lock()["wheel"]["name"]
        settings = ci.render.validate_visual_settings(
            {"font_mode": "locked-shared-fonts"}
        )
        self.assertEqual(
            self.capture.call_args_list,
            [
                mock.call(ci.smoke.OUTPUT_MANIFEST, root, pack, fonttools, pypdf)
                for root in captures
            ],
        )
        self.assertEqual(
            self.measure.call_args_list,
            [
                mock.call(
                    ci.smoke.OUTPUT_MANIFEST,
                    root,
                    pack,
                    fonttools,
                    pypdf,
                    settings=settings,
                    recall_min=0.97,
                )
                for root in captures
            ],
        )
        self.assertEqual(self.validate.call_count, 2)
        self.repeat.assert_called_once_with(
            ci.smoke.OUTPUT_MANIFEST, captures, evidence, pack, fonttools, pypdf
        )
        self.assertEqual(
            self.source.call_args_list,
            [mock.call(self.revision), mock.call(self.revision)],
        )
        for path in evidence:
            self.assertEqual(json.loads(path.read_bytes()), self.report)
        self.assertEqual(
            json.loads((self.output / "repeatability.json").read_bytes()), self.pair
        )
        self.assertEqual((self.output / "licenses/OFL.txt").read_bytes(), b"license")
        with self.assertRaisesRegex(ValueError, "fresh"):
            self.run_ci()

    def test_incomplete_evidence_never_produces_a_repeat_receipt(self):
        self.report["summary"].update(measured=11, skipped=1)
        with self.assertRaisesRegex(ValueError, "complete measured coverage"):
            self.run_ci()
        self.repeat.assert_not_called()
        self.assertFalse((self.output / "repeatability.json").exists())
        self.assertTrue((self.output / "capture-a/partial.bin").exists())

    def test_invalid_evidence_never_produces_a_repeat_receipt(self):
        self.validate.side_effect = ValueError("invalid evidence")
        with self.assertRaisesRegex(ValueError, "invalid evidence"):
            self.run_ci()
        self.repeat.assert_not_called()
        self.assertFalse((self.output / "repeatability.json").exists())

    def test_legacy_evidence_is_rejected_before_retention_or_repeat(self):
        self.report["schema"] = ci.contract.EVIDENCE_SCHEMA
        with self.assertRaisesRegex(ValueError, "capture evidence schema"):
            self.run_ci()
        self.repeat.assert_not_called()
        self.assertFalse((self.output / "evidence-a.json").exists())
        self.assertFalse((self.output / "repeatability.json").exists())

    def test_second_capture_failure_preserves_first_evidence_and_partial_capture(self):
        def fail_second(manifest, path, *args):
            self.capture_run(manifest, path, *args)
            if path.name == "capture-b":
                raise ValueError("capture failed")

        self.capture.side_effect = fail_second
        with self.assertRaisesRegex(ValueError, "capture failed"):
            self.run_ci()
        self.assertTrue((self.output / "evidence-a.json").exists())
        self.assertTrue((self.output / "capture-b/partial.bin").exists())
        self.assertFalse((self.output / "repeatability.json").exists())
        self.repeat.assert_not_called()

    def test_measurement_failure_never_skips_independent_verification(self):
        self.measure.side_effect = [self.report, ValueError("replay failed")]
        with self.assertRaisesRegex(ValueError, "replay failed"):
            self.run_ci()
        self.assertTrue((self.output / "evidence-a.json").exists())
        self.assertFalse((self.output / "evidence-b.json").exists())
        self.assertFalse((self.output / "repeatability.json").exists())
        self.repeat.assert_not_called()

    def test_repeat_failure_preserves_reports_without_success_receipt(self):
        self.repeat.side_effect = ValueError("repeat failed")
        with self.assertRaisesRegex(ValueError, "repeat failed"):
            self.run_ci()
        for label in ("a", "b"):
            self.assertTrue((self.output / f"evidence-{label}.json").exists())
        self.assertFalse((self.output / "repeatability.json").exists())

    def test_revision_change_after_repeat_prevents_success_receipt(self):
        self.source.side_effect = [self.revision, ValueError("source changed")]
        with self.assertRaisesRegex(ValueError, "source changed"):
            self.run_ci()
        self.repeat.assert_called_once()
        self.assertFalse((self.output / "repeatability.json").exists())

    def test_output_and_prepared_overlap_are_rejected(self):
        for output, prepared in (
            (self.output, self.output / "prepared"),
            (self.output / "evidence", self.output),
        ):
            with (
                self.subTest(output=output),
                self.assertRaisesRegex(ValueError, "overlap"),
            ):
                ci.run(output, prepared, self.revision)
        self.capture.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_namespace_import_and_cli_help(self):
        root = Path(__file__).resolve().parents[1]
        for args in (
            [
                "-I",
                "-B",
                "-c",
                "import sys; sys.path.insert(0,sys.argv[1]); from scripts import render_oracle_ci",
                str(root),
            ],
            ["-B", "-m", "scripts.render_oracle_ci", "--help"],
        ):
            with self.subTest(args=args):
                result = subprocess.run(
                    [sys.executable, *args],
                    cwd=root,
                    capture_output=True,
                    text=True,
                    timeout=30,
                )
                self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
