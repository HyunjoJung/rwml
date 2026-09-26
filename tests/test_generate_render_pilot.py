#!/usr/bin/env python3

import importlib.util
import io
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import xml.etree.ElementTree as ET
import zipfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "generate_render_pilot.py"
LOCK = ROOT / "corpus" / "public" / "oracle" / "render-pilot-v1.json"
SPEC = importlib.util.spec_from_file_location("generate_render_pilot", SCRIPT)
generate_render_pilot = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = generate_render_pilot
SPEC.loader.exec_module(generate_render_pilot)


class RenderPilotGeneratorTests(unittest.TestCase):
    def test_pilot_additions_are_canonical_distinct_and_deterministic(self):
        cases = generate_render_pilot.PILOT_CASES

        self.assertEqual(len(cases), 19)
        self.assertEqual([case.case_id for case in cases], sorted(case.case_id for case in cases))
        self.assertEqual(len({case.case_id for case in cases}), 19)
        for case in cases:
            with self.subTest(case=case.case_id):
                first = case.builder()
                second = case.builder()
                self.assertEqual(first, second)
                self.assertTrue(first.startswith(b"PK"))
                self.assertLess(len(first), 256 * 1024)
                with tempfile.TemporaryDirectory() as tmp:
                    path = pathlib.Path(tmp) / "case.docx"
                    path.write_bytes(first)
                    with zipfile.ZipFile(path) as archive:
                        self.assertIn("word/document.xml", archive.namelist())
                        document = archive.read("word/document.xml")
                self.assertNotIn(b"/Users/", document)
                self.assertNotIn(b"/home/", document)

    def test_pilot_preserves_discriminating_coverage(self):
        by_id = {case.case_id: case for case in generate_render_pilot.PILOT_CASES}
        w = "{" + generate_render_pilot.W + "}"

        def document(case_id):
            with zipfile.ZipFile(io.BytesIO(by_id[case_id].builder())) as archive:
                return ET.fromstring(archive.read("word/document.xml"))

        with self.subTest(surface="document-fields"):
            root = document("pilot-fields-document-formula")
            instructions = {e.get(w + "instr") for e in root.iter(w + "fldSimple")}
            self.assertTrue({"NUMWORDS", "NUMCHARS", "NUMPAGES"} <= instructions)

        with self.subTest(surface="unicode"):
            root = document("pilot-unicode-line-breaking")
            text = "".join(root.itertext())
            for fragment in ("한글", "日本語", "中文", "👩‍💻", "Ελληνικά", "кириллица", "\u0301", "\u00ad", "\u200b"):
                self.assertIn(fragment, text)
            self.assertTrue({"cjk", "emoji", "combining-marks"} <= set(by_id["pilot-unicode-line-breaking"].features))

        with self.subTest(surface="mixed-direction"):
            root = document("pilot-rtl-mixed-text")
            paragraphs = list(root.iter(w + "p"))
            self.assertEqual(len(paragraphs), 2)
            self.assertFalse(list(root.iter(w + "br")), "line breaks must not isolate direction changes")
            text = "".join(root.itertext())
            for fragment in ("(A-17)", "123,", "456", "مرحبا", "שלום", "rwml"):
                self.assertIn(fragment, text)

        with self.subTest(surface="rtl-list-numbers"):
            text = "".join(document("pilot-rtl-list").itertext())
            self.assertIn("123", text)
            self.assertIn("45", text)

        with self.subTest(surface="accepted-current-revisions"):
            root = document("pilot-structured-revisions")
            accepted = " ".join(e.text or "" for e in root.iter(w + "t"))
            deleted = " ".join(e.text or "" for e in root.iter(w + "delText"))
            self.assertIn("Rejected", deleted)
            self.assertNotIn("Rejected", accepted, "visible duplicates would hide deletion-view disagreement")

    def test_pilot_lock_binds_exactly_40_documents_and_generator_closure(self):
        lock = generate_render_pilot.build_lock()
        checked_in = generate_render_pilot.load_lock(LOCK)

        self.assertEqual(checked_in, lock)
        self.assertEqual(LOCK.read_bytes(), generate_render_pilot.canonical_json(lock))
        self.assertEqual(lock["schema"], "rwml.render-pilot-lock.v1")
        self.assertEqual(lock["campaign"], "public-render-pilot-v1")
        self.assertEqual(len(lock["documents"]), 40)
        self.assertEqual(
            [row["id"] for row in lock["documents"]],
            sorted(row["id"] for row in lock["documents"]),
        )
        self.assertRegex(lock["generator_closure_sha256"], r"[0-9a-f]{64}\Z")
        self.assertRegex(lock["parent_manifest_sha256"], r"[0-9a-f]{64}\Z")
        generated = [row for row in lock["documents"] if row["source"] == "pilot-generated"]
        parent = [row for row in lock["documents"] if row["source"] == "parent-public"]
        self.assertEqual(len(generated), 19)
        self.assertEqual(len(parent), 21)
        for row in lock["documents"]:
            self.assertEqual(len(row["sha256"]), 64)
            self.assertGreater(row["bytes"], 0)
            self.assertEqual(row["path"], f"documents/{row['id']}.{row['format']}")
        self.assertNotIn(b"/Users/", generate_render_pilot.canonical_json(lock))
        self.assertNotIn(b"/home/", generate_render_pilot.canonical_json(lock))

    def test_materialized_manifest_passes_the_strict_corpus_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "pilot"
            lock = generate_render_pilot.load_lock(LOCK)
            self.assertEqual(lock, generate_render_pilot.build_lock())
            manifest = generate_render_pilot.materialize(output, lock)

            corpus = generate_render_pilot.load_corpus_manifest(manifest)

        self.assertEqual(corpus.campaign, "public-render-pilot-v1")
        self.assertEqual(len(corpus.documents), 40)
        self.assertEqual(corpus.expected_pages, 51)
        features = set().union(*(set(document.features) for document in corpus.documents))
        for feature in (
            "character-paint",
            "mixed-sections",
            "rtl-list",
            "table-merges",
            "unicode-line-breaking",
            "unequal-table-continuation",
        ):
            self.assertIn(feature, features)

    def test_noncanonical_and_modified_locks_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            noncanonical = pathlib.Path(tmp) / "noncanonical.json"
            noncanonical.write_bytes(LOCK.read_bytes() + b"\n")
            with self.assertRaisesRegex(ValueError, "noncanonical"):
                generate_render_pilot.load_lock(noncanonical)

            modified = json.loads(LOCK.read_bytes())
            modified["documents"][0]["bytes"] += 1
            with self.assertRaisesRegex(ValueError, "generator closure"):
                generate_render_pilot.materialize(pathlib.Path(tmp) / "pilot", modified)

    def test_nonfresh_output_is_rejected_without_replacing_existing_inputs(self):
        lock = generate_render_pilot.build_lock()
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "pilot"
            sentinel = output / lock["documents"][0]["path"]
            sentinel.parent.mkdir(parents=True)
            sentinel.write_bytes(b"retained evidence input")
            with self.assertRaisesRegex(ValueError, "fresh"):
                generate_render_pilot.materialize(output, lock)
            self.assertEqual(sentinel.read_bytes(), b"retained evidence input")
            self.assertEqual([p for p in output.rglob("*") if p.is_file()], [sentinel])

    def test_nested_symlink_never_writes_outside_output(self):
        lock = generate_render_pilot.build_lock()
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            external = root / "external"
            external.mkdir()
            sentinel = external / "keep.txt"
            sentinel.write_bytes(b"retained")
            output = root / "pilot"
            output.mkdir()
            (output / "documents").symlink_to(external, target_is_directory=True)
            with self.assertRaises(ValueError):
                generate_render_pilot.materialize(output, lock)
            self.assertEqual(list(external.iterdir()), [sentinel])
            self.assertEqual(sentinel.read_bytes(), b"retained")

    def test_failed_strict_validation_never_publishes_partial_output(self):
        lock = generate_render_pilot.build_lock()
        lock["documents"][0]["expected"]["pages"] = 0
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            with mock.patch.object(generate_render_pilot, "build_lock", return_value=lock):
                with self.assertRaises(ValueError):
                    generate_render_pilot.materialize(root / "pilot", lock)
            self.assertEqual(list(root.iterdir()), [])

    def test_generator_closure_binds_all_local_dependencies(self):
        original = pathlib.Path.read_bytes
        before = generate_render_pilot._generator_closure_sha256()
        for name in (
            "generate_render_pilot.py",
            "gen_public_corpus.py",
            "render_corpus_batch.py",
            "render_oracle_contract.py",
        ):
            source = ROOT / "scripts" / name

            def altered(path):
                return original(path) + (b"\n# Changed source\n" if path == source else b"")

            with self.subTest(source=name), mock.patch.object(
                pathlib.Path, "read_bytes", autospec=True, side_effect=altered
            ):
                self.assertNotEqual(before, generate_render_pilot._generator_closure_sha256())

    def test_symlinked_lock_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            link = pathlib.Path(tmp) / "lock.json"
            link.symlink_to(LOCK)
            with self.assertRaises(ValueError):
                generate_render_pilot.load_lock(link)

    def test_cli_import_modes_repeat_exact_inputs_and_reject_reuse(self):
        commands = ((str(SCRIPT),), ("-m", "scripts.generate_render_pilot"))

        def invoke(command, *arguments):
            return subprocess.run(
                [sys.executable, "-B", *command, *arguments],
                cwd=ROOT, capture_output=True, text=True, timeout=30,
            )

        def payloads(directory):
            return {
                path.relative_to(directory).as_posix(): path.read_bytes()
                for path in directory.rglob("*") if path.is_file()
            }

        with tempfile.TemporaryDirectory() as tmp:
            outputs = [pathlib.Path(tmp) / f"run-{index}" for index in range(2)]
            for command, output in zip(commands, outputs):
                with self.subTest(command=command):
                    checked = invoke(command, "--check")
                    self.assertEqual(checked.returncode, 0, checked.stderr)
                    generated = invoke(command, "--output", str(output))
                    self.assertEqual(generated.returncode, 0, generated.stderr)
            first = payloads(outputs[0])
            self.assertEqual(len(first), 44)
            self.assertEqual(first, payloads(outputs[1]))
            self.assertEqual(
                generate_render_pilot.load_corpus_manifest(
                    outputs[0] / "RENDER_ORACLE.json"
                ).expected_pages,
                51,
            )
            repeated = invoke(commands[0], "--output", str(outputs[0]))
            self.assertEqual(repeated.returncode, 1)
            self.assertIn("fresh", repeated.stderr)
            self.assertEqual(first, payloads(outputs[0]))


if __name__ == "__main__":
    unittest.main()
