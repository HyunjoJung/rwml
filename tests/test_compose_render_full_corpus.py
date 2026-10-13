import copy
import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from scripts import compose_render_full_corpus as full
from scripts import generate_unequal_table_oracle as tables


class RenderFullCompositionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.sources = full.source_batches()

    def test_exact_inventory_retains_every_source_document(self):
        lock, payloads = full.compose(self.sources)
        self.assertEqual(len(lock["documents"]), 800)
        self.assertEqual(len({d["id"] for d in lock["documents"]}), 800)
        self.assertEqual(len({d["sha256"] for d in lock["documents"]}), 800)
        self.assertEqual(sum(d["format"] == "doc" for d in lock["documents"]), 3)
        self.assertEqual(
            sorted(s["documents"] for s in lock["sources"]),
            [3, 40, 48] + [64] * 8 + [197],
        )
        for document in lock["documents"]:
            self.assertEqual(
                hashlib.sha256(payloads[document["path"]]).hexdigest(),
                document["sha256"],
            )
        self.assertEqual(lock["limits"]["max_documents"], 800)
        self.assertEqual(lock["coverage"]["source_batches"], 12)

    def test_canonical_lock_and_strict_materialization(self):
        lock, payloads = full.compose(self.sources)
        self.assertEqual(
            full.DEFAULT_LOCK.read_bytes(), full.batch.canonical_json(lock)
        )
        with tempfile.TemporaryDirectory() as tmp:
            path = full.batch.materialize(Path(tmp) / "full", lock, lock, payloads)
            manifest = full.batch.load_corpus_manifest(path)
            self.assertEqual(len(manifest.documents), 800)
            self.assertEqual(manifest.campaign, "public-render-full-v1")
            self.assertEqual(
                manifest.expected_pages,
                sum(d["expected"]["pages"] for d in lock["documents"]),
            )

    def test_missing_duplicate_and_changed_sources_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "exactly 800"):
            full.compose(self.sources[:-1])
        with self.assertRaisesRegex(ValueError, "duplicate"):
            full.compose(self.sources + (self.sources[0],))
        sources = copy.deepcopy(self.sources)
        sources[1].lock["documents"][0]["id"] = sources[0].lock["documents"][0]["id"]
        with self.assertRaisesRegex(ValueError, "duplicate document id"):
            full.compose(sources)
        sources = copy.deepcopy(self.sources)
        sources[1].lock["documents"][0]["sha256"] = sources[0].lock["documents"][0][
            "sha256"
        ]
        with self.assertRaisesRegex(ValueError, "duplicate document payload"):
            full.compose(sources)
        sources = copy.deepcopy(self.sources)
        first = sources[0].lock["documents"][0]
        sources[0].payloads[first["path"]] += b"changed"
        with self.assertRaisesRegex(ValueError, "payload identity"):
            full.compose(sources)

    def test_closure_binds_the_strict_contract(self):
        original = Path.read_bytes
        contract = full.ROOT / "scripts/render_oracle_contract.py"
        before = full.compose(self.sources)[0]["generator_closure_sha256"]

        def changed(path):
            return original(path) + (b"\n# changed validator\n" if path == contract else b"")

        with mock.patch.object(Path, "read_bytes", autospec=True, side_effect=changed):
            after = full.compose(self.sources)[0]["generator_closure_sha256"]
        self.assertNotEqual(before, after)

    def test_table_source_lock_requires_a_bounded_regular_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            link = Path(tmp) / "table-lock.json"
            link.symlink_to(tables.DEFAULT_LOCK)
            with mock.patch.object(tables, "DEFAULT_LOCK", link):
                with self.assertRaises(ValueError):
                    full.source_batches()

    def test_source_identities_use_only_the_validated_snapshot(self):
        original = Path.read_bytes
        lock_paths = {source.lock_path for source in self.sources}

        def changed_after_validation(path):
            if path in lock_paths:
                return b"unchecked changed lock bytes"
            return original(path)

        with mock.patch.object(Path, "read_bytes", autospec=True, side_effect=changed_after_validation):
            observed = full.source_batches()
        self.assertEqual(
            [source.lock_bytes for source in observed],
            [source.lock_bytes for source in self.sources],
        )

    def test_cli_materializations_preserve_the_full_815_file_map(self):
        with tempfile.TemporaryDirectory() as tmp:
            maps = []
            for index, command in enumerate(((full.__file__,), ("-m", full.__name__))):
                output = Path(tmp) / f"run-{index}"
                for action in (("--check",), ("--output", str(output))):
                    result = subprocess.run(
                        [sys.executable, "-B", *command, *action], cwd=full.ROOT,
                        capture_output=True, text=True, timeout=60,
                    )
                    self.assertEqual(result.returncode, 0, result.stderr)
                mapping = {
                    path.relative_to(output).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
                    for path in output.rglob("*") if path.is_file()
                }
                self.assertEqual(len(mapping), 815)
                self.assertEqual(
                    hashlib.sha256(full.batch.canonical_json(mapping)).hexdigest(),
                    "d6789b599da6a4beff973c030cdf4803dc15ba73e68329b94a74517283ba4c0f",
                )
                self.assertEqual(
                    mapping["RENDER_ORACLE.json"],
                    "81bdf07f31925a5cd45630930182a1bb495f8884c45d708f4a01554f8e20ff0b",
                )
                manifest = full.batch.load_corpus_manifest(output / "RENDER_ORACLE.json")
                self.assertEqual(manifest.expected_pages, 1059)
                result = subprocess.run(
                    [sys.executable, "-B", *command, "--output", str(output)], cwd=full.ROOT,
                    capture_output=True, text=True, timeout=60,
                )
                self.assertEqual(result.returncode, 1)
                self.assertNotIn("Traceback", result.stderr)
                maps.append(mapping)
            self.assertEqual(maps[0], maps[1])

    def test_cli_lock_errors_never_publish(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            stale = root / "stale.json"
            stale.write_bytes(full.DEFAULT_LOCK.read_bytes() + b"\n")
            link = root / "link.json"
            link.symlink_to(full.DEFAULT_LOCK)
            for lock in (root / "missing.json", stale, link):
                for command in ((full.__file__,), ("-m", full.__name__)):
                    with self.subTest(lock=lock.name, command=command):
                        result = subprocess.run(
                            [sys.executable, "-B", *command, "--lock", str(lock),
                             "--output", str(root / "output")], cwd=full.ROOT,
                            capture_output=True, text=True, timeout=60,
                        )
                        self.assertEqual(result.returncode, 1)
                        self.assertNotIn("Traceback", result.stderr)
                        self.assertFalse((root / "output").exists())

    def test_failed_strict_validation_cleans_staging_and_preserves_empty_output(self):
        lock, payloads = full.compose(self.sources)
        lock["documents"][0]["expected"]["pages"] = 0
        for exists in (False, True):
            with self.subTest(exists=exists), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                output = root / "full"
                if exists:
                    output.mkdir()
                with self.assertRaises(ValueError):
                    full.batch.materialize(output, lock, lock, payloads)
                self.assertEqual(list(root.iterdir()), [output] if exists else [])
                if exists:
                    self.assertEqual(list(output.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
