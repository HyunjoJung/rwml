import copy
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from scripts import render_evidence_metrics as metrics
from scripts import render_oracle_contract as contract
from scripts import render_validate as render
from scripts import table_oracle_topology as topology
from tests.test_render_oracle_contract import (
    valid_core_report,
    valid_environment,
    valid_manifest,
    write_manifest,
)


class IntegerMetricContractTests(unittest.TestCase):
    def test_topology_harness_digest_changes_with_transitive_metric_code(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in (
                "table_oracle_topology.py", "generate_unequal_table_oracle.py",
                "render_oracle_contract.py", "render_evidence_metrics.py",
                "render_pdf_diagnostics.py",
            ):
                (root / name).write_bytes(b"before")
            with mock.patch.object(topology, "SCRIPT_PATH", root / "table_oracle_topology.py"):
                before = topology._harness_sha256()
                (root / "render_evidence_metrics.py").write_bytes(b"after")
                self.assertNotEqual(before, topology._harness_sha256())

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

    def test_strict_evidence_binds_integer_schema_and_preserves_raw_counts(self):
        evidence = self.evidence()
        self.assertEqual(evidence["schema"], "rwml.render-oracle-evidence.v4")
        self.assertEqual(evidence["integer_visual_metrics"]["pixels"], 1)
        contract.validate_evidence_report(evidence, self.corpus)

    def test_strict_evidence_rejects_missing_metrics_and_old_schema(self):
        original = self.evidence()
        mutations = (
            lambda value: value.pop("integer_visual_metrics"),
            lambda value: value["rows"][0].pop("integer_visual_metrics"),
            lambda value: value["visual_comparison"].pop("integer_metrics"),
            lambda value: value.update(schema="rwml.render-oracle-evidence.v1"),
        )
        for index, mutate in enumerate(mutations):
            with self.subTest(index=index):
                evidence = copy.deepcopy(original)
                mutate(evidence)
                with self.assertRaises(ValueError):
                    contract.validate_evidence_report(evidence, self.corpus)

    def test_strict_evidence_rejects_repaired_but_wrong_aggregate(self):
        evidence = self.evidence()
        evidence["integer_visual_metrics"] = metrics.image_metrics(
            b"\xff\xff\xff", b"\x00\x00\x00", 1, 1
        )
        with self.assertRaisesRegex(ValueError, "aggregate"):
            contract.validate_evidence_report(evidence, self.corpus)

    def test_strict_evidence_binds_metric_pages_to_compared_pages(self):
        evidence = self.evidence()
        wrong = metrics.aggregate_metrics([evidence["integer_visual_metrics"]] * 2)
        evidence["rows"][0]["integer_visual_metrics"] = wrong
        evidence["integer_visual_metrics"] = copy.deepcopy(wrong)
        with self.assertRaisesRegex(ValueError, "page"):
            contract.validate_evidence_report(evidence, self.corpus)

    def test_strict_evidence_binds_numpy_implementation_to_environment(self):
        for implementation, include_numpy in (
            ("numpy-integer-exact-v1", False),
            ("python-integer-reference-v1", True),
        ):
            with self.subTest(implementation=implementation):
                evidence = self.evidence()
                evidence["visual_comparison"]["integer_metrics"]["implementation"] = implementation
                tools = [tool for tool in evidence["environment"]["tools"]
                         if tool["name"] != "numpy"]
                if include_numpy:
                    tools.insert(0, {"name": "numpy", "version": "2.4.4"})
                evidence["environment"]["tools"] = tools
                with self.assertRaisesRegex(ValueError, "implementation.*environment"):
                    contract.validate_evidence_report(evidence, self.corpus)

    def test_all_skipped_report_requires_unmeasured_integer_aggregate(self):
        row = valid_core_report()["rows"][0]
        row = {key: value for key, value in row.items()
               if key in contract.SKIPPED_ROW_KEYS}
        row.update(status="skip", reason="render-failed")
        core = render.validation_report(
            [render.ValidationRow(**row)], 0.97, integer_metrics=True,
            pdf_diagnostics=True,
            thresholds={"max_skipped": 0},
        )
        evidence = contract.bind_evidence_report(core, self.corpus, valid_environment())
        self.assertIsNone(evidence["integer_visual_metrics"])
        self.assertFalse(evidence["gate"]["passed"])
        evidence["integer_visual_metrics"] = valid_core_report()["integer_visual_metrics"]
        with self.assertRaisesRegex(ValueError, "aggregate"):
            contract.validate_evidence_report(evidence, self.corpus)


@unittest.skipIf(render.Image is None, "Pillow is required")
class IntegerMetricIntegrationTests(unittest.TestCase):
    @unittest.skipIf(render.fitz is None, "PyMuPDF is required")
    def test_real_pdf_metrics_are_pixel_weighted_capped_and_opt_in(self):
        with tempfile.TemporaryDirectory() as temporary:
            reference = Path(temporary) / "reference.pdf"
            candidate = Path(temporary) / "candidate.pdf"
            for path, page_count in ((reference, 2), (candidate, 3)):
                with render.fitz.open() as document:
                    for index in range(page_count):
                        page = document.new_page(width=16 * (index + 1), height=16)
                        if path == candidate and index == 0:
                            page.draw_rect(page.rect, color=None, fill=(0, 0, 0))
                    document.save(path)

            options = dict(dpi=72, foreground_threshold=245, ahash_size=16)
            full = render.compare_pdf_visuals(
                reference, candidate, **options, page_cap=2, integer_metrics=True
            )
            self.assertEqual(full.integer_visual_metrics["pages"], 2)
            self.assertEqual(full.integer_visual_metrics["pixels"], 768)
            self.assertEqual(full.integer_visual_metrics["changed_pixels"], 256)
            self.assertEqual(full.integer_visual_metrics["absolute_error_sum"], 195840)
            self.assertEqual(full.integer_visual_metrics["mismatch_ppm"], 333333)
            self.assertEqual(full.integer_visual_metrics["similarity_ppm"], 666667)
            self.assertEqual(full.unmatched_candidate_pages, 1)
            self.assertEqual(full.capped_matched_pages, 0)
            again = render.compare_pdf_visuals(
                reference, candidate, **options, page_cap=2, integer_metrics=True
            )
            self.assertEqual(full, again)

            capped = render.compare_pdf_visuals(
                reference, candidate, **options, page_cap=1, integer_metrics=True
            )
            self.assertEqual(capped.integer_visual_metrics["pixels"], 256)
            self.assertEqual(capped.integer_visual_metrics["pages"], 1)
            self.assertEqual(capped.integer_visual_metrics["similarity_ppm"], 0)
            self.assertEqual(capped.capped_matched_pages, 1)
            self.assertEqual(capped.unmatched_candidate_pages, 1)
            with mock.patch.object(
                render, "integer_image_metrics", side_effect=AssertionError("legacy metric work")
            ):
                legacy = render.compare_pdf_visuals(
                    reference, candidate, **options, page_cap=2
                )
            self.assertIsNone(legacy.integer_visual_metrics)
            self.assertEqual(legacy.compared_pages, full.compared_pages)
            self.assertEqual(legacy.foreground_ink_iou, full.foreground_ink_iou)

    def compare(self, *, enabled):
        reference = render.Image.new("RGB", (2, 1), "white")
        candidate = render.Image.new("RGB", (2, 1), "black")
        return render.compare_page_images(
            [reference], [candidate], page_cap=1, foreground_threshold=245,
            ahash_size=16, integer_metrics=enabled,
        )

    def test_diagnostic_comparison_preserves_raw_integer_measurements(self):
        visual = self.compare(enabled=True)
        self.assertEqual(visual.integer_visual_metrics["pixels"], 2)
        self.assertEqual(visual.integer_visual_metrics["absolute_error_sum"], 1530)
        self.assertEqual(visual.integer_visual_metrics["similarity_ppm"], 0)

    def test_legacy_comparison_does_not_run_integer_metric_loops(self):
        with mock.patch.object(
            render, "integer_image_metrics", side_effect=AssertionError("legacy metric work")
        ):
            visual = self.compare(enabled=False)
        self.assertIsNone(visual.integer_visual_metrics)

    def test_report_requires_complete_diagnostic_rows_but_keeps_legacy_shape(self):
        visual = self.compare(enabled=True)
        row = render.ValidationRow(
            document="example.docx", status="pass", recall=1.0, compared_pages=1,
            integer_visual_metrics=visual.integer_visual_metrics,
        )
        report = render.validation_report([row], 0.97, integer_metrics=True)
        self.assertEqual(report["integer_visual_metrics"], visual.integer_visual_metrics)
        self.assertEqual(report["visual_comparison"]["integer_metrics"], metrics.metric_contract())
        without = copy.copy(row)
        without.integer_visual_metrics = None
        with self.assertRaisesRegex(ValueError, "integer visual evidence"):
            render.validation_report([row, without], 0.97, integer_metrics=True)
        legacy = render.validation_report([without], 0.97)
        self.assertNotIn("integer_visual_metrics", legacy)
        self.assertNotIn("integer_metrics", legacy["visual_comparison"])

    def test_empty_diagnostic_report_has_explicit_unmeasured_aggregate(self):
        row = render.ValidationRow(document="example.docx", status="skip")
        report = render.validation_report([row], 0.97, integer_metrics=True)
        self.assertIsNone(report["integer_visual_metrics"])
        self.assertIn("integer_metrics", report["visual_comparison"])


if __name__ == "__main__":
    unittest.main()
