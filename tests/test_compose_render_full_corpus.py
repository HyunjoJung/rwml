import copy
import hashlib
from pathlib import Path
import tempfile
import unittest

from scripts import compose_render_full_corpus as full


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


if __name__ == "__main__":
    unittest.main()
