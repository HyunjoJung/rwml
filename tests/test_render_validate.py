import importlib.util
import os
import pathlib
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock
from urllib.parse import unquote, urlparse


SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "scripts" / "render_validate.py"
SPEC = importlib.util.spec_from_file_location("render_validate", SCRIPT)
render_validate = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = render_validate
SPEC.loader.exec_module(render_validate)


class RenderValidateReportTests(unittest.TestCase):
    def test_cli_prefers_warning_free_pymupdf_module(self):
        with tempfile.TemporaryDirectory() as tmp:
            modules = pathlib.Path(tmp)
            (modules / "pymupdf.py").write_text("", encoding="utf-8")
            (modules / "fitz.py").write_text(
                'print("deprecated fitz import reached")\n', encoding="utf-8"
            )
            env = os.environ.copy()
            env["PYTHONPATH"] = str(modules)
            env["PYTHONIOENCODING"] = "cp949"

            completed = subprocess.run(
                [sys.executable, str(SCRIPT), "--help"],
                check=False,
                capture_output=True,
                text=True,
                env=env,
            )

        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertNotIn("deprecated fitz import reached", completed.stdout)

    def test_cli_routes_legacy_fitz_import_output_to_stderr(self):
        with tempfile.TemporaryDirectory() as tmp:
            modules = pathlib.Path(tmp)
            (modules / "pymupdf.py").write_text(
                'raise ImportError("legacy PyMuPDF")\n', encoding="utf-8"
            )
            (modules / "fitz.py").write_text(
                'print("legacy fitz import warning")\n', encoding="utf-8"
            )
            env = os.environ.copy()
            env["PYTHONPATH"] = str(modules)

            completed = subprocess.run(
                [sys.executable, str(SCRIPT), "--help"],
                check=False,
                capture_output=True,
                text=True,
                env=env,
            )

        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertNotIn("legacy fitz import warning", completed.stdout)
        self.assertIn("legacy fitz import warning", completed.stderr)

    def test_validation_report_summarizes_measured_and_skipped_rows(self):
        rows = [
            render_validate.ValidationRow(
                document="good.docx",
                status="pass",
                recall=1.0,
                rwml_pages=2,
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
                rwml_pages=1,
                reference_pages=2,
                page_ratio=0.5,
                ahash_similarity=0.25,
                render_warnings=3,
                render_warning_kinds=[
                    "FloatingShapePlaceholderOnly",
                    "UnsupportedMetafileImages",
                    "OleObjectsPreservedButNotModeled",
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

    def test_resolve_input_paths_reads_manifest_documents(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "synthetic").mkdir()
            first = root / "synthetic" / "first.docx"
            second = root / "synthetic" / "second.docx"
            first.write_bytes(b"first")
            second.write_bytes(b"second")
            manifest = root / "RENDER_MANIFEST.tsv"
            manifest.write_text(
                "# path\tpages\twarnings\n"
                "synthetic/first.docx\t1\t-\n"
                "synthetic/second.docx\t1\tUnsupportedFieldEvaluation\n",
                encoding="utf-8",
            )

            inputs = render_validate.resolve_input_paths([], manifest)

        self.assertEqual(inputs, [first, second])

    def test_resolve_input_campaign_reads_strict_oracle_identity(self):
        root = pathlib.Path(__file__).resolve().parents[1]
        manifest = root / "corpus" / "public" / "RENDER_ORACLE.json"

        inputs, corpus = render_validate.resolve_input_campaign([], manifest)

        self.assertIsNotNone(corpus)
        assert corpus is not None
        self.assertEqual(len(inputs), 21)
        self.assertEqual(inputs, [document.path for document in corpus.documents])
        self.assertEqual(corpus.expected_pages, 26)
        self.assertEqual(
            render_validate.row_identity(
                inputs[0], render_validate.corpus_document_map(corpus)
            ),
            {
                "case_id": "python-docx-blk-inner-content",
                "input_bytes": 11949,
                "input_sha256": (
                    "16243dd05f686d6d5fab331843691492"
                    "ad78ae3eb4f13cc8686077b44718c0ba"
                ),
            },
        )

    def test_validation_report_rejects_document_paths(self):
        row = render_validate.ValidationRow(
            document="private" + "/sample.docx",
            status="skip",
            reason="render failed",
        )

        with self.assertRaisesRegex(ValueError, "document path is invalid"):
            render_validate.validation_report([row], recall_min=0.8)

    def test_validation_report_rejects_malformed_documents(self):
        cases = [
            (1, "document must be a string"),
            ("", "document must not be empty"),
            (" sample.docx", "document must not have surrounding whitespace"),
        ]
        for document, message in cases:
            with self.subTest(document=document):
                row = render_validate.ValidationRow(
                    document=document,
                    status="skip",
                    reason="render failed",
                )

                try:
                    render_validate.validation_report([row], recall_min=0.8)
                except ValueError as exc:
                    self.assertRegex(str(exc), message)
                except Exception as exc:
                    self.fail(
                        f"expected ValueError, got {type(exc).__name__}: {exc}"
                    )
                else:
                    self.fail("ValueError not raised")

    def test_validation_report_rejects_invalid_status(self):
        row = render_validate.ValidationRow(
            document="sample.docx",
            status="pending",
            reason="render pending",
        )

        with self.assertRaisesRegex(ValueError, "status is invalid"):
            render_validate.validation_report([row], recall_min=0.8)

    def test_validation_report_rejects_non_numeric_metrics(self):
        row = render_validate.ValidationRow(
            document="sample.docx",
            status="pass",
            recall="1.0",
        )

        try:
            render_validate.validation_report([row], recall_min=0.8)
        except ValueError as exc:
            self.assertRegex(str(exc), "metric is invalid: recall")
        except Exception as exc:
            self.fail(f"expected ValueError, got {type(exc).__name__}: {exc}")
        else:
            self.fail("ValueError not raised")

    def test_validation_report_rejects_out_of_range_bounded_metrics(self):
        row = render_validate.ValidationRow(
            document="sample.docx",
            status="pass",
            recall=1.1,
        )

        with self.assertRaisesRegex(ValueError, "metric is out of range: recall"):
            render_validate.validation_report([row], recall_min=0.8)

    def test_validation_report_rejects_invalid_count_metrics(self):
        row = render_validate.ValidationRow(
            document="sample.docx",
            status="pass",
            render_warnings=-1,
        )

        with self.assertRaisesRegex(ValueError, "count is invalid: render_warnings"):
            render_validate.validation_report([row], recall_min=0.8)

    def test_validation_report_rejects_invalid_warning_kinds(self):
        cases = [
            ("UnsupportedFieldEvaluation", "render warning kinds must be a list"),
            (["Unsupported Field"], "render warning kind is invalid"),
            (["UnsupportedFieldEvalution"], "unknown render warning kind"),
            (["UnsupportedFieldEvaluation", "UnsupportedFieldEvaluation"], "duplicate"),
        ]
        for kinds, message in cases:
            with self.subTest(kinds=kinds):
                row = render_validate.ValidationRow(
                    document="sample.docx",
                    status="pass",
                    recall=1.0,
                    render_warnings=1 if isinstance(kinds, str) else len(kinds),
                    render_warning_kinds=kinds,
                )

                with self.assertRaisesRegex(ValueError, message):
                    render_validate.validation_report([row], recall_min=0.8)

        row = render_validate.ValidationRow(
            document="sample.docx",
            status="pass",
            recall=1.0,
            render_warnings=2,
            render_warning_kinds=["UnsupportedFieldEvaluation"],
        )
        with self.assertRaisesRegex(ValueError, "render warning count mismatch"):
            render_validate.validation_report([row], recall_min=0.8)

    def test_reference_recall_tokens_drop_volatile_libreoffice_field_text(self):
        tokens = [
            "Stable",
            "Error:",
            "Reference",
            "source",
            "not",
            "found",
            "/workspace/project/rwml/",
            "corpus/public/synthetic/fields.docx",
            "report.docx",
        ]

        self.assertEqual(
            render_validate.reference_recall_tokens(tokens),
            ["Stable", "report.docx"],
        )

    def test_reference_recall_tokens_drop_whitespace_split_volatile_path(self):
        tokens = [
            "Stable",
            "/workspace/New",
            "project/corpus/public/synthetic/",
            "fields.docx",
            "Visible",
            "report.docx",
        ]

        self.assertEqual(
            render_validate.reference_recall_tokens(tokens),
            ["Stable", "Visible", "report.docx"],
        )
        self.assertEqual(
            render_validate.reference_recall_tokens(["fields.docx"]),
            ["fields.docx"],
        )
        self.assertEqual(
            render_validate.reference_recall_tokens(
                ["/workspace/New", "Visible", "fields.docx"]
            ),
            ["Visible", "fields.docx"],
        )

    def test_reference_recall_tokens_drop_libreoffice_shape_fallback_only_when_warned(self):
        tokens = ["Visible", "[shape]"]

        self.assertEqual(render_validate.reference_recall_tokens(tokens), tokens)
        self.assertEqual(
            render_validate.reference_recall_tokens(
                tokens,
                render_warning_kinds=["FloatingShapePlaceholderOnly"],
            ),
            ["Visible"],
        )

    def test_reference_recall_tokens_drop_bounded_libreoffice_ole_labels(self):
        raw = ["Visible", "Object", "1", "Object", "2"]
        warnings = ["OleObjectsPreservedButNotModeled"]

        self.assertEqual(render_validate.reference_recall_tokens(raw), raw)
        self.assertEqual(
            render_validate.reference_recall_tokens(raw, warnings),
            raw,
        )
        self.assertEqual(
            render_validate.reference_recall_tokens(
                raw,
                warnings,
                {"unsupported": {"ole_objects": 1}},
            ),
            ["Visible", "Object", "2"],
        )

    def test_token_recall_accepts_tracked_change_reference_compounds_only_with_context(self):
        ref_tokens = ["StableAddedRemovedMoved", "fromMoved", "to"]
        got_tokens = ["Stable", "AddedMoved", "to"]

        self.assertLess(render_validate.token_recall(ref_tokens, got_tokens), 1.0)
        self.assertEqual(
            render_validate.token_recall(
                ref_tokens,
                got_tokens,
                render_report={
                    "unsupported": {
                        "tracked_insertions": 1,
                        "tracked_deletions": 1,
                        "tracked_moves": 2,
                    }
                },
            ),
            1.0,
        )

    def test_token_recall_accepts_joined_footnote_markers_only_with_context(self):
        ref_tokens = ["Footnoted1", "1A"]
        got_tokens = ["Footnoted", "A"]

        self.assertEqual(render_validate.token_recall(ref_tokens, got_tokens), 0.0)
        self.assertEqual(
            render_validate.token_recall(
                ref_tokens,
                got_tokens,
                render_report={"unsupported": {"footnotes": 1}},
            ),
            1.0,
        )

    def test_token_recall_accepts_split_rtl_list_marker_punctuation(self):
        ref_tokens = ["5", ".عنصر", "خلية", "مباشر", "6", ".פריט", "תא"]
        got_tokens = ["مباشر", "خلية", "عنصر", ".", "5", "פריט", "תא", ".", "6"]

        self.assertEqual(render_validate.token_recall(ref_tokens, got_tokens), 1.0)
        self.assertLess(
            render_validate.token_recall(
                ref_tokens, [token for token in got_tokens if token != "."]
            ),
            1.0,
        )
        self.assertLess(render_validate.token_recall([".item"], ["item", "."]), 1.0)

    def test_token_recall_accepts_one_adjacent_rtl_extraction_transposition(self):
        self.assertEqual(render_validate.token_recall(["أوىل"], ["أولى"]), 1.0)
        self.assertLess(render_validate.token_recall(["أوىل"], ["أيلو"]), 1.0)
        self.assertLess(render_validate.token_recall(["abcd"], ["abdc"]), 1.0)

    def test_validation_report_rejects_measured_skip_rows(self):
        row = render_validate.ValidationRow(
            document="sample.docx",
            status="skip",
            recall=1.0,
            reason="render failed",
        )

        with self.assertRaisesRegex(ValueError, "skipped row has metrics"):
            render_validate.validation_report([row], recall_min=0.8)

    def test_validation_report_rejects_unmeasured_non_skip_rows(self):
        row = render_validate.ValidationRow(
            document="sample.docx",
            status="pass",
        )

        with self.assertRaisesRegex(ValueError, "non-skip row is missing recall"):
            render_validate.validation_report([row], recall_min=0.8)

    def test_validation_report_evaluates_release_thresholds(self):
        rows = [
            render_validate.ValidationRow(
                document="good.docx",
                status="pass",
                recall=0.95,
                rwml_pages=2,
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
                rwml_pages=1,
                reference_pages=2,
                page_ratio=0.5,
                ahash_similarity=0.2,
                render_warnings=3,
                render_warning_kinds=[
                    "FloatingShapePlaceholderOnly",
                    "UnsupportedMetafileImages",
                    "OleObjectsPreservedButNotModeled",
                ],
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

    def test_warning_kinds_rejects_malformed_report_entries(self):
        self.assertEqual(
            render_validate.warning_kinds(
                {
                    "warnings": [
                        {"kind": "UnsupportedFieldEvaluation"},
                        {"kind": "UnsupportedMetafileImages"},
                    ]
                }
            ),
            ["UnsupportedFieldEvaluation", "UnsupportedMetafileImages"],
        )
        cases = [
            {"warnings": "UnsupportedFieldEvaluation"},
            {"warnings": [42]},
            {"warnings": [{"count": 1}]},
            {"warnings": [{"kind": "Unsupported Field"}]},
            {"warnings": [{"kind": "UnsupportedFieldEvalution"}]},
            {
                "warnings": [
                    {"kind": "UnsupportedFieldEvaluation"},
                    {"kind": "UnsupportedFieldEvaluation"},
                ]
            },
        ]
        for report in cases:
            with self.subTest(report=report):
                self.assertIsNone(render_validate.warning_kinds(report))

    def test_render_libreoffice_reports_missing_docker_dependency(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = pathlib.Path(tmp) / "sample.docx"
            src.write_bytes(b"placeholder")
            with mock.patch.object(
                render_validate.subprocess,
                "run",
                side_effect=FileNotFoundError(
                    2, "No such file or directory", "docker"
                ),
            ):
                with self.assertRaisesRegex(
                    RuntimeError, "docker executable not found"
                ):
                    render_validate.render_libreoffice(
                        src, pathlib.Path(tmp), "docker"
                    )

    def test_local_libreoffice_uses_a_fresh_per_document_profile(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            src = root / "sample.docx"
            src.write_bytes(b"placeholder")
            out = root / "out"
            out.mkdir()

            def complete(command, **_kwargs):
                profile_argument = next(
                    token
                    for token in command
                    if token.startswith("-env:UserInstallation=file:")
                )
                profile_uri = profile_argument.split("=", 1)[1]
                profile_path = pathlib.Path(unquote(urlparse(profile_uri).path))
                registry = profile_path / "user" / "registrymodifications.xcu"
                self.assertEqual(
                    registry.read_bytes(),
                    render_validate.LOCAL_ORACLE_PROFILE.read_bytes(),
                )
                (out / "sample.pdf").write_bytes(b"%PDF isolated")
                return mock.Mock(returncode=0)

            with mock.patch.object(
                render_validate.subprocess, "run", side_effect=complete
            ) as run:
                result = render_validate.render_libreoffice(src, out, "local")

            self.assertEqual(result, out / "sample.pdf")
            self.assertEqual(run.call_count, 2)
            initialize = run.call_args_list[0].args[0]
            command = run.call_args_list[1].args[0]
            initialized_profile = [
                token
                for token in initialize
                if token.startswith("-env:UserInstallation=file:")
            ]
            profile = [
                token
                for token in command
                if token.startswith("-env:UserInstallation=file:")
            ]
            self.assertEqual(initialized_profile, profile)
            self.assertEqual(len(profile), 1)
            self.assertIn("--terminate_after_init", initialize)
            self.assertIn("--headless", command)
            self.assertIn("--convert-to", command)

    def test_local_oracle_profile_locks_font_substitution_and_active_content(self):
        text = render_validate.LOCAL_ORACLE_PROFILE.read_text(encoding="utf-8")
        for marker in (
            "DisableMacrosExecution",
            "DisableActiveContent",
            "Font/Substitution/FontPairs",
            "<value>Calibri</value>",
            "<value>Cambria</value>",
            "<value>Arial Unicode MS</value>",
            "<value>Noto Sans</value>",
            "<value>Geeza Pro</value>",
            "<value>Noto Sans Arabic</value>",
            "<value>Lucida Grande</value>",
            "<value>Noto Sans Hebrew</value>",
            "<value>true</value>",
        ):
            self.assertIn(marker, text)

    def test_local_libreoffice_stops_when_profile_initialization_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            src = root / "sample.docx"
            src.write_bytes(b"placeholder")
            out = root / "out"
            out.mkdir()
            with mock.patch.object(
                render_validate.subprocess,
                "run",
                return_value=mock.Mock(returncode=1),
            ) as run:
                result = render_validate.render_libreoffice(src, out, "local")
            self.assertIsNone(result)
            self.assertEqual(run.call_count, 1)

    def test_soffice_auto_mode_prefers_local_libreoffice(self):
        with mock.patch.object(
            render_validate.shutil,
            "which",
            side_effect=lambda name: f"/usr/bin/{name}" if name == "soffice" else None,
        ):
            self.assertEqual(render_validate.resolve_soffice_mode("auto"), "local")

    def test_soffice_auto_mode_falls_back_to_docker(self):
        with mock.patch.object(
            render_validate.shutil,
            "which",
            side_effect=lambda name: f"/usr/bin/{name}" if name == "docker" else None,
        ):
            self.assertEqual(render_validate.resolve_soffice_mode("auto"), "docker")

    def test_soffice_auto_mode_reports_missing_backends(self):
        with mock.patch.object(render_validate.shutil, "which", return_value=None):
            with self.assertRaisesRegex(
                RuntimeError,
                "neither soffice nor docker executable found",
            ):
                render_validate.resolve_soffice_mode("auto")

    def test_validation_gate_rejects_non_finite_thresholds(self):
        with self.assertRaisesRegex(ValueError, "non-finite threshold"):
            render_validate.validation_gate(
                {"below_recall_min": 0, "mean_recall": 1.0},
                {"min_mean_recall": float("nan")},
            )

    def test_strict_campaign_thresholds_fail_closed(self):
        args = mock.Mock(
            min_mean_recall=None,
            min_mean_page_ratio=None,
            max_mean_page_ratio=None,
            min_mean_ahash_similarity=None,
            min_mean_page_ahash_similarity=None,
            min_mean_foreground_ink_iou=None,
            max_mean_render_warnings=None,
            max_skipped=None,
            max_unmatched_candidate_pages=None,
            max_unmatched_reference_pages=None,
            verify_oracle=True,
        )

        thresholds = render_validate.resolve_validation_thresholds(
            args, strict_corpus=True
        )

        self.assertEqual(thresholds["max_skipped"], 0)
        self.assertIsNone(thresholds["max_unmatched_candidate_pages"])
        self.assertIsNone(thresholds["max_unmatched_reference_pages"])
        self.assertTrue(thresholds["require_reference_stable"])

    def test_strict_campaign_thresholds_cannot_be_relaxed(self):
        args = mock.Mock(
            min_mean_recall=None,
            min_mean_page_ratio=None,
            max_mean_page_ratio=None,
            min_mean_ahash_similarity=None,
            min_mean_page_ahash_similarity=None,
            min_mean_foreground_ink_iou=None,
            max_mean_render_warnings=None,
            max_skipped=1,
            max_unmatched_candidate_pages=None,
            max_unmatched_reference_pages=None,
            verify_oracle=True,
        )

        with self.assertRaisesRegex(ValueError, "strict corpus threshold"):
            render_validate.resolve_validation_thresholds(args, strict_corpus=True)

    def test_validation_gate_rejects_negative_count_thresholds(self):
        with self.assertRaisesRegex(ValueError, "negative count threshold"):
            render_validate.validation_gate({"skipped": 0}, {"max_skipped": -1})

    def test_validation_gate_rejects_negative_score_thresholds(self):
        with self.assertRaisesRegex(ValueError, "negative score threshold"):
            render_validate.validation_gate(
                {"below_recall_min": 0, "mean_recall": 1.0},
                {"min_mean_recall": -0.1},
            )

    def test_validation_gate_rejects_bounded_score_thresholds_above_one(self):
        with self.assertRaisesRegex(ValueError, "score threshold above one"):
            render_validate.validation_gate(
                {"below_recall_min": 0, "mean_recall": 1.0},
                {"min_mean_recall": 1.1},
            )

    def test_validation_report_rejects_non_finite_recall_min(self):
        with self.assertRaisesRegex(ValueError, "non-finite recall threshold"):
            render_validate.validation_report([], recall_min=float("nan"))

    def test_validation_report_rejects_negative_recall_min(self):
        with self.assertRaisesRegex(ValueError, "negative recall threshold"):
            render_validate.validation_report([], recall_min=-0.1)

    def test_validation_report_rejects_recall_min_above_one(self):
        with self.assertRaisesRegex(ValueError, "recall threshold above one"):
            render_validate.validation_report([], recall_min=1.1)

    def test_json_report_payload_rejects_non_finite_values(self):
        with self.assertRaisesRegex(ValueError, "Out of range float values"):
            render_validate.json_report_payload(
                {"summary": {"mean_recall": float("nan")}}
            )

    def test_validation_report_summarizes_multi_page_visual_metrics(self):
        rows = [
            render_validate.ValidationRow(
                document="first.docx",
                status="pass",
                recall=1.0,
                mean_page_ahash_similarity=0.8,
                foreground_ink_iou=0.6,
                compared_pages=2,
                unmatched_candidate_pages=1,
                unmatched_reference_pages=0,
                capped_matched_pages=3,
            ),
            render_validate.ValidationRow(
                document="second.docx",
                status="pass",
                recall=1.0,
                mean_page_ahash_similarity=0.4,
                foreground_ink_iou=0.2,
                compared_pages=1,
                unmatched_candidate_pages=0,
                unmatched_reference_pages=2,
                capped_matched_pages=0,
            ),
        ]

        report = render_validate.validation_report(
            rows,
            recall_min=0.8,
            thresholds={
                "min_mean_page_ahash_similarity": 0.7,
                "min_mean_foreground_ink_iou": 0.3,
            },
            visual_settings={
                "dpi": 144,
                "page_cap": 5,
                "foreground_threshold": 240,
                "ahash_size": 16,
                "font_mode": "fixed-noto-subsets",
            },
        )

        self.assertEqual(report["summary"]["mean_page_ahash_similarity"], 0.6)
        self.assertEqual(report["summary"]["mean_foreground_ink_iou"], 0.4)
        self.assertEqual(report["summary"]["compared_pages"], 3)
        self.assertEqual(report["summary"]["unmatched_candidate_pages"], 1)
        self.assertEqual(report["summary"]["unmatched_reference_pages"], 2)
        self.assertEqual(report["summary"]["capped_matched_pages"], 3)
        self.assertEqual(
            report["visual_comparison"],
            {
                "dpi": 144,
                "page_cap": 5,
                "foreground_threshold": 240,
                "ahash_size": 16,
                "font_mode": "fixed-noto-subsets",
                "integer_metrics": render_validate.integer_metric_contract(),
            },
        )
        checks = {check["metric"]: check for check in report["gate"]["checks"]}
        self.assertFalse(checks["mean_page_ahash_similarity"]["passed"])
        self.assertTrue(checks["mean_foreground_ink_iou"]["passed"])

    def test_validation_report_rejects_invalid_visual_metrics_and_counts(self):
        for field, value, message in (
            ("mean_page_ahash_similarity", 1.1, "metric is out of range"),
            ("foreground_ink_iou", float("nan"), "metric is invalid"),
            ("compared_pages", -1, "count is invalid"),
            ("unmatched_candidate_pages", 1.5, "count is invalid"),
            ("unmatched_reference_pages", True, "count is invalid"),
            ("capped_matched_pages", -1, "count is invalid"),
        ):
            with self.subTest(field=field):
                row = render_validate.ValidationRow(
                    document="sample.docx",
                    status="pass",
                    recall=1.0,
                    **{field: value},
                )
                with self.assertRaisesRegex(ValueError, message):
                    render_validate.validation_report([row], recall_min=0.8)

    def test_later_page_metric_can_fail_gate_while_legacy_hash_stays_perfect(self):
        row = render_validate.ValidationRow(
            document="later-page.docx",
            status="pass",
            recall=1.0,
            ahash_similarity=1.0,
            mean_page_ahash_similarity=0.5,
            foreground_ink_iou=0.5,
            compared_pages=2,
            unmatched_candidate_pages=0,
            unmatched_reference_pages=0,
            capped_matched_pages=0,
        )

        report = render_validate.validation_report(
            [row],
            recall_min=0.8,
            thresholds={"min_mean_page_ahash_similarity": 0.9},
        )

        self.assertEqual(report["summary"]["mean_ahash_similarity"], 1.0)
        self.assertFalse(report["gate"]["passed"])

    def test_render_rwml_uses_fixed_fonts_by_default(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            source = root / "sample.docx"
            output = root / "sample.pdf"
            report = root / "sample.json"
            source.write_bytes(b"docx")
            rustup_bin = root / ".cargo" / "bin"
            rustup_bin.mkdir(parents=True)
            cargo = rustup_bin / ("cargo.exe" if os.name == "nt" else "cargo")
            cargo.write_bytes(b"")

            def completed(command, capture_output, env):
                self.assertTrue(capture_output)
                self.assertEqual(
                    env["PATH"].split(os.pathsep)[0],
                    str(rustup_bin),
                )
                output.write_bytes(b"%PDF-1.7")
                report.write_text('{"warnings": []}', encoding="utf-8")
                return mock.Mock(returncode=0)

            with mock.patch.object(render_validate.Path, "home", return_value=root), \
                 mock.patch.object(
                     render_validate.subprocess, "run", side_effect=completed
                 ) as run:
                self.assertEqual(
                    render_validate.render_rwml(source, output, report),
                    {"warnings": []},
                )

        command = run.call_args.args[0]
        self.assertIn("--fixed-fonts", command)


@unittest.skipIf(render_validate.Image is None, "Pillow is not installed")
class RenderValidateImageMetricTests(unittest.TestCase):
    @staticmethod
    def page(size=(200, 200), rectangle=None, fill=255):
        image = render_validate.Image.new("L", size, color=fill)
        if rectangle is not None:
            image.paste(0, rectangle)
        return image

    def test_later_page_change_is_visible_when_first_page_hash_is_unchanged(self):
        first = self.page(rectangle=(20, 20, 80, 50))
        reference_second = self.page(rectangle=(20, 100, 100, 130))
        candidate_second = self.page(rectangle=(100, 100, 180, 130))

        self.assertEqual(render_validate.image_hash_similarity(first, first), 1.0)
        metrics = render_validate.compare_page_images(
            [first, reference_second],
            [first, candidate_second],
            page_cap=8,
            foreground_threshold=245,
            ahash_size=16,
        )

        self.assertEqual(metrics.compared_pages, 2)
        self.assertLess(metrics.mean_page_ahash_similarity, 1.0)
        self.assertEqual(metrics.foreground_ink_iou, 0.5)
        self.assertEqual(metrics.integer_visual_metrics["pages"], 2)
        self.assertEqual(metrics.integer_visual_metrics["pixels"], 80_000)
        self.assertLess(
            metrics.integer_visual_metrics["foreground_f1_ppm"], 1_000_000
        )

    def test_foreground_iou_detects_small_ink_displacement_on_white_page(self):
        reference = self.page(size=(400, 400), rectangle=(20, 20, 60, 25))
        candidate = self.page(size=(400, 400), rectangle=(80, 20, 120, 25))

        self.assertEqual(
            render_validate.foreground_ink_iou_images(
                reference, candidate, threshold=245
            ),
            0.0,
        )

    def test_blank_threshold_and_page_size_normalization_are_explicit(self):
        blank = self.page()
        near_blank = self.page(fill=245)
        foreground = self.page(fill=244)
        self.assertEqual(
            render_validate.foreground_ink_iou_images(blank, near_blank, threshold=245),
            1.0,
        )
        self.assertEqual(
            render_validate.foreground_ink_iou_images(blank, foreground, threshold=245),
            0.0,
        )

        small = self.page(size=(100, 100), rectangle=(10, 10, 20, 20))
        large = self.page(size=(120, 140), rectangle=(10, 10, 20, 20))
        normalized_small, normalized_large = render_validate.normalize_page_pair(
            small, large
        )
        self.assertEqual(normalized_small.size, (120, 140))
        self.assertEqual(normalized_large.size, (120, 140))
        self.assertEqual(small.size, (100, 100))
        self.assertEqual(
            render_validate.foreground_ink_iou_images(small, large, threshold=245),
            1.0,
        )

    def test_page_cap_and_unmatched_counts_are_not_silent(self):
        blank = self.page()
        metrics = render_validate.compare_page_images(
            [blank] * 5,
            [blank] * 3,
            page_cap=2,
            foreground_threshold=245,
            ahash_size=16,
        )

        self.assertEqual(metrics.compared_pages, 2)
        self.assertEqual(metrics.unmatched_candidate_pages, 0)
        self.assertEqual(metrics.unmatched_reference_pages, 2)
        self.assertEqual(metrics.capped_matched_pages, 1)
        self.assertEqual(metrics.mean_page_ahash_similarity, 1.0)
        self.assertEqual(metrics.foreground_ink_iou, 1.0)
        self.assertEqual(metrics.integer_visual_metrics["pages"], 2)
        self.assertEqual(metrics.integer_visual_metrics["similarity_ppm"], 1_000_000)

    def test_validation_report_recomputes_integer_visual_summary(self):
        exact = render_validate.integer_image_metrics(
            b"\xff\xff\xff", b"\xff\xff\xff", 1, 1
        )
        changed = render_validate.integer_image_metrics(
            b"\xff\xff\xff", b"\x00\x00\x00", 1, 1
        )
        rows = [
            render_validate.ValidationRow(
                document="exact.docx",
                status="pass",
                recall=1.0,
                compared_pages=1,
                integer_visual_metrics=exact,
            ),
            render_validate.ValidationRow(
                document="changed.docx",
                status="pass",
                recall=1.0,
                compared_pages=1,
                integer_visual_metrics=changed,
            ),
        ]

        report = render_validate.validation_report(rows, recall_min=0.8)

        aggregate = report["integer_visual_metrics"]
        self.assertEqual(aggregate["pages"], 2)
        self.assertEqual(aggregate["pixels"], 2)
        self.assertEqual(aggregate["changed_pixels"], 1)
        self.assertEqual(aggregate["similarity_ppm"], 500_000)

    def test_validation_report_rejects_partial_integer_visual_evidence(self):
        exact = render_validate.integer_image_metrics(
            b"\xff\xff\xff", b"\xff\xff\xff", 1, 1
        )
        rows = [
            render_validate.ValidationRow(
                document="with-metrics.docx",
                status="pass",
                recall=1.0,
                compared_pages=1,
                integer_visual_metrics=exact,
            ),
            render_validate.ValidationRow(
                document="without-metrics.docx",
                status="pass",
                recall=1.0,
            ),
        ]

        with self.assertRaisesRegex(ValueError, "integer visual evidence is partial"):
            render_validate.validation_report(rows, recall_min=0.8)

    def test_pymupdf_page_diagnostics_are_canonical_and_content_free(self):
        page = types.SimpleNamespace(
            rect=types.SimpleNamespace(width=612.0005, height=792),
            mediabox=types.SimpleNamespace(x0=-1, y0=-2, x1=611, y1=790),
            cropbox=types.SimpleNamespace(x0=0, y0=0, x1=612, y1=792),
            rotation=90,
            get_text=lambda: "Cafe\u0301 \u200fשלום",
        )

        geometry = render_validate.pymupdf_page_geometry(page)
        tokens = render_validate.pymupdf_page_semantic_tokens(
            page, max_codepoints=64, max_tokens=8
        )

        self.assertEqual(geometry["page_width_millipoints"], 612_001)
        self.assertEqual(geometry["media_x0_millipoints"], -1_000)
        self.assertEqual(geometry["rotation_degrees"], 90)
        self.assertEqual(tokens, ("Café", "שלום"))

    def test_pymupdf_word_records_build_bounded_word_and_line_boxes(self):
        records = [
            (0, 0, 10, 10, "first", 0, 0, 0),
            (12, 0, 22, 10, "word", 0, 0, 1),
            (0, 20, 10, 30, "next", 0, 1, 0),
        ]
        page = types.SimpleNamespace(
            get_text=lambda kind, sort=False: records if kind == "words" else ""
        )

        words, lines, used_codepoints, used_tokens = (
            render_validate.pymupdf_page_text_boxes(
                page,
                max_items=8,
                max_codepoints=64,
                max_tokens=8,
            )
        )

        self.assertEqual(len(words), 3)
        self.assertEqual(len(lines), 2)
        self.assertEqual(lines[0].tokens, ("first", "word"))
        self.assertEqual(lines[0].bbox_millipoints, (0, 0, 22_000, 10_000))
        self.assertEqual(used_codepoints, 13)
        self.assertEqual(used_tokens, 3)

    def test_validation_report_recomputes_pdf_diagnostic_aggregates(self):
        integer = render_validate.integer_image_metrics(
            b"\xff\xff\xff", b"\xff\xff\xff", 1, 1
        )
        reference = render_validate.canonical_pdf_page_geometry(
            page_size=(612, 792),
            media_box=(0, 0, 612, 792),
            crop_box=(0, 0, 612, 792),
            rotation_degrees=0,
        )
        candidate = render_validate.canonical_pdf_page_geometry(
            page_size=(612.001, 792),
            media_box=(0, 0, 612.001, 792),
            crop_box=(0, 0, 612.001, 792),
            rotation_degrees=0,
        )
        geometry = render_validate.pdf_geometry_report(
            [render_validate.pdf_page_geometry_metrics(reference, candidate)]
        )
        semantic = render_validate.pdf_semantic_report(
            [render_validate.pdf_semantic_metrics(("a", "b"), ("a", "c"))]
        )
        reference_box = render_validate.canonical_pdf_text_box(
            ("label",), (0, 0, 10, 10)
        )
        candidate_box = render_validate.canonical_pdf_text_box(
            ("label",), (0.001, 0, 10.001, 10)
        )
        text_geometry = render_validate.pdf_text_geometry_report(
            [
                render_validate.pdf_text_geometry_page(
                    [reference_box],
                    [candidate_box],
                    [reference_box],
                    [candidate_box],
                )
            ]
        )
        row = render_validate.ValidationRow(
            document="diagnostic.docx",
            status="pass",
            recall=1.0,
            compared_pages=1,
            integer_visual_metrics=integer,
            pdf_point_geometry=geometry,
            semantic_text_metrics=semantic,
            text_geometry_metrics=text_geometry,
        )

        report = render_validate.validation_report([row], recall_min=0.8)

        self.assertEqual(report["pdf_diagnostic_contract"]["content_retained"], False)
        self.assertEqual(report["pdf_point_geometry"]["pages"], 1)
        self.assertEqual(report["pdf_point_geometry"]["max_abs_delta_millipoints"], 1)
        self.assertEqual(report["semantic_text_metrics"]["pages"], 1)
        self.assertEqual(report["semantic_text_metrics"]["semantic_token_f1_ppm"], 500_000)
        self.assertEqual(report["text_geometry_metrics"]["pages"], 1)
        self.assertEqual(report["text_geometry_metrics"]["word_boxes"]["matched_items"], 1)

    def test_validation_report_rejects_partial_pdf_diagnostics(self):
        integer = render_validate.integer_image_metrics(
            b"\xff\xff\xff", b"\xff\xff\xff", 1, 1
        )
        reference = render_validate.canonical_pdf_page_geometry(
            page_size=(612, 792),
            media_box=(0, 0, 612, 792),
            crop_box=(0, 0, 612, 792),
            rotation_degrees=0,
        )
        geometry = render_validate.pdf_geometry_report(
            [render_validate.pdf_page_geometry_metrics(reference, reference)]
        )
        rows = [
            render_validate.ValidationRow(
                document="complete.docx",
                status="pass",
                recall=1.0,
                compared_pages=1,
                integer_visual_metrics=integer,
                pdf_point_geometry=geometry,
            ),
            render_validate.ValidationRow(
                document="missing.docx",
                status="pass",
                recall=1.0,
                compared_pages=1,
                integer_visual_metrics=integer,
            ),
        ]

        with self.assertRaisesRegex(ValueError, "PDF point geometry is partial"):
            render_validate.validation_report(rows, recall_min=0.8)

    def test_raster_failures_raise_an_explicit_metric_error(self):
        broken_fitz = mock.Mock()
        broken_fitz.open.side_effect = RuntimeError("bad xref")
        with mock.patch.object(render_validate, "fitz", broken_fitz):
            with self.assertRaisesRegex(
                render_validate.VisualMetricError,
                "rasterization failed.*bad xref",
            ):
                render_validate.rasterize_pdf_pages(
                    pathlib.Path("broken.pdf"), dpi=110, page_cap=4
                )

    def test_oversized_raster_and_normalized_canvas_are_rejected_before_allocation(self):
        with self.assertRaisesRegex(
            render_validate.VisualMetricError, "raster page.*pixel safety limit"
        ):
            render_validate.ensure_pixel_budget(
                10_000,
                10_000,
                render_validate.MAX_RASTER_PAGE_PIXELS,
                "raster page",
            )
        with self.assertRaisesRegex(
            render_validate.VisualMetricError, "normalized canvas.*pixel safety limit"
        ):
            render_validate.ensure_pixel_budget(
                10_000,
                10_000,
                render_validate.MAX_NORMALIZED_CANVAS_PIXELS,
                "normalized canvas",
            )


if __name__ == "__main__":
    unittest.main()


class ConformingTextExtractionTests(unittest.TestCase):
    """Text recovered the way a conforming PDF reader recovers it."""

    def cmap(self, body: str) -> bytes:
        return (
            "/CIDInit /ProcSet findresource begin\nbegincmap\n"
            "1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n"
            f"{body}\nendcmap\nend\n"
        ).encode("latin-1")

    def test_parse_tounicode_cmap_reads_bfchar_entries(self):
        data = self.cmap("2 beginbfchar\n<0001> <0631>\n<0002> <0635>\nendbfchar")
        self.assertEqual(
            render_validate.parse_tounicode_cmap(data),
            {"width": 2, "map": {1: "ر", 2: "ص"}},
        )

    def test_parse_tounicode_cmap_reads_both_bfrange_forms(self):
        data = self.cmap(
            "2 beginbfrange\n<0010> <0012> <0041>\n"
            "<0020> <0021> [<0058> <0059>]\nendbfrange"
        )
        parsed = render_validate.parse_tounicode_cmap(data)["map"]
        self.assertEqual(parsed[0x10], "A")
        self.assertEqual(parsed[0x12], "C")
        self.assertEqual(parsed[0x20], "X")
        self.assertEqual(parsed[0x21], "Y")

    def test_parse_tounicode_cmap_reads_single_byte_codespace(self):
        data = (
            "begincmap\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n"
            "1 beginbfchar\n<41> <0041>\nendbfchar\nendcmap\n"
        ).encode("latin-1")
        self.assertEqual(
            render_validate.parse_tounicode_cmap(data), {"width": 1, "map": {0x41: "A"}}
        )

    def test_content_stream_text_maps_glyph_codes_without_inserting_gaps(self):
        cmaps = {"f0": {"width": 2, "map": {1: "ر", 2: "ص"}}}
        content = b"BT /f0 11 Tf 1 0 0 -1 20 30 Tm [(\x00\x01\x00\x02)] TJ ET"
        self.assertEqual(render_validate.content_stream_text(content, cmaps), "رص")

    def test_content_stream_text_prefers_actual_text_over_cluster_glyphs(self):
        cmaps = {"f0": {"width": 2, "map": {3: "X", 5: "ع"}}}
        content = (
            b"BT /f0 11 Tf /Span<</ActualText<FEFF0646>>>BDC "
            b"[(\x00\x03)] TJ (\x00\x04) Tj EMC (\x00\x05) Tj ET"
        )
        self.assertEqual(
            render_validate.content_stream_text(content, cmaps), "نع"
        )

    def test_content_stream_text_keeps_real_space_glyphs(self):
        cmaps = {
            "f0": {"width": 2, "map": {1: "a", 2: "b"}},
            "f1": {"width": 2, "map": {1: " "}},
        }
        content = (
            b"BT /f0 11 Tf (\x00\x01) Tj ET BT /f1 11 Tf (\x00\x01) Tj ET "
            b"BT /f0 11 Tf (\x00\x02) Tj ET"
        )
        self.assertEqual(render_validate.content_stream_text(content, cmaps), "a b")

    def test_content_stream_text_ignores_unmapped_codes_and_unknown_fonts(self):
        cmaps = {"f0": {"width": 2, "map": {1: "a"}}}
        content = b"BT /f0 11 Tf (\x00\x01\x00\x09) Tj ET BT /zz 11 Tf (\x00\x01) Tj ET"
        self.assertEqual(render_validate.content_stream_text(content, cmaps), "a")

    def test_content_stream_text_never_invents_text_from_malformed_input(self):
        self.assertEqual(render_validate.content_stream_text(b"", {}), "")
        self.assertEqual(
            render_validate.content_stream_text(b"BT /f0 11 Tf (unbalanced", {}), ""
        )


class RightToLeftReadingOrderTests(unittest.TestCase):
    def test_reverse_rtl_runs_restores_logical_order(self):
        self.assertEqual(render_validate.reverse_rtl_runs("رصنع"), "عنصر")
        self.assertEqual(render_validate.reverse_rtl_runs("ןנוקמ"), "מקונן")

    def test_reverse_rtl_runs_leaves_latin_untouched(self):
        self.assertEqual(render_validate.reverse_rtl_runs("Body item 4."), "Body item 4.")

    def test_reverse_rtl_runs_only_reverses_the_rtl_run(self):
        self.assertEqual(render_validate.reverse_rtl_runs("ab رصنع cd"), "ab عنصر cd")

    def test_candidate_tokens_never_add_text_absent_from_the_pdf(self):
        # Negative control: the conforming path reads only the content stream,
        # so a page that draws nothing contributes nothing.
        doc = mock.Mock()
        doc.__iter__ = lambda self: iter([])
        self.assertEqual(render_validate.conforming_tokens(doc), [])


class TextObjectBoundaryTests(unittest.TestCase):
    def test_content_stream_text_separates_text_objects(self):
        cmaps = {"f0": {"width": 2, "map": {1: "a", 2: "b"}}}
        content = b"BT /f0 11 Tf (\x00\x01) Tj ET BT /f0 11 Tf (\x00\x02) Tj ET"
        self.assertEqual(render_validate.content_stream_text(content, cmaps), "a b")

    def test_content_stream_text_keeps_one_text_object_intact(self):
        cmaps = {"f0": {"width": 2, "map": {1: "a", 2: "b"}}}
        content = b"BT /f0 11 Tf (\x00\x01) Tj (\x00\x02) Tj ET"
        self.assertEqual(render_validate.content_stream_text(content, cmaps), "ab")


class OracleStabilityTests(unittest.TestCase):
    """The reference renderer must be reproducible, or say so."""

    def test_stability_verdict_reports_matching_digests(self):
        self.assertEqual(
            render_validate.oracle_stability_verdict(["a", "b"], ["a", "b"]), True
        )

    def test_stability_verdict_reports_a_mismatch(self):
        self.assertEqual(
            render_validate.oracle_stability_verdict(["a", "b"], ["a", "c"]), False
        )

    def test_stability_verdict_is_unknown_without_two_samples(self):
        self.assertIsNone(render_validate.oracle_stability_verdict(["a"], []))
        self.assertIsNone(render_validate.oracle_stability_verdict([], ["a"]))

    def test_stability_verdict_reports_a_page_count_change(self):
        self.assertEqual(
            render_validate.oracle_stability_verdict(["a"], ["a", "b"]), False
        )

    def test_reference_pdf_font_identities_read_subset_sfnt_revision(self):
        payload = bytearray(64)
        payload[:4] = b"true"
        payload[4:6] = (1).to_bytes(2, "big")
        payload[12:16] = b"head"
        payload[20:24] = (32).to_bytes(4, "big")
        payload[24:28] = (16).to_bytes(4, "big")
        payload[36:40] = (132055).to_bytes(4, "big")

        page = mock.Mock()
        page.get_fonts.return_value = [
            (7, "ttf", "TrueType", "ABCDEF+NotoSans-Regular", "F1", "", 0)
        ]
        document = mock.MagicMock()
        document.__iter__.return_value = iter([page])
        document.extract_font.return_value = (
            "ABCDEF+NotoSans-Regular",
            "ttf",
            "TrueType",
            bytes(payload),
        )
        fake_fitz = mock.Mock()
        fake_fitz.open.return_value = document
        with mock.patch.object(render_validate, "fitz", fake_fitz):
            identities = render_validate.reference_pdf_font_identities(
                pathlib.Path("reference.pdf")
            )

        self.assertEqual(
            identities,
            [{"postscript_name": "NotoSans-Regular", "sfnt_revision": 132055}],
        )
        document.close.assert_called_once_with()

    def test_summary_records_the_reference_stability(self):
        rows = [
            render_validate.ValidationRow(
                document="d.docx",
                status="pass",
                recall=1.0,
                rwml_pages=1,
                reference_pages=1,
                page_ratio=1.0,
                ahash_similarity=1.0,
                render_warnings=0,
                render_warning_kinds=[],
            )
        ]
        report = render_validate.validation_report(rows, 0.97, reference_stable=False)
        self.assertIs(report["summary"]["reference_stable"], False)

    def test_required_reference_stability_is_a_gate(self):
        summary = {"below_recall_min": 0, "reference_stable": False}
        gate = render_validate.validation_gate(
            summary, {"require_reference_stable": True}
        )
        check = next(
            item for item in gate["checks"] if item["metric"] == "reference_stable"
        )
        self.assertEqual(check["actual"], 0)
        self.assertFalse(check["passed"])
        self.assertFalse(gate["passed"])

        summary["reference_stable"] = True
        gate = render_validate.validation_gate(
            summary, {"require_reference_stable": True}
        )
        check = next(
            item for item in gate["checks"] if item["metric"] == "reference_stable"
        )
        self.assertEqual(check["actual"], 1)
        self.assertTrue(check["passed"])
        self.assertTrue(gate["passed"])

        with self.assertRaisesRegex(ValueError, "must be a boolean"):
            render_validate.validation_gate(
                summary, {"require_reference_stable": 1}
            )
