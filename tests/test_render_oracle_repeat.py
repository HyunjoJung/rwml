import contextlib
import importlib.util
import io
import json
import pathlib
import sys
import unittest
from unittest import mock


SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "scripts" / "render_validate.py"
SPEC = importlib.util.spec_from_file_location("render_validate_repeat", SCRIPT)
render_validate = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = render_validate
SPEC.loader.exec_module(render_validate)


class OracleRepeatExecutionTests(unittest.TestCase):
    def run_campaign(self, *, exports=None, digests=None, font_failures=(), verify=True):
        exports = exports or {}
        digests = digests or {}
        inputs = [pathlib.Path("first.docx"), pathlib.Path("second.docx")]
        visual = render_validate.VisualMetrics(
            1.0, 1.0, 1, 0, 0, 0, None, None, None, None
        )

        def export(source, output, mode):
            repeated = output.parent.name == "oracle-probe"
            name = source.stem + ("-repeat.pdf" if repeated else ".pdf")
            result = exports.get(name, pathlib.Path(name))
            if isinstance(result, Exception):
                raise result
            return result

        def fonts(pdf):
            if pdf.name in font_failures:
                raise ValueError("unlocked font")
            return []

        replacements = {
            "resolve_input_campaign": mock.Mock(return_value=(inputs, None)),
            "resolve_soffice_mode": mock.Mock(return_value="local"),
            "load_font_lock": mock.Mock(return_value={}),
            "reference_pdf_font_identities": mock.Mock(side_effect=fonts),
            "validate_pdf_font_identities": mock.Mock(),
            "render_libreoffice": mock.Mock(side_effect=export),
            "reference_page_digests": mock.Mock(
                side_effect=lambda pdf, **kwargs: digests.get(pdf.name, ["stable"])
            ),
            "render_rwml": mock.Mock(return_value={"warnings": []}),
            "text_recall": mock.Mock(return_value=1.0),
            "page_count": mock.Mock(return_value=1),
            "hash_similarity": mock.Mock(return_value=1.0),
            "compare_pdf_visuals": mock.Mock(return_value=visual),
        }
        argv = ["render_validate", "--json", "--max-skipped", "0"]
        if verify:
            argv.append("--verify-oracle")
        output = io.StringIO()
        with contextlib.ExitStack() as stack:
            stack.enter_context(mock.patch.object(sys, "argv", argv))
            for name, replacement in replacements.items():
                stack.enter_context(mock.patch.object(render_validate, name, replacement))
            stack.enter_context(contextlib.redirect_stdout(output))
            result = render_validate.main()
        return result, json.loads(output.getvalue()), replacements

    def assert_incomplete(self, result, report, name, reason):
        self.assertEqual(result, 1)
        self.assertFalse(report["gate"]["passed"])
        self.assertIsNone(report["summary"]["reference_stable"])
        self.assertEqual(report["summary"]["skipped"], 1)
        row = next(row for row in report["rows"] if row["document"] == name)
        self.assertEqual(row["status"], "skip")
        self.assertEqual(row["reason"], reason)
        self.assertNotIn("recall", row)

    def test_complete_matching_references_pass(self):
        result, report, calls = self.run_campaign()
        self.assertEqual(result, 0)
        self.assertTrue(report["summary"]["reference_stable"])
        self.assertEqual(report["summary"]["measured"], 2)
        self.assertEqual(calls["render_libreoffice"].call_count, 4)

    def test_missing_repeat_cannot_be_hidden_by_another_stable_case(self):
        for name in ("first", "second"):
            with self.subTest(case=name):
                result, report, _ = self.run_campaign(
                    exports={name + "-repeat.pdf": None}
                )
                self.assert_incomplete(
                    result, report, name + ".docx", "reference-repeat-unverified"
                )

    def test_repeat_dependency_failure_is_incomplete(self):
        result, report, _ = self.run_campaign(
            exports={
                "second-repeat.pdf": render_validate.RenderDependencyError("unavailable")
            }
        )
        self.assert_incomplete(
            result, report, "second.docx", "reference-repeat-unverified"
        )

    def test_unreadable_repeat_raster_is_incomplete_on_either_side(self):
        for name in ("first.pdf", "first-repeat.pdf"):
            with self.subTest(pdf=name):
                result, report, _ = self.run_campaign(digests={name: None})
                self.assert_incomplete(
                    result, report, "first.docx", "reference-repeat-unverified"
                )

    def test_unlocked_reference_font_makes_campaign_stability_unknown(self):
        for name in ("first.pdf", "second-repeat.pdf"):
            with self.subTest(pdf=name):
                result, report, _ = self.run_campaign(font_failures=(name,))
                self.assert_incomplete(
                    result,
                    report,
                    name.split("-")[0].removesuffix(".pdf") + ".docx",
                    "render failed",
                )

    def test_missing_initial_reference_makes_campaign_stability_unknown(self):
        result, report, _ = self.run_campaign(exports={"second.pdf": None})
        self.assert_incomplete(result, report, "second.docx", "render failed")

    def test_observed_instability_is_not_downgraded_to_unknown(self):
        result, report, _ = self.run_campaign(
            exports={"second-repeat.pdf": None},
            digests={"first-repeat.pdf": ["changed"]},
        )
        self.assertEqual(result, 1)
        self.assertIs(report["summary"]["reference_stable"], False)
        self.assertEqual(report["summary"]["unstable_references"], ["first.docx"])
        self.assertEqual(report["summary"]["skipped"], 1)

    def test_non_verifying_run_does_not_require_a_repeat(self):
        result, report, calls = self.run_campaign(
            exports={"second-repeat.pdf": None}, verify=False
        )
        self.assertEqual(result, 0)
        self.assertIsNone(report["summary"]["reference_stable"])
        self.assertEqual(calls["render_libreoffice"].call_count, 2)


class OraclePageDigestTests(unittest.TestCase):
    def digest(self, images, pages):
        with mock.patch.object(
            render_validate, "rasterize_pdf_pages", return_value=(images, pages)
        ):
            return render_validate.reference_page_digests(
                pathlib.Path("reference.pdf"), dpi=110, page_cap=2
            )

    def image(self, width=1, height=1):
        image = mock.Mock(width=width, height=height, mode="RGB")
        image.tobytes.return_value = b"\xff" * (width * height * 3)
        return image

    def test_page_cap_cannot_hide_unexamined_reference_pages(self):
        self.assertIsNone(self.digest([self.image()], 2))

    def test_no_pages_cannot_prove_repeatability(self):
        self.assertIsNone(self.digest([], 0))

    def test_page_dimensions_are_part_of_the_digest(self):
        self.assertNotEqual(
            self.digest([self.image(1, 2)], 1),
            self.digest([self.image(2, 1)], 1),
        )

    def test_complete_page_list_is_repeatable(self):
        images = [self.image(), self.image(2, 1)]
        first = self.digest(images, 2)
        self.assertEqual(len(first), 2)
        self.assertEqual(first, self.digest(images, 2))


if __name__ == "__main__":
    unittest.main()
