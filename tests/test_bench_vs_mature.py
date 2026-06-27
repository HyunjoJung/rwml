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
