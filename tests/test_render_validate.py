import importlib.util
import pathlib
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "scripts" / "render_validate.py"
SPEC = importlib.util.spec_from_file_location("render_validate", SCRIPT)
render_validate = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = render_validate
SPEC.loader.exec_module(render_validate)


class RenderValidateReportTests(unittest.TestCase):
    def test_validation_report_summarizes_measured_and_skipped_rows(self):
        rows = [
            render_validate.ValidationRow(
                document="good.docx",
                status="pass",
                recall=1.0,
                rdoc_pages=2,
                reference_pages=2,
                page_ratio=1.0,
                ahash_similarity=0.75,
                render_warnings=1,
                render_warning_kinds=["UnsupportedFieldEvaluation"],
            ),
            render_validate.ValidationRow(
                document="low.docx",
                status="fail",
                recall=0.5,
                rdoc_pages=1,
                reference_pages=2,
                page_ratio=0.5,
                ahash_similarity=0.25,
                render_warnings=3,
                render_warning_kinds=[
                    "FloatingShapePlaceholderOnly",
                    "UnsupportedMetafileImages",
                    "OleObjectsPreservedButNotModeled",
                ],
            ),
            render_validate.ValidationRow(
                document="skipped.docx",
                status="skip",
                reason="render failed",
            ),
        ]

        report = render_validate.validation_report(rows, recall_min=0.8)

        self.assertEqual(report["summary"]["documents"], 3)
        self.assertEqual(report["summary"]["measured"], 2)
        self.assertEqual(report["summary"]["skipped"], 1)
        self.assertEqual(report["summary"]["below_recall_min"], 1)
        self.assertEqual(report["summary"]["mean_recall"], 0.75)
        self.assertEqual(report["summary"]["mean_page_ratio"], 0.75)
        self.assertEqual(report["summary"]["mean_ahash_similarity"], 0.5)
        self.assertEqual(report["summary"]["mean_render_warnings"], 2.0)
        self.assertEqual(
            report["rows"][0]["render_warning_kinds"],
            ["UnsupportedFieldEvaluation"],
        )
        self.assertEqual(report["rows"][2]["reason"], "render failed")

    def test_resolve_input_paths_reads_manifest_documents(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "synthetic").mkdir()
            first = root / "synthetic" / "first.docx"
            second = root / "synthetic" / "second.docx"
            first.write_bytes(b"first")
            second.write_bytes(b"second")
            manifest = root / "RENDER_MANIFEST.tsv"
            manifest.write_text(
                "# path\tpages\twarnings\n"
                "synthetic/first.docx\t1\t-\n"
                "synthetic/second.docx\t1\tUnsupportedFieldEvaluation\n",
                encoding="utf-8",
            )

            inputs = render_validate.resolve_input_paths([], manifest)

        self.assertEqual(inputs, [first, second])

    def test_validation_report_rejects_document_paths(self):
        row = render_validate.ValidationRow(
            document="private" + "/sample.docx",
            status="skip",
            reason="render failed",
        )

        with self.assertRaisesRegex(ValueError, "document path is invalid"):
            render_validate.validation_report([row], recall_min=0.8)

    def test_validation_report_rejects_malformed_documents(self):
        cases = [
            (1, "document must be a string"),
            ("", "document must not be empty"),
            (" sample.docx", "document must not have surrounding whitespace"),
        ]
        for document, message in cases:
            with self.subTest(document=document):
                row = render_validate.ValidationRow(
                    document=document,
                    status="skip",
                    reason="render failed",
                )

                try:
                    render_validate.validation_report([row], recall_min=0.8)
                except ValueError as exc:
                    self.assertRegex(str(exc), message)
                except Exception as exc:
                    self.fail(
                        f"expected ValueError, got {type(exc).__name__}: {exc}"
                    )
                else:
                    self.fail("ValueError not raised")

    def test_validation_report_rejects_invalid_status(self):
        row = render_validate.ValidationRow(
            document="sample.docx",
            status="pending",
            reason="render pending",
        )

        with self.assertRaisesRegex(ValueError, "status is invalid"):
            render_validate.validation_report([row], recall_min=0.8)

    def test_validation_report_rejects_non_numeric_metrics(self):
        row = render_validate.ValidationRow(
            document="sample.docx",
            status="pass",
            recall="1.0",
        )

        try:
            render_validate.validation_report([row], recall_min=0.8)
        except ValueError as exc:
            self.assertRegex(str(exc), "metric is invalid: recall")
        except Exception as exc:
            self.fail(f"expected ValueError, got {type(exc).__name__}: {exc}")
        else:
            self.fail("ValueError not raised")

    def test_validation_report_rejects_out_of_range_bounded_metrics(self):
        row = render_validate.ValidationRow(
            document="sample.docx",
            status="pass",
            recall=1.1,
        )

        with self.assertRaisesRegex(ValueError, "metric is out of range: recall"):
            render_validate.validation_report([row], recall_min=0.8)

    def test_validation_report_rejects_invalid_count_metrics(self):
        row = render_validate.ValidationRow(
            document="sample.docx",
            status="pass",
            render_warnings=-1,
        )

        with self.assertRaisesRegex(ValueError, "count is invalid: render_warnings"):
            render_validate.validation_report([row], recall_min=0.8)

    def test_validation_report_rejects_invalid_warning_kinds(self):
        cases = [
            ("UnsupportedFieldEvaluation", "render warning kinds must be a list"),
            (["Unsupported Field"], "render warning kind is invalid"),
            (["UnsupportedFieldEvalution"], "unknown render warning kind"),
            (["UnsupportedFieldEvaluation", "UnsupportedFieldEvaluation"], "duplicate"),
        ]
        for kinds, message in cases:
            with self.subTest(kinds=kinds):
                row = render_validate.ValidationRow(
                    document="sample.docx",
                    status="pass",
                    recall=1.0,
                    render_warnings=1 if isinstance(kinds, str) else len(kinds),
                    render_warning_kinds=kinds,
                )

                with self.assertRaisesRegex(ValueError, message):
                    render_validate.validation_report([row], recall_min=0.8)

        row = render_validate.ValidationRow(
            document="sample.docx",
            status="pass",
            recall=1.0,
            render_warnings=2,
            render_warning_kinds=["UnsupportedFieldEvaluation"],
        )
        with self.assertRaisesRegex(ValueError, "render warning count mismatch"):
            render_validate.validation_report([row], recall_min=0.8)

    def test_reference_recall_tokens_drop_volatile_libreoffice_field_text(self):
        tokens = [
            "Stable",
            "Error:",
            "Reference",
            "source",
            "not",
            "found",
            "/workspace/project/rdoc/",
            "corpus/public/synthetic/fields.docx",
            "report.docx",
        ]

        self.assertEqual(
            render_validate.reference_recall_tokens(tokens),
            ["Stable", "report.docx"],
        )

    def test_reference_recall_tokens_drop_libreoffice_shape_fallback_only_when_warned(self):
        tokens = ["Visible", "[shape]"]

        self.assertEqual(render_validate.reference_recall_tokens(tokens), tokens)
        self.assertEqual(
            render_validate.reference_recall_tokens(
                tokens,
                render_warning_kinds=["FloatingShapePlaceholderOnly"],
            ),
            ["Visible"],
        )

    def test_validation_report_rejects_measured_skip_rows(self):
        row = render_validate.ValidationRow(
            document="sample.docx",
            status="skip",
            recall=1.0,
            reason="render failed",
        )

        with self.assertRaisesRegex(ValueError, "skipped row has metrics"):
            render_validate.validation_report([row], recall_min=0.8)

    def test_validation_report_rejects_unmeasured_non_skip_rows(self):
        row = render_validate.ValidationRow(
            document="sample.docx",
            status="pass",
        )

        with self.assertRaisesRegex(ValueError, "non-skip row is missing recall"):
            render_validate.validation_report([row], recall_min=0.8)

    def test_validation_report_evaluates_release_thresholds(self):
        rows = [
            render_validate.ValidationRow(
                document="good.docx",
                status="pass",
                recall=0.95,
                rdoc_pages=2,
                reference_pages=2,
                page_ratio=1.0,
                ahash_similarity=0.8,
                render_warnings=1,
                render_warning_kinds=["UnsupportedFieldEvaluation"],
            ),
            render_validate.ValidationRow(
                document="low.docx",
                status="fail",
                recall=0.5,
                rdoc_pages=1,
                reference_pages=2,
                page_ratio=0.5,
                ahash_similarity=0.2,
                render_warnings=3,
                render_warning_kinds=[
                    "FloatingShapePlaceholderOnly",
                    "UnsupportedMetafileImages",
                    "OleObjectsPreservedButNotModeled",
                ],
            ),
            render_validate.ValidationRow(
                document="skipped.docx",
                status="skip",
                reason="render failed",
            ),
        ]

        report = render_validate.validation_report(
            rows,
            recall_min=0.8,
            thresholds={
                "min_mean_recall": 0.8,
                "min_mean_ahash_similarity": 0.6,
                "max_mean_render_warnings": 1.5,
                "max_skipped": 0,
            },
        )

        self.assertFalse(report["gate"]["passed"])
        checks = {check["metric"]: check for check in report["gate"]["checks"]}
        self.assertEqual(checks["below_recall_min"]["actual"], 1)
        self.assertEqual(checks["below_recall_min"]["op"], "<=")
        self.assertEqual(checks["below_recall_min"]["threshold"], 0)
        self.assertFalse(checks["below_recall_min"]["passed"])
        self.assertEqual(checks["mean_recall"]["actual"], 0.725)
        self.assertFalse(checks["mean_recall"]["passed"])
        self.assertEqual(checks["mean_ahash_similarity"]["actual"], 0.5)
        self.assertFalse(checks["mean_ahash_similarity"]["passed"])
        self.assertEqual(checks["mean_render_warnings"]["actual"], 2.0)
        self.assertFalse(checks["mean_render_warnings"]["passed"])
        self.assertEqual(checks["skipped"]["actual"], 1)
        self.assertFalse(checks["skipped"]["passed"])

    def test_warning_kinds_rejects_malformed_report_entries(self):
        self.assertEqual(
            render_validate.warning_kinds(
                {
                    "warnings": [
                        {"kind": "UnsupportedFieldEvaluation"},
                        {"kind": "UnsupportedMetafileImages"},
                    ]
                }
            ),
            ["UnsupportedFieldEvaluation", "UnsupportedMetafileImages"],
        )
        cases = [
            {"warnings": "UnsupportedFieldEvaluation"},
            {"warnings": [42]},
            {"warnings": [{"count": 1}]},
            {"warnings": [{"kind": "Unsupported Field"}]},
            {"warnings": [{"kind": "UnsupportedFieldEvalution"}]},
            {
                "warnings": [
                    {"kind": "UnsupportedFieldEvaluation"},
                    {"kind": "UnsupportedFieldEvaluation"},
                ]
            },
        ]
        for report in cases:
            with self.subTest(report=report):
                self.assertIsNone(render_validate.warning_kinds(report))

    def test_render_libreoffice_reports_missing_docker_dependency(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = pathlib.Path(tmp) / "sample.docx"
            src.write_bytes(b"placeholder")
            with mock.patch.object(
                render_validate.subprocess,
                "run",
                side_effect=FileNotFoundError(
                    2, "No such file or directory", "docker"
                ),
            ):
                with self.assertRaisesRegex(
                    RuntimeError, "docker executable not found"
                ):
                    render_validate.render_libreoffice(
                        src, pathlib.Path(tmp), "docker"
                    )

    def test_soffice_auto_mode_prefers_local_libreoffice(self):
        with mock.patch.object(
            render_validate.shutil,
            "which",
            side_effect=lambda name: f"/usr/bin/{name}" if name == "soffice" else None,
        ):
            self.assertEqual(render_validate.resolve_soffice_mode("auto"), "local")

    def test_soffice_auto_mode_falls_back_to_docker(self):
        with mock.patch.object(
            render_validate.shutil,
            "which",
            side_effect=lambda name: f"/usr/bin/{name}" if name == "docker" else None,
        ):
            self.assertEqual(render_validate.resolve_soffice_mode("auto"), "docker")

    def test_soffice_auto_mode_reports_missing_backends(self):
        with mock.patch.object(render_validate.shutil, "which", return_value=None):
            with self.assertRaisesRegex(
                RuntimeError,
                "neither soffice nor docker executable found",
            ):
                render_validate.resolve_soffice_mode("auto")

    def test_validation_gate_rejects_non_finite_thresholds(self):
        with self.assertRaisesRegex(ValueError, "non-finite threshold"):
            render_validate.validation_gate(
                {"below_recall_min": 0, "mean_recall": 1.0},
                {"min_mean_recall": float("nan")},
            )

    def test_validation_gate_rejects_negative_count_thresholds(self):
        with self.assertRaisesRegex(ValueError, "negative count threshold"):
            render_validate.validation_gate({"skipped": 0}, {"max_skipped": -1})

    def test_validation_gate_rejects_negative_score_thresholds(self):
        with self.assertRaisesRegex(ValueError, "negative score threshold"):
            render_validate.validation_gate(
                {"below_recall_min": 0, "mean_recall": 1.0},
                {"min_mean_recall": -0.1},
            )

    def test_validation_gate_rejects_bounded_score_thresholds_above_one(self):
        with self.assertRaisesRegex(ValueError, "score threshold above one"):
            render_validate.validation_gate(
                {"below_recall_min": 0, "mean_recall": 1.0},
                {"min_mean_recall": 1.1},
            )

    def test_validation_report_rejects_non_finite_recall_min(self):
        with self.assertRaisesRegex(ValueError, "non-finite recall threshold"):
            render_validate.validation_report([], recall_min=float("nan"))

    def test_validation_report_rejects_negative_recall_min(self):
        with self.assertRaisesRegex(ValueError, "negative recall threshold"):
            render_validate.validation_report([], recall_min=-0.1)

    def test_validation_report_rejects_recall_min_above_one(self):
        with self.assertRaisesRegex(ValueError, "recall threshold above one"):
            render_validate.validation_report([], recall_min=1.1)

    def test_json_report_payload_rejects_non_finite_values(self):
        with self.assertRaisesRegex(ValueError, "Out of range float values"):
            render_validate.json_report_payload(
                {"summary": {"mean_recall": float("nan")}}
            )


if __name__ == "__main__":
    unittest.main()
