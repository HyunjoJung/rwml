#!/usr/bin/env python3

import random
import unittest

from scripts import render_evidence_metrics as metrics


class IntegerVisualMetricTests(unittest.TestCase):
    def test_identical_blank_pixel_is_exact_and_empty_masks_are_perfect(self):
        evidence = metrics.image_metrics(b"\xff\xff\xff", b"\xff\xff\xff", 1, 1)

        self.assertEqual(evidence["pages"], 1)
        self.assertEqual(evidence["pixels"], 1)
        self.assertEqual(evidence["changed_pixels"], 0)
        self.assertEqual(evidence["similarity_ppm"], 1_000_000)
        self.assertEqual(evidence["foreground_f1_ppm"], 1_000_000)
        self.assertEqual(evidence["edge_f1_ppm"], 1_000_000)
        self.assertEqual(evidence["text_ink_f1_ppm"], 1_000_000)
        self.assertEqual(evidence["blurred_luma_similarity_ppm"], 1_000_000)
        self.assertEqual(
            evidence["metric_work_units"], metrics.METRIC_WORK_UNITS_PER_PIXEL
        )

    def test_black_candidate_against_blank_reference_has_exact_integer_error(self):
        evidence = metrics.image_metrics(b"\xff\xff\xff", b"\x00\x00\x00", 1, 1)

        self.assertEqual(evidence["changed_pixels"], 1)
        self.assertEqual(evidence["mismatch_ppm"], 1_000_000)
        self.assertEqual(evidence["absolute_error_sum"], 765)
        self.assertEqual(evidence["squared_error_sum"], 195_075)
        self.assertEqual(evidence["max_channel_delta"], 255)
        self.assertEqual(evidence["mean_absolute_error_ppm"], 1_000_000)
        self.assertEqual(evidence["root_mean_square_error_ppm"], 1_000_000)
        self.assertEqual(evidence["foreground_candidate_pixels"], 1)
        self.assertEqual(evidence["foreground_reference_pixels"], 0)
        self.assertEqual(evidence["foreground_precision_ppm"], 0)
        self.assertEqual(evidence["foreground_recall_ppm"], 0)
        self.assertEqual(evidence["foreground_f1_ppm"], 0)

    def test_one_pixel_matching_is_tolerant_but_not_two_pixels(self):
        white = b"\xff\xff\xff"
        black = b"\x00\x00\x00"
        reference = white + black + white + white + white
        one_pixel = white + white + black + white + white
        two_pixels = white + white + white + black + white

        near = metrics.image_metrics(reference, one_pixel, 5, 1)
        far = metrics.image_metrics(reference, two_pixels, 5, 1)

        self.assertEqual(near["foreground_precision_ppm"], 1_000_000)
        self.assertEqual(near["foreground_recall_ppm"], 1_000_000)
        self.assertEqual(near["foreground_f1_ppm"], 1_000_000)
        self.assertEqual(far["foreground_precision_ppm"], 0)
        self.assertEqual(far["foreground_recall_ppm"], 0)
        self.assertEqual(far["foreground_f1_ppm"], 0)

    def test_metric_work_limit_is_checked_before_metric_loops(self):
        image = b"\xff\xff\xff" * 2
        self.assertEqual(
            metrics.image_metrics(
                image,
                image,
                2,
                1,
                max_metric_work_units=2 * metrics.METRIC_WORK_UNITS_PER_PIXEL,
            )["pixels"],
            2,
        )
        with self.assertRaisesRegex(ValueError, "metric work limit"):
            metrics.image_metrics(
                image,
                image,
                2,
                1,
                max_metric_work_units=2 * metrics.METRIC_WORK_UNITS_PER_PIXEL - 1,
            )

    def test_aggregate_metrics_recomputes_weighted_integer_ratios(self):
        exact = metrics.image_metrics(b"\xff\xff\xff", b"\xff\xff\xff", 1, 1)
        changed = metrics.image_metrics(b"\xff\xff\xff", b"\x00\x00\x00", 1, 1)

        aggregate = metrics.aggregate_metrics([exact, changed])

        self.assertEqual(aggregate["pages"], 2)
        self.assertEqual(aggregate["pixels"], 2)
        self.assertEqual(aggregate["changed_pixels"], 1)
        self.assertEqual(aggregate["mismatch_ppm"], 500_000)
        self.assertEqual(aggregate["mean_absolute_error_ppm"], 500_000)
        self.assertEqual(aggregate["similarity_ppm"], 500_000)
        self.assertEqual(aggregate["max_channel_delta"], 255)
        self.assertEqual(aggregate["foreground_f1_ppm"], 0)
        self.assertEqual(aggregate["edge_f1_ppm"], 1_000_000)
        self.assertEqual(aggregate["blurred_luma_similarity_ppm"], 500_000)
        metrics.validate_metrics(aggregate)

    def test_python_and_numpy_implementations_are_exactly_equivalent(self):
        if metrics.numpy_module() is None:
            self.skipTest("NumPy is not installed")
        random_source = random.Random(0x52574D4C)
        for width, height in ((1, 1), (1, 4), (4, 1), (2, 2), (9, 7)):
            with self.subTest(width=width, height=height):
                reference = bytes(
                    random_source.randrange(256) for _ in range(width * height * 3)
                )
                candidate = bytes(
                    random_source.randrange(256) for _ in range(width * height * 3)
                )
                self.assertEqual(
                    metrics.image_metrics_python(reference, candidate, width, height),
                    metrics.image_metrics_numpy(reference, candidate, width, height),
                )

    def test_validator_rejects_missing_and_out_of_range_fields(self):
        evidence = metrics.image_metrics(b"\xff\xff\xff", b"\xff\xff\xff", 1, 1)
        missing = dict(evidence)
        missing.pop("pixels")
        with self.assertRaisesRegex(ValueError, "metric keys"):
            metrics.validate_metrics(missing)

        invalid = dict(evidence)
        invalid["similarity_ppm"] = 1_000_001
        with self.assertRaisesRegex(ValueError, "similarity_ppm"):
            metrics.validate_metrics(invalid)


if __name__ == "__main__":
    unittest.main()
