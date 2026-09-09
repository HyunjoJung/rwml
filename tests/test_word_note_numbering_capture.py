import copy
import io
import tempfile
import unittest
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path
from unittest import mock

from scripts import word_note_numbering_capture as notes

word = notes.word
pagination = notes.pagination


class WordNoteNumberingBackendTests(unittest.TestCase):
    def test_one_case_profile_requires_explicit_dispatch(self):
        result = mock.Mock(returncode=0, stdout="", stderr="")
        with mock.patch.object(word.subprocess, "run", return_value=result) as run:
            word._run_backend(
                powershell="powershell.exe",
                job_path=Path("job.json"),
                timeout_seconds=60,
                profile="note-numbering-v1",
            )
        self.assertEqual(run.call_args.args[0][-2:], ["-Profile", "note-numbering-v1"])

    def test_backend_binds_exact_note_input_and_distinct_schema(self):
        backend = word.BACKEND_PATH.read_text(encoding="utf-8")
        self.assertIn('"note-numbering-v1"', backend)
        self.assertIn('"rwml.word-note-numbering-export-job.v1"', backend)
        self.assertIn('"rwml.word-note-numbering-export-metadata.v1"', backend)
        self.assertIn(
            '"pilot-notes-numbering" = @(5856, '
            '"e3c2192b845dd27396fbd9e9709915578736e2d12433710a9a4e3be66dca8092")',
            backend,
        )


