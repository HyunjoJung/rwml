#!/usr/bin/env python3

import json
import copy
import unittest
from unittest import mock

from scripts import render_pdf_diagnostics as diagnostics


class PdfDiagnosticConsistencyTests(unittest.TestCase):
    def test_bounded_sequences_do_not_drain_iterators_past_the_limit(self):
        def records():
            yield 1
            yield 2
            yield 3
            raise AssertionError("unbounded iteration")

        with self.assertRaises(ValueError):
            diagnostics.canonical_page_geometry(
                page_size=records(), media_box=(0, 0, 10, 10),
                crop_box=(0, 0, 10, 10), rotation_degrees=0,
            )
        with mock.patch.object(diagnostics, "MAX_SEMANTIC_TOKENS", 2):
            with self.assertRaises(ValueError):
                diagnostics.semantic_metrics(records(), ())

    def test_histogram_counts_bound_the_exact_sum(self):
        reference = [
            diagnostics.canonical_text_box((str(index),), (0, 0, 10, 10))
            for index in range(3)
        ]
        candidate = [
            diagnostics.canonical_text_box((str(index),), (delta, 0, 10 + delta, 10))
            for index, delta in enumerate((0, 0.001, 0.002))
        ]
        evidence = diagnostics.unique_text_geometry_metrics(reference, candidate)
        evidence["exact_delta_summaries_millipoints"]["x_min"]["sum"] = 2
        with self.assertRaises(ValueError):
            diagnostics.validate_unique_text_geometry_metrics(evidence)

    def test_contract_requires_exact_types_at_every_depth(self):
        for key, value in (
            ("content_retained", 0),
            ("max_point_magnitude", 1_000_000.0),
            ("max_semantic_tokens", 250_000.0),
        ):
            with self.subTest(key=key):
                contract = diagnostics.diagnostic_contract()
                contract[key] = value
                with self.assertRaises(ValueError):
                    diagnostics.validate_diagnostic_contract(contract)
        contract = diagnostics.diagnostic_contract()
        contract["text_geometry_histogram_millipoints"]["exact_absolute_limit"] = 2.0
        with self.assertRaises(ValueError):
            diagnostics.validate_diagnostic_contract(contract)

    def test_page_geometry_deltas_cannot_be_boolean_or_float(self):
        geometry = PdfPointGeometryTests.geometry()
        original = diagnostics.page_geometry_metrics(geometry, geometry)
        for value in (False, 0.0):
            with self.subTest(value=value):
                page = copy.deepcopy(original)
                page["delta_millipoints"]["page_width"] = value
                with self.assertRaises(ValueError):
                    diagnostics.validate_page_geometry_metrics(page)
                page = copy.deepcopy(original)
                page["rotation_delta_degrees"] = value
                with self.assertRaises(ValueError):
                    diagnostics.validate_page_geometry_metrics(page)

    def test_exact_semantics_cannot_contradict_raw_matches(self):
        changed = diagnostics.semantic_metrics(("alpha",), ("beta",))
        changed["semantic_exact"] = 1
        with self.assertRaises(ValueError):
            diagnostics.validate_semantic_metrics(changed)
        report = diagnostics.semantic_report([
            diagnostics.semantic_metrics(("alpha",), ("beta",))
        ])
        report.update(semantic_exact=1, semantic_exact_pages=1, semantic_page_mismatches=0)
        with self.assertRaises(ValueError):
            diagnostics.validate_semantic_report(report)

    def test_semantic_comparability_must_partition_all_pages(self):
        report = diagnostics.semantic_report([
            diagnostics.semantic_metrics(("alpha",), ("alpha",))
        ])
        report.update(semantic_comparable=0, semantic_comparable_pages=0)
        with self.assertRaises(ValueError):
            diagnostics.validate_semantic_report(report)

    def test_text_geometry_sum_and_overflow_must_agree_with_bounds(self):
        box = diagnostics.canonical_text_box(("fixture",), (0, 0, 10, 10))
        original = diagnostics.unique_text_geometry_metrics([box], [box])
        for key, value in (("sum", 1), ("positive_overflow_items", 1)):
            with self.subTest(key=key):
                evidence = copy.deepcopy(original)
                evidence["exact_delta_summaries_millipoints"]["x_min"][key] = value
                with self.assertRaises(ValueError):
                    diagnostics.validate_unique_text_geometry_metrics(evidence)

    def test_text_geometry_extrema_must_have_histogram_coverage(self):
        box = diagnostics.canonical_text_box(("fixture",), (0, 0, 10, 10))
        evidence = diagnostics.unique_text_geometry_metrics([box], [box])
        evidence["exact_delta_summaries_millipoints"]["x_min"].update(
            min=1000, max=1000, sum=1000
        )
        with self.assertRaises(ValueError):
            diagnostics.validate_unique_text_geometry_metrics(evidence)


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

        self.assertEqual(contract["schema"], "rwml.pdf-diagnostics.v2")
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


