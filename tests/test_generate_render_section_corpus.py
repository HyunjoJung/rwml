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
SCRIPT = ROOT / "scripts" / "generate_render_section_corpus.py"
LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-section-v1.json"
SPEC = importlib.util.spec_from_file_location("generate_render_section_corpus", SCRIPT)
generate_render_section_corpus = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = generate_render_section_corpus
SPEC.loader.exec_module(generate_render_section_corpus)


W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
NS = {"w": W, "r": R}
VAL = f"{{{W}}}val"


def section_properties(document):
    sections = document.findall(".//w:sectPr", NS)
    if len(sections) != 2:
        raise AssertionError("expected one ending and one final section")
    return sections


class RenderSectionCorpusGeneratorTests(unittest.TestCase):
    def test_specs_form_a_complete_six_factor_grid(self):
        specs = generate_render_section_corpus.case_specs()

        self.assertEqual(len(specs), 64)
        self.assertEqual(
            [spec.case_id for spec in specs],
            [f"full-section-{index:03d}" for index in range(64)],
        )
        self.assertEqual(len({spec.factor_state for spec in specs}), 64)
        for position, factor in enumerate(generate_render_section_corpus.FACTOR_NAMES):
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
                    left=generate_render_section_corpus.FACTOR_NAMES[left],
                    right=generate_render_section_corpus.FACTOR_NAMES[right],
                ):
                    self.assertEqual(set(counts.values()), {16})

    def test_generator_rejects_incomplete_and_duplicate_factor_grids(self):
        specs = generate_render_section_corpus.case_specs()
        with self.assertRaisesRegex(ValueError, "case count"):
            generate_render_section_corpus._validate_specs(specs[:-1])

        duplicate = dataclasses.replace(specs[-1], factor_state=specs[0].factor_state)
        with self.assertRaisesRegex(ValueError, "factor vectors"):
            generate_render_section_corpus._validate_specs(specs[:-1] + (duplicate,))

    def test_payloads_are_deterministic_and_markup_matches_factors(self):
        payloads = []
        for spec in generate_render_section_corpus.case_specs():
            first = generate_render_section_corpus.build_case(spec)
            second = generate_render_section_corpus.build_case(spec)
            payloads.append(first)
            with self.subTest(case=spec.case_id):
                self.assertEqual(first, second)
                self.assertTrue(first.startswith(b"PK"))
                self.assertLess(len(first), 96 * 1024)
                with zipfile.ZipFile(io.BytesIO(first)) as archive:
                    self.assertEqual(archive.namelist(), sorted(archive.namelist()))
                    document_bytes = archive.read("word/document.xml")
                    document = ElementTree.fromstring(document_bytes)
                    settings = ElementTree.fromstring(archive.read("word/settings.xml"))
                    running_parts = {
                        name: archive.read(name)
                        for name in archive.namelist()
                        if name.startswith("word/header")
                        or name.startswith("word/footer")
                    }

                ending, final = section_properties(document)
                section_type = ending.find("w:type", NS)
                self.assertIsNotNone(section_type)
                assert section_type is not None
                self.assertEqual(
                    section_type.get(VAL),
                    "oddPage" if spec.odd_page_start else "nextPage",
                )

                page_size = final.find("w:pgSz", NS)
                self.assertIsNotNone(page_size)
                assert page_size is not None
                self.assertEqual(
                    (
                        page_size.get(f"{{{W}}}w"),
                        page_size.get(f"{{{W}}}h"),
                        page_size.get(f"{{{W}}}orient"),
                    ),
                    ("15840", "12240", "landscape")
                    if spec.landscape
                    else ("12240", "15840", None),
                )

                columns = final.find("w:cols", NS)
                self.assertIsNotNone(columns)
                assert columns is not None
                self.assertEqual(columns.get(f"{{{W}}}num"), "2")
                self.assertEqual(
                    columns.get(f"{{{W}}}equalWidth"),
                    "0" if spec.unequal_columns else "1",
                )
                self.assertEqual(
                    columns.get(f"{{{W}}}sep"),
                    "1" if spec.column_separator else None,
                )
                direct_columns = columns.findall("w:col", NS)
                self.assertEqual(len(direct_columns), 2 if spec.unequal_columns else 0)
                if direct_columns:
                    expected_widths = (
                        ("4200", "8760") if spec.landscape else ("3000", "6360")
                    )
                    self.assertEqual(
                        tuple(column.get(f"{{{W}}}w") for column in direct_columns),
                        expected_widths,
                    )
                    self.assertEqual(direct_columns[0].get(f"{{{W}}}space"), "720")

                self.assertEqual(final.find("w:bidi", NS) is not None, spec.rtl_columns)
                margins = final.find("w:pgMar", NS)
                self.assertIsNotNone(margins)
                assert margins is not None
                expected_distance = "720" if spec.inset_running_surfaces else "360"
                self.assertEqual(margins.get(f"{{{W}}}header"), expected_distance)
                self.assertEqual(margins.get(f"{{{W}}}footer"), expected_distance)
                self.assertIsNotNone(final.find("w:titlePg", NS))

                header_types = sorted(
                    reference.get(f"{{{W}}}type")
                    for reference in final.findall("w:headerReference", NS)
                )
                footer_types = sorted(
                    reference.get(f"{{{W}}}type")
                    for reference in final.findall("w:footerReference", NS)
                )
                self.assertEqual(header_types, ["default", "even", "first"])
                self.assertEqual(footer_types, ["default", "even", "first"])
                self.assertIsNotNone(settings.find("w:evenAndOddHeaders", NS))

                breaks = document.findall(".//w:br", NS)
                self.assertEqual(
                    sum(item.get(f"{{{W}}}type") == "column" for item in breaks),
                    3,
                )
                self.assertEqual(
                    sum(item.get(f"{{{W}}}type") == "page" for item in breaks),
                    2,
                )
                self.assertEqual(len(running_parts), 8)
                for label in (
                    b"ENDING DEFAULT HEADER",
                    b"ENDING DEFAULT FOOTER",
                    b"FINAL DEFAULT HEADER",
                    b"FINAL FIRST HEADER",
                    b"FINAL EVEN HEADER",
                    b"FINAL DEFAULT FOOTER",
                    b"FINAL FIRST FOOTER",
                    b"FINAL EVEN FOOTER",
                ):
                    self.assertEqual(
                        sum(label in payload for payload in running_parts.values()), 1
                    )
                self.assertIn(spec.case_id.encode("ascii"), document_bytes)
                self.assertNotIn(b"/Users/", first)
                self.assertNotIn(b"/home/", first)
        self.assertEqual(len(set(payloads)), 64)

    def test_representative_payload_hashes_are_stable(self):
        expected = {
            "full-section-000": "9405230c571bab7cc6872ba5cedb3cb718dc4c04d7a778b98cd6253dd82e02c8",
            "full-section-021": "0982e2ddc9a593302151e6870641babaa93229ec5d4967c047462a76133a5ba7",
            "full-section-063": "182e1bcdb18e352645e7324de6cf03fb3f79db36a7224679c75f79739adb0ce0",
        }
        by_id = {
            spec.case_id: spec for spec in generate_render_section_corpus.case_specs()
        }
        for case_id, expected_sha256 in expected.items():
            payload = generate_render_section_corpus.build_case(by_id[case_id])
            with self.subTest(case=case_id):
                self.assertEqual(hashlib.sha256(payload).hexdigest(), expected_sha256)

    def test_lock_is_canonical_and_records_factorial_coverage(self):
        lock = generate_render_section_corpus.build_lock()
        checked_in = generate_render_section_corpus.load_lock(LOCK)

        self.assertEqual(checked_in, lock)
        self.assertEqual(
            LOCK.read_bytes(), generate_render_section_corpus.canonical_json(lock)
        )
        self.assertEqual(lock["schema"], "rwml.render-corpus-batch-lock.v1")
        self.assertEqual(lock["campaign"], "public-render-full-section-v1")
        self.assertEqual(len(lock["documents"]), 64)
        self.assertEqual(lock["limits"]["max_documents"], 64)
        self.assertEqual(lock["limits"]["max_pages_per_document"], 5)
        self.assertRegex(lock["generator_closure_sha256"], r"[0-9a-f]{64}\Z")
        coverage = lock["coverage"]
        self.assertEqual(coverage["cohort"], "section-columns-running-surfaces")
        self.assertEqual(coverage["case_count"], 64)
        self.assertEqual(coverage["design"], "complete-2-level-factorial")
        self.assertEqual(
            coverage["factor_names"], list(generate_render_section_corpus.FACTOR_NAMES)
        )
        self.assertEqual(
            coverage["factor_case_counts"],
            {factor: 32 for factor in generate_render_section_corpus.FACTOR_NAMES},
        )
        self.assertEqual(len(coverage["pairwise_state_counts"]), 15)
        for row in coverage["pairwise_state_counts"]:
            self.assertEqual(row["states"], {"00": 16, "01": 16, "10": 16, "11": 16})
        self.assertEqual(coverage["interaction_scope"], "final-section-pages")
        canonical = generate_render_section_corpus.canonical_json(lock)
        self.assertNotIn(b"/Users/", canonical)
        self.assertNotIn(b"/home/", canonical)

    def test_materialized_batch_passes_the_strict_corpus_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "section"
            manifest = generate_render_section_corpus.materialize(
                output, generate_render_section_corpus.load_lock(LOCK)
            )
            corpus = generate_render_section_corpus.load_corpus_manifest(manifest)

            self.assertEqual(corpus.campaign, "public-render-full-section-v1")
            self.assertEqual(len(corpus.documents), 64)
            self.assertEqual(corpus.expected_pages, 288)
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
                    ["RENDER_ORACLE.json", "provenance/rwml-render-full-section.md"]
                    + [
                        f"documents/full-section-{index:03d}.docx"
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
                generate_render_section_corpus.load_lock(noncanonical)

            modified = json.loads(LOCK.read_bytes())
            modified["documents"][0]["bytes"] += 1
            with self.assertRaisesRegex(ValueError, "generator closure"):
                generate_render_section_corpus.materialize(
                    temporary / "changed", modified
                )


if __name__ == "__main__":
    unittest.main()
