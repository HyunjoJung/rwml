import hashlib
import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest

from scripts import render_evidence_metrics


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
    if render_evidence_metrics.numpy_module() is not None:
        tools.append({"name": "numpy", "version": "test-version"})
        tools.sort(key=lambda tool: tool["name"])
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


def valid_core_report() -> dict:
    integer_metrics = render_evidence_metrics.image_metrics_python(
        b"\xff\xff\xff", b"\xff\xff\xff", 1, 1
    )
    return {
        "visual_comparison": {
            "dpi": 110,
            "page_cap": 32,
            "foreground_threshold": 245,
            "ahash_size": 16,
            "font_mode": "fixed-noto-subsets",
            "integer_metrics": render_evidence_metrics.metric_contract(),
        },
        "integer_visual_metrics": integer_metrics,
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
                "unmatched_candidate_pages": 0,
                "unmatched_reference_pages": 0,
                "capped_matched_pages": 0,
                "render_warnings": 0,
                "render_warning_kinds": [],
                "integer_visual_metrics": integer_metrics,
            }
        ],
    }


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

        self.assertEqual(evidence["schema"], "rwml.render-oracle-evidence.v2")
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
                        "metric": "mean_recall",
                        "actual": 0.5,
                        "op": ">=",
                        "threshold": 0.9,
                        "passed": False,
                    }
                ],
            }

            with self.assertRaisesRegex(ValueError, "gate passed"):
                render_oracle_contract.validate_evidence_report(evidence, corpus)

    def test_evidence_rejects_invalid_or_inconsistent_integer_metrics(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(root, valid_manifest())
            )
            evidence = render_oracle_contract.bind_evidence_report(
                valid_core_report(), corpus, valid_environment()
            )
            evidence["rows"][0]["integer_visual_metrics"]["pixels"] = 2

            with self.assertRaisesRegex(ValueError, "integer visual"):
                render_oracle_contract.validate_evidence_report(evidence, corpus)

        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(root, valid_manifest())
            )
            evidence = render_oracle_contract.bind_evidence_report(
                valid_core_report(), corpus, valid_environment()
            )
            evidence["integer_visual_metrics"]["changed_pixels"] = 1

            with self.assertRaisesRegex(ValueError, "integer visual"):
                render_oracle_contract.validate_evidence_report(evidence, corpus)

    def test_numpy_metric_contract_requires_numpy_environment_identity(self):
        if render_evidence_metrics.numpy_module() is None:
            self.skipTest("NumPy is not installed")
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(root, valid_manifest())
            )
            environment = valid_environment()
            environment["tools"] = [
                tool for tool in environment["tools"] if tool["name"] != "numpy"
            ]

            with self.assertRaisesRegex(ValueError, "NumPy metric implementation"):
                render_oracle_contract.bind_evidence_report(
                    valid_core_report(), corpus, environment
                )

    def test_evidence_allows_candidate_to_reference_page_ratio_above_one(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            corpus = render_oracle_contract.load_corpus_manifest(
                write_manifest(root, valid_manifest())
            )
            core = valid_core_report()
            core["rows"][0]["rwml_pages"] = 2
            core["rows"][0]["page_ratio"] = 2.0
            core["summary"]["mean_page_ratio"] = 2.0

            evidence = render_oracle_contract.bind_evidence_report(
                core, corpus, valid_environment()
            )

        self.assertEqual(evidence["summary"]["mean_page_ratio"], 2.0)


if __name__ == "__main__":
    unittest.main()