class UniqueTextGeometryTests(unittest.TestCase):
    def test_signed_bucket_boundaries_and_aggregation_are_consistent(self):
        deltas = [0]
        for value in (1, 2, 3, 749, 750, 1000, 1001, 2999, 3000,
                      4999, 5000, 8999, 9000, 10000, 10001, 1000000):
            deltas.extend((-value, value))
        reports = []
        for index, delta in enumerate(deltas):
            with self.subTest(delta=delta):
                bucket = diagnostics._text_geometry_bucket(delta)
                low, high = diagnostics._text_geometry_bucket_bounds(bucket)
                self.assertLessEqual(low, delta)
                self.assertGreaterEqual(high, delta)
                reference = self.box(str(index), (0, 0, 10, 10))
                shift = delta / 1000
                candidate = self.box(str(index), (shift, shift, 10 + shift, 10 + shift))
                page = diagnostics.text_geometry_page(
                    [reference], [candidate], [reference], [candidate]
                )
                reports.append(diagnostics.text_geometry_report([page]))
        aggregate = diagnostics.aggregate_text_geometry_reports(reports)
        self.assertEqual(aggregate["word_boxes"]["matched_items"], len(deltas))
        self.assertEqual(
            aggregate["word_boxes"]["exact_delta_summaries_millipoints"]["x_min"]["sum"], 0
        )
        diagnostics.validate_text_geometry_summary(aggregate)
    @staticmethod
    def box(token, bbox):
        return diagnostics.canonical_text_box((token,), bbox)

    def test_unique_tokens_match_without_retaining_content(self):
        reference = [self.box("private-label", (0, 0, 10, 10))]
        candidate = [self.box("private-label", (0.001, 0.002, 10.003, 10.004))]

        evidence = diagnostics.unique_text_geometry_metrics(reference, candidate)

        self.assertEqual(evidence["matched_items"], 1)
        self.assertEqual(evidence["candidate_unique_items"], 1)
        self.assertEqual(evidence["reference_unique_items"], 1)
        self.assertEqual(evidence["precision_ppm"], 1_000_000)
        self.assertEqual(
            evidence["exact_delta_summaries_millipoints"]["x_min"]["sum"], 1
        )
        self.assertEqual(
            evidence["exact_delta_summaries_millipoints"]["center_x"]["sum"],
            2,
        )
        serialized = json.dumps(evidence, sort_keys=True)
        self.assertNotIn("private-label", serialized)
        diagnostics.validate_unique_text_geometry_metrics(evidence)

    def test_repeated_labels_are_ambiguous_and_never_greedily_paired(self):
        reference = [
            self.box("repeat", (0, 0, 10, 10)),
            self.box("repeat", (0, 20, 10, 30)),
        ]
        candidate = [self.box("repeat", (0, 0, 10, 10))]

        evidence = diagnostics.unique_text_geometry_metrics(reference, candidate)

        self.assertEqual(evidence["matched_items"], 0)
        self.assertEqual(evidence["reference_ambiguous_items"], 2)
        self.assertEqual(evidence["candidate_ambiguous_items"], 0)
        self.assertEqual(evidence["candidate_unmatched_unique_items"], 1)
        self.assertEqual(evidence["precision_ppm"], 0)
        self.assertEqual(evidence["recall_ppm"], 0)

    def test_histograms_are_bounded_and_report_aggregation_recomputes_counts(self):
        exact = diagnostics.text_geometry_page(
            [self.box("word-a", (0, 0, 10, 10))],
            [self.box("word-a", (0, 0, 10, 10))],
            [self.box("line-a", (0, 0, 20, 10))],
            [self.box("line-a", (0, 0, 20, 10))],
        )
        shifted = diagnostics.text_geometry_page(
            [self.box("word-b", (0, 0, 10, 10))],
            [self.box("word-b", (0.003, 0, 10.003, 10))],
            [self.box("line-b", (0, 0, 20, 10))],
            [self.box("line-b", (0.003, 0, 20.003, 10))],
        )

        report = diagnostics.text_geometry_report([exact, shifted])

        self.assertEqual(report["summary"]["pages"], 2)
        self.assertEqual(report["summary"]["word_boxes"]["matched_items"], 2)
        histogram = report["summary"]["word_boxes"][
            "delta_histograms_millipoints"
        ]["x_min"]
        self.assertEqual(
            histogram,
            [
                {"delta_millipoints": 0, "count": 1},
                {"delta_millipoints": 500, "count": 1},
            ],
        )
        self.assertLessEqual(len(histogram), diagnostics.MAX_TEXT_GEOMETRY_BUCKETS)
        diagnostics.validate_text_geometry_report(report)

    def test_text_box_item_limits_are_hard(self):
        box = self.box("one", (0, 0, 10, 10))
        with self.assertRaisesRegex(ValueError, "item limit"):
            diagnostics.unique_text_geometry_metrics(
                [box, box], [box], max_items=1
            )


if __name__ == "__main__":
    unittest.main()
