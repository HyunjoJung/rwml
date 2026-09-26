import hashlib
import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest

from scripts import render_evidence_metrics as metrics
from scripts import render_pdf_diagnostics as pdf_metrics

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "render_oracle_contract.py"
SPEC = importlib.util.spec_from_file_location("render_oracle_contract", SCRIPT)
render_oracle_contract = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = render_oracle_contract
SPEC.loader.exec_module(render_oracle_contract)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def valid_manifest(document: bytes = b"fixture") -> dict:
    return {
        "schema": "rwml.render-oracle-corpus.v1",
        "campaign": "test-campaign",
        "limits": {
            "max_documents": 4,
            "max_input_bytes": 1024,
            "max_total_input_bytes": 2048,
            "max_pages_per_document": 8,
        },
        "provenance": [
            {
                "id": "generated",
                "kind": "generated",
                "license": "MIT",
                "reference": "PROVENANCE.md",
            }
        ],
        "documents": [
            {
                "id": "fixture-basic",
                "path": "synthetic/fixture.docx",
                "format": "docx",
                "bytes": len(document),
                "sha256": sha256(document),
                "provenance": "generated",
                "features": ["paragraphs", "tables"],
                "expected": {"pages": 1, "warnings": []},
            }
        ],
    }


def write_manifest(root: pathlib.Path, data: dict, document: bytes = b"fixture") -> pathlib.Path:
    (root / "synthetic").mkdir()
    (root / "synthetic" / "fixture.docx").write_bytes(document)
    (root / "PROVENANCE.md").write_text("# Synthetic provenance\n", encoding="utf-8")
    manifest = root / "RENDER_ORACLE.json"
    manifest.write_text(
        json.dumps(data, ensure_ascii=True, indent=2) + "\n", encoding="utf-8"
    )
    return manifest


def valid_environment() -> dict:
    tools = [
        {"name": "pillow", "version": "12.3.0"},
        {"name": "pymupdf", "version": "1.28.2"},
        {"name": "python", "version": "3.13.14"},
    ]
    numpy = metrics.numpy_module()
    if numpy is not None:
        tools.insert(0, {"name": "numpy", "version": str(numpy.__version__)})
    return {
        "source_revision": "a" * 40,
        "source_dirty": False,
        "harness_sha256": "b" * 64,
        "cargo_lock_sha256": "c" * 64,
        "renderer": {"name": "rwml", "font_mode": "fixed-noto-subsets"},
        "oracle": {
            "name": "libreoffice",
            "mode": "local",
            "version": "LibreOffice 26.2.3.2",
            "identity_sha256": "d" * 64,
        },
        "platform": {
            "system": "Linux",
            "release": "6.8.0",
            "machine": "x86_64",
        },
        "tools": tools,
    }


def valid_pdf_row(pages=1) -> dict:
    geometry = pdf_metrics.canonical_page_geometry(
        page_size=(72, 72), media_box=(0, 0, 72, 72),
        crop_box=(0, 0, 72, 72), rotation_degrees=0,
    )
    box = pdf_metrics.canonical_text_box(("fixture",), (1, 1, 20, 10))
    return {
        "pdf_point_geometry": pdf_metrics.geometry_report([
            pdf_metrics.page_geometry_metrics(geometry, geometry)
        ] * pages),
        "semantic_text_metrics": pdf_metrics.semantic_report([
            pdf_metrics.semantic_metrics(("fixture",), ("fixture",))
        ] * pages),
        "text_geometry_metrics": pdf_metrics.text_geometry_report([
            pdf_metrics.text_geometry_page([box], [box], [box], [box])
        ] * pages),
    }


