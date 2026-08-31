#!/usr/bin/env python3

import importlib.util
import hashlib
import io
import json
import pathlib
import sys
import tempfile
import unittest
from unittest import mock
import zipfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "generate_render_full_corpus.py"
LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-run-paint-v1.json"
SPEC = importlib.util.spec_from_file_location("generate_render_full_corpus", SCRIPT)
generate_render_full_corpus = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = generate_render_full_corpus
SPEC.loader.exec_module(generate_render_full_corpus)


FEATURE_XML = {
    "bold": b"<w:b/>",
    "caps": b"<w:caps/>",
    "font-color": b"<w:color w:val=",
    "font-size": b"<w:sz w:val=",
    "hidden-text": b"<w:vanish/>",
    "highlight": b"<w:highlight w:val=",
    "italic": b"<w:i/>",
    "small-caps": b"<w:smallCaps/>",
    "strike": b"<w:strike/>",
    "subscript": b'<w:vertAlign w:val="subscript"/>',
    "superscript": b'<w:vertAlign w:val="superscript"/>',
    "underline": b'<w:u w:val="single"/>',
}


class RenderFullCorpusGeneratorTests(unittest.TestCase):
    def test_run_paint_specs_are_distinct_balanced_and_pairwise_complete(self):
        specs = generate_render_full_corpus.case_specs()

        self.assertEqual(len(specs), 64)
        self.assertEqual(
            [spec.case_id for spec in specs],
            [f"full-run-paint-{index:03d}" for index in range(64)],
        )
        self.assertEqual(len({spec.features for spec in specs}), 64)
        lattice = generate_render_full_corpus.LATTICE_FEATURES
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
        broken_masks = generate_render_full_corpus.FEATURE_MASKS[:-1] + (
            ("underline", 0b000001),
        )
        with mock.patch.object(
            generate_render_full_corpus, "FEATURE_MASKS", broken_masks
        ):
            with self.assertRaisesRegex(ValueError, "pairwise lattice"):
                generate_render_full_corpus.case_specs()

    def test_generator_rejects_unbalanced_pairwise_counts(self):
        source_specs = generate_render_full_corpus.case_specs()
        add_caps = {
            spec.index
            for spec in source_specs
            if "bold" in spec.features and "caps" not in spec.features
        }
        remove_caps = {
            spec.index
            for spec in source_specs
            if "bold" not in spec.features and "caps" in spec.features
        }
        add_caps = set(sorted(add_caps)[:8])
        remove_caps = set(sorted(remove_caps)[:8])
        specs = []
        for index, source in enumerate(source_specs):
            features = set(source.features)
            if index in add_caps:
                features.add("caps")
            if index in remove_caps:
                features.remove("caps")
            features.add(f"unique-probe-{index:03d}")
            specs.append(
                generate_render_full_corpus.CaseSpec(
                    index=index,
                    case_id=source.case_id,
                    features=tuple(sorted(features)),
                )
            )

        with self.assertRaisesRegex(ValueError, "pairwise lattice"):
            generate_render_full_corpus._validate_specs(tuple(specs))

    def test_payloads_are_deterministic_and_labels_match_authored_ooxml(self):
        payloads = []
        for spec in generate_render_full_corpus.case_specs():
            first = generate_render_full_corpus.build_case(spec)
            second = generate_render_full_corpus.build_case(spec)
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
            "full-run-paint-000": "dc02a8c66f71f31445048f4636e278a21c70d586189fefde8fcd4783dacc8b99",
            "full-run-paint-021": "7b1ef8cb99e938297cf4a0a3a34dc54863a0613627cf6dfa8a9152f6723c2463",
            "full-run-paint-063": "a0d0d35a7e08a2ab593b07f57924e770ed87bd3128598a9dac6ee8bf5c6dc5f8",
        }
        by_id = {
            spec.case_id: spec for spec in generate_render_full_corpus.case_specs()
        }
        for case_id, expected_sha256 in expected.items():
            payload = generate_render_full_corpus.build_case(by_id[case_id])
            with self.subTest(case=case_id):
                self.assertEqual(hashlib.sha256(payload).hexdigest(), expected_sha256)

    def test_lock_is_canonical_and_records_reviewable_coverage(self):
        lock = generate_render_full_corpus.build_lock()
        checked_in = generate_render_full_corpus.load_lock(LOCK)

        self.assertEqual(checked_in, lock)
        self.assertEqual(
            LOCK.read_bytes(), generate_render_full_corpus.canonical_json(lock)
        )
        self.assertEqual(lock["schema"], "rwml.render-corpus-batch-lock.v1")
        self.assertEqual(lock["campaign"], "public-render-full-run-paint-v1")
        self.assertEqual(len(lock["documents"]), 64)
        self.assertEqual(lock["limits"]["max_documents"], 64)
        self.assertRegex(lock["generator_closure_sha256"], r"[0-9a-f]{64}\Z")
        coverage = lock["coverage"]
        self.assertEqual(coverage["cohort"], "run-paint")
        self.assertEqual(coverage["case_count"], 64)
        self.assertEqual(
            coverage["lattice_features"],
            list(generate_render_full_corpus.LATTICE_FEATURES),
        )
        self.assertEqual(
            coverage["feature_case_counts"],
            {feature: 32 for feature in generate_render_full_corpus.LATTICE_FEATURES},
        )
        self.assertEqual(len(coverage["pairwise_state_counts"]), 66)
        for row in coverage["pairwise_state_counts"]:
            self.assertEqual(row["states"], {"00": 16, "01": 16, "10": 16, "11": 16})
        self.assertNotIn(b"/Users/", generate_render_full_corpus.canonical_json(lock))
        self.assertNotIn(b"/home/", generate_render_full_corpus.canonical_json(lock))

    def test_materialized_batch_passes_the_strict_corpus_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "run-paint"
            manifest = generate_render_full_corpus.materialize(
                output, generate_render_full_corpus.load_lock(LOCK)
            )
            corpus = generate_render_full_corpus.load_corpus_manifest(manifest)

            self.assertEqual(corpus.campaign, "public-render-full-run-paint-v1")
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
                    ["RENDER_ORACLE.json", "provenance/rwml-render-full-run-paint.md"]
                    + [
                        f"documents/full-run-paint-{index:03d}.docx"
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
                generate_render_full_corpus.load_lock(noncanonical)

            modified = json.loads(LOCK.read_bytes())
            modified["documents"][0]["bytes"] += 1
            with self.assertRaisesRegex(ValueError, "generator closure"):
                generate_render_full_corpus.materialize(temporary / "changed", modified)


if __name__ == "__main__":
    unittest.main()
