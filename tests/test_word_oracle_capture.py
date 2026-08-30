import copy
import hashlib
import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "word_oracle_capture.py"
BACKEND = ROOT / "scripts" / "word_oracle_export.ps1"
FONT_LOCK = ROOT / "corpus" / "public" / "oracle" / "word-font-lock.json"


def load_module(name: str, path: pathlib.Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


capture = load_module("word_oracle_capture", SCRIPT)


def runtime_identity():
    return {
        "application": "Microsoft Word",
        "version": "16.0",
        "build": "17928.20156",
        "executable_sha256": "1" * 64,
        "os_version": "10.0.26100",
        "os_build": "26100",
        "machine": "AMD64",
        "powershell_version": "5.1.26100.1",
    }


class WordOracleCaptureTests(unittest.TestCase):
    def test_checked_in_font_lock_is_exact_and_public(self):
        lock = capture.load_word_font_lock(FONT_LOCK)
        self.assertEqual(lock["schema"], "rwml.word-oracle-font-lock.v1")
        self.assertEqual(lock["family"], "Noto Sans")
        self.assertEqual(lock["postscript_name"], "NotoSans-Regular")
        self.assertEqual(lock["license"], "SIL-OFL-1.1")
        self.assertEqual(lock["file"]["name"], "NotoSans-Regular.ttf")
        self.assertEqual(lock["file"]["bytes"], 825628)
        self.assertEqual(
            lock["file"]["sha256"],
            "f5f552c8c5edb61fe6efb824baf4d4de47b1a8689ab4925ff43f7bd6a4ebece5",
        )
        self.assertEqual(lock["source"]["release_tag"], "NotoSans-v2.015")
        self.assertEqual(
            lock["source"]["asset"]["sha256"],
            "0c34df072a3fa7efbb7cbf34950e1f971a4447cffe365d3a359e2d4089b958f5",
        )
        self.assertEqual(
            lock["source"]["asset"]["member"],
            "NotoSans/full/ttf/NotoSans-Regular.ttf",
        )
        self.assertNotIn("/Users/", FONT_LOCK.read_text(encoding="utf-8"))

    def test_font_lock_rejects_duplicate_and_unknown_fields(self):
        original = FONT_LOCK.read_text(encoding="utf-8")
        duplicate = original.replace(
            '  "family": "Noto Sans",',
            '  "family": "Noto Sans",\n  "family": "Noto Sans",',
            1,
        )
        value = json.loads(original)
        value["private_path"] = "not-public"
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            duplicate_path = root / "duplicate.json"
            duplicate_path.write_text(duplicate, encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
                capture.load_word_font_lock(duplicate_path)

            unknown_path = root / "unknown.json"
            unknown_path.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "font lock keys differ"):
                capture.load_word_font_lock(unknown_path)

    def test_export_job_binds_every_input_output_and_font(self):
        lock = capture.load_word_font_lock(FONT_LOCK)
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            generator_output = root / "campaign"
            capture.materialize(generator_output)
            corpus = capture.load_corpus_manifest(
                generator_output / "RENDER_ORACLE.json"
            )
            run_directory = root / "run-a"
            run_directory.mkdir()
            font_path = root / lock["file"]["name"]
            job = capture.build_export_job(
                corpus,
                run_directory,
                font_path,
                lock,
                run_id="run-a",
            )
            tampered = copy.deepcopy(job)
            tampered["export"]["pdfa"] = True
            with self.assertRaisesRegex(ValueError, "fixed contract"):
                capture.validate_export_job(
                    tampered,
                    corpus,
                    run_directory,
                    font_path,
                    lock,
                    expected_run_id="run-a",
                )

        self.assertEqual(job["schema"], "rwml.word-export-job.v1")
        self.assertEqual(job["run_id"], "run-a")
        self.assertEqual(len(job["documents"]), 48)
        self.assertEqual(
            [row["case_id"] for row in job["documents"]],
            [document.case_id for document in corpus.documents],
        )
        self.assertEqual(job["font"]["bytes"], lock["file"]["bytes"])
        self.assertEqual(job["font"]["sha256"], lock["file"]["sha256"])
        self.assertEqual(job["export"], capture.WORD_EXPORT_OPTIONS)
        self.assertTrue(all(row["input"].endswith(".docx") for row in job["documents"]))
        self.assertTrue(all(row["output"].endswith(".pdf") for row in job["documents"]))

    def test_metadata_is_path_neutral_and_bound_to_pdf_set(self):
        lock = capture.load_word_font_lock(FONT_LOCK)
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            campaign = root / "campaign"
            capture.materialize(campaign)
            corpus = capture.load_corpus_manifest(campaign / "RENDER_ORACLE.json")
            pdf_directory = root / "pdf"
            pdf_directory.mkdir()
            documents = []
            for document in corpus.documents:
                payload = f"%PDF synthetic {document.case_id}".encode("ascii")
                path = pdf_directory / f"{document.case_id}.pdf"
                path.write_bytes(payload)
                documents.append(
                    {
                        "case_id": document.case_id,
                        "pdf_bytes": len(payload),
                        "pdf_sha256": hashlib.sha256(payload).hexdigest(),
                    }
                )
            runtime = runtime_identity()
            producer = {
                "name": "microsoft-word",
                "mode": "windows-com",
                "version": "Microsoft Word 16.0 build 17928.20156",
                "identity_sha256": capture.word_producer_identity(runtime),
                "platform": {
                    "system": "Windows",
                    "release": "10.0.26100 build 26100",
                    "machine": "AMD64",
                },
            }
            metadata = {
                "schema": "rwml.word-export-metadata.v1",
                "run_id": "run-a",
                "producer": producer,
                "runtime": runtime,
                "font": {
                    "family": lock["family"],
                    "postscript_name": lock["postscript_name"],
                    "bytes": lock["file"]["bytes"],
                    "sha256": lock["file"]["sha256"],
                    "installed_font_directory": True,
                },
                "export": copy.deepcopy(capture.WORD_EXPORT_OPTIONS),
                "documents": documents,
            }
            validated = capture.validate_export_metadata(
                metadata,
                corpus,
                pdf_directory,
                lock,
                expected_run_id="run-a",
            )
            self.assertEqual(validated, producer)

            tampered = copy.deepcopy(metadata)
            tampered["runtime"]["build"] = "different"
            with self.assertRaisesRegex(ValueError, "producer identity"):
                capture.validate_export_metadata(
                    tampered,
                    corpus,
                    pdf_directory,
                    lock,
                    expected_run_id="run-a",
                )

            tampered = copy.deepcopy(metadata)
            tampered["documents"][0]["pdf_sha256"] = "2" * 64
            with self.assertRaisesRegex(ValueError, "PDF identity"):
                capture.validate_export_metadata(
                    tampered,
                    corpus,
                    pdf_directory,
                    lock,
                    expected_run_id="run-a",
                )

            extra = pdf_directory / "extra.pdf"
            extra.write_bytes(b"%PDF unexpected")
            with self.assertRaisesRegex(ValueError, "exact campaign set"):
                capture.validate_export_metadata(
                    metadata,
                    corpus,
                    pdf_directory,
                    lock,
                    expected_run_id="run-a",
                )

    def test_backend_disables_macros_dialogs_and_uses_fixed_pdf_options(self):
        text = BACKEND.read_text(encoding="utf-8")
        for marker in (
            "$word.AutomationSecurity = 3",
            "$word.DisplayAlerts = 0",
            "$word.Visible = $false",
            "Documents.Open",
            "ExportAsFixedFormat",
            "$word.Quit(0)",
            "Get-FileHash",
            "NotoSans-Regular",
        ):
            self.assertIn(marker, text)
        for forbidden in (
            "Invoke-WebRequest",
            "Invoke-RestMethod",
            "Start-BitsTransfer",
            "System.Net.WebClient",
        ):
            self.assertNotIn(forbidden, text)

    def test_font_name_normalization_is_strict(self):
        self.assertEqual(
            capture.normalize_pdf_font_name("ABCDEF+NotoSans-Regular"),
            "NotoSans-Regular",
        )
        self.assertEqual(
            capture.normalize_pdf_font_name("NotoSans-Regular"),
            "NotoSans-Regular",
        )
        with self.assertRaisesRegex(ValueError, "PDF font name"):
            capture.normalize_pdf_font_name("../../NotoSans-Regular")
        with self.assertRaisesRegex(ValueError, "PDF font name"):
            capture.normalize_pdf_font_name("abcdef+NotoSans-Regular")


if __name__ == "__main__":
    unittest.main()