def valid_core_report() -> dict:
    integer_metrics = metrics.image_metrics(b"\xff\xff\xff", b"\xff\xff\xff", 1, 1)
    pdf_row = valid_pdf_row()
    return {
        "integer_visual_metrics": dict(integer_metrics),
        "pdf_diagnostic_contract": pdf_metrics.diagnostic_contract(),
        "pdf_point_geometry": pdf_row["pdf_point_geometry"]["summary"],
        "semantic_text_metrics": dict(pdf_row["semantic_text_metrics"]),
        "text_geometry_metrics": pdf_row["text_geometry_metrics"]["summary"],
        "visual_comparison": {
            "dpi": 110,
            "page_cap": 32,
            "foreground_threshold": 245,
            "ahash_size": 16,
            "font_mode": "fixed-noto-subsets",
            "integer_metrics": metrics.metric_contract(),
        },
        "summary": {
            "documents": 1,
            "measured": 1,
            "skipped": 0,
            "below_recall_min": 0,
            "recall_min": 0.97,
            "reference_stable": True,
            "unstable_references": [],
            "mean_recall": 1.0,
            "mean_page_ratio": 1.0,
            "mean_ahash_similarity": 1.0,
            "mean_page_ahash_similarity": 1.0,
            "mean_foreground_ink_iou": 1.0,
            "compared_pages": 1,
            "unmatched_candidate_pages": 0,
            "unmatched_reference_pages": 0,
            "capped_matched_pages": 0,
            "mean_render_warnings": 0.0,
        },
        "gate": {"passed": True, "checks": []},
        "rows": [
            {
                "document": "fixture.docx",
                "case_id": "fixture-basic",
                "input_bytes": 7,
                "input_sha256": sha256(b"fixture"),
                "status": "pass",
                "recall": 1.0,
                "rwml_pages": 1,
                "reference_pages": 1,
                "page_ratio": 1.0,
                "ahash_similarity": 1.0,
                "mean_page_ahash_similarity": 1.0,
                "foreground_ink_iou": 1.0,
                "compared_pages": 1,
                "integer_visual_metrics": integer_metrics,
                **pdf_row,
                "unmatched_candidate_pages": 0,
                "unmatched_reference_pages": 0,
                "capped_matched_pages": 0,
                "render_warnings": 0,
                "render_warning_kinds": [],
            }
        ],
    }


def page_core_report(candidate_pages: int, reference_pages: int, page_cap: int) -> dict:
    from scripts import render_validate

    compared = min(candidate_pages, reference_pages, page_cap)
    visual = render_validate.visual_metrics_from_scores(
        [1.0] * compared,
        [1.0] * compared,
        integer_pages=[valid_core_report()["integer_visual_metrics"]] * compared,
        **valid_pdf_row(compared),
        candidate_page_count=candidate_pages,
        reference_page_count=reference_pages,
        page_cap=page_cap,
    )
    row = valid_core_report()["rows"][0]
    row.update(
        rwml_pages=candidate_pages,
        reference_pages=reference_pages,
        page_ratio=round(candidate_pages / reference_pages, 4),
        **vars(visual),
    )
    settings = valid_core_report()["visual_comparison"]
    settings.pop("integer_metrics")
    settings["page_cap"] = page_cap
    return render_validate.validation_report(
        [render_validate.ValidationRow(**row)],
        recall_min=0.97,
        visual_settings=settings,
    )


