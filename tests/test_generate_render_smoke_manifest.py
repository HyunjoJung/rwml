import contextlib
import importlib.util
import io
import pathlib
import sys
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "generate_render_smoke_manifest.py"
SOURCE = ROOT / "corpus" / "public" / "RENDER_ORACLE.json"
OUTPUT = ROOT / "corpus" / "public" / "RENDER_SMOKE_ORACLE.json"


def load_module(name: str, path: pathlib.Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


generator = load_module("generate_render_smoke_manifest", SCRIPT)


EXPECTED_CASE_IDS = (
    "python-docx-test",
    "synthetic-fields",
    "synthetic-floating-wrap-policy",
    "synthetic-floating-z-order-pair",
    "synthetic-kitchen-sink",
    "synthetic-pagination-keep",
    "synthetic-revisions",
    "synthetic-rtl-table",
    "synthetic-style-hidden-tabs-table",
    "synthetic-table-cell-lists",
    "synthetic-two-columns",
    "synthetic-unsupported-objects",
)


class RenderSmokeManifestTests(unittest.TestCase):
    def test_checked_in_smoke_manifest_is_exact_and_strict(self):
        self.assertEqual(generator.SMOKE_CASE_IDS, EXPECTED_CASE_IDS)
        self.assertEqual(OUTPUT.read_bytes(), generator.expected_manifest_bytes())
        smoke = generator.load_corpus_manifest(OUTPUT)
        parent = generator.load_corpus_manifest(SOURCE)

        self.assertEqual(smoke.campaign, "public-corpus-smoke-v1")
        self.assertEqual(
            tuple(document.case_id for document in smoke.documents),
            EXPECTED_CASE_IDS,
        )
        self.assertEqual(len(smoke.documents), 12)
        self.assertEqual(smoke.expected_pages, 15)
        self.assertEqual(sum(document.input_bytes for document in smoke.documents), 69027)

        parent_by_id = {document.case_id: document for document in parent.documents}
        for document in smoke.documents:
            source = parent_by_id[document.case_id]
            self.assertEqual(document.relative_path, source.relative_path)
            self.assertEqual(document.input_bytes, source.input_bytes)
            self.assertEqual(document.sha256, source.sha256)
            self.assertEqual(document.provenance, source.provenance)
            self.assertEqual(document.features, source.features)
            self.assertEqual(document.expected_pages, source.expected_pages)
            self.assertEqual(document.expected_warnings, source.expected_warnings)

    def test_profile_summary_exposes_coverage_and_omissions(self):
        summary = generator.profile_summary()
        self.assertEqual(summary["documents"], 12)
        self.assertEqual(summary["expected_pages"], 15)
        self.assertEqual(summary["input_bytes"], 69027)
        self.assertEqual(summary["parent_features"], 37)
        self.assertEqual(summary["covered_features"], 35)
        self.assertEqual(
            summary["omitted_features"],
            ["alternate-content", "top-bottom-wrap"],
        )
        self.assertEqual(
            summary["expected_warning_kinds"],
            [
                "ChartsPreservedButNotModeled",
                "FloatingShapePlaceholderOnly",
                "OleObjectsPreservedButNotModeled",
                "UnsupportedFieldEvaluation",
                "UnsupportedMetafileImages",
            ],
        )

    def test_check_rejects_stale_and_symlinked_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            output = root / "RENDER_SMOKE_ORACLE.json"
            generator.refresh(output)
            self.assertTrue(generator.check(output))

            output.write_bytes(output.read_bytes() + b" ")
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertFalse(generator.check(output))

            output.unlink()
            output.symlink_to(OUTPUT)
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertFalse(generator.check(output))


if __name__ == "__main__":
    unittest.main()
