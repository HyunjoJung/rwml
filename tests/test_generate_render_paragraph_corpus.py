#!/usr/bin/env python3

import hashlib
import importlib.util
import io
import json
import pathlib
import sys
import tempfile
import unittest
from unittest import mock
import zipfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "generate_render_paragraph_corpus.py"
LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-paragraph-v1.json"
SPEC = importlib.util.spec_from_file_location(
    "generate_render_paragraph_corpus", SCRIPT
)
generate_render_paragraph_corpus = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = generate_render_paragraph_corpus
SPEC.loader.exec_module(generate_render_paragraph_corpus)


FEATURE_XML = {
    "align-center": b'<w:jc w:val="center"/>',
    "align-justify": b'<w:jc w:val="both"/>',
    "align-right": b'<w:jc w:val="right"/>',
    "explicit-tabs": b"<w:tabs>",
    "first-line-indent": b'<w:ind w:firstLine="360"/>',
    "hanging-indent": b'<w:ind w:left="720" w:hanging="360"/>',
    "left-indent": b'<w:ind w:left="720"/>',
    "line-spacing-at-least": b'<w:spacing w:line="330" w:lineRule="atLeast"/>',
    "line-spacing-auto": b'<w:spacing w:line="360" w:lineRule="auto"/>',
    "line-spacing-exact": b'<w:spacing w:line="360" w:lineRule="exact"/>',
    "paragraph-borders": b"<w:pBdr>",
    "paragraph-shading": b'<w:shd w:val="clear" w:color="auto" w:fill="DDEBF7"/>',
    "right-indent": b'<w:ind w:right="720"/>',
    "space-after": b'<w:spacing w:after="240"/>',
    "space-before": b'<w:spacing w:before="240"/>',
}


