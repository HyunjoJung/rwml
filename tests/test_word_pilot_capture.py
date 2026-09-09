import copy
import hashlib
import io
import tempfile
import unittest
from unittest import mock
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

from scripts import word_pilot_capture as pilot

word = pilot.word


class WordPilotCaptureTests(unittest.TestCase):
    def _retained_capture(self, output):
        pilot.materialize(output / "corpus")
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
            documents = []
            for case_id in pilot.INPUTS:
                payload = ("%PDF synthetic " + case_id).encode()
                (run / "pdf" / (case_id + ".pdf")).write_bytes(payload)
                documents.append(
                    {
                        "case_id": case_id,
                        "pdf_bytes": len(payload),
                        "pdf_sha256": pilot.digest(payload),
                    }
                )
            metadata = {
                "schema": "rwml.word-pilot-export-metadata.v1",
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
                "documents": documents,
            }
            word._write_json(run / "metadata.json", metadata)
            evidence = pilot._reconstruct_run(output, run_id)
            word._write_json(run / "observations.json", evidence)
            runs.append(evidence)
        bundle = pilot._bundle(output, "a" * 40, runs)
        word._write_json(output / "CAPTURE.json", bundle)
        return bundle

    def test_three_inputs_match_locked_originals_and_control(self):
        payloads = pilot.input_payloads()
        self.assertEqual(set(payloads), set(pilot.INPUTS))
        for case_id, payload in payloads.items():
            self.assertEqual(len(payload), pilot.INPUTS[case_id][0])
            self.assertEqual(
                hashlib.sha256(payload).hexdigest(), pilot.INPUTS[case_id][1]
            )

    def test_control_changes_only_styles_and_sets_all_font_scripts_and_size(self):
        payloads = pilot.input_payloads()
        with zipfile.ZipFile(
            io.BytesIO(payloads["synthetic-pagination-keep"])
        ) as original:
            with zipfile.ZipFile(
                io.BytesIO(payloads["synthetic-pagination-keep-noto11"])
            ) as control:
                self.assertEqual(original.namelist(), control.namelist())
                changed = [
                    name
                    for name in original.namelist()
                    if original.read(name) != control.read(name)
                ]
                self.assertEqual(changed, ["word/styles.xml"])
                styles = ET.fromstring(control.read("word/styles.xml"))
        ns = {"w": pilot.generator.W}
        props = styles.find("w:docDefaults/w:rPrDefault/w:rPr", ns)
        self.assertIsNotNone(props)
        fonts = props.find("w:rFonts", ns)
        for name in ("ascii", "hAnsi", "eastAsia", "cs"):
            self.assertEqual(fonts.get("{%s}%s" % (ns["w"], name)), "Noto Sans")
        for name in ("sz", "szCs"):
            self.assertEqual(props.find("w:" + name, ns).get("{%s}val" % ns["w"]), "22")

    def test_input_drift_and_arbitrary_profile_cases_are_rejected(self):
        payloads = pilot.input_payloads()
        payloads["synthetic-pagination-keep"] += b"changed"
        with self.assertRaisesRegex(ValueError, "input identity"):
            pilot.validate_inputs(payloads)
        payloads = pilot.input_payloads()
        payloads["extra-case"] = b"anything"
        with self.assertRaisesRegex(ValueError, "exact pilot"):
            pilot.validate_inputs(payloads)

    def test_job_has_distinct_schema_and_exact_bounded_inputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            pilot.materialize(root / "corpus")
            run = root / "run-a"
            run.mkdir()
            job = pilot.build_job(
                root / "corpus", run, root / "NotoSans-Regular.ttf", "run-a"
            )
            self.assertEqual(job["schema"], "rwml.word-pilot-export-job.v1")
            self.assertEqual(
                [row["case_id"] for row in job["documents"]], list(pilot.INPUTS)
            )
            self.assertEqual(job["export"], word.WORD_EXPORT_OPTIONS)
            self.assertTrue(
                all(Path(row["input"]).is_absolute() for row in job["documents"])
            )

    def test_repeatability_checks_geometry_fonts_and_pixels_but_not_pdf_bytes(self):
        reports = {
            case_id: {
                "pdf": {"sha256": "a"},
                "font_programs": [],
                "pages": [
                    {"width_millipoints": 1000, "spans": [], "raster_sha256": "1"}
                ],
            }
            for case_id in pilot.INPUTS
        }
        other = copy.deepcopy(reports)
        other[next(iter(other))]["pdf"]["sha256"] = "b"
        self.assertEqual(
            pilot.compare_observations(reports, other)["exact_documents"], 3
        )
        other[next(iter(other))]["pages"][0]["raster_sha256"] = "2"
        with self.assertRaisesRegex(ValueError, "repeatability"):
            pilot.compare_observations(reports, other)

    def test_existing_matrix_rejects_pilot_manifest(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "corpus"
            pilot.materialize(output)
            with self.assertRaises(ValueError):
                word.load_corpus_manifest(output / "INPUTS.json")

    def test_backend_default_remains_48_and_pilot_checks_each_digest(self):
        backend = word.BACKEND_PATH.read_text(encoding="utf-8")
        self.assertIn("$ExpectedDocumentCount = 48", backend)
        self.assertIn("rwml.word-export-job.v1", backend)
        self.assertIn("rwml.word-pilot-export-job.v1", backend)
        self.assertIn("$ExpectedPilotInputs", backend)
        for _, digest in pilot.INPUTS.values():
            self.assertIn(digest, backend)

    @unittest.skipIf(word.topology.pymupdf is None, "PyMuPDF is unavailable")
    def test_pdf_observations_record_original_font_and_reject_explicit_fallback(self):
        pdf_module = word.topology.pymupdf
        if pdf_module.__version__ != pilot.PYMUPDF_VERSION:
            self.skipTest("pinned PyMuPDF is unavailable")
        font = word.ROOT / "rwml-fonts/fonts/NotoSansKR-rwml-subset.ttf"
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "original.pdf"
            with pdf_module.open() as pdf:
                page = pdf.new_page()
                page.insert_text(
                    (72, 72),
                    "First line\nSecond line",
                    fontname="FixtureFont",
                    fontfile=str(font),
                    fontsize=11,
                )
                pdf.save(path)
            report = pilot.observe_pdf(path, pilot.ORIGINAL_CASE)
            self.assertEqual(report["pdf"]["pages"], 1)
            self.assertEqual(len(report["pages"][0]["spans"]), 2)
            self.assertEqual(report["pages"][0]["spans"][0]["size_millipoints"], 11000)
            self.assertTrue(report["font_programs"][0]["sha256"])
            self.assertTrue(report["pages"][0]["raster"]["sha256"])
            self.assertGreater(
                report["pages"][0]["spans"][1]["origin_millipoints"][1],
                report["pages"][0]["spans"][0]["origin_millipoints"][1],
            )
            with self.assertRaisesRegex(ValueError, "PDF font name|unexpected font"):
                pilot.observe_pdf(path, "synthetic-pagination-keep-noto11")

    def test_missing_analysis_runtime_fails_before_word(self):
        with mock.patch.object(word.topology, "pymupdf", None):
            with self.assertRaisesRegex(ValueError, "PyMuPDF 1.28.2"):
                pilot._analysis_runtime()

    def test_shared_backend_requires_explicit_pilot_profile(self):
        result = mock.Mock(returncode=0, stdout="", stderr="")
        with mock.patch.object(word.subprocess, "run", return_value=result) as run:
            word._run_backend(
                powershell="powershell.exe",
                job_path=Path("job.json"),
                timeout_seconds=60,
                profile=pilot.PROFILE,
            )
            self.assertEqual(run.call_args.args[0][-2:], ["-Profile", pilot.PROFILE])
            with self.assertRaisesRegex(ValueError, "profile"):
                word._run_backend(
                    powershell="powershell.exe",
                    job_path=Path("job.json"),
                    timeout_seconds=60,
                    profile="arbitrary",
                )
            self.assertEqual(run.call_count, 1)

    def test_retained_input_mutation_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "corpus"
            pilot.materialize(output)
            path = output / "synthetic-pagination-keep-noto11.docx"
            path.write_bytes(path.read_bytes() + b"tampered")
            with self.assertRaisesRegex(ValueError, "input identity"):
                pilot.validate_input_directory(output)

    def test_verify_reextracts_and_rejects_retained_observation_and_receipt_mutation(
        self,
    ):
        observation = {
            "pdf": {"pages": 1},
            "font_programs": [],
            "pages": [{"spans": [], "raster": {"sha256": "1" * 64}}],
        }
        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch.object(
                pilot, "observe_pdf", return_value=observation
            ) as extract:
                with mock.patch.object(word, "_git_revision", return_value="a" * 40):
                    with mock.patch.object(word, "_git_dirty", return_value=False):
                        output = Path(tmp) / "capture"
                        bundle = self._retained_capture(output)
                        extract.reset_mock()
                        self.assertEqual(pilot.verify(output), bundle)
                        self.assertEqual(extract.call_count, 6)
                        path = output / "run-a/observations.json"
                        retained = word._load_json(path)
                        modified = copy.deepcopy(retained)
                        modified["observations"][pilot.ORIGINAL_CASE]["pages"][0][
                            "raster"
                        ]["sha256"] = "2" * 64
                        word._write_json(path, modified)
                        with self.assertRaisesRegex(ValueError, "observations differ"):
                            pilot.verify(output)
                        word._write_json(path, retained)
                        modified = copy.deepcopy(bundle)
                        modified["repeatability"]["exact_documents"] = 2
                        word._write_json(output / "CAPTURE.json", modified)
                        with self.assertRaisesRegex(ValueError, "receipt differs"):
                            pilot.verify(output)

    def test_producer_drift_prevents_success_receipt(self):
        observations = {case_id: {"pages": []} for case_id in pilot.INPUTS}
        runs = [
            {"producer": {"identity_sha256": "1" * 64}, "observations": observations},
            {"producer": {"identity_sha256": "2" * 64}, "observations": observations},
        ]
        with self.assertRaisesRegex(ValueError, "producer changed"):
            pilot._bundle(Path("unused"), "a" * 40, runs)


if __name__ == "__main__":
    unittest.main()
