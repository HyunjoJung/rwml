#!/usr/bin/env python3

import dataclasses
import gzip
import hashlib
import importlib.util
import io
import json
import pathlib
import struct
import sys
import tempfile
import unittest
from xml.etree import ElementTree
import zipfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "generate_render_metafile_corpus.py"
LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-metafile-v1.json"
SPEC = importlib.util.spec_from_file_location("generate_render_metafile_corpus", SCRIPT)
generate_render_metafile_corpus = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = generate_render_metafile_corpus
SPEC.loader.exec_module(generate_render_metafile_corpus)


W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"
PIC = "http://schemas.openxmlformats.org/drawingml/2006/picture"
NS = {"w": W, "r": R, "a": A, "pic": PIC}


def expected_rgba() -> bytes:
    colors = (
        (0xFF, 0x00, 0x00, 0xFF),
        (0x00, 0xFF, 0x00, 0xFF),
        (0x00, 0x00, 0xFF, 0xFF),
        (0xFF, 0xFF, 0x00, 0xFF),
    )
    out = bytearray()
    for y in range(80):
        for x in range(160):
            index = (2 if y >= 40 else 0) + (1 if x >= 80 else 0)
            out.extend(colors[index])
    return bytes(out)


def decode_single_dib(raw_metafile: bytes) -> bytes:
    candidates = []
    for offset in range(len(raw_metafile) - 40):
        if raw_metafile[offset : offset + 4] != struct.pack("<I", 40):
            continue
        width, height, planes, bit_count = struct.unpack_from(
            "<iiHH", raw_metafile, offset + 4
        )
        if (width, height, planes, bit_count) in {
            (160, -80, 1, 8),
            (160, -80, 1, 16),
        }:
            candidates.append(offset)
    if len(candidates) != 1:
        raise AssertionError(f"expected one DIB, found {len(candidates)}")

    offset = candidates[0]
    width, height, _, bit_count = struct.unpack_from("<iiHH", raw_metafile, offset + 4)
    compression, image_bytes, colors_used = struct.unpack_from(
        "<II8xI", raw_metafile, offset + 16
    )
    extra = colors_used * 4 if bit_count == 8 else 12
    bmi = raw_metafile[offset : offset + 40 + extra]
    bits = raw_metafile[offset + 40 + extra : offset + 40 + extra + image_bytes]
    if (width, height) != (160, -80):
        raise AssertionError(f"unexpected DIB dimensions: {(width, height)}")

    out = bytearray()
    if bit_count == 8:
        if (compression, colors_used) != (0, 4):
            raise AssertionError("unexpected indexed DIB metadata")
        palette = [
            (bmi[pos + 2], bmi[pos + 1], bmi[pos], 0xFF) for pos in range(40, 56, 4)
        ]
        if len(bits) != 160 * 80:
            raise AssertionError("unexpected indexed DIB byte count")
        for index in bits:
            out.extend(palette[index])
    else:
        if (bit_count, compression, colors_used) != (16, 3, 0):
            raise AssertionError("unexpected bitfield DIB metadata")
        if struct.unpack_from("<III", bmi, 40) != (0xF800, 0x07E0, 0x001F):
            raise AssertionError("unexpected bitfield masks")
        if len(bits) != 160 * 80 * 2:
            raise AssertionError("unexpected bitfield DIB byte count")
        for (value,) in struct.iter_unpack("<H", bits):
            out.extend(
                (
                    ((value >> 11) & 0x1F) * 255 // 31,
                    ((value >> 5) & 0x3F) * 255 // 63,
                    (value & 0x1F) * 255 // 31,
                    0xFF,
                )
            )
    return bytes(out)