class RenderParagraphCorpusGeneratorTests(unittest.TestCase):
    def test_specs_are_distinct_balanced_and_pairwise_complete(self):
        specs = generate_render_paragraph_corpus.case_specs()

        self.assertEqual(len(specs), 64)
        self.assertEqual(
            [spec.case_id for spec in specs],
            [f"full-paragraph-{index:03d}" for index in range(64)],
        )
        self.assertEqual(len({spec.features for spec in specs}), 64)
        lattice = generate_render_paragraph_corpus.LATTICE_FEATURES
        self.assertEqual(set(lattice), set(FEATURE_XML))
        for feature in lattice:
            self.assertEqual(
                sum(feature in spec.features for spec in specs),
                32,
                feature,
            )
        for left_index, left in enumerate(lattice):
            for right in lattice[left_index + 1 :]:
                counts = {
                    state: sum(
                        (left in spec.features, right in spec.features) == state
                        for spec in specs
                    )
                    for state in (
                        (False, False),
                        (False, True),
                        (True, False),
                        (True, True),
                    )
                }
                with self.subTest(left=left, right=right):
                    self.assertEqual(set(counts.values()), {16})

    def test_generator_rejects_an_incomplete_lattice(self):
        broken_masks = generate_render_paragraph_corpus.FEATURE_MASKS[:-1] + (
            ("space-before", 0b000001),
        )
        with mock.patch.object(
            generate_render_paragraph_corpus, "FEATURE_MASKS", broken_masks
        ):
            with self.assertRaisesRegex(ValueError, "pairwise lattice"):
                generate_render_paragraph_corpus.case_specs()

    def test_generator_rejects_unbalanced_pairwise_counts(self):
        source_specs = generate_render_paragraph_corpus.case_specs()
        add_center = {
            spec.index
            for spec in source_specs
            if "align-right" in spec.features and "align-center" not in spec.features
        }
        remove_center = {
            spec.index
            for spec in source_specs
            if "align-right" not in spec.features and "align-center" in spec.features
        }
        add_center = set(sorted(add_center)[:8])
        remove_center = set(sorted(remove_center)[:8])
        specs = []
        for index, source in enumerate(source_specs):
            features = set(source.features)
            if index in add_center:
                features.add("align-center")
            if index in remove_center:
                features.remove("align-center")
            features.add(f"unique-probe-{index:03d}")
            specs.append(
                generate_render_paragraph_corpus.CaseSpec(
                    index=index,
                    case_id=source.case_id,
                    features=tuple(sorted(features)),
                )
            )

        with self.assertRaisesRegex(ValueError, "pairwise lattice"):
            generate_render_paragraph_corpus._validate_specs(tuple(specs))

    def test_payloads_are_deterministic_and_labels_match_authored_ooxml(self):
        payloads = []
        for spec in generate_render_paragraph_corpus.case_specs():
            first = generate_render_paragraph_corpus.build_case(spec)
            second = generate_render_paragraph_corpus.build_case(spec)
            payloads.append(first)
            with self.subTest(case=spec.case_id):
                self.assertEqual(first, second)
                self.assertTrue(first.startswith(b"PK"))
                self.assertLess(len(first), 64 * 1024)
                with zipfile.ZipFile(io.BytesIO(first)) as archive:
                    self.assertEqual(archive.namelist(), sorted(archive.namelist()))
                    document = archive.read("word/document.xml")
                self.assertIn(spec.case_id.encode("ascii"), document)
                for feature, token in FEATURE_XML.items():
                    self.assertEqual(
                        token in document, feature in spec.features, feature
                    )
                self.assertNotIn(b"/Users/", document)
                self.assertNotIn(b"/home/", document)
        self.assertEqual(len(set(payloads)), 64)

    def test_representative_payload_hashes_are_stable(self):
        expected = {
            "full-paragraph-000": "774a67924fbaa05d74d7d6916146d9d2348e9c813cac4b70e784b83182cd3d9e",
            "full-paragraph-021": "6817008b59f6a7b09d8f72bb82b41466538f368f891e762a1c22eff3dd2597bf",
            "full-paragraph-063": "97c6fdc55a80646acd76740711d200a6f0f2251db630e8cc7ffced944f6c796b",
        }
        by_id = {
            spec.case_id: spec for spec in generate_render_paragraph_corpus.case_specs()
        }
        for case_id, expected_sha256 in expected.items():
            payload = generate_render_paragraph_corpus.build_case(by_id[case_id])
            with self.subTest(case=case_id):
                self.assertEqual(hashlib.sha256(payload).hexdigest(), expected_sha256)

    def test_lock_is_canonical_and_records_reviewable_coverage(self):
        lock = generate_render_paragraph_corpus.build_lock()
        checked_in = generate_render_paragraph_corpus.load_lock(LOCK)

        self.assertEqual(checked_in, lock)
        self.assertEqual(
            LOCK.read_bytes(), generate_render_paragraph_corpus.canonical_json(lock)
        )
        self.assertEqual(lock["schema"], "rwml.render-corpus-batch-lock.v1")
        self.assertEqual(lock["campaign"], "public-render-full-paragraph-v1")
        self.assertEqual(len(lock["documents"]), 64)
        self.assertEqual(lock["limits"]["max_documents"], 64)
        self.assertRegex(lock["generator_closure_sha256"], r"[0-9a-f]{64}\Z")
        coverage = lock["coverage"]
        self.assertEqual(coverage["cohort"], "paragraph-geometry")
        self.assertEqual(coverage["case_count"], 64)
        self.assertEqual(
            coverage["lattice_features"],
            list(generate_render_paragraph_corpus.LATTICE_FEATURES),
        )
        self.assertEqual(
            coverage["feature_case_counts"],
            {
                feature: 32
                for feature in generate_render_paragraph_corpus.LATTICE_FEATURES
            },
        )
        self.assertEqual(len(coverage["pairwise_state_counts"]), 105)
        for row in coverage["pairwise_state_counts"]:
            self.assertEqual(row["states"], {"00": 16, "01": 16, "10": 16, "11": 16})
        self.assertNotIn(
            b"/Users/", generate_render_paragraph_corpus.canonical_json(lock)
        )
        self.assertNotIn(
            b"/home/", generate_render_paragraph_corpus.canonical_json(lock)
        )

    def test_materialized_batch_passes_the_strict_corpus_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "paragraph"
            manifest = generate_render_paragraph_corpus.materialize(
                output, generate_render_paragraph_corpus.load_lock(LOCK)
            )
            corpus = generate_render_paragraph_corpus.load_corpus_manifest(manifest)

            self.assertEqual(corpus.campaign, "public-render-full-paragraph-v1")
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
                    ["RENDER_ORACLE.json", "provenance/rwml-render-full-paragraph.md"]
                    + [
                        f"documents/full-paragraph-{index:03d}.docx"
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
                generate_render_paragraph_corpus.load_lock(noncanonical)

            modified = json.loads(LOCK.read_bytes())
            modified["documents"][0]["bytes"] += 1
            with self.assertRaisesRegex(ValueError, "generator closure"):
                generate_render_paragraph_corpus.materialize(
                    temporary / "changed", modified
                )


if __name__ == "__main__":
    unittest.main()
