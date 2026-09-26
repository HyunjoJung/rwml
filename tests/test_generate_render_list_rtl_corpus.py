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
SCRIPT = ROOT / "scripts" / "generate_render_list_rtl_corpus.py"
LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-list-rtl-v1.json"
SPEC = importlib.util.spec_from_file_location("generate_render_list_rtl_corpus", SCRIPT)
generate_render_list_rtl_corpus = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = generate_render_list_rtl_corpus
SPEC.loader.exec_module(generate_render_list_rtl_corpus)


W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
NS = {"w": W}


def paragraph_text(paragraph):
    return "".join(node.text or "" for node in paragraph.findall(".//w:t", NS))


def primary_paragraph(document, case_id):
    prefix = f"PRIMARY {case_id} "
    matches = [
        paragraph
        for paragraph in document.findall(".//w:body/w:p", NS)
        if paragraph_text(paragraph).startswith(prefix)
    ]
    if len(matches) != 1:
        raise AssertionError(f"expected one primary paragraph for {case_id}")
    return matches[0]


class RenderListRtlCorpusGeneratorTests(unittest.TestCase):
    def test_specs_form_a_complete_six_factor_grid(self):
        specs = generate_render_list_rtl_corpus.case_specs()

        self.assertEqual(len(specs), 64)
        self.assertEqual(
            [spec.case_id for spec in specs],
            [f"full-list-rtl-{index:03d}" for index in range(64)],
        )
        self.assertEqual(len({spec.factor_state for spec in specs}), 64)
        for position, factor in enumerate(generate_render_list_rtl_corpus.FACTOR_NAMES):
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
                    left=generate_render_list_rtl_corpus.FACTOR_NAMES[left],
                    right=generate_render_list_rtl_corpus.FACTOR_NAMES[right],
                ):
                    self.assertEqual(set(counts.values()), {16})

    def test_generator_rejects_incomplete_and_duplicate_factor_grids(self):
        specs = generate_render_list_rtl_corpus.case_specs()
        with self.assertRaisesRegex(ValueError, "case count"):
            generate_render_list_rtl_corpus._validate_specs(specs[:-1])

        duplicate = dataclasses.replace(specs[-1], factor_state=specs[0].factor_state)
        with self.assertRaisesRegex(ValueError, "factor vectors"):
            generate_render_list_rtl_corpus._validate_specs(specs[:-1] + (duplicate,))

    def test_payloads_are_deterministic_and_primary_markup_matches_factors(self):
        payloads = []
        for spec in generate_render_list_rtl_corpus.case_specs():
            first = generate_render_list_rtl_corpus.build_case(spec)
            second = generate_render_list_rtl_corpus.build_case(spec)
            payloads.append(first)
            with self.subTest(case=spec.case_id):
                self.assertEqual(first, second)
                self.assertTrue(first.startswith(b"PK"))
                self.assertLess(len(first), 64 * 1024)
                with zipfile.ZipFile(io.BytesIO(first)) as archive:
                    self.assertEqual(archive.namelist(), sorted(archive.namelist()))
                    document_bytes = archive.read("word/document.xml")
                    numbering = archive.read("word/numbering.xml")
                document = ElementTree.fromstring(document_bytes)
                primary = primary_paragraph(document, spec.case_id)
                ppr = primary.find("w:pPr", NS)
                self.assertIsNotNone(ppr)
                assert ppr is not None
                self.assertEqual(
                    ppr.find("w:bidi", NS) is not None, spec.paragraph_bidi
                )
                self.assertEqual(
                    ppr.find("w:tabs", NS) is not None,
                    spec.explicit_tabs,
                )
                self.assertEqual(
                    primary.find("w:r/w:rPr/w:rtl", NS) is not None,
                    spec.run_rtl,
                )
                self.assertEqual(
                    primary.find(".//w:tab", NS) is not None,
                    spec.explicit_tabs,
                )
                self.assertEqual(
                    ppr.find("w:numPr/w:numId", NS).get(f"{{{W}}}val"),
                    "11" if spec.ordered else "15",
                )
                self.assertEqual(
                    ppr.find("w:numPr/w:ilvl", NS).get(f"{{{W}}}val"),
                    "1" if spec.level_one else "0",
                )
                self.assertEqual(
                    ppr.find("w:jc", NS).get(f"{{{W}}}val"),
                    "start",
                )
                text = paragraph_text(primary)
                self.assertIn(
                    "\u0627\u0644\u0639\u0631\u0628\u064a\u0629"
                    if spec.arabic
                    else "\u05e2\u05d1\u05e8\u05d9\u05ea",
                    text,
                )
                self.assertIn(b'<w:startOverride w:val="5"/>', numbering)
                self.assertIn(b'<w:numFmt w:val="lowerLetter"/>', numbering)
                self.assertNotIn(b"/Users/", document_bytes)
                self.assertNotIn(b"/home/", document_bytes)
        self.assertEqual(len(set(payloads)), 64)

    def test_primary_label_expectations_cover_each_factor_bucket(self):
        counts = {}
        for spec in generate_render_list_rtl_corpus.case_specs():
            label = generate_render_list_rtl_corpus.primary_expected_label(spec)
            counts[label] = counts.get(label, 0) + 1
        self.assertEqual(
            counts,
            {"1.": 16, "1.a)": 16, "\u2022": 16, "\u25e6": 16},
        )

    def test_representative_payload_hashes_are_stable(self):
        expected = {
            "full-list-rtl-000": "6261986cf717584f3c1daf282af01ae869f08f4023129b8656af6c82a0b4b655",
            "full-list-rtl-021": "3154dcf9fd44ada83e0ce377aad5c1d5f8ec9ba472e9d17aafab80257a93dea7",
            "full-list-rtl-063": "e5fa8f8f44d7806c1ff9c60aaf2642c04af4a8067d0cb112db20bbc8dfbcebc3",
        }
        by_id = {
            spec.case_id: spec for spec in generate_render_list_rtl_corpus.case_specs()
        }
        for case_id, expected_sha256 in expected.items():
            payload = generate_render_list_rtl_corpus.build_case(by_id[case_id])
            with self.subTest(case=case_id):
                self.assertEqual(hashlib.sha256(payload).hexdigest(), expected_sha256)

    def test_lock_is_canonical_and_records_factorial_coverage(self):
        lock = generate_render_list_rtl_corpus.build_lock()
        checked_in = generate_render_list_rtl_corpus.load_lock(LOCK)

        self.assertEqual(checked_in, lock)
        self.assertEqual(
            LOCK.read_bytes(), generate_render_list_rtl_corpus.canonical_json(lock)
        )
        self.assertEqual(lock["schema"], "rwml.render-corpus-batch-lock.v1")
        self.assertEqual(lock["campaign"], "public-render-full-list-rtl-v1")
        self.assertEqual(len(lock["documents"]), 64)
        self.assertEqual(lock["limits"]["max_documents"], 64)
        self.assertRegex(lock["generator_closure_sha256"], r"[0-9a-f]{64}\Z")
        coverage = lock["coverage"]
        self.assertEqual(coverage["cohort"], "list-rtl")
        self.assertEqual(coverage["case_count"], 64)
        self.assertEqual(coverage["design"], "complete-2-level-factorial")
        self.assertEqual(
            coverage["factor_names"], list(generate_render_list_rtl_corpus.FACTOR_NAMES)
        )
        self.assertEqual(
            coverage["factor_case_counts"],
            {factor: 32 for factor in generate_render_list_rtl_corpus.FACTOR_NAMES},
        )
        self.assertEqual(len(coverage["pairwise_state_counts"]), 15)
        for row in coverage["pairwise_state_counts"]:
            self.assertEqual(row["states"], {"00": 16, "01": 16, "10": 16, "11": 16})
        self.assertEqual(
            coverage["primary_label_case_counts"],
            {"1.": 16, "1.a)": 16, "\u2022": 16, "\u25e6": 16},
        )
        canonical = generate_render_list_rtl_corpus.canonical_json(lock)
        self.assertNotIn(b"/Users/", canonical)
        self.assertNotIn(b"/home/", canonical)

    def test_materialized_batch_passes_the_strict_corpus_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "list-rtl"
            manifest = generate_render_list_rtl_corpus.materialize(
                output, generate_render_list_rtl_corpus.load_lock(LOCK)
            )
            corpus = generate_render_list_rtl_corpus.load_corpus_manifest(manifest)

            self.assertEqual(corpus.campaign, "public-render-full-list-rtl-v1")
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
                    ["RENDER_ORACLE.json", "provenance/rwml-render-full-list-rtl.md"]
                    + [
                        f"documents/full-list-rtl-{index:03d}.docx"
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
                generate_render_list_rtl_corpus.load_lock(noncanonical)

            modified = json.loads(LOCK.read_bytes())
            modified["documents"][0]["bytes"] += 1
            with self.assertRaisesRegex(ValueError, "generator closure"):
                generate_render_list_rtl_corpus.materialize(
                    temporary / "changed", modified
                )


if __name__ == "__main__":
    unittest.main()
