#!/usr/bin/env python3

import hashlib
import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "generate_render_legacy_corpus.py"
LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-legacy-v1.json"
SPEC = importlib.util.spec_from_file_location("generate_render_legacy_corpus", SCRIPT)
generate_render_legacy_corpus = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = generate_render_legacy_corpus
SPEC.loader.exec_module(generate_render_legacy_corpus)


class RenderLegacyCorpusGeneratorTests(unittest.TestCase):
    def test_specs_are_the_exact_public_legacy_benchmark_inventory(self):
        specs = generate_render_legacy_corpus.case_specs()

        self.assertEqual(
            [spec.case_id for spec in specs],
            [
                "full-legacy-floating-text-bearing",
                "full-legacy-floating-wrap-policy",
                "full-legacy-nested-tables",
            ],
        )
        self.assertEqual(
            [spec.source_doc.name for spec in specs],
            [
                "floating_text_bearing.doc",
                "floating_wrap_policy.doc",
                "nested_tables.doc",
            ],
        )
        self.assertEqual(
            {spec.source_doc for spec in specs},
            set((ROOT / "corpus" / "public" / "benchmark" / "sample").glob("*.doc")),
        )

    def test_committed_inputs_sources_and_goldens_have_stable_identities(self):
        expected = {
            "full-legacy-floating-text-bearing": {
                "doc": "6a60920d650d9ec55372eef7607f708dfe5d856875c4d0162273856c1caefdf4",
                "docx": "6f91bc55068f9d2d794058083eebf221d8bd781ac28ae6ae384ae8a88cf38c2a",
                "poi": "bf108533cceb86c267387a2018c334261840b5f0924b2364f4fbf67d5cb7963d",
                "lo": "31eb7c3414b593fe340a00677361f4c11183d667a9496a6af9672c4101158a24",
            },
            "full-legacy-floating-wrap-policy": {
                "doc": "65965d61822fdc4377ecc1754506494b9daf7b63572c39d68d5eac008e9f74f1",
                "docx": "dcaab9d6c4e258223d4dadc8c2a598ca7115be1b70b3e2f32170511c086948dd",
                "poi": "3ee35f014bd769e20137c4ccedbb02aef533b1b79dcaf8fdd679c4040c5f03ba",
                "lo": "756099d2a94ee340c9f8db3b37d956576ea316038e8f131ca77d9466983a0490",
            },
            "full-legacy-nested-tables": {
                "doc": "9997f2b1eaa132c23e96d133c8b1e26f5df0f86702ae96e5655a3edf56e238fd",
                "docx": "77bb54f41e5f6148bffb29238985898a62cb14dbc5004f1a2bbe408307f719c0",
                "poi": "4e34d321f48b899460d158d6c089ed4ec3bd5699ac098d52aae3604c4e4096ab",
                "lo": "1f4f217444e74f7ee83a12d44ee8150e8833b6531188c20b43760ff24394b9c6",
            },
        }

        for spec in generate_render_legacy_corpus.case_specs():
            observed = {
                "doc": hashlib.sha256(spec.source_doc.read_bytes()).hexdigest(),
                "docx": hashlib.sha256(spec.source_docx.read_bytes()).hexdigest(),
                "poi": hashlib.sha256(spec.poi_golden.read_bytes()).hexdigest(),
                "lo": hashlib.sha256(spec.libreoffice_golden.read_bytes()).hexdigest(),
            }
            with self.subTest(case=spec.case_id):
                self.assertEqual(observed, expected[spec.case_id])
                self.assertEqual(spec.source_doc.stat().st_size, 9216)

    def test_lock_is_canonical_and_binds_conversion_and_reference_evidence(self):
        lock = generate_render_legacy_corpus.build_lock()
        checked_in = generate_render_legacy_corpus.load_lock(LOCK)

        self.assertEqual(checked_in, lock)
        self.assertEqual(
            LOCK.read_bytes(), generate_render_legacy_corpus.canonical_json(lock)
        )
        self.assertEqual(lock["schema"], "rwml.render-corpus-batch-lock.v1")
        self.assertEqual(lock["campaign"], "public-render-full-legacy-v1")
        self.assertEqual(lock["limits"]["max_documents"], 3)
        self.assertEqual(lock["coverage"]["case_count"], 3)
        self.assertEqual(lock["coverage"]["design"], "fixed-reviewed-subset")
        self.assertEqual(
            lock["coverage"]["conversion"],
            {
                "date": "2026-07-15",
                "filter": "MS Word 97",
                "producer": "LibreOffice",
                "version": "26.2.3.2",
            },
        )
        self.assertEqual(
            lock["coverage"]["reference_extractors"],
            ["Apache POI 5.2.3", "LibreOffice 26.2.3.2"],
        )
        self.assertEqual(
            lock["provenance"],
            [
                {
                    "bytes": len(
                        generate_render_legacy_corpus.PROVENANCE_TEXT.encode("utf-8")
                    ),
                    "id": "rwml-render-full-legacy",
                    "kind": "converted",
                    "license": "MIT",
                    "reference": "provenance/rwml-render-full-legacy.md",
                    "sha256": hashlib.sha256(
                        generate_render_legacy_corpus.PROVENANCE_TEXT.encode("utf-8")
                    ).hexdigest(),
                }
            ],
        )
        self.assertRegex(lock["generator_closure_sha256"], r"[0-9a-f]{64}\Z")
        for document in lock["documents"]:
            self.assertEqual(document["format"], "doc")
            self.assertEqual(document["source"], "converted")
            self.assertEqual(document["expected"], {"pages": 1, "warnings": []})
            self.assertEqual(set(document["references"]), {"apache-poi", "libreoffice"})
            self.assertEqual(document["references"]["apache-poi"]["version"], "5.2.3")
            self.assertEqual(
                document["references"]["libreoffice"]["version"], "26.2.3.2"
            )
            self.assertTrue(document["source_docx"]["path"].endswith(".docx"))
        canonical = generate_render_legacy_corpus.canonical_json(lock)
        self.assertNotIn(b"/Users/", canonical)
        self.assertNotIn(b"/home/", canonical)

    def test_materialized_batch_is_exact_and_passes_the_strict_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "legacy"
            manifest = generate_render_legacy_corpus.materialize(
                output, generate_render_legacy_corpus.load_lock(LOCK)
            )
            corpus = generate_render_legacy_corpus.load_corpus_manifest(manifest)

            self.assertEqual(corpus.campaign, "public-render-full-legacy-v1")
            self.assertEqual(len(corpus.documents), 3)
            self.assertEqual(corpus.expected_pages, 3)
            self.assertEqual({document.format for document in corpus.documents}, {"doc"})
            self.assertEqual(len({document.sha256 for document in corpus.documents}), 3)
            for spec, document in zip(
                generate_render_legacy_corpus.case_specs(), corpus.documents, strict=True
            ):
                self.assertEqual(document.path.read_bytes(), spec.source_doc.read_bytes())
            self.assertEqual(
                sorted(
                    path.relative_to(output).as_posix()
                    for path in output.rglob("*")
                    if path.is_file()
                ),
                [
                    "RENDER_ORACLE.json",
                    "documents/full-legacy-floating-text-bearing.doc",
                    "documents/full-legacy-floating-wrap-policy.doc",
                    "documents/full-legacy-nested-tables.doc",
                    "provenance/rwml-render-full-legacy.md",
                ],
            )

    def test_noncanonical_and_modified_locks_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            temporary = pathlib.Path(tmp)
            noncanonical = temporary / "noncanonical.json"
            noncanonical.write_bytes(LOCK.read_bytes() + b"\n")
            with self.assertRaisesRegex(ValueError, "noncanonical"):
                generate_render_legacy_corpus.load_lock(noncanonical)

            modified = json.loads(LOCK.read_bytes())
            modified["documents"][0]["bytes"] += 1
            with self.assertRaisesRegex(ValueError, "generator closure"):
                generate_render_legacy_corpus.materialize(
                    temporary / "changed", modified
                )


if __name__ == "__main__":
    unittest.main()
