import importlib.util
import pathlib
import sys
import unittest


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
                render_warning_kinds=["FloatingShapePlaceholderOnly"],
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

    def test_validation_gate_rejects_non_finite_thresholds(self):
        with self.assertRaisesRegex(ValueError, "non-finite threshold"):
            render_validate.validation_gate(
                {"below_recall_min": 0, "mean_recall": 1.0},
                {"min_mean_recall": float("nan")},
            )

    def test_validation_gate_rejects_negative_count_thresholds(self):
        with self.assertRaisesRegex(ValueError, "negative count threshold"):
            render_validate.validation_gate({"skipped": 0}, {"max_skipped": -1})

    def test_validation_report_rejects_non_finite_recall_min(self):
        with self.assertRaisesRegex(ValueError, "non-finite recall threshold"):
            render_validate.validation_report([], recall_min=float("nan"))

    def test_json_report_payload_rejects_non_finite_values(self):
        with self.assertRaisesRegex(ValueError, "Out of range float values"):
            render_validate.json_report_payload(
                {"summary": {"mean_recall": float("nan")}}
            )


if __name__ == "__main__":
    unittest.main()
