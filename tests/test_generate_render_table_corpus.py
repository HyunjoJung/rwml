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
SCRIPT = ROOT / "scripts" / "generate_render_table_corpus.py"
LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-table-v1.json"
SPEC = importlib.util.spec_from_file_location("generate_render_table_corpus", SCRIPT)
generate_render_table_corpus = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = generate_render_table_corpus
SPEC.loader.exec_module(generate_render_table_corpus)


W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
NS = {"w": W}
VAL = f"{{{W}}}val"


def primary_table(document):
    tables = document.findall(".//w:body/w:tbl", NS)
    if len(tables) != 1:
        raise AssertionError("expected one primary body table")
    return tables[0]


class RenderTableCorpusGeneratorTests(unittest.TestCase):
    def test_specs_form_a_complete_six_factor_grid(self):
        specs = generate_render_table_corpus.case_specs()

        self.assertEqual(len(specs), 64)
        self.assertEqual(
            [spec.case_id for spec in specs],
            [f"full-table-{index:03d}" for index in range(64)],
        )
        self.assertEqual(len({spec.factor_state for spec in specs}), 64)
        for position, factor in enumerate(generate_render_table_corpus.FACTOR_NAMES):
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
                    left=generate_render_table_corpus.FACTOR_NAMES[left],
                    right=generate_render_table_corpus.FACTOR_NAMES[right],
                ):
                    self.assertEqual(set(counts.values()), {16})

    def test_generator_rejects_incomplete_and_duplicate_factor_grids(self):
        specs = generate_render_table_corpus.case_specs()
        with self.assertRaisesRegex(ValueError, "case count"):
            generate_render_table_corpus._validate_specs(specs[:-1])

        duplicate = dataclasses.replace(specs[-1], factor_state=specs[0].factor_state)
        with self.assertRaisesRegex(ValueError, "factor vectors"):
            generate_render_table_corpus._validate_specs(specs[:-1] + (duplicate,))

    def test_payloads_are_deterministic_and_primary_markup_matches_factors(self):
        payloads = []
        for spec in generate_render_table_corpus.case_specs():
            first = generate_render_table_corpus.build_case(spec)
            second = generate_render_table_corpus.build_case(spec)
            payloads.append(first)
            with self.subTest(case=spec.case_id):
                self.assertEqual(first, second)
                self.assertTrue(first.startswith(b"PK"))
                self.assertLess(len(first), 64 * 1024)
                with zipfile.ZipFile(io.BytesIO(first)) as archive:
                    self.assertEqual(archive.namelist(), sorted(archive.namelist()))
                    document_bytes = archive.read("word/document.xml")
                document = ElementTree.fromstring(document_bytes)
                table = primary_table(document)
                table_properties = table.find("w:tblPr", NS)
                self.assertIsNotNone(table_properties)
                assert table_properties is not None
                self.assertEqual(
                    table_properties.find("w:bidiVisual", NS) is not None,
                    spec.bidi_visual,
                )

                rows = table.findall("w:tr", NS)
                self.assertEqual(len(rows), 3)
                top_cells = rows[0].findall("w:tc", NS)
                self.assertEqual(len(top_cells), 2 if spec.horizontal_span else 3)
                span = top_cells[0].find("w:tcPr/w:gridSpan", NS)
                self.assertEqual(span is not None, spec.horizontal_span)
                if span is not None:
                    self.assertEqual(span.get(VAL), "2")

                middle_last = rows[1].findall("w:tc", NS)[-1]
                bottom_last = rows[2].findall("w:tc", NS)[-1]
                restart = middle_last.find("w:tcPr/w:vMerge", NS)
                continuation = bottom_last.find("w:tcPr/w:vMerge", NS)
                self.assertEqual(restart is not None, spec.vertical_span)
                self.assertEqual(continuation is not None, spec.vertical_span)
                if restart is not None:
                    self.assertEqual(restart.get(VAL), "restart")
                if continuation is not None:
                    self.assertIsNone(continuation.get(VAL))
                self.assertEqual(
                    middle_last.find("w:tcPr/w:vAlign", NS).get(VAL),
                    "bottom",
                )

                shading = top_cells[0].find("w:tcPr/w:shd", NS)
                self.assertEqual(shading is not None, spec.cell_shading)
                if shading is not None:
                    self.assertEqual(shading.get(f"{{{W}}}fill"), "DDEBF7")
                margins = top_cells[0].find("w:tcPr/w:tcMar", NS)
                self.assertEqual(margins is not None, spec.direct_cell_margins)
                if margins is not None:
                    self.assertEqual(
                        [
                            margins.find(f"w:{side}", NS).get(f"{{{W}}}w")
                            for side in ("top", "right", "bottom", "left")
                        ],
                        ["240", "240", "240", "240"],
                    )

                top_border = table_properties.find("w:tblBorders/w:top", NS)
                self.assertIsNotNone(top_border)
                assert top_border is not None
                self.assertEqual(
                    (
                        top_border.get(VAL),
                        top_border.get(f"{{{W}}}color"),
                        top_border.get(f"{{{W}}}sz"),
                    ),
                    ("double", "C00000", "16")
                    if spec.asymmetric_borders
                    else ("single", "000000", "8"),
                )
                self.assertIn(spec.case_id.encode("ascii"), document_bytes)
                self.assertNotIn(b"/Users/", document_bytes)
                self.assertNotIn(b"/home/", document_bytes)
        self.assertEqual(len(set(payloads)), 64)

    def test_representative_payload_hashes_are_stable(self):
        expected = {
            "full-table-000": "8462c6c2564646d211d794a7a21cd47d91eed6e68a16427634a6880bee1b0cb0",
            "full-table-021": "1a072c806f4d52f0af36f0208aecbf298e24ace50998f4ccd610954e0094f88b",
            "full-table-063": "69769fbf580a1e1e7ff54d2d0527afdc1891d1d34db1987dc9bc85033fba3dbb",
        }
        by_id = {
            spec.case_id: spec for spec in generate_render_table_corpus.case_specs()
        }
        for case_id, expected_sha256 in expected.items():
            payload = generate_render_table_corpus.build_case(by_id[case_id])
            with self.subTest(case=case_id):
                self.assertEqual(hashlib.sha256(payload).hexdigest(), expected_sha256)

    def test_lock_is_canonical_and_records_factorial_coverage(self):
        lock = generate_render_table_corpus.build_lock()
        checked_in = generate_render_table_corpus.load_lock(LOCK)

        self.assertEqual(checked_in, lock)
        self.assertEqual(
            LOCK.read_bytes(), generate_render_table_corpus.canonical_json(lock)
        )
        self.assertEqual(lock["schema"], "rwml.render-corpus-batch-lock.v1")
        self.assertEqual(lock["campaign"], "public-render-full-table-v1")
        self.assertEqual(len(lock["documents"]), 64)
        self.assertEqual(lock["limits"]["max_documents"], 64)
        self.assertRegex(lock["generator_closure_sha256"], r"[0-9a-f]{64}\Z")
        coverage = lock["coverage"]
        self.assertEqual(coverage["cohort"], "table-topology-paint")
        self.assertEqual(coverage["case_count"], 64)
        self.assertEqual(coverage["design"], "complete-2-level-factorial")
        self.assertEqual(
            coverage["factor_names"], list(generate_render_table_corpus.FACTOR_NAMES)
        )
        self.assertEqual(
            coverage["factor_case_counts"],
            {factor: 32 for factor in generate_render_table_corpus.FACTOR_NAMES},
        )
        self.assertEqual(len(coverage["pairwise_state_counts"]), 15)
        for row in coverage["pairwise_state_counts"]:
            self.assertEqual(row["states"], {"00": 16, "01": 16, "10": 16, "11": 16})
        self.assertEqual(coverage["interaction_scope"], "primary-table")
        canonical = generate_render_table_corpus.canonical_json(lock)
        self.assertNotIn(b"/Users/", canonical)
        self.assertNotIn(b"/home/", canonical)

    def test_materialized_batch_passes_the_strict_corpus_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "table"
            manifest = generate_render_table_corpus.materialize(
                output, generate_render_table_corpus.load_lock(LOCK)
            )
            corpus = generate_render_table_corpus.load_corpus_manifest(manifest)

            self.assertEqual(corpus.campaign, "public-render-full-table-v1")
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
                    ["RENDER_ORACLE.json", "provenance/rwml-render-full-table.md"]
                    + [f"documents/full-table-{index:03d}.docx" for index in range(64)]
                ),
            )

    def test_noncanonical_and_modified_locks_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            temporary = pathlib.Path(tmp)
            noncanonical = temporary / "noncanonical.json"
            noncanonical.write_bytes(LOCK.read_bytes() + b"\n")
            with self.assertRaisesRegex(ValueError, "noncanonical"):
                generate_render_table_corpus.load_lock(noncanonical)

            modified = json.loads(LOCK.read_bytes())
            modified["documents"][0]["bytes"] += 1
            with self.assertRaisesRegex(ValueError, "generator closure"):
                generate_render_table_corpus.materialize(
                    temporary / "changed", modified
                )


if __name__ == "__main__":
    unittest.main()
