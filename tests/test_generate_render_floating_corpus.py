#!/usr/bin/env python3

import dataclasses
import hashlib
import importlib.util
import io
import json
import pathlib
import sys
import tempfile
import unittest
from xml.etree import ElementTree
import zipfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "generate_render_floating_corpus.py"
LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-floating-v1.json"
SPEC = importlib.util.spec_from_file_location("generate_render_floating_corpus", SCRIPT)
generate_render_floating_corpus = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = generate_render_floating_corpus
SPEC.loader.exec_module(generate_render_floating_corpus)


W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
WP = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
WPS = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape"
NS = {"w": W, "wp": WP, "wps": WPS}


class RenderFloatingCorpusGeneratorTests(unittest.TestCase):
    def test_specs_form_a_complete_six_factor_grid(self):
        specs = generate_render_floating_corpus.case_specs()

        self.assertEqual(len(specs), 64)
        self.assertEqual(
            [spec.case_id for spec in specs],
            [f"full-floating-{index:03d}" for index in range(64)],
        )
        self.assertEqual(len({spec.factor_state for spec in specs}), 64)
        for position, factor in enumerate(generate_render_floating_corpus.FACTOR_NAMES):
            self.assertEqual(
                sum(spec.factor_state[position] for spec in specs),
                32,
                factor,
            )
        for left in range(6):
            for right in range(left + 1, 6):
                counts = {
                    state: sum(
                        (spec.factor_state[left], spec.factor_state[right]) == state
                        for spec in specs
                    )
                    for state in (
                        (False, False),
                        (False, True),
                        (True, False),
                        (True, True),
                    )
                }
                with self.subTest(
                    left=generate_render_floating_corpus.FACTOR_NAMES[left],
                    right=generate_render_floating_corpus.FACTOR_NAMES[right],
                ):
                    self.assertEqual(set(counts.values()), {16})

    def test_generator_rejects_incomplete_and_duplicate_factor_grids(self):
        specs = generate_render_floating_corpus.case_specs()
        with self.assertRaisesRegex(ValueError, "case count"):
            generate_render_floating_corpus._validate_specs(specs[:-1])

        duplicate = dataclasses.replace(specs[-1], factor_state=specs[0].factor_state)
        with self.assertRaisesRegex(ValueError, "factor vectors"):
            generate_render_floating_corpus._validate_specs(specs[:-1] + (duplicate,))

    def test_case_properties_map_factor_bits_to_anchor_geometry(self):
        for spec in generate_render_floating_corpus.case_specs():
            with self.subTest(case=spec.case_id):
                self.assertEqual(
                    spec.horizontal_reference,
                    "margin" if spec.horizontal_margin else "page",
                )
                self.assertEqual(
                    spec.horizontal_offset_emu,
                    3_200_400 if spec.far_horizontal else 457_200,
                )
                self.assertEqual(
                    spec.vertical_reference,
                    "margin" if spec.vertical_margin else "page",
                )
                self.assertEqual(
                    spec.vertical_offset_emu,
                    2_514_600 if spec.low_vertical else 457_200,
                )
                self.assertEqual(spec.behind_doc, not spec.front)

    def test_payloads_are_deterministic_and_markup_matches_factors(self):
        payloads = []
        for spec in generate_render_floating_corpus.case_specs():
            first = generate_render_floating_corpus.build_case(spec)
            second = generate_render_floating_corpus.build_case(spec)
            payloads.append(first)
            with self.subTest(case=spec.case_id):
                self.assertEqual(first, second)
                self.assertTrue(first.startswith(b"PK"))
                self.assertLess(len(first), 64 * 1024)
                with zipfile.ZipFile(io.BytesIO(first)) as archive:
                    self.assertEqual(archive.namelist(), sorted(archive.namelist()))
                    self.assertEqual(
                        set(archive.namelist()),
                        {
                            "[Content_Types].xml",
                            "_rels/.rels",
                            "word/_rels/document.xml.rels",
                            "word/document.xml",
                            "word/styles.xml",
                        },
                    )
                    document_bytes = archive.read("word/document.xml")
                    document = ElementTree.fromstring(document_bytes)

                anchors = document.findall(".//wp:anchor", NS)
                self.assertEqual(len(anchors), 1)
                anchor = anchors[0]
                self.assertEqual(anchor.get("relativeHeight"), "500")
                self.assertEqual(anchor.get("behindDoc"), "0" if spec.front else "1")
                self.assertEqual(anchor.get("distT"), "114300")
                self.assertEqual(anchor.get("distB"), "114300")
                self.assertEqual(anchor.get("distL"), "0")
                self.assertEqual(anchor.get("distR"), "0")

                horizontal = anchor.find("wp:positionH", NS)
                vertical = anchor.find("wp:positionV", NS)
                self.assertIsNotNone(horizontal)
                self.assertIsNotNone(vertical)
                assert horizontal is not None and vertical is not None
                self.assertEqual(
                    horizontal.get("relativeFrom"), spec.horizontal_reference
                )
                self.assertEqual(vertical.get("relativeFrom"), spec.vertical_reference)
                self.assertEqual(
                    horizontal.findtext("wp:posOffset", namespaces=NS),
                    str(spec.horizontal_offset_emu),
                )
                self.assertEqual(
                    vertical.findtext("wp:posOffset", namespaces=NS),
                    str(spec.vertical_offset_emu),
                )

                extent = anchor.find("wp:extent", NS)
                self.assertIsNotNone(extent)
                assert extent is not None
                self.assertEqual(
                    (extent.get("cx"), extent.get("cy")), ("2286000", "1371600")
                )
                self.assertEqual(
                    len(anchor.findall("wp:wrapTopAndBottom", NS)),
                    1 if spec.top_bottom else 0,
                )
                self.assertEqual(
                    len(anchor.findall("wp:wrapNone", NS)),
                    0 if spec.top_bottom else 1,
                )
                self.assertEqual(
                    anchor.findtext(".//wps:txbx//w:t", namespaces=NS),
                    "Primary floating text",
                )
                body_paragraphs = document.findall("./w:body/w:p", NS)
                self.assertEqual(len(body_paragraphs), 4)
                self.assertIsNotNone(body_paragraphs[1].find(".//wp:anchor", NS))
                self.assertIsNone(body_paragraphs[2].find(".//wp:anchor", NS))
                flow_text = "".join(body_paragraphs[2].itertext())
                self.assertTrue(flow_text.startswith("flow token 1 "))
                self.assertTrue(flow_text.endswith("flow token 180"))
                doc_pr = anchor.find("wp:docPr", NS)
                self.assertIsNotNone(doc_pr)
                assert doc_pr is not None
                self.assertEqual(doc_pr.get("name"), "Primary floating control")
                self.assertEqual(doc_pr.get("descr"), spec.case_id)
                self.assertIn(b"Anchor lead ", document_bytes)
                self.assertIn(b" flow token 180", document_bytes)
                self.assertNotIn(b"a:blip", document_bytes)
                self.assertNotIn(b"/Users/", first)
                self.assertNotIn(b"/home/", first)
        self.assertEqual(len(set(payloads)), 64)

    def test_representative_payload_hashes_are_stable(self):
        expected = {
            "full-floating-000": "4fae3bf836aa04213bdb987a71ced520682ca37880287a2d4b090ec1b31182ce",
            "full-floating-021": "f805286030b35cc3ada8a3923b4a4e93f9dc4ca85c328063c68a370acae4be8d",
            "full-floating-063": "047740997f7dd14096478a24338b1bc482169201919f541031d002f9e09f0b3f",
        }
        by_id = {
            spec.case_id: spec for spec in generate_render_floating_corpus.case_specs()
        }
        for case_id, expected_sha256 in expected.items():
            payload = generate_render_floating_corpus.build_case(by_id[case_id])
            with self.subTest(case=case_id):
                self.assertEqual(hashlib.sha256(payload).hexdigest(), expected_sha256)

    def test_lock_is_canonical_and_records_factorial_coverage(self):
        lock = generate_render_floating_corpus.build_lock()
        checked_in = generate_render_floating_corpus.load_lock(LOCK)

        self.assertEqual(checked_in, lock)
        self.assertEqual(
            LOCK.read_bytes(), generate_render_floating_corpus.canonical_json(lock)
        )
        self.assertEqual(lock["schema"], "rwml.render-corpus-batch-lock.v1")
        self.assertEqual(lock["campaign"], "public-render-full-floating-v1")
        self.assertEqual(len(lock["documents"]), 64)
        self.assertEqual(lock["limits"]["max_documents"], 64)
        self.assertRegex(lock["generator_closure_sha256"], r"[0-9a-f]{64}\Z")
        coverage = lock["coverage"]
        self.assertEqual(coverage["cohort"], "floating-geometry-interactions")
        self.assertEqual(coverage["case_count"], 64)
        self.assertEqual(coverage["design"], "complete-2-level-factorial")
        self.assertEqual(
            coverage["factor_names"],
            list(generate_render_floating_corpus.FACTOR_NAMES),
        )
        self.assertEqual(
            coverage["factor_case_counts"],
            {factor: 32 for factor in generate_render_floating_corpus.FACTOR_NAMES},
        )
        self.assertEqual(len(coverage["pairwise_state_counts"]), 15)
        for row in coverage["pairwise_state_counts"]:
            self.assertEqual(row["states"], {"00": 16, "01": 16, "10": 16, "11": 16})
        self.assertEqual(coverage["interaction_scope"], "primary-floating-shape")
        for document in lock["documents"]:
            self.assertEqual(
                document["expected"],
                {"pages": 1, "warnings": ["FloatingShapePlaceholderOnly"]},
            )
        canonical = generate_render_floating_corpus.canonical_json(lock)
        self.assertNotIn(b"/Users/", canonical)
        self.assertNotIn(b"/home/", canonical)

    def test_materialized_batch_passes_the_strict_corpus_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "floating"
            manifest = generate_render_floating_corpus.materialize(
                output, generate_render_floating_corpus.load_lock(LOCK)
            )
            corpus = generate_render_floating_corpus.load_corpus_manifest(manifest)

            self.assertEqual(corpus.campaign, "public-render-full-floating-v1")
            self.assertEqual(len(corpus.documents), 64)
            self.assertEqual(corpus.expected_pages, 64)
            self.assertEqual(
                len({document.sha256 for document in corpus.documents}), 64
            )
            self.assertEqual(
                sorted(
                    path.relative_to(output).as_posix()
                    for path in output.rglob("*")
                    if path.is_file()
                ),
                sorted(
                    ["RENDER_ORACLE.json", "provenance/rwml-render-full-floating.md"]
                    + [
                        f"documents/full-floating-{index:03d}.docx"
                        for index in range(64)
                    ]
                ),
            )

    def test_noncanonical_and_modified_locks_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            temporary = pathlib.Path(tmp)
            noncanonical = temporary / "noncanonical.json"
            noncanonical.write_bytes(LOCK.read_bytes() + b"\n")
            with self.assertRaisesRegex(ValueError, "noncanonical"):
                generate_render_floating_corpus.load_lock(noncanonical)

            modified = json.loads(LOCK.read_bytes())
            modified["documents"][0]["bytes"] += 1
            with self.assertRaisesRegex(ValueError, "generator closure"):
                generate_render_floating_corpus.materialize(
                    temporary / "changed", modified
                )


if __name__ == "__main__":
    unittest.main()