class WordNoteNumberingCaptureTests(unittest.TestCase):
    def _retained_capture(self, output):
        notes.materialize(output / "corpus")
        runtime = {
            "application": "Microsoft Word",
            "version": "16.0",
            "build": "1.0",
            "executable_sha256": "1" * 64,
            "os_version": "10.0.1",
            "os_build": "1",
            "machine": "AMD64",
            "powershell_version": "5.1.1",
        }
        producer = {
            "name": "microsoft-word",
            "mode": "windows-com",
            "version": "Microsoft Word 16.0 build 1.0",
            "identity_sha256": word.word_producer_identity(runtime),
            "platform": {
                "system": "Windows",
                "release": "10.0.1 build 1",
                "machine": "AMD64",
            },
        }
        lock = word.load_word_font_lock()
        runs = []
        for run_id in ("run-a", "run-b"):
            run = output / run_id
            (run / "pdf").mkdir(parents=True)
            payload = ("%PDF synthetic " + run_id).encode()
            (run / "pdf" / (notes.CASE_ID + ".pdf")).write_bytes(payload)
            metadata = {
                "schema": "rwml.word-note-numbering-export-metadata.v1",
                "run_id": run_id,
                "producer": producer,
                "runtime": runtime,
                "font": {
                    "family": lock["family"],
                    "postscript_name": lock["postscript_name"],
                    "bytes": lock["file"]["bytes"],
                    "sha256": lock["file"]["sha256"],
                    "installed_font_directory": True,
                },
                "export": word.WORD_EXPORT_OPTIONS,
                "documents": [{
                    "case_id": notes.CASE_ID,
                    "pdf_bytes": len(payload),
                    "pdf_sha256": notes.digest(payload),
                }],
            }
            word._write_json(run / "metadata.json", metadata)
            evidence = notes._reconstruct_run(output, run_id)
            word._write_json(run / "observations.json", evidence)
            runs.append(evidence)
        bundle = notes._bundle(output, "a" * 40, runs)
        word._write_json(output / "CAPTURE.json", bundle)
        return bundle

    def _observation(self):
        return {
            "pdf": {"bytes": 100, "sha256": "1" * 64, "pages": 1},
            "font_programs": [{"pdf_font_name": "NotoSans-Regular", "sha256": "2" * 64}],
            "pages": [{
                "fonts": ["NotoSans-Regular"],
                "spans": [{"text": "c First pilot footnote."}],
                "raster": {"sha256": "3" * 64},
            }],
        }

    def test_input_is_exact_locked_pilot_document_without_rewriting(self):
        payloads = notes.input_payloads()
        self.assertEqual(list(payloads), ["pilot-notes-numbering"])
        self.assertEqual(
            payloads[notes.CASE_ID],
            notes.pilot._payloads()["documents/pilot-notes-numbering.docx"],
        )
        self.assertEqual(
            (len(payloads[notes.CASE_ID]), notes.digest(payloads[notes.CASE_ID])),
            (5856, "e3c2192b845dd27396fbd9e9709915578736e2d12433710a9a4e3be66dca8092"),
        )

    def test_original_note_formats_start_values_and_font_are_preserved(self):
        payload = notes.input_payloads()[notes.CASE_ID]
        ns = {"w": notes.generator.W}
        val = "{%s}val" % ns["w"]
        with zipfile.ZipFile(io.BytesIO(payload)) as archive:
            settings = ET.fromstring(archive.read("word/settings.xml"))
            styles = ET.fromstring(archive.read("word/styles.xml"))
        for kind, fmt, start in (
            ("footnotePr", "lowerLetter", "3"),
            ("endnotePr", "lowerRoman", "2"),
        ):
            props = settings.find("w:" + kind, ns)
            self.assertEqual(props.find("w:numFmt", ns).get(val), fmt)
            self.assertEqual(props.find("w:numStart", ns).get(val), start)
        props = styles.find("w:docDefaults/w:rPrDefault/w:rPr", ns)
        for slot in ("ascii", "hAnsi", "eastAsia", "cs"):
            self.assertEqual(
                props.find("w:rFonts", ns).get("{%s}%s" % (ns["w"], slot)),
                "Noto Sans",
            )
        for tag in ("sz", "szCs"):
            self.assertEqual(props.find("w:" + tag, ns).get(val), "20")

    def test_input_mutation_and_extra_case_are_rejected(self):
        payloads = notes.input_payloads()
        for changed in (
            {},
            {**payloads, "extra": b"x"},
            {notes.CASE_ID: payloads[notes.CASE_ID] + b"x"},
        ):
            with self.subTest(keys=list(changed)), self.assertRaises(ValueError):
                notes.validate_inputs(changed)

    def test_job_schema_inventory_and_run_identity_are_fixed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            notes.materialize(root / "corpus")
            job = notes.build_job(root / "corpus", root / "run-a", root / "font.ttf", "run-a")
            self.assertEqual(job["schema"], "rwml.word-note-numbering-export-job.v1")
            self.assertEqual([row["case_id"] for row in job["documents"]], [notes.CASE_ID])
            self.assertEqual(job["documents"][0]["input_sha256"], notes.INPUTS[notes.CASE_ID][1])
            self.assertEqual(job["export"], word.WORD_EXPORT_OPTIONS)
            with self.assertRaisesRegex(ValueError, "run ID"):
                notes.build_job(root / "corpus", root / "run-c", root / "font.ttf", "run-c")

    def test_existing_profiles_reject_note_manifest_and_note_rejects_pilot(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            notes.materialize(root / "notes")
            pagination.materialize(root / "pagination")
            with self.assertRaises(ValueError):
                word.load_corpus_manifest(root / "notes/INPUTS.json")
            with self.assertRaisesRegex(ValueError, "exact pilot"):
                pagination.validate_input_directory(root / "notes")
            with self.assertRaisesRegex(ValueError, "exact note-numbering"):
                notes.validate_input_directory(root / "pagination")
            self.assertEqual(len(pagination.INPUTS), 3)

    def test_retained_provenance_manifest_and_input_drift_are_rejected(self):
        for name in ("PROVENANCE.md", "INPUTS.json", notes.CASE_ID + ".docx"):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as tmp:
                output = Path(tmp) / "corpus"
                notes.materialize(output)
                path = output / name
                path.write_bytes(path.read_bytes() + b"tampered")
                with self.assertRaises(ValueError):
                    notes.validate_input_directory(output)

    def test_repeatability_compares_labels_geometry_fonts_and_pixels(self):
        reference = {notes.CASE_ID: self._observation()}
        candidate = copy.deepcopy(reference)
        candidate[notes.CASE_ID]["pdf"]["sha256"] = "4" * 64
        self.assertTrue(notes.compare_observations(candidate, reference)["passed"])
        for field, value in (
            ("spans", [{"text": "1 First pilot footnote."}]),
            ("fonts", ["OtherFont"]),
            ("raster", {"sha256": "5" * 64}),
        ):
            candidate = copy.deepcopy(reference)
            candidate[notes.CASE_ID]["pages"][0][field] = value
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "repeatability"):
                notes.compare_observations(candidate, reference)
        with self.assertRaises(ValueError):
            notes.compare_observations({}, reference)

    def test_verify_reextracts_both_pdfs_and_rejects_observation_and_receipt_drift(self):
        with (
            tempfile.TemporaryDirectory() as tmp,
            mock.patch.object(notes, "observe_pdf", return_value=self._observation()) as extract,
            mock.patch.object(word, "_git_revision", return_value="a" * 40),
            mock.patch.object(word, "_git_dirty", return_value=False),
        ):
            output = Path(tmp) / "capture"
            bundle = self._retained_capture(output)
            extract.reset_mock()
            self.assertEqual(notes.verify(output), bundle)
            self.assertEqual(extract.call_count, 2)
            self.assertEqual(
                [call.args for call in extract.call_args_list],
                [
                    (output / run / "pdf" / (notes.CASE_ID + ".pdf"), notes.CASE_ID)
                    for run in ("run-a", "run-b")
                ],
            )
            path = output / "run-a/observations.json"
            retained = word._load_json(path)
            changed = copy.deepcopy(retained)
            changed["observations"][notes.CASE_ID]["pages"][0]["spans"][0]["text"] = "1"
            word._write_json(path, changed)
            with self.assertRaisesRegex(ValueError, "observations differ"):
                notes.verify(output)
            word._write_json(path, retained)
            for field, value in (
                ("profile", pagination.PROFILE),
                ("harness", {}),
                ("font", {}),
                ("analysis", {}),
                ("source_revision", "b" * 40),
            ):
                changed = copy.deepcopy(bundle)
                changed[field] = value
                word._write_json(output / "CAPTURE.json", changed)
                with self.subTest(field=field), self.assertRaisesRegex(
                    ValueError, "receipt differs"
                ):
                    notes.verify(output)

    def test_metadata_runtime_input_profile_and_pdf_identities_are_checked(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            notes, "observe_pdf", return_value=self._observation()
        ):
            output = Path(tmp) / "capture"
            self._retained_capture(output)
            path = output / "run-a/metadata.json"
            original = word._load_json(path)
            mutations = [
                {"schema": "rwml.word-pilot-export-metadata.v1"},
                {"run_id": "run-b"},
                {"documents": []},
                {"runtime": {**original["runtime"], "build": "2.0"}},
                {"font": {**original["font"], "sha256": "0" * 64}},
                {"export": {**original["export"], "pdfa": True}},
            ]
            for mutation in mutations:
                word._write_json(path, {**original, **mutation})
                with self.subTest(fields=list(mutation)), self.assertRaises(ValueError):
                    notes.load_metadata(output / "run-a", "run-a")
            word._write_json(path, original)
            pdf = output / "run-a/pdf" / (notes.CASE_ID + ".pdf")
            pdf.write_bytes(pdf.read_bytes() + b"tampered")
            with self.assertRaisesRegex(ValueError, "PDF identity"):
                notes.load_metadata(output / "run-a", "run-a")

    def test_producer_drift_prevents_success_receipt(self):
        observations = {notes.CASE_ID: self._observation()}
        runs = [
            {"producer": {"identity_sha256": "1" * 64}, "observations": observations},
            {"producer": {"identity_sha256": "2" * 64}, "observations": observations},
        ]
        with self.assertRaisesRegex(ValueError, "producer changed"):
            notes._bundle(Path("unused"), "a" * 40, runs)

    def test_unknown_case_and_missing_pinned_runtime_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "unknown note-numbering"):
            notes.observe_pdf(Path("unused.pdf"), pagination.ORIGINAL_CASE)
        with mock.patch.object(word.topology, "pymupdf", None):
            with self.assertRaisesRegex(ValueError, "PyMuPDF 1.28.2"):
                notes._analysis_runtime()

    @unittest.skipIf(word.topology.pymupdf is None, "PyMuPDF is unavailable")
    def test_real_embedded_fallback_font_is_rejected(self):
        module = word.topology.pymupdf
        if module.__version__ != notes.PYMUPDF_VERSION:
            self.skipTest("pinned PyMuPDF is unavailable")
        font = word.ROOT / "rwml-fonts/fonts/NotoSansKR-rwml-subset.ttf"
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "fallback.pdf"
            with module.open() as pdf:
                page = pdf.new_page()
                page.insert_text(
                    (72, 72),
                    "c First pilot footnote.",
                    fontname="FixtureFont",
                    fontfile=str(font),
                    fontsize=10,
                )
                pdf.save(path)
            with self.assertRaisesRegex(ValueError, "PDF font name|unexpected font"):
                notes.observe_pdf(path, notes.CASE_ID)


if __name__ == "__main__":
    unittest.main()
