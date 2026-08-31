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
SCRIPT = ROOT / "scripts" / "generate_render_note_field_corpus.py"
LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-note-field-v1.json"
SPEC = importlib.util.spec_from_file_location(
    "generate_render_note_field_corpus", SCRIPT
)
generate_render_note_field_corpus = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = generate_render_note_field_corpus
SPEC.loader.exec_module(generate_render_note_field_corpus)


W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
NS = {"w": W, "r": R}
VAL = f"{{{W}}}val"


class RenderNoteFieldCorpusGeneratorTests(unittest.TestCase):
    def test_specs_form_a_complete_six_factor_grid(self):
        specs = generate_render_note_field_corpus.case_specs()

        self.assertEqual(len(specs), 64)
        self.assertEqual(
            [spec.case_id for spec in specs],
            [f"full-note-field-{index:03d}" for index in range(64)],
        )
        self.assertEqual(len({spec.factor_state for spec in specs}), 64)
        for position, factor in enumerate(
            generate_render_note_field_corpus.FACTOR_NAMES
        ):
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
                    left=generate_render_note_field_corpus.FACTOR_NAMES[left],
                    right=generate_render_note_field_corpus.FACTOR_NAMES[right],
                ):
                    self.assertEqual(set(counts.values()), {16})

    def test_generator_rejects_incomplete_and_duplicate_factor_grids(self):
        specs = generate_render_note_field_corpus.case_specs()
        with self.assertRaisesRegex(ValueError, "case count"):
            generate_render_note_field_corpus._validate_specs(specs[:-1])

        duplicate = dataclasses.replace(specs[-1], factor_state=specs[0].factor_state)
        with self.assertRaisesRegex(ValueError, "factor vectors"):
            generate_render_note_field_corpus._validate_specs(specs[:-1] + (duplicate,))

    def test_primary_marker_expectation_tracks_start_and_format(self):
        expected = {
            (False, False): "1",
            (True, False): "5",
            (False, True): "i",
            (True, True): "v",
        }
        for spec in generate_render_note_field_corpus.case_specs():
            state = (spec.start_at_five, spec.lower_roman)
            with self.subTest(case=spec.case_id):
                self.assertEqual(spec.expected_primary_marker, expected[state])

    def test_payloads_are_deterministic_and_markup_matches_factors(self):
        payloads = []
        for spec in generate_render_note_field_corpus.case_specs():
            first = generate_render_note_field_corpus.build_case(spec)
            second = generate_render_note_field_corpus.build_case(spec)
            payloads.append(first)
            with self.subTest(case=spec.case_id):
                self.assertEqual(first, second)
                self.assertTrue(first.startswith(b"PK"))
                self.assertLess(len(first), 64 * 1024)
                with zipfile.ZipFile(io.BytesIO(first)) as archive:
                    self.assertEqual(archive.namelist(), sorted(archive.namelist()))
                    names = set(archive.namelist())
                    document_bytes = archive.read("word/document.xml")
                    document = ElementTree.fromstring(document_bytes)
                    settings = ElementTree.fromstring(archive.read("word/settings.xml"))
                    relationships = ElementTree.fromstring(
                        archive.read("word/_rels/document.xml.rels")
                    )
                    note_name = (
                        "word/endnotes.xml" if spec.endnote else "word/footnotes.xml"
                    )
                    note_bytes = archive.read(note_name)
                    notes = ElementTree.fromstring(note_bytes)

                self.assertEqual("word/endnotes.xml" in names, spec.endnote)
                self.assertEqual("word/footnotes.xml" in names, not spec.endnote)
                note_kind = "endnote" if spec.endnote else "footnote"
                reference_tag = f"w:{note_kind}Reference"
                marker_tag = f"w:{note_kind}Ref"
                property_tag = f"w:{note_kind}Pr"

                settings_properties = settings.find(property_tag, NS)
                self.assertIsNotNone(settings_properties)
                assert settings_properties is not None
                self.assertEqual(
                    settings_properties.find("w:numStart", NS).get(VAL),
                    "5" if spec.start_at_five else "1",
                )
                self.assertEqual(
                    settings_properties.find("w:numFmt", NS).get(VAL),
                    "lowerRoman" if spec.lower_roman else "decimal",
                )

                references = document.findall(f".//{reference_tag}", NS)
                self.assertEqual(
                    sorted(reference.get(f"{{{W}}}id") for reference in references),
                    ["4", "777", "90"],
                )
                custom = next(
                    reference
                    for reference in references
                    if reference.get(f"{{{W}}}id") == "90"
                )
                self.assertEqual(custom.get(f"{{{W}}}customMarkFollows"), "1")
                deleted = document.find(f".//w:del//{reference_tag}", NS)
                self.assertIsNotNone(deleted)
                assert deleted is not None
                self.assertEqual(deleted.get(f"{{{W}}}id"), "777")

                inserted_references = document.findall(f".//w:ins//{reference_tag}", NS)
                self.assertEqual(
                    [item.get(f"{{{W}}}id") for item in inserted_references],
                    ["4"] if spec.accepted_insertion else [],
                )
                self.assertEqual(
                    len(document.findall(".//w:ins", NS)),
                    2 if spec.accepted_insertion else 0,
                )
                self.assertIsNotNone(
                    document.find('.//w:bookmarkStart[@w:name="PrimaryNote"]', NS)
                )

                tables = document.findall(".//w:body/w:tbl", NS)
                self.assertEqual(len(tables), 1 if spec.table_cell else 0)
                if tables:
                    self.assertIsNotNone(
                        tables[0].find(f".//{reference_tag}[@w:id='4']", NS)
                    )
                else:
                    self.assertIsNotNone(
                        document.find(f".//{reference_tag}[@w:id='4']", NS)
                    )

                simple_fields = document.findall(".//w:fldSimple", NS)
                instruction_text = "".join(
                    item.text or "" for item in document.findall(".//w:instrText", NS)
                )
                self.assertEqual(len(simple_fields), 0 if spec.complex_noteref else 1)
                if simple_fields:
                    self.assertIn(
                        "NOTEREF PrimaryNote", simple_fields[0].get(f"{{{W}}}instr")
                    )
                self.assertEqual(
                    "NOTEREF PrimaryNote" in instruction_text, spec.complex_noteref
                )
                self.assertEqual(
                    len(document.findall(".//w:fldChar", NS)),
                    3 if spec.complex_noteref else 0,
                )

                note_entries = notes.findall(f"w:{note_kind}", NS)
                self.assertEqual(
                    sorted(entry.get(f"{{{W}}}id") for entry in note_entries),
                    ["-1", "0", "4", "90"],
                )
                primary_note = next(
                    entry for entry in note_entries if entry.get(f"{{{W}}}id") == "4"
                )
                custom_note = next(
                    entry for entry in note_entries if entry.get(f"{{{W}}}id") == "90"
                )
                self.assertIsNotNone(primary_note.find(f".//{marker_tag}", NS))
                self.assertIsNotNone(custom_note.find(f".//{marker_tag}", NS))
                self.assertIsNotNone(primary_note.find(".//w:ins", NS))
                self.assertIsNotNone(primary_note.find(".//w:del", NS))
                formula = primary_note.find(".//w:fldSimple", NS)
                self.assertIsNotNone(formula)
                assert formula is not None
                self.assertIn("= 6 * 7", formula.get(f"{{{W}}}instr"))

                relationship_types = {
                    item.get("Type"): item.get("Target") for item in relationships
                }
                self.assertEqual(
                    relationship_types[f"{R}/{note_kind}s"],
                    f"{note_kind}s.xml",
                )
                self.assertIn(spec.case_id.encode("ascii"), document_bytes)
                self.assertIn(b"accepted note text", note_bytes)
                self.assertIn(b"rejected note text", note_bytes)
                self.assertNotIn(b"/Users/", first)
                self.assertNotIn(b"/home/", first)
        self.assertEqual(len(set(payloads)), 64)

    def test_representative_payload_hashes_are_stable(self):
        expected = {
            "full-note-field-000": "b827c0d7a168f3fe3d4886dcc0b6ed6dc6dbbe94eb0778de88c92b08af7a9bed",
            "full-note-field-021": "a9bad2efa823091c8c059ffc03dc5d9991e9abbc90247391064781d83b301585",
            "full-note-field-063": "c2d3e61af6fdf592cfd71f4418042f913f7807192d88f0b36f5b5162a85e3735",
        }
        by_id = {
            spec.case_id: spec
            for spec in generate_render_note_field_corpus.case_specs()
        }
        for case_id, expected_sha256 in expected.items():
            payload = generate_render_note_field_corpus.build_case(by_id[case_id])
            with self.subTest(case=case_id):
                self.assertEqual(hashlib.sha256(payload).hexdigest(), expected_sha256)

    def test_lock_is_canonical_and_records_factorial_coverage(self):
        lock = generate_render_note_field_corpus.build_lock()
        checked_in = generate_render_note_field_corpus.load_lock(LOCK)

        self.assertEqual(checked_in, lock)
        self.assertEqual(
            LOCK.read_bytes(), generate_render_note_field_corpus.canonical_json(lock)
        )
        self.assertEqual(lock["schema"], "rwml.render-corpus-batch-lock.v1")
        self.assertEqual(lock["campaign"], "public-render-full-note-field-v1")
        self.assertEqual(len(lock["documents"]), 64)
        self.assertEqual(lock["limits"]["max_documents"], 64)
        self.assertRegex(lock["generator_closure_sha256"], r"[0-9a-f]{64}\Z")
        coverage = lock["coverage"]
        self.assertEqual(coverage["cohort"], "note-field-revision-interactions")
        self.assertEqual(coverage["case_count"], 64)
        self.assertEqual(coverage["design"], "complete-2-level-factorial")
        self.assertEqual(
            coverage["factor_names"],
            list(generate_render_note_field_corpus.FACTOR_NAMES),
        )
        self.assertEqual(
            coverage["factor_case_counts"],
            {factor: 32 for factor in generate_render_note_field_corpus.FACTOR_NAMES},
        )
        self.assertEqual(len(coverage["pairwise_state_counts"]), 15)
        for row in coverage["pairwise_state_counts"]:
            self.assertEqual(row["states"], {"00": 16, "01": 16, "10": 16, "11": 16})
        self.assertEqual(coverage["interaction_scope"], "primary-note-and-noteref")
        canonical = generate_render_note_field_corpus.canonical_json(lock)
        self.assertNotIn(b"/Users/", canonical)
        self.assertNotIn(b"/home/", canonical)

    def test_materialized_batch_passes_the_strict_corpus_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "note-field"
            manifest = generate_render_note_field_corpus.materialize(
                output, generate_render_note_field_corpus.load_lock(LOCK)
            )
            corpus = generate_render_note_field_corpus.load_corpus_manifest(manifest)

            self.assertEqual(corpus.campaign, "public-render-full-note-field-v1")
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
                    ["RENDER_ORACLE.json", "provenance/rwml-render-full-note-field.md"]
                    + [
                        f"documents/full-note-field-{index:03d}.docx"
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
                generate_render_note_field_corpus.load_lock(noncanonical)

            modified = json.loads(LOCK.read_bytes())
            modified["documents"][0]["bytes"] += 1
            with self.assertRaisesRegex(ValueError, "generator closure"):
                generate_render_note_field_corpus.materialize(
                    temporary / "changed", modified
                )


if __name__ == "__main__":
    unittest.main()
