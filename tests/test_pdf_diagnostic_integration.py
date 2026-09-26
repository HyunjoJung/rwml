import copy
import importlib
import json
from pathlib import Path
import tempfile
import sys
import unittest
from unittest import mock

from scripts import render_oracle_contract as contract
from scripts import render_validate as render
from scripts import table_oracle_topology as topology
from tests.test_render_oracle_contract import (
    valid_core_report, valid_environment, valid_manifest, valid_pdf_row, write_manifest,
)


class PdfDiagnosticContractTests(unittest.TestCase):
    def test_all_harnesses_bind_the_pdf_diagnostic_module(self):
        for module_name, function_name in (
            ("libreoffice_table_capture", "harness_identity"),
            ("word_oracle_capture", "_harness_identity"),
        ):
            scripts = str(Path(__file__).resolve().parents[1] / "scripts")
            with mock.patch.object(sys, "path", [scripts, *sys.path]):
                module = importlib.import_module(module_name)
            with self.subTest(module=module_name):
                self.assertIn("render_pdf_diagnostics.py", getattr(module, function_name)())
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in ("render_validate.py", "table_oracle_topology.py",
                         "generate_unequal_table_oracle.py", "render_oracle_contract.py",
                         "render_evidence_metrics.py", "render_pdf_diagnostics.py"):
                (root / name).write_bytes(b"before")
            with (
                mock.patch.object(render, "__file__", str(root / "render_validate.py")),
                mock.patch.object(topology, "SCRIPT_PATH", root / "table_oracle_topology.py"),
            ):
                before = (render._harness_sha256(), topology._harness_sha256())
                (root / "render_pdf_diagnostics.py").write_bytes(b"after")
                for old, new in zip(before, (render._harness_sha256(), topology._harness_sha256())):
                    self.assertNotEqual(old, new)

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.corpus = contract.load_corpus_manifest(
            write_manifest(Path(temporary.name), valid_manifest())
        )

    def evidence(self):
        return contract.bind_evidence_report(
            valid_core_report(), self.corpus, valid_environment()
        )

    def test_strict_report_binds_complete_pdf_diagnostics(self):
        evidence = self.evidence()
        self.assertEqual(evidence["schema"], "rwml.render-oracle-evidence.v4")
        self.assertEqual(evidence["pdf_point_geometry"]["pages"], 1)
        self.assertEqual(evidence["semantic_text_metrics"]["semantic_exact_pages"], 1)
        self.assertEqual(evidence["text_geometry_metrics"]["word_boxes"]["matched_items"], 1)

    def test_strict_report_rejects_missing_rows_and_repaired_wrong_aggregates(self):
        for key in valid_pdf_row():
            with self.subTest(key=key):
                evidence = self.evidence()
                evidence["rows"][0].pop(key)
                with self.assertRaises(ValueError):
                    contract.validate_evidence_report(evidence, self.corpus)
                evidence = self.evidence()
                wrong = valid_pdf_row(2)[key]
                evidence[key] = wrong if key == "semantic_text_metrics" else wrong["summary"]
                with self.assertRaisesRegex(ValueError, "aggregate"):
                    contract.validate_evidence_report(evidence, self.corpus)

    def test_pdf_page_coverage_cannot_be_repaired_by_changing_the_aggregate(self):
        for key, wrong in valid_pdf_row(2).items():
            with self.subTest(key=key):
                evidence = self.evidence()
                evidence["rows"][0][key] = wrong
                evidence[key] = wrong if key == "semantic_text_metrics" else wrong["summary"]
                with self.assertRaisesRegex(ValueError, "page"):
                    contract.validate_evidence_report(evidence, self.corpus)

    def test_report_rejects_partial_diagnostics_and_explicitly_records_all_skips(self):
        row = render.ValidationRow(**valid_core_report()["rows"][0])
        for key in valid_pdf_row():
            with self.subTest(key=key):
                partial = copy.deepcopy(row)
                setattr(partial, key, None)
                with self.assertRaisesRegex(ValueError, "partial"):
                    render.validation_report([partial], 0.97, pdf_diagnostics=True)
        skipped = {
            key: value for key, value in valid_core_report()["rows"][0].items()
            if key in contract.SKIPPED_ROW_KEYS
        }
        skipped.update(status="skip", reason="render-failed")
        core = render.validation_report(
            [render.ValidationRow(**skipped)], 0.97,
            integer_metrics=True, pdf_diagnostics=True, thresholds={"max_skipped": 0},
        )
        evidence = contract.bind_evidence_report(core, self.corpus, valid_environment())
        self.assertFalse(evidence["gate"]["passed"])
        for key in valid_pdf_row():
            self.assertIsNone(evidence[key])


