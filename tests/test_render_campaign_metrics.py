import contextlib
import copy
import io
import json
import os
import shutil
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import render_campaign_metrics as metrics
from test_render_oracle_contract import (
    valid_core_report,
    valid_environment,
    valid_manifest,
    write_manifest,
)


class CaptureMetricContractTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.manifest = write_manifest(self.root, valid_manifest())
        self.corpus = metrics.contract.load_corpus_manifest(self.manifest)

    def evidence(self):
        core = valid_core_report()
        core["visual_comparison"]["font_mode"] = "locked-shared-fonts"
        core["summary"]["reference_stable"] = None
        core["gate"] = metrics.render.validation_gate(
            core["summary"], {"max_skipped": 0}
        )
        environment = valid_environment()
        environment["platform"]["system"] = metrics.platform.system()
        environment["renderer"]["font_mode"] = "locked-shared-fonts"
        environment["oracle"]["mode"] = "locked-container"
        binding = {
            "schema": metrics.capture.SCHEMA,
            "sha256": "1" * 64,
            "environment_sha256": environment["oracle"]["identity_sha256"],
            "source_revision": environment["source_revision"],
            "campaign": self.corpus.identity(),
            "renderer_sha256": "2" * 64,
            "font_scope": "declared-font-resources",
            "measurement": metrics.measurement_contract(),
            "cases": [
                {
                    "case_id": self.corpus.documents[0].case_id,
                    "input_sha256": self.corpus.documents[0].sha256,
                    **{
                        key: "3" * 64
                        for key in (
                            "native_pdf_sha256",
                            "reference_pdf_sha256",
                            "native_fonts_sha256",
                            "reference_fonts_sha256",
                            "native_report_sha256",
                        )
                    },
                    "reference_page_digests": ["4" * 64],
                }
            ],
        }
        return metrics.contract.bind_evidence_report(
            core, self.corpus, environment, capture=binding
        )

    def test_capture_schema_keeps_v4_separate(self):
        evidence = self.evidence()
        self.assertEqual(evidence["schema"], metrics.contract.CAPTURE_EVIDENCE_SCHEMA)
        metrics.contract.validate_evidence_report(evidence, self.corpus)
        original = metrics.contract.bind_evidence_report(
            valid_core_report(),
            self.corpus,
            valid_environment(),
        )
        self.assertEqual(original["schema"], metrics.contract.EVIDENCE_SCHEMA)
        for mode in ("downgrade", "missing", "historical"):
            changed = copy.deepcopy(evidence)
            if mode == "downgrade":
                changed["schema"] = metrics.contract.EVIDENCE_SCHEMA
                del changed["capture"]
            elif mode == "missing":
                del changed["capture"]
            else:
                changed["schema"] = "rwml.render-oracle-evidence.v7"
            with self.subTest(mode=mode), self.assertRaises(ValueError):
                metrics.contract.validate_evidence_report(changed, self.corpus)

    def test_capture_binding_rejects_incomplete_or_conflicting_claims(self):
        evidence = self.evidence()
        mutations = (
            ("cases", []),
            ("cases", evidence["capture"]["cases"] * 2),
            ("source_revision", "b" * 40),
            ("environment_sha256", "e" * 64),
            ("font_scope", "all-glyphs"),
            ("measurement", {}),
        )
        for key, value in mutations:
            changed = copy.deepcopy(evidence)
            changed["capture"][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                metrics.contract.validate_evidence_report(changed, self.corpus)
        for key, value in (
            ("input_sha256", "f" * 64),
            ("native_report_sha256", "bad"),
            ("reference_page_digests", []),
            ("reference_page_digests", ["4" * 64] * 2),
        ):
            changed = copy.deepcopy(evidence)
            changed["capture"]["cases"][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                metrics.contract.validate_evidence_report(changed, self.corpus)

    def test_capture_rejects_dirty_mixed_profiles_and_false_repeat_claim(self):
        for field in ("dirty", "renderer", "oracle", "visual", "stable", "gate"):
            changed = self.evidence()
            if field == "dirty":
                changed["environment"]["source_dirty"] = True
            elif field == "renderer":
                changed["environment"]["renderer"]["font_mode"] = "system"
            elif field == "oracle":
                changed["environment"]["oracle"]["mode"] = "local"
            elif field == "visual":
                changed["visual_comparison"]["font_mode"] = "system"
            elif field == "stable":
                changed["summary"]["reference_stable"] = True
            else:
                changed["gate"] = {"passed": True, "checks": []}
            with self.subTest(field=field), self.assertRaises(ValueError):
                metrics.contract.validate_evidence_report(changed, self.corpus)

    def test_namespace_import_does_not_require_scripts_path(self):
        result = subprocess.run(
            [
                sys.executable,
                "-I",
                "-B",
                "-c",
                "import sys; sys.path.insert(0,sys.argv[1]); from scripts import render_campaign_metrics",
                str(metrics.ROOT),
            ],
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_cli_rejects_release_or_partial_profile_overrides(self):
        with (
            mock.patch.object(
                sys, "argv", ["render_campaign_metrics", "measure", "--system-fonts"]
            ),
            mock.patch.object(metrics, "run") as run,
            contextlib.redirect_stderr(io.StringIO()),
            self.assertRaises(SystemExit),
        ):
            metrics.main()
        run.assert_not_called()


@unittest.skipIf(
    metrics.render.fitz is None or metrics.render.Image is None,
    "PDF dependencies not installed",
)
class CaptureMetricWorkerTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        for name, text in (("reference.pdf", "one two three"), ("native.pdf", "one")):
            document = metrics.render.fitz.open()
            page = document.new_page(width=100, height=100)
            page.insert_text((5, 30), text, fontsize=7)
            document.save(self.root / name)
            document.close()
        (self.root / "native-report.json").write_text('{"warnings":[]}')
        self.request = {
            "schema": metrics.REQUEST_SCHEMA,
            "case": {
                "document": "fixture.docx",
                "case_id": "fixture-basic",
                "input_bytes": 7,
                "input_sha256": metrics.capture.digest(b"fixture"),
            },
            "visual": metrics.visual_settings({"dpi": 36}),
            "recall_min": 0.97,
            "max_pages": 8,
        }

    def test_worker_preserves_failed_recall_and_complete_reference_rasters(self):
        result = metrics.measure_case(self.root, self.request)
        self.assertEqual(result["row"]["status"], "fail")
        self.assertAlmostEqual(result["row"]["recall"], 1 / 3)
        self.assertEqual(len(result["reference_page_digests"]), 1)
        self.assertEqual(result["row"]["compared_pages"], 1)
        self.assertIsNotNone(result["row"]["integer_visual_metrics"])
        self.assertEqual(result, metrics.measure_case(self.root, self.request))

    def test_invalid_worker_timeout_fails_before_writing_request(self):
        for value in (True, None, "1", 0, -1, float("nan"), 181):
            with self.subTest(value=value), self.assertRaises(ValueError):
                metrics.run_worker(self.root, self.request, timeout=value)
        self.assertFalse((self.root / "request.json").exists())

    @unittest.skipUnless(os.name == "posix", "bounded metric worker requires POSIX")
    def test_real_worker_uses_bounded_process_and_identical_metrics(self):
        self.assertNotIsInstance(metrics.run_worker, mock.Mock)
        result = metrics.run_worker(self.root, self.request, timeout=30)
        self.assertTrue((self.root / "request.json").is_file())
        self.assertEqual(result, metrics.measure_case(self.root, self.request))


@unittest.skipIf(
    metrics.render.fitz is None or metrics.render.Image is None,
    "PDF dependencies not installed",
)
class CaptureMetricCompositionTests(unittest.TestCase):
    def setUp(self):
        CaptureMetricWorkerTests.setUp(self)
        inputs = self.root / "inputs"
        inputs.mkdir()
        self.manifest = write_manifest(inputs, valid_manifest())
        self.corpus = metrics.contract.load_corpus_manifest(self.manifest)
        self.capture_root = self.root / "capture-a"
        self.capture_root.mkdir()
        self.case = self.capture_root / "cases/fixture-basic"
        self.case.mkdir(parents=True)
        for name in ("native.pdf", "native-report.json"):
            shutil.copyfile(self.root / name, self.case / name)
        (self.case / "input.docx").write_bytes(b"fixture")
        (self.case / "SHA256SUMS").write_text(
            metrics.capture.digest(b"fixture") + "  input.docx\n"
        )
        for engine in ("native", "reference"):
            (self.case / (engine + "-fonts.json")).write_text('{"resources":[]}')
        reference = self.case / "reference"
        reference.mkdir()
        for name in metrics.capture.runtime.CAPTURE_MEMBERS:
            (reference / name).write_bytes(
                b""
                if name != "output.pdf"
                else (self.root / "reference.pdf").read_bytes()
            )
        fonts = self.capture_root / "fonts"
        fonts.mkdir()
        self.lock = type("Lock", (), {"fonts": [{"name": "font.ttf"}]})()
        self.sources = {"font.ttf": b"font"}
        for name, payload in metrics.capture.font_files(
            self.lock.fonts, self.sources
        ).items():
            (fonts / name).write_bytes(payload)
        (self.capture_root / "renderer").write_bytes(b"renderer")
        identity = lambda path: metrics.capture.identity(path.read_bytes())
        self.row = {
            "case_id": "fixture-basic",
            "input": metrics.capture.identity(b"fixture"),
            "native_report": identity(self.case / "native-report.json"),
            "reference_auxiliary": {
                name: identity(reference / name)
                for name in metrics.capture.runtime.CAPTURE_MEMBERS
                if name != "output.pdf"
            },
            "native": {
                "pdf": identity(self.case / "native.pdf"),
                "font_checks": identity(self.case / "native-fonts.json"),
            },
            "reference": {
                "pdf": identity(reference / "output.pdf"),
                "font_checks": identity(self.case / "reference-fonts.json"),
            },
        }
        self.bundle = {
            "environment": {"runtime": "locked"},
            "source_revision": "a" * 40,
            "renderer": {
                **metrics.capture.identity(b"renderer"),
                "cargo_lock_sha256": "c" * 64,
            },
            "rows": [self.row],
        }
        (self.capture_root / "CAPTURE.json").write_bytes(
            metrics.capture.canonical(self.bundle)
        )
        env = valid_environment()
        env["platform"]["system"] = metrics.platform.system()
        env["renderer"]["font_mode"] = "locked-shared-fonts"
        env["oracle"]["mode"] = "locked-container"
        env["oracle"]["identity_sha256"] = metrics.capture.digest(
            metrics.capture.canonical(self.bundle["environment"])
        )
        self.stack = contextlib.ExitStack()
        self.addCleanup(self.stack.close)
        self.stack.enter_context(mock.patch.object(metrics, "ROOT", self.root))
        self.stack.enter_context(
            mock.patch.object(metrics, "environment", return_value=env)
        )
        self.stack.enter_context(
            mock.patch.object(
                metrics, "harness_identity", return_value=env["harness_sha256"]
            )
        )
        self.verify_capture = self.stack.enter_context(
            mock.patch.object(metrics.capture, "run", return_value=self.bundle)
        )
        self.stack.enter_context(
            mock.patch.object(
                metrics.capture,
                "prepare_environment",
                return_value=(self.bundle["environment"], self.sources, self.lock, {}),
            )
        )
        self.stack.enter_context(
            mock.patch.object(
                metrics.capture.table_capture, "source_revision", return_value="a" * 40
            )
        )
        self.stack.enter_context(
            mock.patch.object(
                metrics,
                "run_worker",
                side_effect=lambda path, request, **kw: metrics.measure_case(
                    path, request
                ),
            )
        )
        self.stack.enter_context(contextlib.redirect_stderr(io.StringIO()))

    def call_run(self, root=None, evidence=None):
        return metrics.run(
            self.manifest,
            root or self.capture_root,
            self.root / "pack",
            self.root / "ft",
            self.root / "pp",
            settings={"dpi": 36},
            evidence=evidence,
        )

    def test_composition_recomputes_failed_metrics_without_rewriting_inputs(self):
        before = {
            path: path.read_bytes()
            for path in self.capture_root.rglob("*")
            if path.is_file()
        }
        result = self.call_run()
        self.assertFalse(result["gate"]["passed"])
        report = self.root / "evidence.json"
        report.write_bytes(metrics.capture.canonical(result))
        self.assertEqual(self.call_run(evidence=report), result)
        self.assertEqual(self.verify_capture.call_count, 2)
        self.assertTrue(
            all(call.kwargs["verify"] for call in self.verify_capture.call_args_list)
        )
        self.assertTrue(
            all(path.read_bytes() == payload for path, payload in before.items())
        )
        self.assertEqual(
            list((self.root / "target/render-oracle/capture-metrics").iterdir()), []
        )

    def test_consistent_fabricated_metrics_pass_structure_but_fail_recomputation(self):
        result = self.call_run()
        result["rows"][0]["recall"] = 1.0
        result["rows"][0]["status"] = "pass"
        result["summary"].update(mean_recall=1.0, below_recall_min=0)
        result["gate"] = metrics.render.validation_gate(
            result["summary"], {"max_skipped": 0}
        )
        metrics.contract.validate_evidence_report(result, self.corpus)
        path = self.root / "forged.json"
        payload = metrics.capture.canonical(result)
        path.write_bytes(payload)
        with self.assertRaisesRegex(ValueError, "recomputed"):
            self.call_run(evidence=path)
        self.assertEqual(path.read_bytes(), payload)

    def test_worker_failure_cleans_only_its_scratch(self):
        with mock.patch.object(
            metrics, "run_worker", side_effect=ValueError("worker failed")
        ):
            with self.assertRaisesRegex(ValueError, "worker failed"):
                self.call_run()
        self.assertEqual(
            list((self.root / "target/render-oracle/capture-metrics").iterdir()), []
        )
        self.assertTrue((self.capture_root / "CAPTURE.json").is_file())

    def test_changed_original_during_worker_is_rejected(self):
        def alter(directory, request, **kw):
            result = metrics.measure_case(directory, request)
            (self.case / "native.pdf").write_bytes(b"changed")
            return result

        with mock.patch.object(metrics, "run_worker", side_effect=alter):
            with self.assertRaisesRegex(ValueError, "metric input"):
                self.call_run()

    def test_repeat_recomputes_both_reports_and_retains_failed_fidelity(self):
        second = self.root / "capture-b"
        shutil.copytree(self.capture_root, second)
        reports = (self.root / "a.json", self.root / "b.json")
        for root, path in zip((self.capture_root, second), reports):
            path.write_bytes(metrics.capture.canonical(self.call_run(root)))
        self.verify_capture.reset_mock()
        repeat = metrics.verify_repeat(
            self.manifest,
            (self.capture_root, second),
            reports,
            self.root / "pack",
            self.root / "ft",
            self.root / "pp",
        )
        self.assertEqual(self.verify_capture.call_count, 2)
        self.assertEqual(repeat["summary"]["documents"], 1)
        self.assertFalse(repeat["summary"]["fidelity_gate_passed"])
        self.assertTrue(repeat["summary"]["metrics_recomputed"])
        for path in reports:
            forged = json.loads(path.read_bytes())
            forged["rows"][0]["recall"] = 1.0
            forged["rows"][0]["status"] = "pass"
            forged["summary"].update(mean_recall=1.0, below_recall_min=0)
            forged["gate"] = metrics.render.validation_gate(
                forged["summary"], {"max_skipped": 0}
            )
            path.write_bytes(metrics.capture.canonical(forged))
        with self.assertRaisesRegex(ValueError, "recomputed"):
            metrics.verify_repeat(
                self.manifest,
                (self.capture_root, second),
                reports,
                self.root / "pack",
                self.root / "ft",
                self.root / "pp",
            )

    def repeat_inputs(self):
        second = self.root / "capture-b"
        shutil.copytree(self.capture_root, second)
        reports = (self.root / "a.json", self.root / "b.json")
        for root, path in zip((self.capture_root, second), reports):
            path.write_bytes(metrics.capture.canonical(self.call_run(root)))
        return (self.capture_root, second), reports

    def test_repeat_rejects_artifact_changes_after_second_verification(self):
        roots, reports = self.repeat_inputs()
        paths = [
            self.case / "reference/output.pdf",
            self.case / "reference-fonts.json",
            self.case / "reference/warmup.log",
            self.capture_root / "fonts/font.ttf",
            self.case / "input.docx",
        ]
        actual_run = metrics.run
        for target in paths:
            original = target.read_bytes()

            def change_after_second(*args, **kwargs):
                result = actual_run(*args, **kwargs)
                if args[1] == roots[1]:
                    target.write_bytes(b"changed")
                return result

            try:
                with (
                    self.subTest(path=target.relative_to(self.capture_root)),
                    mock.patch.object(
                        metrics,
                        "run",
                        side_effect=change_after_second,
                    ),
                    self.assertRaises(ValueError),
                ):
                    metrics.verify_repeat(
                        self.manifest,
                        roots,
                        reports,
                        self.root / "pack",
                        self.root / "ft",
                        self.root / "pp",
                    )
            finally:
                target.write_bytes(original)

    def test_repeat_rejects_identically_changed_native_bytes_after_verification(self):
        roots, reports = self.repeat_inputs()
        actual_run = metrics.run

        def change_after_second(*args, **kwargs):
            result = actual_run(*args, **kwargs)
            if args[1] == roots[1]:
                for root in roots:
                    (root / "cases/fixture-basic/native.pdf").write_bytes(
                        b"same forgery"
                    )
            return result

        with (
            mock.patch.object(metrics, "run", side_effect=change_after_second),
            self.assertRaises(ValueError),
        ):
            metrics.verify_repeat(
                self.manifest,
                roots,
                reports,
                self.root / "pack",
                self.root / "ft",
                self.root / "pp",
            )

    def test_repeat_rejects_elapsed_pair_budget(self):
        roots, reports = self.repeat_inputs()
        clock = [0.0]
        actual_run = metrics.run

        def exceed_after_second(*args, **kwargs):
            result = actual_run(*args, **kwargs)
            if args[1] == roots[1]:
                clock[0] = (
                    2
                    * (metrics.capture.MAX_CAMPAIGN_SECONDS + metrics.MAX_BATCH_SECONDS)
                    + 1
                )
            return result

        with (
            mock.patch.object(metrics.time, "monotonic", side_effect=lambda: clock[0]),
            mock.patch.object(metrics, "run", side_effect=exceed_after_second),
            self.assertRaisesRegex(ValueError, "timed out"),
        ):
            metrics.verify_repeat(
                self.manifest,
                roots,
                reports,
                self.root / "pack",
                self.root / "ft",
                self.root / "pp",
            )

    def test_repeat_rechecks_source_after_both_measurements(self):
        roots, reports = self.repeat_inputs()
        checks = [0]

        def source_revision(*args):
            checks[0] += 1
            if checks[0] > 2:
                raise ValueError("source changed")
            return "a" * 40

        with (
            mock.patch.object(
                metrics.capture.table_capture,
                "source_revision",
                side_effect=source_revision,
            ),
            self.assertRaisesRegex(ValueError, "source changed"),
        ):
            metrics.verify_repeat(
                self.manifest,
                roots,
                reports,
                self.root / "pack",
                self.root / "ft",
                self.root / "pp",
            )

    def test_repeat_requires_distinct_roots_and_reports_before_replay(self):
        roots, reports = self.repeat_inputs()
        self.verify_capture.reset_mock()
        for paths, receipts in ((roots[:1] * 2, reports), (roots, reports[:1] * 2)):
            with (
                self.subTest(paths=paths),
                self.assertRaisesRegex(ValueError, "distinct"),
            ):
                metrics.verify_repeat(
                    self.manifest,
                    paths,
                    receipts,
                    self.root / "pack",
                    self.root / "ft",
                    self.root / "pp",
                )
        self.verify_capture.assert_not_called()

    def test_verify_rejects_requested_profile_mismatch_before_replay(self):
        report = self.call_run()
        path = self.root / "evidence.json"
        path.write_bytes(metrics.capture.canonical(report))
        for settings, recall in (({"dpi": 72}, 0.97), ({"dpi": 36}, 0.9)):
            self.verify_capture.reset_mock()
            with (
                self.subTest(settings=settings, recall=recall),
                self.assertRaises(ValueError),
            ):
                metrics.run(
                    self.manifest,
                    self.capture_root,
                    self.root / "pack",
                    self.root / "ft",
                    self.root / "pp",
                    settings=settings,
                    recall_min=recall,
                    evidence=path,
                )
            self.verify_capture.assert_not_called()

    def test_worker_rejects_page_cap_instead_of_partial_success(self):
        document = metrics.render.fitz.open(self.root / "reference.pdf")
        document.new_page(width=100, height=100)
        document.save(self.root / "more.pdf")
        document.close()
        (self.root / "more.pdf").replace(self.root / "reference.pdf")
        self.request["visual"]["page_cap"] = 1
        with self.assertRaisesRegex(ValueError, "page"):
            metrics.measure_case(self.root, self.request)

    def test_profile_validation_is_typed_and_bounded(self):
        for bad in (True, -1, float("nan"), float("inf"), 2, "0.97"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                metrics.validate_recall(bad)
        with self.assertRaises(ValueError):
            metrics.visual_settings({"font_mode": "system"})


if __name__ == "__main__":
    unittest.main()
