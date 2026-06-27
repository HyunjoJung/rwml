import importlib.util
import pathlib
import sys
import tempfile
import unittest


SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "scripts" / "bench_vs_mature.py"
SPEC = importlib.util.spec_from_file_location("bench_vs_mature", SCRIPT)
bench_vs_mature = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = bench_vs_mature
SPEC.loader.exec_module(bench_vs_mature)


class BenchVsMatureReportTests(unittest.TestCase):
    def test_clean_golden_removes_bom_and_logging_noise(self):
        text = (
            "\ufeffFirst token\n"
            "ERROR StatusLogger Log4j2 could not find a logging implementation\n"
            "Second token\n"
        )

        self.assertEqual(
            bench_vs_mature.clean_golden(text),
            "First token\nSecond token",
        )

    def test_benchmark_report_summarizes_rows_with_release_metadata(self):
        rows = [
            {
                "file": "alpha",
                "poi_recall": 1.0,
                "poi_prec": 0.8,
                "poi_f1": 0.888888,
                "lo_recall": 0.75,
                "lo_prec": 0.6,
                "lo_f1": 0.666666,
            },
            {"file": "broken", "rdoc": "ERROR"},
            {
                "file": "beta",
                "poi_recall": 0.5,
                "poi_prec": 1.0,
                "poi_f1": 0.666666,
            },
        ]

        report = bench_vs_mature.benchmark_report(
            rows,
            version="0.1.0",
            git_rev="abc123",
        )

        self.assertEqual(report["schema"], "rdoc.benchmark-report.v1")
        self.assertEqual(report["benchmark"], "extract-vs-mature")
        self.assertEqual(report["version"], "0.1.0")
        self.assertEqual(report["git_rev"], "abc123")
        self.assertEqual(report["summary"]["files"], 3)
        self.assertEqual(report["summary"]["scored"], 2)
        self.assertEqual(report["summary"]["errors"], 1)
        self.assertEqual(report["summary"]["poi_recall_mean"], 0.75)
        self.assertEqual(report["summary"]["poi_recall_median"], 0.75)
        self.assertEqual(report["summary"]["poi_f1_mean"], 0.7778)
        self.assertEqual(report["summary"]["lo_recall_mean"], 0.75)
        self.assertEqual(report["rows"], rows)
        self.assertNotIn("corpus", report)

    def test_benchmark_report_rejects_malformed_release_metadata(self):
        rows = [{"file": "alpha", "poi_recall": 1.0, "poi_f1": 1.0}]
        cases = [
            ("version", {"version": 1}, "version must be a string"),
            ("version", {"version": ""}, "version must not be empty"),
            ("version", {"version": "0.1.0 beta"}, "version must not contain whitespace"),
            ("git_rev", {"git_rev": " abc123"}, "git_rev must not have surrounding whitespace"),
        ]
        for label, kwargs, message in cases:
            with self.subTest(label=label, message=message):
                with self.assertRaisesRegex(ValueError, message):
                    bench_vs_mature.benchmark_report(rows, **kwargs)

    def test_benchmark_report_rejects_file_paths(self):
        rows = [{"file": "private" + "/alpha", "poi_recall": 1.0, "poi_f1": 1.0}]

        with self.assertRaisesRegex(ValueError, "file path is invalid"):
            bench_vs_mature.benchmark_report(rows)

    def test_benchmark_report_rejects_missing_files(self):
        rows = [{"poi_recall": 1.0, "poi_f1": 1.0}]

        with self.assertRaisesRegex(ValueError, "file is required"):
            bench_vs_mature.benchmark_report(rows)

    def test_benchmark_report_rejects_empty_or_padded_files(self):
        cases = [
            ("", "file must not be empty"),
            (" alpha", "file must not have surrounding whitespace"),
        ]
        for file_name, message in cases:
            with self.subTest(file=file_name):
                rows = [{"file": file_name, "poi_recall": 1.0, "poi_f1": 1.0}]

                with self.assertRaisesRegex(ValueError, message):
                    bench_vs_mature.benchmark_report(rows)

    def test_benchmark_report_rejects_non_string_files(self):
        rows = [{"file": 1, "poi_recall": 1.0, "poi_f1": 1.0}]

        try:
            bench_vs_mature.benchmark_report(rows)
        except ValueError as exc:
            self.assertRegex(str(exc), "file must be a string")
        except Exception as exc:
            self.fail(f"expected ValueError, got {type(exc).__name__}: {exc}")
        else:
            self.fail("ValueError not raised")

    def test_benchmark_report_rejects_non_numeric_scores(self):
        rows = [{"file": "alpha", "poi_recall": "1.0", "poi_f1": 1.0}]

        try:
            bench_vs_mature.benchmark_report(rows)
        except ValueError as exc:
            self.assertRegex(str(exc), "score is invalid: poi_recall")
        except Exception as exc:
            self.fail(f"expected ValueError, got {type(exc).__name__}: {exc}")
        else:
            self.fail("ValueError not raised")

    def test_benchmark_report_rejects_out_of_range_scores(self):
        rows = [{"file": "alpha", "poi_recall": 1.1, "poi_f1": 1.0}]

        with self.assertRaisesRegex(ValueError, "score is out of range: poi_recall"):
            bench_vs_mature.benchmark_report(rows)

    def test_benchmark_report_rejects_invalid_rdoc_markers(self):
        rows = [{"file": "broken", "rdoc": "FAIL"}]

        with self.assertRaisesRegex(ValueError, "rdoc marker is invalid"):
            bench_vs_mature.benchmark_report(rows)

    def test_benchmark_report_rejects_scored_error_rows(self):
        rows = [{"file": "broken", "rdoc": "ERROR", "poi_recall": 1.0}]

        with self.assertRaisesRegex(ValueError, "error row has scores"):
            bench_vs_mature.benchmark_report(rows)

    def test_benchmark_report_evaluates_release_thresholds(self):
        rows = [
            {
                "file": "alpha",
                "poi_recall": 1.0,
                "poi_prec": 0.8,
                "poi_f1": 0.888888,
                "lo_recall": 0.75,
                "lo_prec": 0.6,
                "lo_f1": 0.666666,
            },
            {"file": "broken", "rdoc": "ERROR"},
            {
                "file": "beta",
                "poi_recall": 0.5,
                "poi_prec": 1.0,
                "poi_f1": 0.666666,
            },
        ]

        report = bench_vs_mature.benchmark_report(
            rows,
            thresholds={
                "min_poi_recall_mean": 0.8,
                "min_poi_f1_mean": 0.8,
                "min_lo_recall_mean": 0.7,
                "max_errors": 0,
                "min_scored": 3,
            },
        )

        self.assertFalse(report["gate"]["passed"])
        checks = {check["metric"]: check for check in report["gate"]["checks"]}
        self.assertEqual(checks["poi_recall_mean"]["actual"], 0.75)
        self.assertEqual(checks["poi_recall_mean"]["op"], ">=")
        self.assertEqual(checks["poi_recall_mean"]["threshold"], 0.8)
        self.assertFalse(checks["poi_recall_mean"]["passed"])
        self.assertEqual(checks["poi_f1_mean"]["actual"], 0.7778)
        self.assertFalse(checks["poi_f1_mean"]["passed"])
        self.assertEqual(checks["lo_recall_mean"]["actual"], 0.75)
        self.assertTrue(checks["lo_recall_mean"]["passed"])
        self.assertEqual(checks["errors"]["actual"], 1)
        self.assertFalse(checks["errors"]["passed"])
        self.assertEqual(checks["scored"]["actual"], 2)
        self.assertFalse(checks["scored"]["passed"])

    def test_benchmark_gate_rejects_non_finite_thresholds(self):
        with self.assertRaisesRegex(ValueError, "non-finite threshold"):
            bench_vs_mature.benchmark_gate(
                {"poi_recall_mean": 1.0},
                {"min_poi_recall_mean": float("nan")},
            )

    def test_benchmark_gate_rejects_negative_count_thresholds(self):
        with self.assertRaisesRegex(ValueError, "negative count threshold"):
            bench_vs_mature.benchmark_gate({"scored": 0}, {"min_scored": -1})

    def test_benchmark_gate_rejects_negative_score_thresholds(self):
        with self.assertRaisesRegex(ValueError, "negative score threshold"):
            bench_vs_mature.benchmark_gate(
                {"poi_recall_mean": 1.0},
                {"min_poi_recall_mean": -0.1},
            )

    def test_benchmark_gate_rejects_score_thresholds_above_one(self):
        with self.assertRaisesRegex(ValueError, "score threshold above one"):
            bench_vs_mature.benchmark_gate(
                {"poi_f1_mean": 1.0},
                {"min_poi_f1_mean": 1.1},
            )

    def test_write_json_report_rejects_non_finite_values(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "benchmark.json"

            with self.assertRaisesRegex(ValueError, "Out of range float values"):
                bench_vs_mature.write_json_report(
                    {"summary": {"poi_recall_mean": float("nan")}},
                    output,
                )

            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
