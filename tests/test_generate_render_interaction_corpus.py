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
SCRIPT = ROOT / "scripts" / "generate_render_interaction_corpus.py"
LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-interaction-v1.json"
SPEC = importlib.util.spec_from_file_location(
    "generate_render_interaction_corpus", SCRIPT
)
generate_render_interaction_corpus = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = generate_render_interaction_corpus
SPEC.loader.exec_module(generate_render_interaction_corpus)


W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
WP = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
NS = {"w": W, "wp": WP}


class RenderInteractionCorpusGeneratorTests(unittest.TestCase):
    def test_specs_form_the_balanced_197_row_lattice_slice(self):
        specs = generate_render_interaction_corpus.case_specs()

        self.assertEqual(len(specs), 197)
        self.assertEqual(
            [spec.case_id for spec in specs],
            [f"full-interaction-{index:03d}" for index in range(197)],
        )
        self.assertEqual(len({spec.design_row for spec in specs}), 197)
        self.assertEqual(len({spec.factor_state for spec in specs}), 197)
        self.assertEqual(len({spec.features for spec in specs}), 197)
        for position, factor in enumerate(
            generate_render_interaction_corpus.FACTOR_NAMES
        ):
            count = sum(spec.factor_state[position] for spec in specs)
            self.assertIn(count, (98, 99), factor)
        for left in range(16):
            for right in range(left + 1, 16):
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
                    left=generate_render_interaction_corpus.FACTOR_NAMES[left],
                    right=generate_render_interaction_corpus.FACTOR_NAMES[right],
                ):
                    self.assertGreaterEqual(min(counts.values()), 47)
                    self.assertLessEqual(max(counts.values()), 52)

    def test_generator_rejects_incomplete_and_duplicate_designs(self):
        specs = generate_render_interaction_corpus.case_specs()
        with self.assertRaisesRegex(ValueError, "case count"):
            generate_render_interaction_corpus._validate_specs(specs[:-1])

        duplicate = dataclasses.replace(
            specs[-1],
            design_row=specs[0].design_row,
            factor_state=specs[0].factor_state,
        )
        with self.assertRaisesRegex(ValueError, "design rows"):
            generate_render_interaction_corpus._validate_specs(
                specs[:-1] + (duplicate,)
            )

    def test_payloads_are_deterministic_and_map_every_factor_to_ooxml(self):
        payloads = []
        for spec in generate_render_interaction_corpus.case_specs():
            first = generate_render_interaction_corpus.build_case(spec)
            second = generate_render_interaction_corpus.build_case(spec)
            payloads.append(first)
            with self.subTest(case=spec.case_id):
                self.assertEqual(first, second)
                self.assertTrue(first.startswith(b"PK"))
                self.assertLess(len(first), 128 * 1024)
                with zipfile.ZipFile(io.BytesIO(first)) as archive:
                    names = archive.namelist()
                    self.assertEqual(names, sorted(names))
                    self.assertIn("word/document.xml", names)
                    self.assertIn("word/styles.xml", names)
                    self.assertIn("word/numbering.xml", names)
                    self.assertIn("word/header1.xml", names)
                    note_name = (
                        "word/endnotes.xml" if spec.endnote else "word/footnotes.xml"
                    )
                    self.assertIn(note_name, names)
                    self.assertNotIn(
                        "word/footnotes.xml" if spec.endnote else "word/endnotes.xml",
                        names,
                    )
                    media_name = (
                        "word/media/image1.emf"
                        if spec.metafile
                        else "word/media/image1.png"
                    )
                    self.assertIn(media_name, names)
                    if spec.first_page_header:
                        self.assertIn("word/header2.xml", names)
                    else:
                        self.assertNotIn("word/header2.xml", names)
                    document_bytes = archive.read("word/document.xml")
                    document = ElementTree.fromstring(document_bytes)

                paragraphs = document.findall("./w:body/w:p", NS)
                self.assertGreaterEqual(len(paragraphs), 5)
                control = paragraphs[0]
                list_paragraph = paragraphs[1]
                self.assertEqual(
                    len(control.findall(".//w:highlight", NS)),
                    1 if spec.highlight else 0,
                )
                self.assertEqual(
                    len(control.findall(".//w:vertAlign", NS)),
                    1 if spec.superscript else 0,
                )
                self.assertEqual(
                    len(control.findall("./w:pPr/w:shd", NS)),
                    1 if spec.paragraph_shading else 0,
                )
                self.assertEqual(
                    len(control.findall("./w:pPr/w:tabs", NS)),
                    1 if spec.hanging_tab else 0,
                )
                spacing = control.find("./w:pPr/w:spacing", NS)
                self.assertIsNotNone(spacing)
                assert spacing is not None
                self.assertEqual(
                    spacing.get(f"{{{W}}}lineRule"),
                    "exact" if spec.exact_line_spacing else "atLeast",
                )
                num_id = list_paragraph.find("./w:pPr/w:numPr/w:numId", NS)
                self.assertIsNotNone(num_id)
                assert num_id is not None
                self.assertEqual(
                    num_id.get(f"{{{W}}}val"), "2" if spec.ordered_list else "1"
                )
                self.assertEqual(
                    len(list_paragraph.findall("./w:pPr/w:bidi", NS)),
                    1 if spec.paragraph_bidi else 0,
                )
                tables = document.findall(".//w:tbl", NS)
                self.assertEqual(len(tables), 2 if spec.nested_table else 1)
                self.assertEqual(
                    len(tables[0].findall("./w:tblPr/w:bidiVisual", NS)),
                    1 if spec.table_bidi else 0,
                )
                section = document.find("./w:body/w:sectPr", NS)
                self.assertIsNotNone(section)
                assert section is not None
                columns = section.find("w:cols", NS)
                if spec.unequal_columns:
                    self.assertIsNotNone(columns)
                    assert columns is not None
                    self.assertEqual(len(columns.findall("w:col", NS)), 2)
                else:
                    self.assertIsNone(columns)
                self.assertEqual(
                    len(section.findall("w:titlePg", NS)),
                    1 if spec.first_page_header else 0,
                )
                self.assertEqual(
                    len(document.findall(".//w:ins//w:instrText", NS)),
                    1 if spec.field_in_insertion else 0,
                )
                self.assertEqual(
                    len(document.findall(".//wp:anchor", NS)),
                    1 if spec.floating_top_bottom else 0,
                )
                self.assertEqual(
                    len(document.findall(".//w:keepNext", NS)),
                    1 if spec.keep_controls else 0,
                )
                self.assertIn(spec.case_id.encode("ascii"), document_bytes)
                self.assertNotIn(b"/Users/", first)
                self.assertNotIn(b"/home/", first)
        self.assertEqual(len(set(payloads)), 197)

    def test_representative_payload_hashes_are_stable(self):
        expected = {
            "full-interaction-000": "b917fef7717c853b04ab2eee000c0ec6b6ec32dc5f2c08f3cff0c2412fc700ce",
            "full-interaction-098": "8ea915dc9fb2f95f75d9c69b9842abe91f0e98fb7c5afe7a87ead2fd921acd62",
            "full-interaction-196": "d6e22b8768dd421b62177eaa29dbe45375342f57789436fb1275d3dc9359543a",
        }
        by_id = {
            spec.case_id: spec
            for spec in generate_render_interaction_corpus.case_specs()
        }
        for case_id, expected_sha256 in expected.items():
            payload = generate_render_interaction_corpus.build_case(by_id[case_id])
            with self.subTest(case=case_id):
                self.assertEqual(hashlib.sha256(payload).hexdigest(), expected_sha256)

    def test_lock_is_canonical_and_records_cross_subsystem_coverage(self):
        lock = generate_render_interaction_corpus.build_lock()
        checked_in = generate_render_interaction_corpus.load_lock(LOCK)

        self.assertEqual(checked_in, lock)
        self.assertEqual(
            LOCK.read_bytes(),
            generate_render_interaction_corpus.canonical_json(lock),
        )
        self.assertEqual(lock["schema"], "rwml.render-corpus-batch-lock.v1")
        self.assertEqual(lock["campaign"], "public-render-full-interaction-v1")
        self.assertEqual(len(lock["documents"]), 197)
        self.assertEqual(lock["limits"]["max_documents"], 197)
        coverage = lock["coverage"]
        self.assertEqual(coverage["cohort"], "cross-subsystem-interactions")
        self.assertEqual(coverage["case_count"], 197)
        self.assertEqual(
            coverage["design"], "deterministic-197-row-binary-lattice-slice"
        )
        self.assertEqual(
            coverage["factor_names"],
            list(generate_render_interaction_corpus.FACTOR_NAMES),
        )
        self.assertEqual(len(coverage["pairwise_state_counts"]), 120)
        for row in coverage["pairwise_state_counts"]:
            self.assertGreaterEqual(min(row["states"].values()), 47)
            self.assertLessEqual(max(row["states"].values()), 52)
        for spec, document in zip(
            generate_render_interaction_corpus.case_specs(),
            lock["documents"],
            strict=True,
        ):
            self.assertEqual(
                document["expected"],
                {
                    "pages": 1,
                    "warnings": (
                        ["FloatingShapePlaceholderOnly"]
                        if spec.floating_top_bottom
                        else []
                    ),
                },
            )
        canonical = generate_render_interaction_corpus.canonical_json(lock)
        self.assertNotIn(b"/Users/", canonical)
        self.assertNotIn(b"/home/", canonical)

    def test_materialized_batch_passes_the_strict_corpus_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "interaction"
            manifest = generate_render_interaction_corpus.materialize(
                output, generate_render_interaction_corpus.load_lock(LOCK)
            )
            corpus = generate_render_interaction_corpus.load_corpus_manifest(manifest)

            self.assertEqual(corpus.campaign, "public-render-full-interaction-v1")
            self.assertEqual(len(corpus.documents), 197)
            self.assertEqual(corpus.expected_pages, 197)
            self.assertEqual(
                len({document.sha256 for document in corpus.documents}), 197
            )
            self.assertEqual(len(list((output / "documents").glob("*.docx"))), 197)

    def test_noncanonical_and_modified_locks_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            temporary = pathlib.Path(tmp)
            noncanonical = temporary / "noncanonical.json"
            noncanonical.write_bytes(LOCK.read_bytes() + b"\n")
            with self.assertRaisesRegex(ValueError, "noncanonical"):
                generate_render_interaction_corpus.load_lock(noncanonical)

            modified = json.loads(LOCK.read_bytes())
            modified["documents"][0]["bytes"] += 1
            with self.assertRaisesRegex(ValueError, "generator closure"):
                generate_render_interaction_corpus.materialize(
                    temporary / "changed", modified
                )


if __name__ == "__main__":
    unittest.main()
