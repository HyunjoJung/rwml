#!/usr/bin/env python3

import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest
import zipfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "generate_render_pilot.py"
LOCK = ROOT / "corpus" / "public" / "oracle" / "render-pilot-v1.json"
SPEC = importlib.util.spec_from_file_location("generate_render_pilot", SCRIPT)
generate_render_pilot = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = generate_render_pilot
SPEC.loader.exec_module(generate_render_pilot)


class RenderPilotGeneratorTests(unittest.TestCase):
    def test_pilot_additions_are_canonical_distinct_and_deterministic(self):
        cases = generate_render_pilot.PILOT_CASES

        self.assertEqual(len(cases), 19)
        self.assertEqual([case.case_id for case in cases], sorted(case.case_id for case in cases))
        self.assertEqual(len({case.case_id for case in cases}), 19)
        for case in cases:
            with self.subTest(case=case.case_id):
                first = case.builder()
                second = case.builder()
                self.assertEqual(first, second)
                self.assertTrue(first.startswith(b"PK"))
                self.assertLess(len(first), 256 * 1024)
                with tempfile.TemporaryDirectory() as tmp:
                    path = pathlib.Path(tmp) / "case.docx"
                    path.write_bytes(first)
                    with zipfile.ZipFile(path) as archive:
                        self.assertIn("word/document.xml", archive.namelist())
                        document = archive.read("word/document.xml")
                self.assertNotIn(b"/Users/", document)
                self.assertNotIn(b"/home/", document)

    def test_pilot_lock_binds_exactly_40_documents_and_generator_closure(self):
        lock = generate_render_pilot.build_lock()
        checked_in = generate_render_pilot.load_lock(LOCK)

        self.assertEqual(checked_in, lock)
        self.assertEqual(LOCK.read_bytes(), generate_render_pilot.canonical_json(lock))
        self.assertEqual(lock["schema"], "rwml.render-pilot-lock.v1")
        self.assertEqual(lock["campaign"], "public-render-pilot-v1")
        self.assertEqual(len(lock["documents"]), 40)
        self.assertEqual(
            [row["id"] for row in lock["documents"]],
            sorted(row["id"] for row in lock["documents"]),
        )
        self.assertRegex(lock["generator_closure_sha256"], r"[0-9a-f]{64}\Z")
        self.assertRegex(lock["parent_manifest_sha256"], r"[0-9a-f]{64}\Z")
        generated = [row for row in lock["documents"] if row["source"] == "pilot-generated"]
        parent = [row for row in lock["documents"] if row["source"] == "parent-public"]
        self.assertEqual(len(generated), 19)
        self.assertEqual(len(parent), 21)
        for row in lock["documents"]:
            self.assertEqual(len(row["sha256"]), 64)
            self.assertGreater(row["bytes"], 0)
            self.assertEqual(row["path"], f"documents/{row['id']}.{row['format']}")
        self.assertNotIn(b"/Users/", generate_render_pilot.canonical_json(lock))
        self.assertNotIn(b"/home/", generate_render_pilot.canonical_json(lock))

    def test_materialized_manifest_passes_the_strict_corpus_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "pilot"
            lock = generate_render_pilot.load_lock(LOCK)
            self.assertEqual(lock, generate_render_pilot.build_lock())
            manifest = generate_render_pilot.materialize(output, lock)

            corpus = generate_render_pilot.load_corpus_manifest(manifest)

        self.assertEqual(corpus.campaign, "public-render-pilot-v1")
        self.assertEqual(len(corpus.documents), 40)
        self.assertEqual(corpus.expected_pages, 51)
        features = set().union(*(set(document.features) for document in corpus.documents))
        for feature in (
            "character-paint",
            "mixed-sections",
            "rtl-list",
            "table-merges",
            "unicode-line-breaking",
            "unequal-table-continuation",
        ):
            self.assertIn(feature, features)

    def test_noncanonical_and_modified_locks_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            noncanonical = pathlib.Path(tmp) / "noncanonical.json"
            noncanonical.write_bytes(LOCK.read_bytes() + b"\n")
            with self.assertRaisesRegex(ValueError, "noncanonical"):
                generate_render_pilot.load_lock(noncanonical)

            modified = json.loads(LOCK.read_bytes())
            modified["documents"][0]["bytes"] += 1
            with self.assertRaisesRegex(ValueError, "generator closure"):
                generate_render_pilot.materialize(pathlib.Path(tmp) / "pilot", modified)


if __name__ == "__main__":
    unittest.main()