@unittest.skipIf(render.fitz is None or render.Image is None, "PDF/image libraries are required")
class PdfDiagnosticRenderingTests(unittest.TestCase):
    def test_page_geometry_and_duplicate_labels_survive_real_pdf_extraction(self):
        with render.fitz.open() as reference, render.fitz.open() as candidate:
            for document in (reference, candidate):
                page = document.new_page(width=144, height=120)
                page.insert_text((24, 40), "repeat", fontsize=10)
                page.insert_text((24, 65), "repeat", fontsize=10)
            candidate[0].set_cropbox(render.fitz.Rect(4, 0, 144, 120))
            candidate[0].set_rotation(90)
            report = render.compare_pdf_diagnostics(reference, candidate, 1)
        page = report["pdf_point_geometry"]["pages"][0]
        self.assertEqual(page["rotation_delta_degrees"], 90)
        self.assertEqual(page["delta_millipoints"]["crop_x0"], 4000)
        self.assertEqual(page["candidate"]["page_width_millipoints"], 120000)
        self.assertEqual(page["candidate"]["page_height_millipoints"], 140000)
        self.assertEqual(report["semantic_text_metrics"]["semantic_exact"], 1)
        words = report["text_geometry_metrics"]["summary"]["word_boxes"]
        self.assertEqual(words["reference_ambiguous_items"], 2)
        self.assertEqual(words["candidate_ambiguous_items"], 2)
        self.assertEqual(words["matched_items"], 0)

    def test_text_budget_is_shared_across_pages_and_sides_are_independent(self):
        with render.fitz.open() as reference, render.fitz.open() as candidate:
            for document in (reference, candidate):
                for _ in range(2):
                    page = document.new_page(width=144, height=96)
                    page.insert_text((24, 40), "alpha beta", fontsize=10)
            with mock.patch.object(render.pdf_metrics, "MAX_SEMANTIC_TOKENS", 3):
                with self.assertRaisesRegex(ValueError, "token limit"):
                    render.compare_pdf_diagnostics(reference, candidate, 2)
            with mock.patch.object(render.pdf_metrics, "MAX_SEMANTIC_TOKENS", 4):
                report = render.compare_pdf_diagnostics(reference, candidate, 2)
            self.assertEqual(report["semantic_text_metrics"]["semantic_token_matched_items"], 4)

    def test_invalid_word_records_are_rejected_without_partial_geometry(self):
        for record in (
            (0, 0, 10, 10, "text"),
            (0, 0, 10, 10, "text", False, 0, 0),
            (0, 0, 0, 10, "text", 0, 0, 0),
            (0, 0, float("inf"), 10, "text", 0, 0, 0),
        ):
            with self.subTest(record=record):
                page = mock.Mock()
                page.get_text.return_value = [record]
                with self.assertRaises(ValueError):
                    render.pymupdf_page_text_boxes(
                        page, max_items=8, max_codepoints=64, max_tokens=8
                    )

    def test_real_pdf_diagnostics_are_capped_content_free_and_opt_in(self):
        with tempfile.TemporaryDirectory() as temporary:
            reference = Path(temporary) / "reference.pdf"
            candidate = Path(temporary) / "candidate.pdf"
            for path, offset in ((reference, 0), (candidate, 2)):
                with render.fitz.open() as document:
                    for text in ("Alpha", "Beta"):
                        page = document.new_page(width=144, height=96)
                        page.insert_text((24 + offset, 40), text, fontsize=10)
                    document.save(path)
            options = dict(dpi=72, foreground_threshold=245, ahash_size=16)
            full = render.compare_pdf_visuals(
                reference, candidate, **options, page_cap=2,
                integer_metrics=True, pdf_diagnostics=True,
            )
            self.assertEqual(full.pdf_point_geometry["summary"]["pages"], 2)
            self.assertEqual(full.semantic_text_metrics["semantic_exact_pages"], 2)
            boxes = full.text_geometry_metrics["summary"]["word_boxes"]
            self.assertEqual(boxes["matched_items"], 2)
            self.assertEqual(boxes["exact_delta_summaries_millipoints"]["x_min"]["sum"], 4000)
            serialized = json.dumps(vars(full), sort_keys=True)
            self.assertNotIn("Alpha", serialized)
            self.assertNotIn("Beta", serialized)
            again = render.compare_pdf_visuals(
                reference, candidate, **options, page_cap=2,
                integer_metrics=True, pdf_diagnostics=True,
            )
            self.assertEqual(full, again)
            capped = render.compare_pdf_visuals(
                reference, candidate, **options, page_cap=1, pdf_diagnostics=True,
            )
            self.assertEqual(capped.pdf_point_geometry["summary"]["pages"], 1)
            self.assertEqual(capped.semantic_text_metrics["pages"], 1)
            self.assertEqual(capped.text_geometry_metrics["summary"]["pages"], 1)
            self.assertEqual(capped.capped_matched_pages, 1)
            with mock.patch.object(
                render, "pymupdf_page_geometry", side_effect=AssertionError("legacy PDF diagnostics")
            ):
                legacy = render.compare_pdf_visuals(reference, candidate, **options, page_cap=2)
            self.assertIsNone(legacy.pdf_point_geometry)
            self.assertIsNone(legacy.semantic_text_metrics)
            self.assertIsNone(legacy.text_geometry_metrics)

    def test_word_record_limits_are_validated_before_extraction(self):
        page = mock.Mock()
        page.get_text.return_value = []
        for options in (
            dict(max_items=0, max_codepoints=10, max_tokens=10),
            dict(max_items=10, max_codepoints=-1, max_tokens=10),
            dict(max_items=10, max_codepoints=10, max_tokens=True),
        ):
            with self.subTest(options=options):
                with self.assertRaises(ValueError):
                    render.pymupdf_page_text_boxes(page, **options)
        page.get_text.assert_not_called()


if __name__ == "__main__":
    unittest.main()
