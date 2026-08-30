#!/usr/bin/env python3

import json
import unittest

from scripts import render_pdf_diagnostics as diagnostics


class PdfPointGeometryTests(unittest.TestCase):
    @staticmethod
    def geometry(
        *,
        width=612,
        height=792,
        media=(-1, -2, 611, 790),
        crop=(0, 0, 612, 792),
        rotation=0,
    ):
        return diagnostics.canonical_page_geometry(
            page_size=(width, height),
            media_box=media,
            crop_box=crop,
            rotation_degrees=rotation,
        )

    def test_canonical_geometry_rounds_half_away_and_keeps_negative_origins(self):
        geometry = self.geometry(
            width="612.0005",
            height="791.9995",
            media=("-1.0005", "-2.0005", "611.0005", "790.0005"),
        )

        self.assertEqual(geometry["page_width_millipoints"], 612_001)
        self.assertEqual(geometry["page_height_millipoints"], 792_000)
        self.assertEqual(geometry["media_x0_millipoints"], -1_001)
        self.assertEqual(geometry["media_y0_millipoints"], -2_001)

    def test_page_metrics_retain_exact_deltas_without_text(self):
        reference = self.geometry()
        candidate = self.geometry(
            width="612.0005",
            media=(-1, -2, "611.001", 790),
            crop=(1, 0, 613, 792),
            rotation=90,
        )

        page = diagnostics.page_geometry_metrics(reference, candidate)

        self.assertEqual(page["delta_millipoints"]["page_width"], 1)
        self.assertEqual(page["delta_millipoints"]["media_x1"], 1)
        self.assertEqual(page["delta_millipoints"]["crop_x0"], 1_000)
        self.assertEqual(page["rotation_delta_degrees"], 90)
        diagnostics.validate_page_geometry_metrics(page)

    def test_geometry_aggregate_recomputes_per_axis_summaries(self):
        reference = self.geometry()
        first = diagnostics.page_geometry_metrics(
            reference,
            self.geometry(width="612.001", crop=(1, 0, 613, 792)),
        )
        second = diagnostics.page_geometry_metrics(
            reference,
            self.geometry(width="611.998", crop=(-2, 0, 610, 792), rotation=90),
        )

        report = diagnostics.geometry_report([first, second])

        self.assertEqual(report["summary"]["pages"], 2)
        self.assertEqual(report["summary"]["point_mismatched_pages"], 2)
        self.assertEqual(report["summary"]["rotation_mismatched_pages"], 1)
        self.assertEqual(report["summary"]["max_abs_delta_millipoints"], 2_000)
        width = report["summary"]["delta_summaries_millipoints"]["page_width"]
        self.assertEqual(width["sum"], -1)
        self.assertEqual(width["min"], -2)
        self.assertEqual(width["max"], 1)
        self.assertEqual(width["nonzero_pages"], 2)
        diagnostics.validate_geometry_report(report)

    def test_geometry_rejects_degenerate_boxes_and_invalid_rotation(self):
        with self.assertRaisesRegex(ValueError, "media box"):
            self.geometry(media=(0, 0, 0, 10))
        with self.assertRaisesRegex(ValueError, "rotation"):
            self.geometry(rotation=45)
        with self.assertRaisesRegex(ValueError, "page size"):
            diagnostics.canonical_page_geometry(
                page_size=612,
                media_box=(0, 0, 612, 792),
                crop_box=(0, 0, 612, 792),
                rotation_degrees=0,
            )


class PdfSemanticMetricTests(unittest.TestCase):
    def test_normalization_is_nfc_and_drops_layout_direction_controls(self):
        tokens = diagnostics.normalize_semantic_tokens(
            "Cafe\u0301 \u200fשלום \u00adtest",
            max_codepoints=64,
            max_tokens=8,
        )

        self.assertEqual(tokens, ("Café", "שלום", "test"))

    def test_semantic_counts_distinguish_content_from_order(self):
        reference = ("alpha", "beta")
        candidate = ("beta", "alpha")

        evidence = diagnostics.semantic_metrics(reference, candidate)

        self.assertEqual(evidence["semantic_exact"], 0)
        self.assertEqual(evidence["semantic_token_f1_ppm"], 1_000_000)
        self.assertEqual(evidence["semantic_codepoint_f1_ppm"], 1_000_000)
        self.assertEqual(evidence["semantic_bigram_f1_ppm"], 0)
        serialized = json.dumps(evidence, sort_keys=True)
        self.assertNotIn("alpha", serialized)
        self.assertNotIn("beta", serialized)
        diagnostics.validate_semantic_metrics(evidence)

    def test_diagnostic_contract_discloses_limits_and_retains_no_content(self):
        contract = diagnostics.diagnostic_contract()

        self.assertEqual(contract["schema"], "rwml.pdf-diagnostics.v1")
        self.assertFalse(contract["content_retained"])
        self.assertEqual(contract["point_unit"], "millipoints")
        self.assertEqual(contract["delta_direction"], "candidate-minus-reference")
        diagnostics.validate_diagnostic_contract(contract)

    def test_empty_and_one_sided_empty_semantics_are_explicit(self):
        both_empty = diagnostics.semantic_metrics((), ())
        one_sided = diagnostics.semantic_metrics((), ("text",))

        self.assertEqual(both_empty["semantic_comparable"], 1)
        self.assertEqual(both_empty["semantic_token_f1_ppm"], 1_000_000)
        self.assertEqual(one_sided["semantic_comparable"], 0)
        self.assertEqual(one_sided["semantic_one_sided_empty"], 1)
        self.assertEqual(one_sided["semantic_token_f1_ppm"], 0)

    def test_semantic_report_aggregates_pages_from_raw_counts(self):
        exact = diagnostics.semantic_metrics(("a", "b"), ("a", "b"))
        changed = diagnostics.semantic_metrics(("x",), ("y",))

        report = diagnostics.semantic_report([exact, changed])

        self.assertEqual(report["pages"], 2)
        self.assertEqual(report["semantic_exact_pages"], 1)
        self.assertEqual(report["semantic_page_mismatches"], 1)
        self.assertEqual(report["semantic_token_reference_items"], 3)
        self.assertEqual(report["semantic_token_candidate_items"], 3)
        self.assertEqual(report["semantic_token_matched_items"], 2)
        self.assertEqual(report["semantic_token_f1_ppm"], 666_667)
        diagnostics.validate_semantic_report(report)

    def test_semantic_limits_and_tampering_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "raw codepoint limit"):
            diagnostics.normalize_semantic_tokens(
                "abcdef", max_codepoints=1, max_tokens=8
            )
        with self.assertRaisesRegex(ValueError, "token limit"):
            diagnostics.normalize_semantic_tokens(
                "a b c", max_codepoints=8, max_tokens=2
            )

        evidence = diagnostics.semantic_metrics(("a",), ("a",))
        evidence["semantic_token_f1_ppm"] = 0
        with self.assertRaisesRegex(ValueError, "semantic_token_f1_ppm"):
            diagnostics.validate_semantic_metrics(evidence)


if __name__ == "__main__":
    unittest.main()