class RenderMetafileCorpusGeneratorTests(unittest.TestCase):
    def test_specs_form_a_complete_six_factor_grid(self):
        specs = generate_render_metafile_corpus.case_specs()

        self.assertEqual(len(specs), 64)
        self.assertEqual(
            [spec.case_id for spec in specs],
            [f"full-metafile-{index:03d}" for index in range(64)],
        )
        self.assertEqual(len({spec.factor_state for spec in specs}), 64)
        for position, factor in enumerate(generate_render_metafile_corpus.FACTOR_NAMES):
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
                    left=generate_render_metafile_corpus.FACTOR_NAMES[left],
                    right=generate_render_metafile_corpus.FACTOR_NAMES[right],
                ):
                    self.assertEqual(set(counts.values()), {16})

    def test_generator_rejects_incomplete_and_duplicate_factor_grids(self):
        specs = generate_render_metafile_corpus.case_specs()
        with self.assertRaisesRegex(ValueError, "case count"):
            generate_render_metafile_corpus._validate_specs(specs[:-1])

        duplicate = dataclasses.replace(specs[-1], factor_state=specs[0].factor_state)
        with self.assertRaisesRegex(ValueError, "factor vectors"):
            generate_render_metafile_corpus._validate_specs(specs[:-1] + (duplicate,))

    def test_payloads_are_deterministic_and_markup_matches_factors(self):
        payloads = []
        expected_pixels = expected_rgba()
        for spec in generate_render_metafile_corpus.case_specs():
            first = generate_render_metafile_corpus.build_case(spec)
            second = generate_render_metafile_corpus.build_case(spec)
            payloads.append(first)
            with self.subTest(case=spec.case_id):
                self.assertEqual(first, second)
                self.assertTrue(first.startswith(b"PK"))
                self.assertLess(len(first), 64 * 1024)
                with zipfile.ZipFile(io.BytesIO(first)) as archive:
                    self.assertEqual(archive.namelist(), sorted(archive.namelist()))
                    media_path = f"word/media/{spec.media_name}"
                    self.assertIn(media_path, archive.namelist())
                    media = archive.read(media_path)
                    document_bytes = archive.read("word/document.xml")
                    document = ElementTree.fromstring(document_bytes)
                    relationships = ElementTree.fromstring(
                        archive.read("word/_rels/document.xml.rels")
                    )
                    content_types = ElementTree.fromstring(
                        archive.read("[Content_Types].xml")
                    )

                self.assertEqual(
                    media, generate_render_metafile_corpus.build_metafile(spec)
                )
                raw = gzip.decompress(media) if spec.compressed else media
                if spec.wmf:
                    self.assertEqual(struct.unpack_from("<I", raw, 0)[0], 0x9AC6CDD7)
                    self.assertEqual(
                        struct.unpack_from("<H", raw, 44)[0],
                        0x0D33 if spec.setdib else 0x0940,
                    )
                else:
                    self.assertEqual(struct.unpack_from("<I", raw, 0)[0], 1)
                    self.assertEqual(raw[40:44], b" EMF")
                    self.assertEqual(
                        struct.unpack_from("<I", raw, 88)[0],
                        80 if spec.setdib else 76,
                    )
                self.assertEqual(decode_single_dib(raw), expected_pixels)

                tables = document.findall(".//w:body/w:tbl", NS)
                self.assertEqual(len(tables), 1 if spec.table_cell else 0)
                self.assertEqual(len(document.findall(".//a:blip", NS)), 1)
                blip = document.find(".//a:blip", NS)
                self.assertIsNotNone(blip)
                assert blip is not None
                self.assertEqual(blip.get(f"{{{R}}}embed"), "rIdMeta")
                transform = document.find(".//a:xfrm", NS)
                self.assertIsNotNone(transform)
                assert transform is not None
                self.assertEqual(
                    transform.get("rot"), "5400000" if spec.quarter_turn else None
                )
                self.assertIn(spec.case_id.encode("ascii"), document_bytes)

                relationship = next(
                    item for item in relationships if item.get("Id") == "rIdMeta"
                )
                self.assertEqual(relationship.get("Target"), f"media/{spec.media_name}")
                defaults = {
                    item.get("Extension"): item.get("ContentType")
                    for item in content_types
                    if item.tag.endswith("Default")
                }
                self.assertEqual(defaults[spec.extension], spec.content_type)
                self.assertNotIn(b"/Users/", first)
                self.assertNotIn(b"/home/", first)
        self.assertEqual(len(set(payloads)), 64)

    def test_gzip_cases_wrap_the_exact_matching_raw_metafile(self):
        specs = generate_render_metafile_corpus.case_specs()
        by_state = {spec.factor_state: spec for spec in specs}
        for compressed in (spec for spec in specs if spec.compressed):
            raw_state = list(compressed.factor_state)
            raw_state[1] = False
            raw = by_state[tuple(raw_state)]
            payload = generate_render_metafile_corpus.build_metafile(compressed)
            with self.subTest(case=compressed.case_id):
                self.assertEqual(payload[:4], b"\x1f\x8b\x08\x00")
                self.assertEqual(payload[4:8], b"\x00\x00\x00\x00")
                self.assertEqual(
                    gzip.decompress(payload),
                    generate_render_metafile_corpus.build_metafile(raw),
                )

    def test_representative_payload_hashes_are_stable(self):
        expected = {
            "full-metafile-000": "e8fb38b7c749f36592fba625e1a8c5044199f7165a99b22bfc45c7c9166e933a",
            "full-metafile-021": "ae312dab9cf2f5c5a7ffe2175d71d867121f527a21271cfaf990807cb2a8c100",
            "full-metafile-063": "d95f50046b86c5c2e4d5cbb975388a9ff2047885e62ba59a98f18d064c43ef47",
        }
        by_id = {
            spec.case_id: spec for spec in generate_render_metafile_corpus.case_specs()
        }
        for case_id, expected_sha256 in expected.items():
            payload = generate_render_metafile_corpus.build_case(by_id[case_id])
            with self.subTest(case=case_id):
                self.assertEqual(hashlib.sha256(payload).hexdigest(), expected_sha256)

    def test_lock_is_canonical_and_records_factorial_coverage(self):
        lock = generate_render_metafile_corpus.build_lock()
        checked_in = generate_render_metafile_corpus.load_lock(LOCK)

        self.assertEqual(checked_in, lock)
        self.assertEqual(
            LOCK.read_bytes(), generate_render_metafile_corpus.canonical_json(lock)
        )
        self.assertEqual(lock["schema"], "rwml.render-corpus-batch-lock.v1")
        self.assertEqual(lock["campaign"], "public-render-full-metafile-v1")
        self.assertEqual(len(lock["documents"]), 64)
        self.assertEqual(lock["limits"]["max_documents"], 64)
        self.assertRegex(lock["generator_closure_sha256"], r"[0-9a-f]{64}\Z")
        coverage = lock["coverage"]
        self.assertEqual(coverage["cohort"], "metafile-raster-interactions")
        self.assertEqual(coverage["case_count"], 64)
        self.assertEqual(coverage["design"], "complete-2-level-factorial")
        self.assertEqual(
            coverage["factor_names"],
            list(generate_render_metafile_corpus.FACTOR_NAMES),
        )
        self.assertEqual(
            coverage["factor_case_counts"],
            {factor: 32 for factor in generate_render_metafile_corpus.FACTOR_NAMES},
        )
        self.assertEqual(len(coverage["pairwise_state_counts"]), 15)
        for row in coverage["pairwise_state_counts"]:
            self.assertEqual(row["states"], {"00": 16, "01": 16, "10": 16, "11": 16})
        self.assertEqual(coverage["interaction_scope"], "single-raster-image")
        canonical = generate_render_metafile_corpus.canonical_json(lock)
        self.assertNotIn(b"/Users/", canonical)
        self.assertNotIn(b"/home/", canonical)

    def test_materialized_batch_passes_the_strict_corpus_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "metafile"
            manifest = generate_render_metafile_corpus.materialize(
                output, generate_render_metafile_corpus.load_lock(LOCK)
            )
            corpus = generate_render_metafile_corpus.load_corpus_manifest(manifest)

            self.assertEqual(corpus.campaign, "public-render-full-metafile-v1")
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
                    ["RENDER_ORACLE.json", "provenance/rwml-render-full-metafile.md"]
                    + [
                        f"documents/full-metafile-{index:03d}.docx"
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
                generate_render_metafile_corpus.load_lock(noncanonical)

            modified = json.loads(LOCK.read_bytes())
            modified["documents"][0]["bytes"] += 1
            with self.assertRaisesRegex(ValueError, "generator closure"):
                generate_render_metafile_corpus.materialize(
                    temporary / "changed", modified
                )


if __name__ == "__main__":
    unittest.main()
