import hashlib
from pathlib import Path
import tempfile
import unittest

from scripts import render_corpus_batch as batch


class RenderCorpusBatchTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.payloads = {"documents/test.doc": b"test", "provenance/test.md": b"MIT"}
        self.lock = {
            "schema": "rwml.render-corpus-batch-lock.v1",
            "campaign": "batch-test",
            "limits": {
                "max_documents": 1,
                "max_input_bytes": 1024,
                "max_total_input_bytes": 1024,
                "max_pages_per_document": 1,
            },
            "provenance": [
                {
                    "id": "test",
                    "kind": "generated",
                    "license": "MIT",
                    "reference": "provenance/test.md",
                    "bytes": 3,
                    "sha256": hashlib.sha256(b"MIT").hexdigest(),
                }
            ],
            "documents": [
                {
                    "id": "test",
                    "path": "documents/test.doc",
                    "format": "doc",
                    "bytes": 4,
                    "sha256": hashlib.sha256(b"test").hexdigest(),
                    "provenance": "test",
                    "features": ["text"],
                    "expected": {"pages": 1, "warnings": []},
                }
            ],
        }

    def test_writes_exact_payloads_and_strict_manifest(self):
        output = self.root / "output"
        manifest = batch.materialize(output, self.lock, self.lock, self.payloads)
        corpus = batch.load_corpus_manifest(manifest)
        self.assertEqual(corpus.campaign, "batch-test")
        self.assertEqual(corpus.expected_pages, 1)
        self.assertEqual((output / "documents/test.doc").read_bytes(), b"test")
        self.assertEqual((output / "provenance/test.md").read_bytes(), b"MIT")

    def test_changed_payload_or_provenance_never_publishes_output(self):
        for path in self.payloads:
            with self.subTest(path=path):
                output = self.root / "output"
                payloads = {**self.payloads, path: b"altered"}
                with self.assertRaisesRegex(ValueError, "payload identity"):
                    batch.materialize(output, self.lock, self.lock, payloads)
                self.assertFalse(output.exists())

    def test_rejects_extra_payloads_stale_locks_and_nonfresh_output(self):
        output = self.root / "output"
        with self.assertRaisesRegex(ValueError, "payload paths"):
            batch.materialize(
                output, self.lock, self.lock, {**self.payloads, "extra": b"extra"}
            )
        with self.assertRaisesRegex(ValueError, "generator closure"):
            batch.materialize(output, {}, self.lock, self.payloads)
        output.mkdir()
        sentinel = output / "keep"
        sentinel.write_bytes(b"keep")
        with self.assertRaisesRegex(ValueError, "fresh"):
            batch.materialize(output, self.lock, self.lock, self.payloads)
        self.assertEqual(sentinel.read_bytes(), b"keep")

    def test_canonical_lock_and_generator_identity(self):
        path = self.root / "lock.json"
        path.write_bytes(batch.canonical_json(self.lock))
        self.assertEqual(batch.load_lock(path, self.lock), self.lock)
        path.write_bytes(path.read_bytes() + b"\n")
        with self.assertRaisesRegex(ValueError, "noncanonical"):
            batch.load_lock(path, self.lock)
        source = self.root / "source.py"
        source.write_bytes(b"before")
        first = batch.generator_closure_sha256(self.root, (source,))
        source.write_bytes(b"after")
        self.assertNotEqual(first, batch.generator_closure_sha256(self.root, (source,)))

    def test_symlinked_lock_is_not_accepted_as_regular_input(self):
        source = self.root / "lock.json"
        source.write_bytes(batch.canonical_json(self.lock))
        link = self.root / "linked.json"
        link.symlink_to(source)
        with self.assertRaises(ValueError):
            batch.load_lock(link, self.lock)

    def test_strict_contract_failure_cleans_staging_without_publication(self):
        self.lock["documents"][0]["expected"]["pages"] = 0
        with self.assertRaises(ValueError):
            batch.materialize(self.root / "output", self.lock, self.lock, self.payloads)
        self.assertEqual(list(self.root.iterdir()), [])

    def test_empty_directory_is_fresh_but_file_and_symlink_are_not(self):
        empty = self.root / "empty"
        empty.mkdir()
        batch.materialize(empty, self.lock, self.lock, self.payloads)
        self.assertTrue((empty / "RENDER_ORACLE.json").is_file())
        occupied = self.root / "occupied"
        occupied.write_bytes(b"keep")
        link = self.root / "linked"
        link.symlink_to(empty, target_is_directory=True)
        for output in (occupied, link):
            with self.subTest(output=output.name), self.assertRaises(ValueError):
                batch.materialize(output, self.lock, self.lock, self.payloads)
        self.assertEqual(occupied.read_bytes(), b"keep")
        self.assertTrue(link.is_symlink())

    def test_lock_reader_rejects_missing_empty_directory_and_oversized_files(self):
        path = self.root / "lock.json"
        for payload in (None, b"", batch.canonical_json(self.lock) + b" "):
            if payload is not None:
                path.write_bytes(payload)
            with self.subTest(payload=payload), self.assertRaises(ValueError):
                batch.load_lock(path, self.lock)
        with self.assertRaises(ValueError):
            batch.load_lock(self.root, self.lock)

    def test_noncanonical_and_traversal_payload_paths_never_publish(self):
        for path in ("../outside.doc", "/outside.doc", "a//b.doc", "a/./b.doc",
                     "a\\b.doc", "a:b.doc", "RENDER_ORACLE.json"):
            with self.subTest(path=path):
                self.lock["documents"][0]["path"] = path
                payloads = {path: b"test", "provenance/test.md": b"MIT"}
                with self.assertRaisesRegex(ValueError, "payload path"):
                    batch.materialize(self.root / "output", self.lock, self.lock, payloads)
                self.assertEqual(list(self.root.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