class RenderOracleCorpusContractTests(unittest.TestCase):
    def test_public_oracle_lock_matches_the_established_render_inventory(self):
        corpus_root = ROOT / "corpus" / "public"
        corpus = render_oracle_contract.load_corpus_manifest(
            corpus_root / "RENDER_ORACLE.json"
        )
        tsv_paths = {
            line.split("\t", 1)[0]
            for line in (corpus_root / "RENDER_MANIFEST.tsv")
            .read_text(encoding="utf-8")
            .splitlines()
            if line and not line.startswith("#")
        }

        self.assertEqual(len(corpus.documents), 21)
        self.assertEqual(corpus.expected_pages, 26)
        self.assertEqual(
            {document.relative_path for document in corpus.documents}, tsv_paths
        )
        tsv_expected = {}
        for line in (corpus_root / "RENDER_MANIFEST.tsv").read_text(
            encoding="utf-8"
        ).splitlines():
            if not line or line.startswith("#"):
                continue
            relative, pages, warnings = line.split("\t")
            tsv_expected[relative] = (
                int(pages),
                () if warnings == "-" else tuple(sorted(warnings.split("|"))),
            )
        self.assertEqual(
            {
                document.relative_path: (
                    document.expected_pages,
                    document.expected_warnings,
                )
                for document in corpus.documents
            },
            tsv_expected,
        )
        self.assertEqual(
            sum(document.input_bytes for document in corpus.documents), 238180
        )

    def test_valid_manifest_binds_exact_input_identity(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            path = write_manifest(root, valid_manifest())

            corpus = render_oracle_contract.load_corpus_manifest(path)

        self.assertEqual(corpus.schema, "rwml.render-oracle-corpus.v1")
        self.assertEqual(corpus.campaign, "test-campaign")
        self.assertEqual(len(corpus.documents), 1)
        self.assertEqual(corpus.documents[0].case_id, "fixture-basic")
        self.assertEqual(corpus.documents[0].path.name, "fixture.docx")
        self.assertEqual(corpus.expected_pages, 1)
        self.assertRegex(corpus.manifest_sha256, r"\A[0-9a-f]{64}\Z")
        self.assertRegex(corpus.corpus_root_sha256, r"\A[0-9a-f]{64}\Z")

    def test_manifest_rejects_input_hash_or_size_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            path = write_manifest(root, valid_manifest())
            (root / "synthetic" / "fixture.docx").write_bytes(b"changed")

            with self.assertRaisesRegex(ValueError, "input identity mismatch"):
                render_oracle_contract.load_corpus_manifest(path)

    def test_manifest_rejects_duplicate_json_keys(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "synthetic").mkdir()
            (root / "synthetic" / "fixture.docx").write_bytes(b"fixture")
            path = root / "RENDER_ORACLE.json"
            path.write_text(
                '{"schema":"rwml.render-oracle-corpus.v1",'
                '"schema":"rwml.render-oracle-corpus.v1"}',
                encoding="utf-8",
            )

            with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
                render_oracle_contract.load_corpus_manifest(path)

    def test_manifest_rejects_excessive_json_depth_without_recursing(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "RENDER_ORACLE.json"
            path.write_text(
                '{"schema":' + "[" * 2000 + "0" + "]" * 2000 + "}",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(ValueError, "JSON depth limit"):
                render_oracle_contract.load_corpus_manifest(path)

    def test_manifest_rejects_unsafe_paths_and_unknown_keys(self):
        cases = [
            ("path", "../private.docx", "unsafe document path"),
            ("feature", "Tables", "feature label"),
            ("unknown", True, "document keys"),
        ]
        for kind, value, message in cases:
            with self.subTest(kind=kind):
                with tempfile.TemporaryDirectory() as tmp:
                    root = pathlib.Path(tmp)
                    data = valid_manifest()
                    if kind == "path":
                        data["documents"][0]["path"] = value
                    elif kind == "feature":
                        data["documents"][0]["features"][0] = value
                    else:
                        data["documents"][0]["unknown"] = value
                    path = write_manifest(root, data)

                    with self.assertRaisesRegex(ValueError, message):
                        render_oracle_contract.load_corpus_manifest(path)

    def test_manifest_rejects_unreferenced_or_missing_provenance(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            data = valid_manifest()
            data["documents"][0]["provenance"] = "missing"
            path = write_manifest(root, data)

            with self.assertRaisesRegex(ValueError, "unknown provenance"):
                render_oracle_contract.load_corpus_manifest(path)

    def test_manifest_rejects_document_and_byte_limit_overruns(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            data = valid_manifest()
            data["limits"]["max_input_bytes"] = 6
            path = write_manifest(root, data)

            with self.assertRaisesRegex(ValueError, "max_input_bytes"):
                render_oracle_contract.load_corpus_manifest(path)


class RenderOracleEvidenceContractTests(unittest.TestCase):
    def test_evidence_binds_manifest_environment_and_rows(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(root, valid_manifest())
            )

            evidence = render_oracle_contract.bind_evidence_report(
                valid_core_report(), corpus, valid_environment()
            )
            render_oracle_contract.validate_evidence_report(evidence, corpus)

        self.assertEqual(evidence["schema"], "rwml.render-oracle-evidence.v4")
        self.assertEqual(evidence["campaign"]["name"], "test-campaign")
        self.assertEqual(evidence["campaign"]["documents"], 1)
        self.assertEqual(evidence["campaign"]["expected_pages"], 1)
        self.assertNotIn("path", json.dumps(evidence["environment"]))

    def test_evidence_rejects_manifest_identity_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(root, valid_manifest())
            )
            evidence = render_oracle_contract.bind_evidence_report(
                valid_core_report(), corpus, valid_environment()
            )
            evidence["campaign"]["manifest_sha256"] = "0" * 64

            with self.assertRaisesRegex(ValueError, "campaign identity"):
                render_oracle_contract.validate_evidence_report(evidence, corpus)

    def test_evidence_rejects_missing_or_mismatched_case_rows(self):
        cases = [
            ("missing", None, "row coverage"),
            ("hash", "0" * 64, "row input identity"),
            ("path", "/" + "Users/example/private", "path-neutral"),
        ]
        for kind, value, message in cases:
            with self.subTest(kind=kind):
                with tempfile.TemporaryDirectory() as tmp:
                    root = pathlib.Path(tmp)
                    corpus = render_oracle_contract.load_corpus_manifest(
                        write_manifest(root, valid_manifest())
                    )
                    evidence = render_oracle_contract.bind_evidence_report(
                        valid_core_report(), corpus, valid_environment()
                    )
                    if kind == "missing":
                        evidence["rows"] = []
                    elif kind == "hash":
                        evidence["rows"][0]["input_sha256"] = value
                    else:
                        evidence["environment"]["oracle"]["version"] = value

                    with self.assertRaisesRegex(ValueError, message):
                        render_oracle_contract.validate_evidence_report(
                            evidence, corpus
                        )

    def test_evidence_rejects_incomplete_rows_and_inconsistent_summary(self):
        cases = [
            ("row", "rwml_pages", "row keys"),
            ("summary", "measured", "summary measured"),
            ("visual", "extra", "visual comparison keys"),
        ]
        for kind, key, message in cases:
            with self.subTest(kind=kind):
                with tempfile.TemporaryDirectory() as tmp:
                    root = pathlib.Path(tmp)
                    corpus = render_oracle_contract.load_corpus_manifest(
                        write_manifest(root, valid_manifest())
                    )
                    evidence = render_oracle_contract.bind_evidence_report(
                        valid_core_report(), corpus, valid_environment()
                    )
                    if kind == "row":
                        evidence["rows"][0].pop(key)
                    elif kind == "summary":
                        evidence["summary"][key] = 0
                    else:
                        evidence["visual_comparison"][key] = 1

                    with self.assertRaisesRegex(ValueError, message):
                        render_oracle_contract.validate_evidence_report(
                            evidence, corpus
                        )

    def test_evidence_rejects_gate_values_that_disagree_with_summary(self):
        cases = [
            ("below_recall_min", 0, "<=", 0, True),
            ("mean_recall", 1.0, ">=", 0.97, True),
            ("mean_recall", None, ">=", 0.97, False),
        ]
        with tempfile.TemporaryDirectory() as tmp:
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(pathlib.Path(tmp), valid_manifest())
            )
            for metric, actual, op, threshold, passed in cases:
                with self.subTest(metric=metric, actual=actual):
                    core = valid_core_report()
                    core["rows"][0].update(status="fail", recall=0.5)
                    core["summary"].update(below_recall_min=1, mean_recall=0.5)
                    core["gate"] = {
                        "passed": passed,
                        "checks": [
                            {
                                "metric": metric,
                                "actual": actual,
                                "op": op,
                                "threshold": threshold,
                                "passed": passed,
                            }
                        ],
                    }
                    with self.assertRaisesRegex(ValueError, "gate actual.*summary"):
                        render_oracle_contract.bind_evidence_report(
                            core, corpus, valid_environment()
                        )

    def test_evidence_rejects_gate_metrics_without_numeric_summary_values(self):
        with tempfile.TemporaryDirectory() as tmp:
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(pathlib.Path(tmp), valid_manifest())
            )
            for metric in (
                "unmeasured_score", "reference_stable", "unstable_references"
            ):
                with self.subTest(metric=metric):
                    core = valid_core_report()
                    core["gate"] = {
                        "passed": True,
                        "checks": [
                            {
                                "metric": metric,
                                "actual": 1,
                                "op": ">=",
                                "threshold": 1,
                                "passed": True,
                            }
                        ],
                    }
                    with self.assertRaisesRegex(ValueError, "gate metric.*summary"):
                        render_oracle_contract.bind_evidence_report(
                            core, corpus, valid_environment()
                        )

    def test_evidence_accepts_produced_passing_and_failing_gates(self):
        from scripts import render_validate

        thresholds = {
            "min_mean_recall": 0.97,
            "min_mean_page_ratio": 0.9,
            "max_mean_page_ratio": 1.1,
            "min_mean_ahash_similarity": 0.9,
            "min_mean_page_ahash_similarity": 0.9,
            "min_mean_foreground_ink_iou": 0.9,
            "max_mean_render_warnings": 0,
            "max_skipped": 0,
            "max_unmatched_candidate_pages": 0,
            "max_unmatched_reference_pages": 0,
        }
        with tempfile.TemporaryDirectory() as tmp:
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(pathlib.Path(tmp), valid_manifest())
            )
            for recall in (1.0, 0.5):
                with self.subTest(recall=recall):
                    core = valid_core_report()
                    passed = recall >= core["summary"]["recall_min"]
                    core["rows"][0].update(
                        status="pass" if passed else "fail", recall=recall
                    )
                    core["summary"].update(
                        mean_recall=recall, below_recall_min=int(not passed)
                    )
                    core["gate"] = render_validate.validation_gate(
                        core["summary"], thresholds
                    )
                    evidence = render_oracle_contract.bind_evidence_report(
                        core, corpus, valid_environment()
                    )
                    self.assertEqual(len(evidence["gate"]["checks"]), 11)
                    self.assertIs(evidence["gate"]["passed"], passed)

    def test_evidence_accepts_missing_measurements_as_failed_gate_checks(self):
        from scripts import render_validate

        core = render_validate.validation_report(
            [
                render_validate.ValidationRow(
                    document="fixture.docx", status="skip", reason="render-failed"
                )
            ],
            recall_min=0.97,
            thresholds={"min_mean_recall": 0.97, "max_skipped": 0},
            integer_metrics=True,
            pdf_diagnostics=True,
        )
        core["rows"][0].update(
            case_id="fixture-basic", input_bytes=7, input_sha256=sha256(b"fixture")
        )
        with tempfile.TemporaryDirectory() as tmp:
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(pathlib.Path(tmp), valid_manifest())
            )
            evidence = render_oracle_contract.bind_evidence_report(
                core, corpus, valid_environment()
            )
        checks = {check["metric"]: check for check in evidence["gate"]["checks"]}
        self.assertIsNone(checks["mean_recall"]["actual"])
        self.assertFalse(checks["mean_recall"]["passed"])
        self.assertEqual(checks["skipped"]["actual"], 1)
        self.assertFalse(evidence["gate"]["passed"])

    def test_evidence_rejects_a_contradictory_gate(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(root, valid_manifest())
            )
            evidence = render_oracle_contract.bind_evidence_report(
                valid_core_report(), corpus, valid_environment()
            )
            evidence["gate"] = {
                "passed": True,
                "checks": [
                    {
                        "metric": "mean_page_ratio",
                        "actual": 1.0,
                        "op": "<=",
                        "threshold": 0.9,
                        "passed": False,
                    }
                ],
            }

            with self.assertRaisesRegex(ValueError, "gate passed"):
                render_oracle_contract.validate_evidence_report(evidence, corpus)

    def test_evidence_allows_candidate_to_reference_page_ratio_above_one(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(root, valid_manifest())
            )
            core = valid_core_report()
            core["rows"][0]["rwml_pages"] = 2
            core["rows"][0]["page_ratio"] = 2.0
            core["rows"][0]["unmatched_candidate_pages"] = 1
            core["summary"]["mean_page_ratio"] = 2.0
            core["summary"]["unmatched_candidate_pages"] = 1

            evidence = render_oracle_contract.bind_evidence_report(
                core, corpus, valid_environment()
            )

        self.assertEqual(evidence["summary"]["mean_page_ratio"], 2.0)

    def test_evidence_rejects_inconsistent_page_accounting(self):
        cases = [
            ("rwml_pages", 2, None),
            ("reference_pages", 2, None),
            ("rwml_pages", 0, None),
            ("reference_pages", 0, None),
            ("page_ratio", 1.01, "mean_page_ratio"),
            ("compared_pages", 0, "compared_pages"),
            ("compared_pages", 2, "compared_pages"),
            ("unmatched_candidate_pages", 1, "unmatched_candidate_pages"),
            ("unmatched_reference_pages", 1, "unmatched_reference_pages"),
            ("capped_matched_pages", 1, "capped_matched_pages"),
        ]
        with tempfile.TemporaryDirectory() as tmp:
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(pathlib.Path(tmp), valid_manifest())
            )
            for key, value, summary_key in cases:
                with self.subTest(key=key, value=value):
                    core = valid_core_report()
                    core["rows"][0][key] = value
                    if summary_key is not None:
                        core["summary"][summary_key] = value
                    with self.assertRaisesRegex(ValueError, "page"):
                        render_oracle_contract.bind_evidence_report(
                            core, corpus, valid_environment()
                        )

    def test_evidence_accepts_produced_page_accounting_and_rounding(self):
        cases = [
            (1, 1, 32, 1.0),
            (2, 1, 32, 2.0),
            (1, 2, 32, 0.5),
            (2, 3, 32, 0.6667),
            (3, 2, 32, 1.5),
            (5, 3, 2, 1.6667),
            (3, 5, 2, 0.6),
            (3, 3, 2, 1.0),
            (1, 1, 1, 1.0),
        ]
        with tempfile.TemporaryDirectory() as tmp:
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(pathlib.Path(tmp), valid_manifest())
            )
            for candidate_pages, reference_pages, page_cap, ratio in cases:
                with self.subTest(pages=(candidate_pages, reference_pages, page_cap)):
                    evidence = render_oracle_contract.bind_evidence_report(
                        page_core_report(candidate_pages, reference_pages, page_cap),
                        corpus,
                        valid_environment(),
                    )
                    row = evidence["rows"][0]
                    self.assertEqual(row["page_ratio"], ratio)
                    accounted = row["compared_pages"] + row["capped_matched_pages"]
                    self.assertEqual(
                        accounted + row["unmatched_candidate_pages"], candidate_pages
                    )
                    self.assertEqual(
                        accounted + row["unmatched_reference_pages"], reference_pages
                    )

    def test_evidence_rejects_page_accounting_that_ignores_the_cap(self):
        with tempfile.TemporaryDirectory() as tmp:
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(pathlib.Path(tmp), valid_manifest())
            )
            core = page_core_report(5, 3, 2)
            for key, value in (("compared_pages", 3), ("capped_matched_pages", 0)):
                core["rows"][0][key] = value
                core["summary"][key] = value
            with self.assertRaisesRegex(ValueError, "page"):
                render_oracle_contract.bind_evidence_report(
                    core, corpus, valid_environment()
                )


if __name__ == "__main__":
    unittest.main()
