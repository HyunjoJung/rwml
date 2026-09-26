import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import zipfile

from scripts import generate_render_full_corpus as runs
from scripts import generate_render_list_rtl_corpus as lists
from scripts import generate_render_paragraph_corpus as paragraphs
from scripts import public_hygiene_audit as hygiene


ROOT = Path(__file__).resolve().parents[1]
COHORTS = (runs, paragraphs, lists)
FILE_MAP_HASHES = {
    runs.CAMPAIGN: "006631126d8e25216c9495bbfacc565ff6d061bfb9791053e98f0e6d2c55ec72",
    paragraphs.CAMPAIGN: "54d1eb7c679307830210dbc5fca93ae5204aa8a996dc5ccd6ca5c1700d70a75f",
    lists.CAMPAIGN: "e72596d8e5f85f4a2ba6301d5123e4984037a9adf4b63a0d248b83313dd9f785",
}


def file_map(root):
    return {
        path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in root.rglob("*") if path.is_file()
    }


class RenderCoreCorpusTests(unittest.TestCase):
    def test_failed_contract_never_publishes_partial_output(self):
        for module in COHORTS:
            with self.subTest(cohort=module.CAMPAIGN), tempfile.TemporaryDirectory() as tmp:
                lock = module.build_lock()
                lock["documents"][0]["expected"]["pages"] = 0
                with mock.patch.object(module, "build_lock", return_value=lock):
                    with self.assertRaises(ValueError):
                        module.materialize(Path(tmp) / "output", lock)
                self.assertEqual(list(Path(tmp).iterdir()), [])

    def test_failed_contract_preserves_an_existing_empty_directory(self):
        for module in COHORTS:
            with self.subTest(cohort=module.CAMPAIGN), tempfile.TemporaryDirectory() as tmp:
                output = Path(tmp) / "output"
                output.mkdir()
                lock = module.build_lock()
                lock["documents"][0]["expected"]["pages"] = 0
                with mock.patch.object(module, "build_lock", return_value=lock):
                    with self.assertRaises(ValueError):
                        module.materialize(output, lock)
                self.assertTrue(output.is_dir())
                self.assertEqual(list(output.iterdir()), [])
                self.assertEqual(list(Path(tmp).iterdir()), [output])

    def test_altered_payload_is_rejected_before_publication(self):
        for module in COHORTS:
            with self.subTest(cohort=module.CAMPAIGN), tempfile.TemporaryDirectory() as tmp:
                lock = module.build_lock()
                payloads = module._payloads()
                payloads[lock["documents"][0]["path"]] = b"altered input"
                with mock.patch.object(module, "_payloads", return_value=payloads):
                    with self.assertRaises(ValueError):
                        module.materialize(Path(tmp) / "output", lock)
                self.assertEqual(list(Path(tmp).iterdir()), [])

    def test_symlinked_lock_is_not_regular_input(self):
        for module in COHORTS:
            with self.subTest(cohort=module.CAMPAIGN), tempfile.TemporaryDirectory() as tmp:
                link = Path(tmp) / "lock.json"
                link.symlink_to(module.DEFAULT_LOCK)
                with self.assertRaises(ValueError):
                    module.load_lock(link)

    def test_dependency_identity_binds_all_local_modules(self):
        original = Path.read_bytes
        for module in COHORTS:
            before = module._generator_closure_sha256()
            for source in (
                module.SCRIPT_PATH,
                ROOT / "scripts/gen_public_corpus.py",
                ROOT / "scripts/render_corpus_batch.py",
                ROOT / "scripts/render_oracle_contract.py",
            ):
                def altered(path):
                    return original(path) + (b"\n# Changed source\n" if path == source else b"")

                with self.subTest(cohort=module.CAMPAIGN, source=source.name):
                    with mock.patch.object(Path, "read_bytes", autospec=True, side_effect=altered):
                        self.assertNotEqual(before, module._generator_closure_sha256())

    def test_occupied_and_nested_symlink_outputs_are_untouched(self):
        for module in COHORTS:
            with self.subTest(cohort=module.CAMPAIGN), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                external = root / "external"
                external.mkdir()
                sentinel = external / "keep"
                sentinel.write_bytes(b"retained input")
                output = root / "output"
                output.mkdir()
                link = output / "documents"
                link.symlink_to(external, target_is_directory=True)
                lock = module.build_lock()
                for destination in (output, external, sentinel, link):
                    with self.subTest(destination=destination.name), self.assertRaises(ValueError):
                        module.materialize(destination, lock)
                self.assertEqual(list(external.iterdir()), [sentinel])
                self.assertEqual(sentinel.read_bytes(), b"retained input")
                self.assertEqual(list(output.iterdir()), [link])
                self.assertTrue(link.is_symlink())

    def test_cli_modes_repeat_the_complete_pinned_file_sets(self):
        for module in COHORTS:
            with self.subTest(cohort=module.CAMPAIGN), tempfile.TemporaryDirectory() as tmp:
                observed = []
                commands = ((str(module.SCRIPT_PATH),), ("-m", module.__name__))
                for index, command in enumerate(commands):
                    output = Path(tmp) / f"run-{index}"
                    for arguments in (("--check",), ("--output", str(output))):
                        result = subprocess.run(
                            [sys.executable, "-B", *command, *arguments],
                            cwd=ROOT, capture_output=True, text=True, timeout=30,
                        )
                        self.assertEqual(result.returncode, 0, result.stderr)
                    snapshot = file_map(output)
                    self.assertEqual(len(snapshot), 66)
                    identity = (json.dumps(snapshot, indent=2, sort_keys=True) + "\n").encode()
                    self.assertEqual(hashlib.sha256(identity).hexdigest(), FILE_MAP_HASHES[module.CAMPAIGN])
                    corpus = module.load_corpus_manifest(output / "RENDER_ORACLE.json")
                    self.assertEqual(len(corpus.documents), 64)
                    self.assertEqual(corpus.expected_pages, 64)
                    observed.append(snapshot)
                self.assertEqual(observed[0], observed[1])

    def test_cohorts_are_disjoint_in_ids_paths_and_payloads(self):
        documents = [document for module in COHORTS for document in module.load_lock()["documents"]]
        self.assertEqual(len(documents), 192)
        for key in ("id", "path", "sha256"):
            self.assertEqual(len({document[key] for document in documents}), 192, key)

    def test_every_generated_office_package_passes_bounded_hygiene(self):
        for module in COHORTS:
            for spec in module.case_specs():
                with self.subTest(case=spec.case_id):
                    with zipfile.ZipFile(io.BytesIO(module.build_case(spec))) as archive:
                        self.assertEqual(hygiene.audit_office_zip(archive, spec.relative_path), [])


if __name__ == "__main__":
    unittest.main()
