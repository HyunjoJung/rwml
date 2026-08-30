import hashlib
import importlib.util
import io
import itertools
import json
import pathlib
import re
import sys
import tempfile
import unittest
import zipfile
from contextlib import redirect_stderr


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "generate_unequal_table_oracle.py"
LOCK = ROOT / "corpus" / "public" / "oracle" / "unequal-table-v1.json"
CONTRACT_SCRIPT = ROOT / "scripts" / "render_oracle_contract.py"


def load_module(name: str, path: pathlib.Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


generator = load_module("generate_unequal_table_oracle", SCRIPT)
contract = load_module("unequal_table_render_oracle_contract", CONTRACT_SCRIPT)


class UnequalTableOracleGeneratorTests(unittest.TestCase):
    def test_matrix_is_complete_canonical_and_sorted(self):
        expected = set(
            itertools.product(
                generator.LAYOUTS,
                generator.WIDTH_POLICIES,
                generator.FRAGMENT_CLASSES,
                generator.HANDOFF_CLASSES,
            )
        )
        observed = {
            (case.layout, case.width_policy, case.fragment, case.handoff)
            for case in generator.CASES
        }

        self.assertEqual(len(generator.CASES), 48)
        self.assertEqual(observed, expected)
        self.assertEqual(
            [case.case_id for case in generator.CASES],
            sorted(case.case_id for case in generator.CASES),
        )
        self.assertEqual(
            len({case.relative_path for case in generator.CASES}), 48
        )
        for index, case in enumerate(generator.CASES):
            self.assertEqual(case.index, index)
            self.assertRegex(case.case_id, r"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$")
            self.assertEqual(
                case.relative_path,
                f"documents/{case.case_id}.docx",
            )

    def test_checked_in_lock_matches_exact_generated_inputs(self):
        generator.verify_lock(LOCK)
        lock = json.loads(LOCK.read_text(encoding="utf-8"))
        artifacts = generator.campaign_artifacts()

        self.assertEqual(lock["schema"], "rwml.unequal-table-corpus-lock.v1")
        self.assertEqual(lock["campaign"], generator.CAMPAIGN)
        self.assertEqual(lock["generator"]["path"], SCRIPT.relative_to(ROOT).as_posix())
        self.assertEqual(
            lock["generator"]["sha256"], hashlib.sha256(SCRIPT.read_bytes()).hexdigest()
        )
        self.assertEqual(len(lock["documents"]), 48)

        for row in lock["documents"]:
            payload = artifacts[row["path"]]
            self.assertEqual(row["bytes"], len(payload))
            self.assertEqual(row["sha256"], hashlib.sha256(payload).hexdigest())

        self.assertEqual(
            lock["corpus_root_sha256"],
            generator.corpus_root_sha256(lock["documents"]),
        )

    def test_generated_campaign_satisfies_strict_public_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = pathlib.Path(tmp) / "unequal-table"
            generator.materialize(output, lock_path=LOCK)

            manifest = contract.load_corpus_manifest(output / "RENDER_ORACLE.json")
            lock = json.loads(LOCK.read_text(encoding="utf-8"))
            self.assertEqual(manifest.schema, contract.CORPUS_SCHEMA)
            self.assertEqual(manifest.campaign, generator.CAMPAIGN)
            self.assertEqual(len(manifest.documents), 48)
            self.assertEqual(manifest.expected_pages, 72)
            self.assertEqual(
                manifest.corpus_root_sha256, lock["corpus_root_sha256"]
            )
            self.assertTrue(generator.check_materialized(output, lock_path=LOCK))

            first = output / manifest.documents[0].relative_path
            first.write_bytes(b"stale")
            with redirect_stderr(io.StringIO()):
                self.assertFalse(generator.check_materialized(output, lock_path=LOCK))
            self.assertEqual(first.read_bytes(), b"stale")

    def test_documents_have_fixed_package_metadata_and_target_markup(self):
        tokens = set()
        for case in generator.CASES:
            with self.subTest(case=case.case_id):
                payload = generator.build_document(case)
                self.assertEqual(payload, generator.build_document(case))
                with zipfile.ZipFile(io.BytesIO(payload)) as archive:
                    self.assertEqual(
                        [info.filename for info in archive.infolist()],
                        [
                            "[Content_Types].xml",
                            "_rels/.rels",
                            "word/document.xml",
                            "word/_rels/document.xml.rels",
                            "word/styles.xml",
                        ],
                    )
                    self.assertTrue(
                        all(info.date_time == (1980, 1, 1, 0, 0, 0) for info in archive.infolist())
                    )
                    self.assertTrue(
                        all(info.create_system == 3 for info in archive.infolist())
                    )
                    document_xml = archive.read("word/document.xml").decode("utf-8")
                    styles_xml = archive.read("word/styles.xml").decode("utf-8")

                self.assertIn('w:pgSz w:w="7200" w:h="7200"', document_xml)
                self.assertIn(
                    'w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720"',
                    document_xml,
                )
                self.assertIn("<w:tblBorders>", document_xml)
                self.assertIn("Noto Sans", styles_xml)
                self.assertEqual(
                    document_xml.count('<w:br w:type="column"/>'),
                    case.column_count - 1 if case.handoff == "page" else 0,
                )

                if case.layout == "equal":
                    self.assertIn(
                        '<w:cols w:num="2" w:equalWidth="1" w:space="360"/>',
                        document_xml,
                    )
                else:
                    self.assertIn(
                        f'<w:cols w:num="{case.column_count}" w:equalWidth="0">',
                        document_xml,
                    )
                    self.assertIn("<w:col ", document_xml)

                width_marker = {
                    "auto": '<w:tblW w:w="0" w:type="auto"/>',
                    "dxa": '<w:tblW w:w="2400" w:type="dxa"/>',
                    "pct": '<w:tblW w:w="5000" w:type="pct"/>',
                }[case.width_policy]
                self.assertIn(width_marker, document_xml)
                self.assertEqual(
                    document_xml.count("<w:tr>"),
                    20 if case.fragment == "row-boundary" else 1,
                )

                case_tokens = set(re.findall(r">(T\d{2}[LR]\d{2})</w:t>", document_xml))
                self.assertEqual(
                    len(case_tokens),
                    40 if case.fragment == "row-boundary" else 52,
                )
                self.assertTrue(tokens.isdisjoint(case_tokens))
                tokens.update(case_tokens)

        self.assertEqual(len(tokens), 48 * 46)

    def test_no_generated_docx_is_committed_into_public_corpus(self):
        lock = json.loads(LOCK.read_text(encoding="utf-8"))
        committed = [
            LOCK.parent / row["path"]
            for row in lock["documents"]
            if (LOCK.parent / row["path"]).exists()
        ]
        self.assertEqual(committed, [])


if __name__ == "__main__":
    unittest.main()
