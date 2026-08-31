import copy
import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import render_campaign_repeat as repeat  # noqa: E402


SHA = "a" * 64
REVISION = "b" * 40


def identity(payload: bytes) -> dict[str, object]:
    return {"bytes": len(payload), "sha256": repeat.capture.digest(payload)}


class RepeatVerifierTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.first = self.root / "first"
        self.second = self.root / "second"
        self.first_evidence = self.root / "first-evidence.json"
        self.second_evidence = self.root / "second-evidence.json"
        self.manifest = self.root / "manifest.json"
        self.pack = self.root / "pack"
        self.fonttools = self.root / "fonttools.whl"
        self.pypdf = self.root / "pypdf.whl"
        self.document = SimpleNamespace(case_id="fixture-basic", sha256=SHA)
        self.corpus = SimpleNamespace(
            path=self.manifest,
            documents=(self.document,),
            identity=lambda: {"name": "fixture", "documents": 1},
        )
        self.bundles = [
            self.write_capture(self.first, b"native", b"report", b"reference-one"),
            self.write_capture(self.second, b"native", b"report", b"reference-two"),
        ]
        self.evidence = [
            self.write_evidence(self.first_evidence, self.first, self.bundles[0]),
            self.write_evidence(self.second_evidence, self.second, self.bundles[1]),
        ]

    def write_capture(
        self, root: Path, native: bytes, report: bytes, reference: bytes
    ) -> dict:
        case = root / "cases" / self.document.case_id
        (case / "reference").mkdir(parents=True)
        (root / "CAPTURE.json").write_text(json.dumps({"capture": root.name}) + "\n")
        (case / "native.pdf").write_bytes(native)
        (case / "native-report.json").write_bytes(report)
        (case / "native-fonts.json").write_bytes(b"native fonts")
        (case / "reference/output.pdf").write_bytes(reference)
        (case / "reference-fonts.json").write_bytes(b"reference fonts")
        return {
            "schema": repeat.capture.SCHEMA,
            "scope": "diagnostic-capture-not-release-evidence",
            "source_revision": REVISION,
            "campaign": self.corpus.identity(),
            "environment": {"locked": True},
            "renderer": {"sha256": "c" * 64},
            "limits": {"seconds": 1, "bytes": 1},
            "rows": [
                {
                    "case_id": self.document.case_id,
                    "input": {"bytes": 1, "sha256": SHA},
                    "native_report": identity(report),
                    "native": {
                        "pdf": identity(native),
                        "font_checks": identity(b"native fonts"),
                    },
                    "reference": {
                        "pdf": identity(reference),
                        "font_checks": identity(b"reference fonts"),
                    },
                }
            ],
        }

    def write_evidence(self, path: Path, root: Path, bundle: dict) -> dict:
        evidence = {
            "schema": "rwml.render-oracle-evidence.v5",
            "campaign": self.corpus.identity(),
            "environment": {"fixed": True},
            "rows": [{"case_id": self.document.case_id, "recall": 1.0}],
            "summary": {"documents": 1},
            "gate": {"passed": True},
            "visual_comparison": {"dpi": 110, "page_cap": 32},
            "capture": repeat.capture_binding(root, bundle, self.corpus),
        }
        path.write_text(json.dumps(evidence))
        return evidence

    def verify(self):
        with (
            mock.patch.object(
                repeat.contract, "load_corpus_manifest", return_value=self.corpus
            ),
            mock.patch.object(repeat.capture, "run", side_effect=self.bundles),
            mock.patch.object(
                repeat.contract, "load_evidence_report", side_effect=self.evidence
            ),
            mock.patch.object(
                repeat.render, "reference_page_digests", return_value=["raster"]
            ),
            mock.patch.object(
                repeat,
                "replay_native_outputs",
                return_value={
                    self.document.case_id: {"pdf": b"native", "report": b"report"}
                },
            ),
        ):
            return repeat.verify_repeated_campaign(
                self.manifest,
                self.first,
                self.second,
                self.first_evidence,
                self.second_evidence,
                self.pack,
                self.fonttools,
                self.pypdf,
            )

    def test_complete_pair_binds_exact_native_metrics_and_reference_rasters(self):
        result = self.verify()
        self.assertEqual(result["schema"], repeat.SCHEMA)
        self.assertEqual(result["summary"]["documents"], 1)
        self.assertEqual(result["summary"]["reference_pages"], 1)
        self.assertEqual(result["summary"]["native_pdf_exact"], 1)
        self.assertEqual(result["summary"]["native_report_exact"], 1)
        self.assertEqual(result["summary"]["metric_reports_exact"], True)
        self.assertEqual(result["cases"][0]["reference_page_digests"], ["raster"])
        self.assertNotEqual(
            result["captures"][0]["capture_sha256"],
            result["captures"][1]["capture_sha256"],
        )
        self.assertNotIn(str(self.root), json.dumps(result))

    def test_distinct_capture_roots_are_required(self):
        with self.assertRaisesRegex(ValueError, "distinct"):
            repeat.verify_repeated_campaign(
                self.manifest,
                self.first,
                self.first,
                self.first_evidence,
                self.second_evidence,
                self.pack,
                self.fonttools,
                self.pypdf,
            )

    def test_capture_identity_drift_is_rejected(self):
        for key in ("source_revision", "campaign", "environment", "renderer", "limits"):
            with self.subTest(key=key):
                changed = copy.deepcopy(self.bundles[1])
                changed[key] = "changed"
                original = self.bundles[1]
                self.bundles[1] = changed
                try:
                    with self.assertRaisesRegex(ValueError, "capture pair"):
                        self.verify()
                finally:
                    self.bundles[1] = original

    def test_repaired_native_artifact_identity_is_rejected(self):
        path = self.first / "cases/fixture-basic/native.pdf"
        path.write_bytes(b"tampered")
        repaired = copy.deepcopy(self.bundles[0])
        repaired["rows"][0]["native"]["pdf"] = identity(b"tampered")
        self.bundles[0] = repaired
        repaired_evidence = copy.deepcopy(self.evidence[0])
        repaired_evidence["capture"] = repeat.capture_binding(
            self.first, repaired, self.corpus
        )
        self.evidence[0] = repaired_evidence
        with self.assertRaisesRegex(ValueError, "native PDF"):
            self.verify()

    def test_matching_repaired_native_artifacts_fail_independent_replay(self):
        for index, root in enumerate((self.first, self.second)):
            (root / "cases/fixture-basic/native.pdf").write_bytes(b"tampered")
            repaired = copy.deepcopy(self.bundles[index])
            repaired["rows"][0]["native"]["pdf"] = identity(b"tampered")
            self.bundles[index] = repaired
            repaired_evidence = copy.deepcopy(self.evidence[index])
            repaired_evidence["capture"] = repeat.capture_binding(
                root, repaired, self.corpus
            )
            self.evidence[index] = repaired_evidence
        with self.assertRaisesRegex(ValueError, "native PDF replay"):
            self.verify()

    def test_reference_raster_must_be_complete_and_equal(self):
        for values in ((["first"], ["second"]), (None, ["second"])):
            with self.subTest(values=values):
                with (
                    mock.patch.object(
                        repeat.contract,
                        "load_corpus_manifest",
                        return_value=self.corpus,
                    ),
                    mock.patch.object(repeat.capture, "run", side_effect=self.bundles),
                    mock.patch.object(
                        repeat.contract,
                        "load_evidence_report",
                        side_effect=self.evidence,
                    ),
                    mock.patch.object(
                        repeat.render,
                        "reference_page_digests",
                        side_effect=values,
                    ),
                    mock.patch.object(
                        repeat,
                        "replay_native_outputs",
                        return_value={
                            self.document.case_id: {
                                "pdf": b"native",
                                "report": b"report",
                            }
                        },
                    ),
                    self.assertRaisesRegex(ValueError, "reference raster"),
                ):
                    repeat.verify_repeated_campaign(
                        self.manifest,
                        self.first,
                        self.second,
                        self.first_evidence,
                        self.second_evidence,
                        self.pack,
                        self.fonttools,
                        self.pypdf,
                    )

    def test_metric_report_or_capture_binding_difference_is_rejected(self):
        changed = copy.deepcopy(self.evidence[1])
        changed["summary"]["documents"] = 2
        self.evidence[1] = changed
        with self.assertRaisesRegex(ValueError, "metric reports"):
            self.verify()
        self.evidence[1] = copy.deepcopy(self.evidence[0])
        self.evidence[1]["capture"]["sha256"] = "f" * 64
        with self.assertRaisesRegex(ValueError, "evidence capture binding"):
            self.verify()

    def test_cli_writes_one_fresh_canonical_receipt_outside_capture_roots(self):
        result = self.verify()
        output = self.root / "repeat.json"
        argv = [
            "render_campaign_repeat.py",
            "--manifest",
            str(self.manifest),
            "--first-capture",
            str(self.first),
            "--second-capture",
            str(self.second),
            "--first-evidence",
            str(self.first_evidence),
            "--second-evidence",
            str(self.second_evidence),
            "--font-pack",
            str(self.pack),
            "--fonttools-wheel",
            str(self.fonttools),
            "--pypdf-wheel",
            str(self.pypdf),
            "--output",
            str(output),
        ]
        with (
            mock.patch.object(sys, "argv", argv),
            mock.patch.object(
                repeat, "verify_repeated_campaign", return_value=result
            ) as verify,
            contextlib.redirect_stdout(io.StringIO()),
        ):
            self.assertEqual(repeat.main(), 0)
        self.assertEqual(output.read_bytes(), repeat.capture.canonical(result) + b"\n")
        verify.assert_called_once()

        with (
            mock.patch.object(sys, "argv", argv),
            mock.patch.object(repeat, "verify_repeated_campaign") as verify,
            contextlib.redirect_stderr(io.StringIO()),
        ):
            self.assertEqual(repeat.main(), 1)
        verify.assert_not_called()

        argv[-1] = str(self.first / "repeat.json")
        with (
            mock.patch.object(sys, "argv", argv),
            mock.patch.object(repeat, "verify_repeated_campaign") as verify,
            contextlib.redirect_stderr(io.StringIO()),
        ):
            self.assertEqual(repeat.main(), 1)
        verify.assert_not_called()


if __name__ == "__main__":
    unittest.main()
