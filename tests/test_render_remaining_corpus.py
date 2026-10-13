import hashlib
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import zipfile

from scripts import generate_render_interaction_corpus as interaction
from scripts import generate_render_legacy_corpus as legacy
from scripts import public_hygiene_audit as hygiene
from scripts import render_corpus_batch as batch


ROOT = Path(__file__).resolve().parents[1]
COHORTS = (
    (legacy, 3, "1b36e3ae41d0a1d8575f54a65d5d49cd87c356dc46a6e286747699e2c8db7339"),
    (interaction, 197, "2dcdd5afbb2e1ebce16af80797670a96a2672f3de36152fdffe417575e52fe9e"),
)


class RemainingRenderCorpusTests(unittest.TestCase):
    def test_legacy_closure_binds_the_strict_reader_and_validator(self):
        original = legacy._read_source
        contract = ROOT / "scripts/render_oracle_contract.py"
        before = legacy.build_lock()["generator_closure_sha256"]
        with mock.patch.object(legacy, "_read_source", side_effect=lambda path, maximum:
            original(path, maximum) + (b"\n# changed contract\n" if path == contract else b"")):
            self.assertNotEqual(before, legacy.build_lock()["generator_closure_sha256"])

    def test_interaction_closure_binds_the_strict_validator(self):
        original = Path.read_bytes
        contract = ROOT / "scripts/render_oracle_contract.py"
        before = interaction.build_lock()["generator_closure_sha256"]
        with mock.patch.object(Path, "read_bytes", autospec=True, side_effect=lambda path:
            original(path) + (b"\n# changed contract\n" if path == contract else b"")):
            self.assertNotEqual(before, interaction.build_lock()["generator_closure_sha256"])

    def test_oversized_legacy_source_is_rejected_before_a_bulk_read(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "oversized.doc"
            source.write_bytes(b"x" * (legacy.MAX_CASE_BYTES + 1))
            with mock.patch.object(legacy, "ROOT", root), mock.patch.object(
                Path, "read_bytes", side_effect=AssertionError("unbounded source read")
            ):
                with self.assertRaises(ValueError):
                    legacy._read_source(source, legacy.MAX_CASE_BYTES)

    def test_invalid_legacy_manifest_never_reaches_the_inventory_parser(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "manifest.tsv"
            for content in (b"x" * (legacy.MAX_CASE_BYTES + 1), b"\xff"):
                source.write_bytes(content)
                with self.subTest(content_bytes=len(content)), mock.patch.object(
                    legacy, "ROOT", root
                ), mock.patch.object(legacy, "BENCHMARK_MANIFEST", source), mock.patch.object(
                    legacy, "legacy_benchmark_inputs"
                ) as parser:
                    with self.assertRaises(ValueError):
                        legacy.case_specs()
                    parser.assert_not_called()

    def test_empty_symlink_directory_and_outside_legacy_sources_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inside = root / "repo"
            inside.mkdir()
            empty = inside / "empty"
            empty.touch()
            outside = root / "outside"
            outside.write_bytes(b"retained")
            link = inside / "link"
            link.symlink_to(outside)
            with mock.patch.object(legacy, "ROOT", inside):
                for source in (empty, link, inside, outside):
                    with self.subTest(source=source.name), self.assertRaises(ValueError):
                        legacy._read_source(source, 128)
            self.assertEqual(outside.read_bytes(), b"retained")

    def test_script_and_package_modes_preserve_complete_output_maps(self):
        for module, count, digest in COHORTS:
            with self.subTest(cohort=module.CAMPAIGN), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                outputs = []
                for index, command in enumerate(((module.__file__,), ("-m", module.__name__))):
                    output = root / f"run-{index}"
                    for action in (("--check",), ("--output", str(output))):
                        result = subprocess.run([sys.executable, "-B", *command, *action],
                            cwd=ROOT, capture_output=True, text=True, timeout=30)
                        self.assertEqual(result.returncode, 0, result.stderr)
                    mapping = {p.relative_to(output).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                        for p in output.rglob("*") if p.is_file()}
                    self.assertEqual(len(mapping), count + 2)
                    self.assertEqual(hashlib.sha256(batch.canonical_json(mapping)).hexdigest(), digest)
                    corpus = batch.load_corpus_manifest(output / "RENDER_ORACLE.json")
                    self.assertEqual(len(corpus.documents), count)
                    self.assertEqual(corpus.expected_pages, count)
                    repeated = subprocess.run([sys.executable, "-B", *command, "--output", str(output)],
                        cwd=ROOT, capture_output=True, text=True, timeout=30)
                    self.assertEqual(repeated.returncode, 1)
                    self.assertNotIn("Traceback", repeated.stderr)
                    outputs.append(mapping)
                self.assertEqual(outputs[0], outputs[1])

    def test_cli_rejects_missing_stale_and_symlinked_locks_without_output(self):
        for module, _, _ in COHORTS:
            with self.subTest(cohort=module.CAMPAIGN), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                stale = root / "stale.json"
                stale.write_bytes(module.DEFAULT_LOCK.read_bytes() + b"\n")
                link = root / "link.json"
                link.symlink_to(module.DEFAULT_LOCK)
                for command in ((module.__file__,), ("-m", module.__name__)):
                    for lock in (root / "missing.json", stale, link):
                        for action in (("--check",), ("--output", str(root / "output"))):
                            with self.subTest(command=command, lock=lock.name, action=action[0]):
                                result = subprocess.run(
                                    [sys.executable, "-B", *command, "--lock", str(lock), *action],
                                    cwd=ROOT, capture_output=True, text=True, timeout=30)
                                self.assertEqual(result.returncode, 1)
                                self.assertNotIn("Traceback", result.stderr)
                                self.assertFalse((root / "output").exists())

    def test_interaction_packages_pass_hygiene_and_keep_warning_expectations(self):
        lock, payloads = interaction._build_batch()
        self.assertEqual(sum(row["expected"]["warnings"] == ["FloatingShapePlaceholderOnly"]
            for row in lock["documents"]), 98)
        for row in lock["documents"]:
            with self.subTest(case=row["id"]), zipfile.ZipFile(io.BytesIO(payloads[row["path"]])) as archive:
                self.assertEqual(hygiene.audit_office_zip(archive, row["path"]), [])


if __name__ == "__main__":
    unittest.main()
